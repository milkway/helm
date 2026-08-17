//! Ciclo de vida das sessões: spawn, reconexão com backoff, detach.
//!
//! Cada sessão tem id estável e um loop próprio em thread dedicada.
//! Queda inesperada → backoff exponencial 1–30s, 5 tentativas → estado de
//! erro com novo ciclo a cada 60s (interrompível por retry_session).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use portable_pty::CommandBuilder;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use zeroize::Zeroizing;

use crate::db::{self, Db, Host};
use crate::session::pty;
use crate::vault::{self, Vault};

const MAX_ATTEMPTS: u32 = 5;
const ERROR_RETRY_SECS: u64 = 60;
/// Dez segundos filtram falhas de autenticação/handshake sem punir tanto links
/// instáveis; quedas após isso reiniciam o backoff como uma conexão real.
const MIN_STABLE_SECS: u64 = 10;
const EXIT_STATUS_WAIT: Duration = Duration::from_secs(5);

pub struct Session {
    #[allow(dead_code)] // usado nas fases de latência
    host_id: Option<String>,
    askpass_secret: Option<Zeroizing<String>>,
    askpass_disabled: AtomicBool,
    closed: AtomicBool,
    detached: AtomicBool,
    retry_now: AtomicBool,
    size: Mutex<(u16, u16)>,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    io: Mutex<Option<Io>>,
    // detecção de atenção (Fase 8)
    connected: AtomicBool,
    attention: AtomicBool,
    sudo_prompt: AtomicBool,
    sudo_dismissed: AtomicBool,
    sudo_credential_id: Option<String>,
    output_tail: Mutex<String>,
    last_output: Mutex<Instant>,
    last_input: Mutex<Instant>,
}

struct Io {
    master: Box<dyn portable_pty::MasterPty + Send>,
    killer: Box<dyn portable_pty::ChildKiller + Send + Sync>,
}

struct AskpassAttempt {
    helper: PathBuf,
    secret_file: PathBuf,
}

impl Drop for AskpassAttempt {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.secret_file);
    }
}

#[derive(Default)]
pub struct Sessions(pub Mutex<HashMap<String, Arc<Session>>>);

#[derive(Clone, Serialize)]
struct OutputPayload<'a> {
    id: &'a str,
    /// Chunk de bytes do PTY em base64 — preserva sequências UTF-8/ANSI
    /// partidas na fronteira do chunk.
    data: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusPayload<'a> {
    id: &'a str,
    status: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    attempt: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    delay_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_code: Option<u32>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AttentionPayload<'a> {
    id: &'a str,
    active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'a str>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SudoPromptPayload<'a> {
    id: &'a str,
    active: bool,
    context: String,
}

fn set_attention(app: &AppHandle, session: &Session, id: &str, active: bool, reason: Option<&str>) {
    let was = session.attention.swap(active, Ordering::Relaxed);
    if was != active {
        let _ = app.emit("attention", AttentionPayload { id, active, reason });
    }
}

fn set_sudo_prompt(
    app: &AppHandle,
    session: &Session,
    id: &str,
    active: bool,
    context: &str,
) {
    let active = active && session.sudo_credential_id.is_some();
    let was = session.sudo_prompt.swap(active, Ordering::Relaxed);
    if was != active {
        let context = if active {
            context.to_string()
        } else {
            String::new()
        };
        let _ = app.emit(
            "sudo-prompt",
            SudoPromptPayload {
                id,
                active,
                context,
            },
        );
    }
}

/// Remove sequências ANSI/escape para analisar o texto "cru" do prompt.
/// Itera por chars (não bytes) para preservar UTF-8 — caracteres multibyte
/// como `❯` ou acentos precisam sobreviver à limpeza p/ a heurística casar.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('[') => {
                chars.next(); // '['
                // consome parâmetros até a letra final (ASCII)
                while let Some(&pc) = chars.peek() {
                    chars.next();
                    if pc.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next(); // ']'
                // OSC até BEL ou ST (ESC); o ESC de um ST é reprocessado no loop
                while let Some(&pc) = chars.peek() {
                    if pc == '\u{07}' || pc == '\u{1b}' {
                        break;
                    }
                    chars.next();
                }
                if chars.peek() == Some(&'\u{07}') {
                    chars.next(); // consome o BEL terminador
                }
            }
            _ => { /* ESC solto: descarta */ }
        }
    }
    out
}

/// Heurística: a cauda do output parece um prompt interativo aguardando input?
/// Prompts de shell "ociosos" ($ # >) NÃO contam — só perguntas/confirmações.
fn looks_like_prompt(tail: &str) -> bool {
    let clean = strip_ansi(tail);
    let trimmed = clean.trim_end();
    let last = trimmed.lines().last().unwrap_or("").trim();
    let low = trimmed.to_lowercase();

    if last.ends_with('?') || last.ends_with("? ") {
        return true;
    }
    const NEEDLES: &[&str] = &[
        "(y/n)", "[y/n]", "(yes/no)", "(y/n/a)", "[y/n/a]",
        "password:", "passphrase", "password for",
        "do you want", "are you sure", "proceed?", "continue?",
        "overwrite?", "confirm", "press enter", "❯",
        "always allow", "allow once", "1. yes",
        "aguardando", "waiting for input",
    ];
    NEEDLES.iter().any(|n| low.contains(n))
}

