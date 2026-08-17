//! Ciclo de vida das sessões: spawn, reconexão com backoff, detach.
//!
//! Cada sessão tem id estável e um loop próprio em thread dedicada.
//! Queda inesperada → backoff exponencial 1–30s, 5 tentativas → estado de
//! erro com novo ciclo a cada 60s (interrompível por retry_session).

use std::collections::HashMap;
use std::io::{Read, Write};
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
/// viveu menos que isto com output = tentativa falhada, não conexão real
const MIN_STABLE_SECS: u64 = 30;
const EXIT_STATUS_WAIT: Duration = Duration::from_secs(1);

pub struct Session {
    #[allow(dead_code)] // usado nas fases de latência
    host_id: Option<String>,
    askpass_secret: Option<Zeroizing<String>>,
    closed: AtomicBool,
    detached: AtomicBool,
    retry_now: AtomicBool,
    size: Mutex<(u16, u16)>,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    io: Mutex<Option<Io>>,
    // detecção de atenção (Fase 8)
    connected: AtomicBool,
    attention: AtomicBool,
    output_tail: Mutex<String>,
    last_output: Mutex<Instant>,
    last_input: Mutex<Instant>,
}

struct Io {
    master: Box<dyn portable_pty::MasterPty + Send>,
    killer: Box<dyn portable_pty::ChildKiller + Send + Sync>,
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

fn set_attention(app: &AppHandle, session: &Session, id: &str, active: bool, reason: Option<&str>) {
    let was = session.attention.swap(active, Ordering::Relaxed);
    if was != active {
        let _ = app.emit("attention", AttentionPayload { id, active, reason });
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
        "aguardando", "waiting for input",
    ];
    NEEDLES.iter().any(|n| low.contains(n))
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

fn classify_exit_code(exit_code: Option<u32>) -> ExitKind {
    // O ssh reserva 255 para falhas próprias (conexão/autenticação). Qualquer
    // outro status veio do comando/shell remoto e representa encerramento.
    match exit_code {
        Some(255) => ExitKind::Failure,
        Some(_) => ExitKind::Clean,
        None => ExitKind::Unknown,
    }
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
}

/// Comando remoto conforme o modo. clmux: attach se a sessão existir; senão
/// cria e digita `claude` nela (send-keys) — "abre o Claude dentro do tmux".
fn remote_command(mode: &str, name: &str, dir: Option<&str>) -> Option<String> {
    let name_q = shell_quote(name);
    let cd = dir
        .filter(|d| !d.is_empty())
        .map(|d| format!("cd {} && ", shell_quote(d)))
        .unwrap_or_default();
    match mode {
        "tmux" => Some(format!("{cd}tmux new -As {name_q}")),
        "clmux" => Some(format!(
            "{cd}if tmux has-session -t {name_q} 2>/dev/null; then tmux attach -t {name_q}; \
             else tmux new -s {name_q} \\; send-keys claude Enter; fi"
        )),
        // shell na pasta do projeto: cd + shell de login interativo
        "shell" if !cd.is_empty() => Some(format!("{cd}exec \"${{SHELL:-bash}}\" -l")),
        _ => None,
    }
}

fn build_command(
    host: Option<&Host>,
    params: Option<&SessionParams>,
    askpass_secret: Option<&str>,
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
            cmd.arg("-tt");
            cmd.arg("-o");
            cmd.arg("ConnectTimeout=10");
            cmd.arg("-o");
            cmd.arg("ServerAliveInterval=15");
            cmd.arg("-o");
            cmd.arg("ServerAliveCountMax=3");
            if let Some(secret) = askpass_secret {
                match std::env::current_exe() {
                    Ok(helper) => {
                        cmd.env("SSH_ASKPASS", helper);
                        cmd.env("SSH_ASKPASS_REQUIRE", "force");
                        cmd.env("HELM_ASKPASS_MODE", "1");
                        cmd.env("HELM_ASKPASS_SECRET", secret);
                        cmd.arg("-o");
                        cmd.arg("NumberOfPasswordPrompts=1");
                        cmd.arg("-o");
                        cmd.arg("StrictHostKeyChecking=accept-new");
                    }
                    Err(_) => eprintln!(
                        "[session] helper SSH_ASKPASS indisponível; usando autenticação interativa"
                    ),
                }
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

            if let Some(remote) = remote_command(&mode, &name, dir.as_deref()) {
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
        let askpass_secret = session
            .askpass_secret
            .as_ref()
            .map(|secret| secret.as_str());
        let cmd = match build_command(host.as_ref(), params.as_ref(), askpass_secret) {
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
        if classify_exit_code(exit_code) == ExitKind::Clean {
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

fn start_session(
    app: AppHandle,
    sessions: &State<'_, Sessions>,
    id: String,
    host: Option<Host>,
    size: (u16, u16),
    params: Option<SessionParams>,
    askpass_secret: Option<Zeroizing<String>>,
) {
    let session = Arc::new(Session {
        host_id: host.as_ref().map(|h| h.id.clone()),
        askpass_secret,
        closed: AtomicBool::new(false),
        detached: AtomicBool::new(false),
        retry_now: AtomicBool::new(false),
        size: Mutex::new(size),
        writer: Mutex::new(None),
        io: Mutex::new(None),
        connected: AtomicBool::new(false),
        attention: AtomicBool::new(false),
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
    let askpass_secret = resolve_ssh_password(&db, &vault, &host);
    start_session(
        app,
        &sessions,
        id,
        Some(host),
        (cols, rows),
        params,
        askpass_secret,
    );
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
    use super::{classify_exit_code, looks_like_prompt, strip_ansi, ExitKind};

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
    fn classifica_exit_code_para_reconexao() {
        assert_eq!(classify_exit_code(Some(0)), ExitKind::Clean);
        assert_eq!(classify_exit_code(Some(1)), ExitKind::Clean);
        assert_eq!(classify_exit_code(Some(255)), ExitKind::Failure);
        assert_eq!(classify_exit_code(None), ExitKind::Unknown);
    }
}