/// Prompt ativo do sudo padrão. Só considera a última linha não-vazia da
/// cauda, para não reativar um prompt antigo depois que o comando continuou.
fn is_active_sudo_prompt(tail: &str) -> bool {
    let clean = strip_ansi(tail);
    let last = clean
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim_end();
    last.strip_prefix("[sudo] password for ")
        .and_then(|rest| rest.strip_suffix(':'))
        .is_some_and(|user| !user.is_empty())
}

/// Contexto curto e sem escapes para o usuário conferir de onde veio o
/// prompt. Mantém o fim para nunca cortar justamente a linha do sudo.
fn sudo_prompt_context(tail: &str) -> String {
    const MAX_CONTEXT_CHARS: usize = 200;

    let clean = strip_ansi(tail);
    let lines: Vec<&str> = clean
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let context = lines[lines.len().saturating_sub(3)..].join("\n");
    let char_count = context.chars().count();
    if char_count <= MAX_CONTEXT_CHARS {
        context
    } else {
        let keep = MAX_CONTEXT_CHARS - 1;
        format!("…{}", context.chars().skip(char_count - keep).collect::<String>())
    }
}

fn emit_status(app: &AppHandle, id: &str, status: &str, attempt: Option<u32>, delay: Option<u64>) {
    emit_status_with_exit_code(app, id, status, attempt, delay, None);
}

fn emit_status_with_exit_code(
    app: &AppHandle,
    id: &str,
    status: &str,
    attempt: Option<u32>,
    delay: Option<u64>,
    exit_code: Option<u32>,
) {
    let _ = app.emit(
        "session-status",
        StatusPayload {
            id,
            status,
            attempt,
            delay_secs: delay,
            exit_code,
        },
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExitKind {
    Clean,
    Failure,
    Unknown,
}

fn classify_exit_status(exit_status: Option<&portable_pty::ExitStatus>) -> ExitKind {
    // portable-pty representa morte por sinal com código 1; o sinal precisa
    // prevalecer sobre o código para que kill/OOM/sleep-wake reconectem.
    match exit_status {
        Some(status) if status.signal().is_some() => ExitKind::Failure,
        Some(status) if status.exit_code() == 255 => ExitKind::Failure,
        Some(_) => ExitKind::Clean,
        None => ExitKind::Unknown,
    }
}

fn askpass_session_tag(id: &str) -> u64 {
    let mut hash = 1469598103934665603u64;
    for byte in id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1099511628211);
    }
    hash
}

#[cfg(unix)]
fn ensure_private_askpass_dir(dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    match std::fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = std::fs::symlink_metadata(dir).map_err(|e| e.to_string())?;
            if metadata.file_type().is_dir() && metadata.permissions().mode() & 0o777 == 0o700 {
                Ok(())
            } else {
                Err(format!("diretório askpass inseguro: {}", dir.display()))
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(not(unix))]
fn ensure_private_askpass_dir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir(dir)
        .map_err(|e| e.to_string())
        .or_else(|e| if dir.is_dir() { Ok(()) } else { Err(e) })
}

fn create_askpass_attempt(
    app: &AppHandle,
    id: &str,
    counter: u64,
    secret: &str,
) -> Result<AskpassAttempt, String> {
    let helper = std::env::current_exe().map_err(|e| e.to_string())?;
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&app_data_dir).map_err(|e| e.to_string())?;
    let dir = app_data_dir.join("askpass");
    ensure_private_askpass_dir(&dir)?;
    let secret_file = dir.join(format!(
        "secret-{:016x}-{}-{counter}",
        askpass_session_tag(id),
        std::process::id()
    ));

    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&secret_file)
            .map_err(|e| e.to_string())?
    };
    #[cfg(not(unix))]
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&secret_file)
        .map_err(|e| e.to_string())?;

    #[cfg(unix)]
    if let Err(e) = file.set_permissions({
        use std::os::unix::fs::PermissionsExt;
        std::fs::Permissions::from_mode(0o600)
    }) {
        drop(file);
        let _ = std::fs::remove_file(&secret_file);
        return Err(e.to_string());
    }

    if let Err(e) = file.write_all(secret.as_bytes()).and_then(|_| file.flush()) {
        drop(file);
        let _ = std::fs::remove_file(&secret_file);
        return Err(e.to_string());
    }

    Ok(AskpassAttempt {
        helper,
        secret_file,
    })
}

/// Nome de sessão tmux derivado do nome do host (tmux não aceita ':' e '.').
pub fn tmux_session_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '-' })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() { "helm".into() } else { s }
}

/// Quoting seguro para shell remoto (single quotes).
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Parâmetros por sessão (modal Nova sessão, 1c). Sem eles, valem os
/// defaults do host (startup_mode / auto_attach / project_dir).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionParams {
    /// shell | tmux | clmux
    pub mode: String,
    pub session_name: Option<String>,
    pub project_dir: Option<String>,
    pub agent: Option<String>,
}

fn agent_binary(agent: Option<&str>) -> Result<&'static str, String> {
    match agent.unwrap_or("claude") {
        "claude" => Ok("claude"),
        "codex" => Ok("codex"),
        value => Err(format!(
            "agente de IA inválido: {value:?}; use apenas 'claude' ou 'codex'"
        )),
    }
}

/// Comando remoto conforme o modo. clmux: attach se a sessão existir; senão
/// cria e digita o agente permitido nela (send-keys).
fn remote_command(
    mode: &str,
    name: &str,
    dir: Option<&str>,
    agent: Option<&str>,
) -> Result<Option<String>, String> {
    let name_q = shell_quote(name);
    let cd = dir
        .filter(|d| !d.is_empty())
        .map(|d| format!("cd {} && ", shell_quote(d)))
        .unwrap_or_default();
    Ok(match mode {
        "tmux" => Some(format!("{cd}tmux new -As {name_q}")),
        "clmux" => {
            let agent = agent_binary(agent)?;
            Some(format!(
                "{cd}if tmux has-session -t {name_q} 2>/dev/null; then tmux attach -t {name_q}; \
                 else tmux new -s {name_q} \\; send-keys {agent} Enter; fi"
            ))
        }
        // shell na pasta do projeto: cd + shell de login interativo
        "shell" if !cd.is_empty() => Some(format!("{cd}exec \"${{SHELL:-bash}}\" -l")),
        _ => None,
    })
}

fn build_command(
    host: Option<&Host>,
    params: Option<&SessionParams>,
    askpass: Option<&AskpassAttempt>,
) -> Result<CommandBuilder, String> {
    match host {
        None => {
            let mut cmd = CommandBuilder::new(pty::default_shell());
            cmd.arg("-l"); // login shell, como em qualquer app de terminal
            cmd.env("TERM", "xterm-256color");
            cmd.env("COLORTERM", "truecolor");
            if let Some(home) = std::env::var_os("HOME") {
                cmd.cwd(home);
            }
            Ok(cmd)
        }
        Some(host) => {
            // Nunca deixar host/user virarem flags do ssh (ex.: "-oProxyCommand=…").
            if host.host.starts_with('-') {
                return Err(format!("host inválido: {:?}", host.host));
            }
            if let Some(user) = &host.user {
                if user.starts_with('-') {
                    return Err(format!("user inválido: {user:?}"));
                }
            }
            let mut cmd = CommandBuilder::new("ssh");
            // Não herda configuração askpass do processo pai nem a variável
            // antiga que carregava o segredo diretamente no ambiente.
            for key in [
                "SSH_ASKPASS",
                "SSH_ASKPASS_REQUIRE",
                "HELM_ASKPASS_MODE",
                "HELM_ASKPASS_SECRET",
                "HELM_ASKPASS_SECRET_FILE",
            ] {
                cmd.env_remove(key);
            }
            cmd.arg("-tt");
            cmd.arg("-o");
            cmd.arg("ConnectTimeout=10");
            cmd.arg("-o");
            cmd.arg("ServerAliveInterval=15");
            cmd.arg("-o");
            cmd.arg("ServerAliveCountMax=3");
            if let Some(askpass) = askpass {
                cmd.env("SSH_ASKPASS", &askpass.helper);
                cmd.env("SSH_ASKPASS_REQUIRE", "force");
                cmd.env("HELM_ASKPASS_MODE", "1");
                cmd.env("HELM_ASKPASS_SECRET_FILE", &askpass.secret_file);
                cmd.arg("-o");
                cmd.arg("NumberOfPasswordPrompts=1");
            }
            if let Some(port) = host.port {
                cmd.arg("-p");
                cmd.arg(port.to_string());
            }
            let target = match &host.user {
                Some(user) if !user.is_empty() => format!("{user}@{}", host.host),
                _ => host.host.clone(),
            };
            cmd.arg("--"); // terminador de argv: o alvo jamais é interpretado como flag
            cmd.arg(target);

            // modo efetivo: params > startup_mode do host; auto_attach eleva
            // shell para tmux (re-attach automático, Fase 3)
            let mode = match params {
                Some(p) => p.mode.clone(),
                None if host.auto_attach && host.startup_mode == "shell" => "tmux".into(),
                None => host.startup_mode.clone(),
            };
            let name = params
                .and_then(|p| p.session_name.clone())
                .map(|n| tmux_session_name(&n))
                .unwrap_or_else(|| tmux_session_name(&host.name));
            let dir = params
                .and_then(|p| p.project_dir.clone())
                .or_else(|| host.project_dir.clone());

            let agent = params.and_then(|p| p.agent.as_deref());
            if let Some(remote) = remote_command(&mode, &name, dir.as_deref(), agent)? {
                cmd.arg(remote);
            }
            cmd.env("TERM", "xterm-256color");
            Ok(cmd)
        }
    }
}

/// Sleep em fatias, interrompível por closed/retry_now. Retorna true se
/// foi interrompido por retry_now.
fn interruptible_sleep(session: &Session, secs: u64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if session.closed.load(Ordering::Relaxed) {
            return false;
        }
        if session.retry_now.swap(false, Ordering::Relaxed) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    false
}

fn manager_loop(
    app: AppHandle,
    id: String,
    session: Arc<Session>,
    host: Option<Host>,
    params: Option<SessionParams>,
) {
    let auto_reconnect = host.as_ref().map(|h| h.auto_reconnect).unwrap_or(false);
    let vpn_profile = host
        .as_ref()
        .and_then(|h| h.vpn_profile.clone())
        .filter(|p| !p.is_empty());
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut attempt: u32 = 0;
    let mut askpass_counter: u64 = 0;

    // Host exige VPN → conecta antes do SSH (sequência do design 4a).
    if let Some(profile) = &vpn_profile {
        emit_status(&app, &id, "vpn", None, None);
        if let Some(vpn) = app.try_state::<crate::vpn::Vpn>() {
            if let Err(e) = crate::vpn::acquire(&app, &vpn, profile) {
                eprintln!("[session {id}] VPN '{profile}' falhou: {e}");
                emit_status(&app, &id, "error", Some(0), None);
                if let Some(sessions) = app.try_state::<Sessions>() {
                    let mut map = sessions.0.lock().unwrap();
                    if map.get(&id).is_some_and(|s| Arc::ptr_eq(s, &session)) {
                        map.remove(&id);
                    }
                }
                return;
            }
        }
    }

    loop {
        if session.closed.load(Ordering::Relaxed) {
            break;
        }

        if attempt == 0 {
            emit_status(&app, &id, "connecting", None, None);
        }

        let (cols, rows) = *session.size.lock().unwrap();
        let askpass = if !session.askpass_disabled.load(Ordering::Relaxed) {
            session.askpass_secret.as_ref().and_then(|secret| {
                askpass_counter = askpass_counter.saturating_add(1);
                match create_askpass_attempt(&app, &id, askpass_counter, secret) {
                    Ok(askpass) => Some(askpass),
                    Err(e) => {
                        eprintln!(
                            "[session {id}] preparação do askpass falhou ({e}); usando autenticação interativa"
                        );
                        session.askpass_disabled.store(true, Ordering::Relaxed);
                        None
                    }
                }
            })
        } else {
            None
        };
        let used_askpass = askpass.is_some();
        let cmd = match build_command(host.as_ref(), params.as_ref(), askpass.as_ref()) {
            Ok(cmd) => cmd,
            Err(e) => {
                eprintln!("[session {id}] comando inválido: {e}");
                emit_status(&app, &id, "error", Some(attempt), None);
                break;
            }
        };

        let spawn_result = pty::spawn(cmd, cols, rows);
        let (lived, got_output, exit_status) = match spawn_result {
            Err(e) => {
                eprintln!("[session {id}] spawn falhou: {e}");
                (Duration::ZERO, false, None)
            }
            Ok(mut handles) => {
                *session.writer.lock().unwrap() = Some(handles.writer);
                *session.io.lock().unwrap() = Some(Io {
                    master: handles.master,
                    killer: handles.killer,
                });
                // close_session pode ter sido chamado entre o teste de `closed`
                // no início do loop e a publicação dos handles do novo PTY.
                if session.closed.load(Ordering::Relaxed) {
                    close_session_inner(&session);
                }
                let started = Instant::now();
                let mut got_output = false;
                let mut buf = [0u8; 8192];
                session.connected.store(true, Ordering::Relaxed);
                loop {
                    match handles.reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if !got_output {
                                got_output = true;
                                emit_status(&app, &id, "connected", None, None);
                            }
                            // rastreia a cauda do output para a heurística de atenção
                            {
                                let mut tail = session.output_tail.lock().unwrap();
                                tail.push_str(&String::from_utf8_lossy(&buf[..n]));
                                let len = tail.len();
                                if len > 400 {
                                    // corta em fronteira de char: len-400 pode cair no
                                    // meio de um multibyte e panicar (envenenaria o lock)
                                    let mut cut = len - 400;
                                    while cut < len && !tail.is_char_boundary(cut) {
                                        cut += 1;
                                    }
                                    *tail = tail[cut..].to_string();
                                }
                                let sudo_active = is_active_sudo_prompt(&tail);
                                if !sudo_active {
                                    session.sudo_dismissed.store(false, Ordering::Relaxed);
                                }
                                let visible = sudo_active
                                    && !session.sudo_dismissed.load(Ordering::Relaxed);
                                let context = if visible {
                                    sudo_prompt_context(&tail)
                                } else {
                                    String::new()
                                };
                                set_sudo_prompt(&app, &session, &id, visible, &context);
                            }
                            *session.last_output.lock().unwrap() = Instant::now();
                            // novo output = atividade → limpa atenção
                            set_attention(&app, &session, &id, false, None);
                            let _ = app.emit(
                                "session-output",
                                OutputPayload {
                                    id: &id,
                                    data: b64.encode(&buf[..n]),
                                },
                            );
                        }
                    }
                }
                session.connected.store(false, Ordering::Relaxed);
                set_attention(&app, &session, &id, false, None);
                {
                    let _tail = session.output_tail.lock().unwrap();
                    session.sudo_dismissed.store(false, Ordering::Relaxed);
                    set_sudo_prompt(&app, &session, &id, false, "");
                }
                *session.io.lock().unwrap() = None;
                *session.writer.lock().unwrap() = None;
                let exit_status = match handles.exit_status.recv_timeout(EXIT_STATUS_WAIT) {
                    Ok(Ok(status)) => Some(status),
                    Ok(Err(e)) => {
                        eprintln!("[session {id}] falha ao obter status do processo: {e}");
                        None
                    }
                    Err(e) => {
                        eprintln!("[session {id}] status do processo indisponível após EOF: {e}");
                        None
                    }
                };
                (started.elapsed(), got_output, exit_status)
            }
        };
        // Normalmente o helper já consumiu e apagou o arquivo; cobre spawn/SSH
        // que falharam antes de chamá-lo e qualquer outra sobra da tentativa.
        drop(askpass);

        let exit_code = exit_status.as_ref().map(|status| status.exit_code());
        if let Some(status) = &exit_status {
            if let Some(signal) = status.signal() {
                eprintln!(
                    "[session {id}] EOF — processo encerrou com código {} ({signal})",
                    status.exit_code()
                );
            } else {
                eprintln!(
                    "[session {id}] EOF — processo encerrou com código {}",
                    status.exit_code()
                );
            }
        }

        if session.closed.load(Ordering::Relaxed) {
            emit_status_with_exit_code(&app, &id, "exited", None, None, exit_code);
            break;
        }
        if session.detached.load(Ordering::Relaxed) {
            eprintln!("[session {id}] detach — tmux preservado no servidor");
            emit_status_with_exit_code(&app, &id, "detached", None, None, exit_code);
            break;
        }
        // `got_output` é o mesmo marco que emite o estado "connected" acima.
        if used_askpass && exit_code == Some(255) && !got_output {
            session.askpass_disabled.store(true, Ordering::Relaxed);
            eprintln!("[session] askpass falhou; caindo para autenticação interativa");
            continue;
        }
        if classify_exit_status(exit_status.as_ref()) == ExitKind::Clean {
            eprintln!("[session {id}] fechamento normal — reconexão não necessária");
            emit_status_with_exit_code(&app, &id, "exited", None, None, exit_code);
            break;
        }
        if !auto_reconnect {
            emit_status_with_exit_code(&app, &id, "exited", None, None, exit_code);
            break;
        }

        // conexão estável zera o contador de tentativas
        if got_output && lived >= Duration::from_secs(MIN_STABLE_SECS) {
            attempt = 0;
        }
        attempt += 1;

        if attempt > MAX_ATTEMPTS {
            eprintln!("[session {id}] {MAX_ATTEMPTS} tentativas falharam — retry em {ERROR_RETRY_SECS}s");
            emit_status_with_exit_code(
                &app,
                &id,
                "error",
                Some(MAX_ATTEMPTS),
                Some(ERROR_RETRY_SECS),
                exit_code,
            );
            interruptible_sleep(&session, ERROR_RETRY_SECS);
            if session.closed.load(Ordering::Relaxed) {
                break;
            }
            attempt = 0; // novo ciclo completo
            continue;
        }

        let delay = (1u64 << (attempt - 1)).min(30);
        if let Some(code) = exit_code {
            eprintln!(
                "[session {id}] reconectando após código {code} — tentativa {attempt}/{MAX_ATTEMPTS} em {delay}s"
            );
        } else {
            eprintln!("[session {id}] reconectando — tentativa {attempt}/{MAX_ATTEMPTS} em {delay}s");
        }
        emit_status_with_exit_code(
            &app,
            &id,
            "reconnecting",
            Some(attempt),
            Some(delay),
            exit_code,
        );
        interruptible_sleep(&session, delay);
    }

    // libera a VPN (refcount--; desconecta se foi o último host a usá-la)
    if let Some(profile) = &vpn_profile {
        if let Some(vpn) = app.try_state::<crate::vpn::Vpn>() {
            crate::vpn::release(&app, &vpn, profile);
        }
    }

    // remove do registro ao terminar de vez — só se ainda for ESTA sessão
    // (um id reutilizado pode ter substituído o Arc no mapa)
    if let Some(sessions) = app.try_state::<Sessions>() {
        let mut map = sessions.0.lock().unwrap();
        if map.get(&id).is_some_and(|s| Arc::ptr_eq(s, &session)) {
            map.remove(&id);
        }
    }
}

/// Credenciais resolvidas no open, guardadas na sessão para reconexão/sudo.
#[derive(Default)]
struct SessionAuth {
    askpass_secret: Option<Zeroizing<String>>,
    sudo_credential_id: Option<String>,
}

fn start_session(
    app: AppHandle,
    sessions: &State<'_, Sessions>,
    id: String,
    host: Option<Host>,
    size: (u16, u16),
    params: Option<SessionParams>,
    auth: SessionAuth,
) {
    let session = Arc::new(Session {
        host_id: host.as_ref().map(|h| h.id.clone()),
        askpass_secret: auth.askpass_secret,
        askpass_disabled: AtomicBool::new(false),
        closed: AtomicBool::new(false),
        detached: AtomicBool::new(false),
        retry_now: AtomicBool::new(false),
        size: Mutex::new(size),
        writer: Mutex::new(None),
        io: Mutex::new(None),
        connected: AtomicBool::new(false),
        attention: AtomicBool::new(false),
        sudo_prompt: AtomicBool::new(false),
        sudo_dismissed: AtomicBool::new(false),
        sudo_credential_id: auth.sudo_credential_id,
        output_tail: Mutex::new(String::new()),
        last_output: Mutex::new(Instant::now()),
        last_input: Mutex::new(Instant::now()),
    });
    {
        // rejeita id duplicado: sobrescrever o Arc deixaria a thread anterior
        // órfã (ssh/PTY vivos) e a limpeza dela apagaria a entrada nova do mapa
        let mut map = sessions.0.lock().unwrap();
        if map.contains_key(&id) {
            eprintln!("[session {id}] já existe — abertura duplicada ignorada");
            return;
        }
        map.insert(id.clone(), session.clone());
    }
    std::thread::spawn(move || manager_loop(app, id, session, host, params));
}

fn resolve_sudo_credential(db: &State<'_, Db>, host: &Host) -> Option<String> {
    let credential_id = host.credential_ref.as_deref()?;
    let eligible = {
        let conn = db.0.lock().unwrap();
        db::has_sudo_password_credential(&conn, credential_id)
    };
    match eligible {
        Ok(true) => Some(credential_id.to_string()),
        Ok(false) => None,
        Err(e) => {
            eprintln!("[session] falha ao consultar credencial sudo ({e})");
            None
        }
    }
}

fn resolve_ssh_password(
    db: &State<'_, Db>,
    vault: &State<'_, Vault>,
    host: &Host,
) -> Option<Zeroizing<String>> {
    let credential_id = host.credential_ref.as_deref()?;
    let eligible = {
        let conn = db.0.lock().unwrap();
        db::has_ssh_password_credential(&conn, credential_id)
    };
    match eligible {
        Ok(true) => {}
        // credencial só de sudo (sem escopo ssh) é configuração normal — sem aviso
        Ok(false) => return None,
        Err(e) => {
            eprintln!("[session] falha ao consultar credencial SSH ({e}); usando autenticação interativa");
            return None;
        }
    }

    match vault::get_secret(db, vault, credential_id) {
        Ok(secret) => Some(Zeroizing::new(secret)),
        Err(_) => {
            eprintln!(
                "[session] cofre ou segredo SSH indisponível; usando autenticação interativa"
            );
            None
        }
    }
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // comando Tauri: injeções de State + params
pub fn open_ssh_session(
    app: AppHandle,
    sessions: State<'_, Sessions>,
    db: State<'_, Db>,
    vault: State<'_, Vault>,
    id: String,
    host_id: String,
    cols: u16,
    rows: u16,
    params: Option<SessionParams>,
) -> Result<(), String> {
    let host = db::get_host(&db, &host_id)?;
    if params.as_ref().is_some_and(|params| params.mode == "clmux") {
        agent_binary(params.as_ref().and_then(|params| params.agent.as_deref()))?;
    }
    let auth = SessionAuth {
        askpass_secret: resolve_ssh_password(&db, &vault, &host),
        sudo_credential_id: resolve_sudo_credential(&db, &host),
    };
    start_session(app, &sessions, id, Some(host), (cols, rows), params, auth);
    Ok(())
}

#[tauri::command]
pub fn authorize_sudo(
    app: AppHandle,
    sessions: State<'_, Sessions>,
    db: State<'_, Db>,
    vault: State<'_, Vault>,
    id: String,
) -> Result<(), String> {
    let session = get(&sessions, &id)?;
    if !session.sudo_prompt.load(Ordering::Relaxed) {
        return Err("a sessão não está aguardando uma senha de sudo".into());
    }
    let credential_id = session
        .sudo_credential_id
        .as_deref()
        .ok_or("a sessão não tem credencial sudo elegível")?;

    // Busca o segredo ANTES de segurar o lock da cauda: o I/O do keychain
    // (securityd) pode demorar e não deve bloquear a thread leitora do PTY.
    let secret = Zeroizing::new(vault::get_secret(&db, &vault, credential_id)?);

    // Mantém a cauda estável entre a revalidação e a injeção. Assim, output
    // concorrente não pode transformar o clique em autorização de um prompt
    // já obsoleto.
    let mut tail = session.output_tail.lock().unwrap();
    if !session.sudo_prompt.load(Ordering::Relaxed) || !is_active_sudo_prompt(&tail) {
        return Err("o prompt de sudo não está mais ativo".into());
    }
    let mut writer = session.writer.lock().unwrap();
    let writer = writer.as_mut().ok_or("sessão sem PTY ativo")?;
    writer.write_all(secret.as_bytes()).map_err(|e| e.to_string())?;
    writer.write_all(b"\n").map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())?;
    tail.clear();
    session.sudo_dismissed.store(false, Ordering::Relaxed);
    set_sudo_prompt(&app, &session, &id, false, "");
    drop(tail);

    *session.last_input.lock().unwrap() = Instant::now();
    set_attention(&app, &session, &id, false, None);
    Ok(())
}

#[tauri::command]
pub fn dismiss_sudo_prompt(
    app: AppHandle,
    sessions: State<'_, Sessions>,
    id: String,
) -> Result<(), String> {
    let session = get(&sessions, &id)?;
    let tail = session.output_tail.lock().unwrap();
    session
        .sudo_dismissed
        .store(is_active_sudo_prompt(&tail), Ordering::Relaxed);
    set_sudo_prompt(&app, &session, &id, false, "");
    Ok(())
}

#[tauri::command]
pub fn write_stdin(
    app: AppHandle,
    sessions: State<'_, Sessions>,
    id: String,
    data: String,
) -> Result<(), String> {
    let session = get(&sessions, &id)?;
    // tecla do usuário → registra input e limpa atenção
    *session.last_input.lock().unwrap() = Instant::now();
    set_attention(&app, &session, &id, false, None);
    // O lock de `io` também protege killer/master; uma escrita bloqueante não
    // pode impedir close_session de alcançar o killer. O mutex exclusivo do
    // writer mantém write_all + flush serializados e, portanto, em ordem.
    let mut writer = session.writer.lock().unwrap();
    let writer = writer.as_mut().ok_or("sessão sem PTY ativo")?;
    writer.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())
}

const ATTENTION_SETTLE: Duration = Duration::from_secs(2);
const ATTENTION_IDLE: Duration = Duration::from_secs(5);

/// Thread que detecta sessões aguardando input: prompt interativo na cauda do
/// output + output parado + sem tecla do usuário por alguns segundos.
pub fn spawn_attention_monitor(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        let Some(sessions) = app.try_state::<Sessions>() else { continue };
        let snapshot: Vec<(String, Arc<Session>)> = {
            let map = sessions.0.lock().unwrap();
            map.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
        };
        for (id, session) in snapshot {
            if !session.connected.load(Ordering::Relaxed)
                || session.attention.load(Ordering::Relaxed)
            {
                continue;
            }
            let settled = session.last_output.lock().unwrap().elapsed() >= ATTENTION_SETTLE;
            let idle = session.last_input.lock().unwrap().elapsed() >= ATTENTION_IDLE;
            if settled && idle {
                let tail = session.output_tail.lock().unwrap().clone();
                if looks_like_prompt(&tail) {
                    set_attention(&app, &session, &id, true, Some("aguardando input"));
                }
            }
        }
    });
}

#[tauri::command]
pub fn resize_pty(
    sessions: State<'_, Sessions>,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let session = get(&sessions, &id)?;
    *session.size.lock().unwrap() = (cols, rows);
    if let Some(io) = session.io.lock().unwrap().as_ref() {
        io.master
            .resize(portable_pty::PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn close_session(sessions: State<'_, Sessions>, id: String) -> Result<(), String> {
    let session = {
        let map = sessions.0.lock().unwrap();
        map.get(&id).cloned()
    };
    if let Some(session) = session {
        close_session_inner(&session);
    }
    Ok(())
}

fn kill_child(io: &mut Io) {
    let _ = io.killer.kill();
}

fn close_session_inner(session: &Session) {
    session.closed.store(true, Ordering::Relaxed);
    if let Some(io) = session.io.lock().unwrap().as_mut() {
        kill_child(io);
    }
    session.writer.lock().unwrap().take();
}

/// Fecha todas as sessões sem permitir que um writer bloqueado segure o exit.
/// As threads de sessão removem suas próprias entradas do mapa ao terminarem.
pub fn shutdown_sessions(sessions: &Sessions, timeout: Duration) {
    let snapshot: Vec<Arc<Session>> = {
        let map = sessions.0.lock().unwrap();
        map.values().cloned().collect()
    };
    for session in &snapshot {
        session.closed.store(true, Ordering::Relaxed);
    }

    let deadline = Instant::now() + timeout;
    loop {
        for session in &snapshot {
            match session.io.try_lock() {
                Ok(mut io) => {
                    if let Some(io) = io.as_mut() {
                        kill_child(io);
                    }
                }
                Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                    if let Some(io) = poisoned.into_inner().as_mut() {
                        kill_child(io);
                    }
                }
                Err(std::sync::TryLockError::WouldBlock) => {}
            }
            match session.writer.try_lock() {
                Ok(mut writer) => {
                    writer.take();
                }
                Err(std::sync::TryLockError::Poisoned(poisoned)) => {
                    poisoned.into_inner().take();
                }
                Err(std::sync::TryLockError::WouldBlock) => {}
            }
        }

        let any_registered = {
            let map = sessions.0.lock().unwrap();
            snapshot
                .iter()
                .any(|session| map.values().any(|current| Arc::ptr_eq(current, session)))
        };
        let now = Instant::now();
        if !any_registered || now >= deadline {
            break;
        }
        std::thread::sleep(
            Duration::from_millis(20).min(deadline.saturating_duration_since(now)),
        );
    }
}

/// Detach do tmux: injeta prefixo Ctrl+B + d — o comando remoto termina,
/// o ssh sai limpo e a sessão tmux continua viva no servidor.
#[tauri::command]
pub fn detach_session(sessions: State<'_, Sessions>, id: String) -> Result<(), String> {
    let session = get(&sessions, &id)?;
    session.detached.store(true, Ordering::Relaxed);
    let mut writer = session.writer.lock().unwrap();
    let result = match writer.as_mut() {
        Some(writer) => writer
            .write_all(b"\x02d")
            .and_then(|_| writer.flush())
            .map_err(|e| e.to_string()),
        None => Err("sessão sem PTY ativo".to_string()),
    };
    if result.is_err() {
        session.detached.store(false, Ordering::Relaxed);
    }
    result
}

/// Interrompe a espera do ciclo de erro e tenta reconectar já.
#[tauri::command]
pub fn retry_session(sessions: State<'_, Sessions>, id: String) -> Result<(), String> {
    let session = get(&sessions, &id)?;
    session.retry_now.store(true, Ordering::Relaxed);
    Ok(())
}

fn get(sessions: &State<'_, Sessions>, id: &str) -> Result<Arc<Session>, String> {
    sessions
        .0
        .lock()
        .unwrap()
        .get(id)
        .cloned()
        .ok_or_else(|| format!("sessão desconhecida: {id}"))
}

#[cfg(test)]
mod tests {
    use super::{
        agent_binary, classify_exit_status, is_active_sudo_prompt, looks_like_prompt, strip_ansi,
        sudo_prompt_context, ExitKind,
    };
    use portable_pty::ExitStatus;

    #[test]
    fn strip_ansi_preserva_multibyte() {
        // CSI colorindo um prompt com `❯` (multibyte): a cor some, o char fica
        let s = "\x1b[38;2;224;161;94m❯\x1b[0m ";
        assert_eq!(strip_ansi(s), "❯ ");
    }

    #[test]
    fn strip_ansi_remove_osc_e_acentos_sobrevivem() {
        // OSC (título de janela) terminado por BEL + texto acentuado
        let s = "\x1b]0;título\x07são cinco?";
        assert_eq!(strip_ansi(s), "são cinco?");
    }

    #[test]
    fn prompt_needle_chevron_casa() {
        // regressão: `❯` era mangleado p/ Latin-1 e a needle nunca casava
        assert!(looks_like_prompt("\x1b[32m❯\x1b[0m"));
        assert!(looks_like_prompt("Overwrite? "));
        assert!(!looks_like_prompt("user@host:~$ "));
    }

    #[test]
    fn reconhece_prompts_de_permissao_dos_agentes_sem_estados_transitorios() {
        let claude = "Bash command\n  rm -rf build\n\n❯ 1. Yes\n  2. Yes, and don't ask again\n  3. No";
        let codex = "Run this command?\n\n  1. Allow once\n  2. Always allow for this session\n  3. Cancel";

        assert!(looks_like_prompt(claude));
        assert!(looks_like_prompt(codex));
        assert!(looks_like_prompt("Claude Code permission\n  1. Yes\n  2. No"));
        assert!(!looks_like_prompt("Working (12s · esc to interrupt)"));
    }

    #[test]
    fn reconhece_apenas_prompt_sudo_ativo_no_fim_da_cauda() {
        assert!(is_active_sudo_prompt("[sudo] password for deploy:"));
        assert!(is_active_sudo_prompt("output\n[sudo] password for deploy:   "));
        assert!(!is_active_sudo_prompt(
            "[sudo] password for deploy:\nserviço reiniciado"
        ));
        assert!(!is_active_sudo_prompt("Password:"));
        assert!(!is_active_sudo_prompt(" [sudo] password for deploy:"));
    }

    #[test]
    fn prompt_sudo_consumido_nao_casa_novamente() {
        let mut tail = String::from("sudo systemctl restart api\n[sudo] password for deploy:");
        assert!(is_active_sudo_prompt(&tail));
        tail.clear();
        assert!(!is_active_sudo_prompt(&tail));
        tail.push_str("\r\n");
        assert!(!is_active_sudo_prompt(&tail));
    }

    #[test]
    fn contexto_sudo_usa_as_tres_ultimas_linhas_limpas() {
        let tail = "ignorada\n\x1b[32msudo systemctl restart api\x1b[0m\n\nfalha anterior\n[sudo] password for deploy:";
        assert_eq!(
            sudo_prompt_context(tail),
            "sudo systemctl restart api\nfalha anterior\n[sudo] password for deploy:"
        );
    }

    #[test]
    fn agente_usa_allowlist_estrita() {
        assert_eq!(agent_binary(None).unwrap(), "claude");
        assert_eq!(agent_binary(Some("claude")).unwrap(), "claude");
        assert_eq!(agent_binary(Some("codex")).unwrap(), "codex");
        assert!(agent_binary(Some("codex; touch /tmp/pwned")).is_err());
    }

    #[test]
    fn classifica_exit_status_para_reconexao() {
        let success = ExitStatus::with_exit_code(0);
        let remote_failure = ExitStatus::with_exit_code(1);
        let ssh_failure = ExitStatus::with_exit_code(255);
        let killed = ExitStatus::with_signal("Killed");

        assert_eq!(classify_exit_status(Some(&success)), ExitKind::Clean);
        assert_eq!(classify_exit_status(Some(&remote_failure)), ExitKind::Clean);
        assert_eq!(classify_exit_status(Some(&ssh_failure)), ExitKind::Failure);
        assert_eq!(classify_exit_status(Some(&killed)), ExitKind::Failure);
        assert_eq!(classify_exit_status(None), ExitKind::Unknown);
    }
}
