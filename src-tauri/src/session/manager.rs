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
const SUDO_AUTHORIZE_COOLDOWN: Duration = Duration::from_secs(3);

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
    sudo_label_mismatch_warned: AtomicBool,
    sudo_credential_unmatched: AtomicBool,
    sudo_credential_id: Mutex<Option<String>>,
    output_tail: Mutex<Tail>,
    last_output: Mutex<Instant>,
    last_input: Mutex<Instant>,
}

struct Io {
    master: Box<dyn portable_pty::MasterPty + Send>,
    killer: Box<dyn portable_pty::ChildKiller + Send + Sync>,
}

#[derive(Default)]
struct Tail {
    buf: String,
    start_offset: u64,
    consumed_before: u64,
    dismissed: Option<(u64, String)>,
    active: Option<ActiveSudoPrompt>,
    consumed_line: Option<String>,
    last_authorized_at: Option<Instant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveSudoPrompt {
    offset: u64,
    normalized_line: String,
}

impl Tail {
    fn stream_len(&self) -> u64 {
        self.start_offset + self.buf.len() as u64
    }

    fn push(&mut self, text: &str) {
        self.buf.push_str(text);
        let len = self.buf.len();
        if len <= 400 {
            return;
        }

        let normal_cut = len - 400;
        let protected_cut = self
            .active
            .as_ref()
            .and_then(|prompt| prompt.offset.checked_sub(self.start_offset))
            .filter(|offset| *offset <= len as u64)
            .map_or(normal_cut, |offset| normal_cut.min(offset as usize));
        // Enquanto um prompt está publicado, conserva desde o início de seu
        // segmento. O teto absoluto evita crescimento sem limite se um TUI
        // continuar redesenhando sem CR/LF.
        let mut cut = protected_cut.max(len.saturating_sub(4096));
        // Corta em fronteira de char: o índice pode cair no meio de um
        // multibyte e panicar (envenenaria o lock).
        while cut < len && !self.buf.is_char_boundary(cut) {
            cut += 1;
        }
        self.buf.drain(..cut);
        self.start_offset += cut as u64;
    }

    fn reset(&mut self) {
        self.buf.clear();
        self.start_offset = 0;
        self.consumed_before = 0;
        self.dismissed = None;
        self.active = None;
        self.consumed_line = None;
        self.last_authorized_at = None;
    }
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
    credential: &'a str,
    prompt_token: Option<u64>,
}

fn set_attention(app: &AppHandle, session: &Session, id: &str, active: bool, reason: Option<&str>) {
    let was = session.attention.swap(active, Ordering::Relaxed);
    if was != active {
        let _ = app.emit("attention", AttentionPayload { id, active, reason });
    }
}

fn emit_sudo_prompt(
    app: &AppHandle,
    id: &str,
    active: bool,
    context: String,
    credential: &'static str,
    prompt_token: Option<u64>,
) {
    let _ = app.emit(
        "sudo-prompt",
        SudoPromptPayload {
            id,
            active,
            context,
            credential,
            prompt_token,
        },
    );
}

fn clear_sudo_prompt(app: &AppHandle, id: &str, tail: &mut Tail) {
    if tail.active.take().is_some() {
        emit_sudo_prompt(app, id, false, String::new(), "none", None);
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
    if NEEDLES.iter().any(|n| low.contains(n)) {
        return true;
    }

    const MENU_NEEDLES: &[&str] = &["always allow", "allow once", "1. yes"];
    clean
        .split(['\r', '\n'])
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .rev()
        .take(3)
        .any(|line| {
            let line = line.to_lowercase();
            MENU_NEEDLES.iter().any(|needle| line.contains(needle))
        })
}

const SUDO_PROMPT_PREFIXES: &[&str] = &[
    "[sudo] password for ",
    "[sudo] senha para ",
    "[sudo] contraseña para ",
    "[sudo] mot de passe de ",
];

fn sudo_prompt_bounds(line: &str) -> Option<(usize, usize)> {
    let (prompt_start, rest_start) = line
        .char_indices()
        .filter_map(|(start, _)| {
            SUDO_PROMPT_PREFIXES.iter().find_map(|prefix| {
                let mut original = line[start..].char_indices();
                for expected in prefix.chars() {
                    let (_, actual) = original.next()?;
                    if !actual.to_lowercase().eq(expected.to_lowercase()) {
                        return None;
                    }
                }
                let end = original.next().map_or(line.len(), |(offset, _)| start + offset);
                Some((start, end))
            })
        })
        .max_by_key(|(start, _)| *start)?;
    let rest = &line[rest_start..];
    let colon = rest.find(':')?;
    (!rest[..colon].trim().is_empty()).then_some((prompt_start, rest_start + colon + 1))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SudoPromptMatch {
    /// Início do último segmento no buffer cru. O offset do prompt usa
    /// exatamente este ponto, sem tentar remapear índices do texto limpo.
    start: usize,
    offset: u64,
    line: String,
}

fn active_sudo_prompt(tail: &str, start_offset: u64) -> Option<SudoPromptMatch> {
    let segment_start = tail.rfind(['\r', '\n']).map_or(0, |separator| separator + 1);
    let clean = strip_ansi(&tail[segment_start..]);
    let (prompt_start, prompt_end) = sudo_prompt_bounds(&clean)?;
    Some(SudoPromptMatch {
        start: segment_start,
        offset: start_offset + segment_start as u64,
        line: clean[prompt_start..prompt_end].trim().to_string(),
    })
}

/// Prompt ativo do sudo padrão. CR também separa segmentos porque aplicações
/// de terminal podem redesenhar sem LF. O prompt precisa estar no último
/// segmento; texto colado depois de `:` por CUP (status do tmux) é
/// tolerado, mas qualquer output em uma linha/segmento posterior o invalida.
/// Contexto curto e sem escapes para o usuário conferir de onde veio o
/// prompt. Mantém o fim para nunca cortar justamente a linha do sudo.
fn sudo_prompt_context(tail: &str, prompt: &SudoPromptMatch) -> String {
    const MAX_CONTEXT_CHARS: usize = 200;

    let preceding_clean = strip_ansi(&tail[..prompt.start]);
    let lines: Vec<&str> = preceding_clean
        .split(['\r', '\n'])
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let mut context_lines = lines[lines.len().saturating_sub(2)..].to_vec();
    context_lines.push(prompt.line.trim());
    let context = context_lines.join("\n");
    let char_count = context.chars().count();
    if char_count <= MAX_CONTEXT_CHARS {
        context
    } else {
        let keep = MAX_CONTEXT_CHARS - 1;
        format!("…{}", context.chars().skip(char_count - keep).collect::<String>())
    }
}

fn normalized_sudo_prompt_line(prompt: &SudoPromptMatch) -> String {
    prompt.line.trim().to_lowercase()
}

fn sudo_prompt_identity(prompt: &SudoPromptMatch) -> ActiveSudoPrompt {
    ActiveSudoPrompt {
        offset: prompt.offset,
        normalized_line: normalized_sudo_prompt_line(prompt),
    }
}

fn should_emit_sudo_prompt(active: Option<&ActiveSudoPrompt>, prompt: &SudoPromptMatch) -> bool {
    active != Some(&sudo_prompt_identity(prompt))
}

fn authorization_matches_prompt(
    tail: &Tail,
    prompt: &SudoPromptMatch,
    prompt_token: u64,
    expected: &ActiveSudoPrompt,
) -> bool {
    prompt.offset == prompt_token
        && sudo_prompt_identity(prompt) == *expected
        && tail.active.as_ref() == Some(expected)
}

/// Decodifica apenas o texto usado pela heurística da cauda, conservando uma
/// sequência UTF-8 válida porém incompleta até o próximo read. Bytes realmente
/// inválidos seguem a semântica lossy; os bytes crus emitidos ao front não
/// passam por esta função.
fn decode_utf8_for_tail(pending: &mut Vec<u8>, chunk: &[u8]) -> String {
    let mut bytes = std::mem::take(pending);
    bytes.extend_from_slice(chunk);
    let mut remaining = bytes.as_slice();
    let mut decoded = String::with_capacity(bytes.len());

    loop {
        match std::str::from_utf8(remaining) {
            Ok(valid) => {
                decoded.push_str(valid);
                break;
            }
            Err(error) => {
                let valid_up_to = error.valid_up_to();
                decoded.push_str(
                    std::str::from_utf8(&remaining[..valid_up_to])
                        .expect("valid_up_to sempre termina em UTF-8 válido"),
                );
                match error.error_len() {
                    Some(invalid_len) => {
                        let invalid_end = valid_up_to + invalid_len;
                        decoded.push_str(&String::from_utf8_lossy(
                            &remaining[valid_up_to..invalid_end],
                        ));
                        remaining = &remaining[invalid_end..];
                    }
                    None => {
                        pending.extend_from_slice(&remaining[valid_up_to..]);
                        break;
                    }
                }
            }
        }
    }

    decoded
}

const SUDO_RETRY_MARKERS: &[&str] = &[
    "sorry, try again",
    "desculpe, tente novamente",
    "lo siento, vuelva a intentarlo",
    "lo siento, inténtelo de nuevo",
    "désolé, réessayez",
    "essayez de nouveau",
];

fn absolute_tail_slice(tail: &Tail, start_offset: u64, end_offset: u64) -> &str {
    let mut start = start_offset
        .saturating_sub(tail.start_offset)
        .min(tail.buf.len() as u64) as usize;
    let mut end = end_offset
        .saturating_sub(tail.start_offset)
        .min(tail.buf.len() as u64) as usize;
    while start < tail.buf.len() && !tail.buf.is_char_boundary(start) {
        start += 1;
    }
    while end > start && !tail.buf.is_char_boundary(end) {
        end -= 1;
    }
    &tail.buf[start..end]
}

fn has_sudo_retry_between(tail: &Tail, start_offset: u64, prompt_offset: u64) -> bool {
    let text = strip_ansi(absolute_tail_slice(tail, start_offset, prompt_offset)).to_lowercase();
    SUDO_RETRY_MARKERS.iter().any(|marker| text.contains(marker))
}

impl Tail {
    fn active_sudo_prompt(&self) -> Option<SudoPromptMatch> {
        active_sudo_prompt(&self.buf, self.start_offset)
    }

    fn visible_sudo_prompt(&self) -> Option<SudoPromptMatch> {
        let prompt = self.active_sudo_prompt()?;
        if prompt.offset < self.consumed_before {
            return None;
        }

        let normalized = normalized_sudo_prompt_line(&prompt);
        if self.consumed_line.as_deref() == Some(normalized.as_str())
            && !has_sudo_retry_between(self, self.consumed_before, prompt.offset)
        {
            return None;
        }
        if let Some((dismissed_offset, dismissed_line)) = &self.dismissed {
            if prompt.offset == *dismissed_offset
                || (dismissed_line == &normalized
                    && !has_sudo_retry_between(self, *dismissed_offset, prompt.offset))
            {
                return None;
            }
        }
        Some(prompt)
    }
}

fn tail_after_consumed(tail: &Tail, consumed_before: u64) -> &str {
    let mut start = consumed_before
        .saturating_sub(tail.start_offset)
        .min(tail.buf.len() as u64) as usize;
    while start < tail.buf.len() && !tail.buf.is_char_boundary(start) {
        start += 1;
    }
    &tail.buf[start..]
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

fn reset_sudo_tracking(app: &AppHandle, session: &Session, id: &str) {
    let mut tail = session.output_tail.lock().unwrap();
    clear_sudo_prompt(app, id, &mut tail);
    tail.reset();
}

fn sudo_credential_availability(session: &Session) -> &'static str {
    if session.sudo_credential_id.lock().unwrap().is_some() {
        "ok"
    } else if session.sudo_credential_unmatched.load(Ordering::Relaxed) {
        "unmatched"
    } else {
        "none"
    }
}

fn ensure_sudo_credential(app: &AppHandle, session: &Session) -> &'static str {
    if session.sudo_credential_id.lock().unwrap().is_some() {
        return "ok";
    }
    let (Some(host_id), Some(db)) = (session.host_id.as_deref(), app.try_state::<Db>()) else {
        return sudo_credential_availability(session);
    };
    let (host, candidates) = match db::get_host_and_sudo_password_credentials(&db, host_id) {
        Ok(result) => result,
        Err(e) => {
            eprintln!("[session] falha ao reler host para credencial sudo ({e})");
            return sudo_credential_availability(session);
        }
    };
    let (credential_id, unmatched_exists) =
        db::resolve_sudo_password_credential(&host, &candidates);
    let Some(credential_id) = credential_id else {
        session
            .sudo_credential_unmatched
            .store(unmatched_exists, Ordering::Relaxed);
        if unmatched_exists
            && !session
                .sudo_label_mismatch_warned
                .swap(true, Ordering::Relaxed)
        {
            eprintln!(
                "[session] credencial sudo existe mas o label não casa com host/name/user@host — renomeie para 'sudo · user@<host>'"
            );
        }
        return if unmatched_exists { "unmatched" } else { "none" };
    };
    let mut current = session.sudo_credential_id.lock().unwrap();
    if current.is_none() {
        *current = Some(credential_id);
    }
    session
        .sudo_credential_unmatched
        .store(false, Ordering::Relaxed);
    "ok"
}

fn update_sudo_prompt_for_output(
    app: &AppHandle,
    session: &Session,
    id: &str,
    text: &str,
    credential_checked_offset: &mut Option<u64>,
) {
    let detected_prompt = {
        let mut tail = session.output_tail.lock().unwrap();
        tail.push(text);
        tail.visible_sudo_prompt()
            .map(|prompt| sudo_prompt_identity(&prompt))
    };

    let credential = if let Some(prompt) = &detected_prompt {
        if session.sudo_credential_id.lock().unwrap().is_none()
            && *credential_checked_offset != Some(prompt.offset)
        {
            *credential_checked_offset = Some(prompt.offset);
            // SELECTs e qualquer outra resolução acontecem sem o lock da
            // cauda. Ao voltar, offset e linha são revalidados antes da publicação.
            ensure_sudo_credential(app, session)
        } else {
            sudo_credential_availability(session)
        }
    } else {
        "none"
    };

    let mut tail = session.output_tail.lock().unwrap();
    let visible = tail
        .visible_sudo_prompt()
        .filter(|prompt| Some(&sudo_prompt_identity(prompt)) == detected_prompt.as_ref());
    match (visible, credential) {
        (Some(prompt), "ok" | "unmatched")
            if should_emit_sudo_prompt(tail.active.as_ref(), &prompt) =>
        {
            let context = sudo_prompt_context(&tail.buf, &prompt);
            let identity = sudo_prompt_identity(&prompt);
            let prompt_token = identity.offset;
            tail.active = Some(identity);
            emit_sudo_prompt(app, id, true, context, credential, Some(prompt_token));
        }
        (Some(_), "ok" | "unmatched") => {}
        _ => clear_sudo_prompt(app, id, &mut tail),
    }
}

fn configured_default_agent(db: &State<'_, Db>) -> &'static str {
    match db::get_pref(db.clone(), "ui.defaultAgent".into()) {
        Ok(Some(value)) => {
            let agent = agent_binary(Some(&value)).unwrap_or("claude");
            if agent != value.as_str() {
                eprintln!("[session] agente padrão inválido {value:?}; usando 'claude'");
            }
            agent
        }
        Ok(_) => "claude",
        Err(e) => {
            eprintln!("[session] falha ao ler agente padrão ({e}); usando 'claude'");
            "claude"
        }
    }
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

        // Cada PTY começa sem qualquer evidência ou estado de sudo herdado da
        // tentativa anterior, inclusive quando o spawn anterior nem chegou a ler.
        reset_sudo_tracking(&app, &session, &id);

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
                let mut sudo_credential_checked_offset = None;
                let mut utf8_tail_pending = Vec::new();
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
                            let tail_text = decode_utf8_for_tail(&mut utf8_tail_pending, &buf[..n]);
                            if !tail_text.is_empty() {
                                update_sudo_prompt_for_output(
                                    &app,
                                    &session,
                                    &id,
                                    &tail_text,
                                    &mut sudo_credential_checked_offset,
                                );
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
                // EOF invalida a cauda antes de qualquer espera/reconexão.
                utf8_tail_pending.clear();
                reset_sudo_tracking(&app, &session, &id);
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

/// Credenciais resolvidas no open e guardadas na sessão; sudo também pode ser
/// resolvido de novo, de forma lazy, quando um prompt aparecer.
#[derive(Default)]
struct SessionAuth {
    askpass_secret: Option<Zeroizing<String>>,
    sudo_credential_id: Option<String>,
    sudo_credential_unmatched: bool,
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
        sudo_label_mismatch_warned: AtomicBool::new(false),
        sudo_credential_unmatched: AtomicBool::new(auth.sudo_credential_unmatched),
        sudo_credential_id: Mutex::new(auth.sudo_credential_id),
        output_tail: Mutex::new(Tail::default()),
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
    let (host, sudo_candidates) =
        db::get_host_and_sudo_password_credentials(&db, &host_id)?;
    let mut params = params.unwrap_or_else(|| SessionParams {
        mode: if host.auto_attach && host.startup_mode == "shell" {
            "tmux".into()
        } else {
            host.startup_mode.clone()
        },
        session_name: None,
        project_dir: None,
        agent: None,
    });
    if params.mode == "clmux" {
        let agent = match params.agent.as_deref() {
            Some(agent) => agent_binary(Some(agent))?,
            None => configured_default_agent(&db),
        };
        params.agent = Some(agent.to_string());
    }
    let (sudo_credential_id, sudo_credential_unmatched) =
        db::resolve_sudo_password_credential(&host, &sudo_candidates);
    let auth = SessionAuth {
        askpass_secret: resolve_ssh_password(&db, &vault, &host),
        sudo_credential_id,
        sudo_credential_unmatched,
    };
    start_session(app, &sessions, id, Some(host), (cols, rows), Some(params), auth);
    Ok(())
}

#[tauri::command]
pub fn authorize_sudo(
    app: AppHandle,
    sessions: State<'_, Sessions>,
    db: State<'_, Db>,
    vault: State<'_, Vault>,
    id: String,
    prompt_token: u64,
) -> Result<(), String> {
    let session = get(&sessions, &id)?;
    let authorization_prompt = {
        let mut tail = session.output_tail.lock().unwrap();
        ensure_sudo_authorize_cooldown(&tail)?;
        let Some(prompt) = tail.visible_sudo_prompt() else {
            clear_sudo_prompt(&app, &id, &mut tail);
            return Err("PROMPT_CHANGED: o prompt de sudo mudou".into());
        };
        let identity = sudo_prompt_identity(&prompt);
        if !authorization_matches_prompt(&tail, &prompt, prompt_token, &identity) {
            return Err("PROMPT_CHANGED: o prompt de sudo mudou".into());
        }
        identity
    };
    let credential_id = session
        .sudo_credential_id
        .lock()
        .unwrap()
        .clone()
        .ok_or("a sessão não tem credencial sudo elegível")?;

    // Busca o segredo ANTES de segurar o lock da cauda: o I/O do keychain
    // (securityd) pode demorar e não deve bloquear a thread leitora do PTY.
    let secret = Zeroizing::new(vault::get_secret(&db, &vault, &credential_id)?);

    // A revalidação final e a reserva são atômicas sob a cauda. O writer é
    // tomado na ordem cauda → writer; a cauda é solta antes do I/O bloqueante.
    let mut reservation = None;
    let write_result = (|| -> Result<(), String> {
        let mut tail = session.output_tail.lock().unwrap();
        ensure_sudo_authorize_cooldown(&tail)?;
        let current_prompt = tail.visible_sudo_prompt();
        let unchanged = current_prompt
            .as_ref()
            .is_some_and(|prompt| {
                authorization_matches_prompt(
                    &tail,
                    prompt,
                    prompt_token,
                    &authorization_prompt,
                )
            });
        if !unchanged {
            // Não apaga um prompt novo que possa ter substituído o token.
            if current_prompt.is_none() || tail.active.as_ref() == Some(&authorization_prompt) {
                clear_sudo_prompt(&app, &id, &mut tail);
            }
            return Err("PROMPT_CHANGED: o prompt de sudo mudou".into());
        }

        let mut writer = session.writer.lock().unwrap();
        let writer = writer.as_mut().ok_or("sessão sem PTY ativo")?;
        let prompt = current_prompt.unwrap();
        reservation = Some((
            tail.consumed_before,
            tail.consumed_line.clone(),
            tail.active.clone(),
            tail.last_authorized_at,
        ));
        tail.consumed_before = tail.stream_len();
        tail.consumed_line = Some(normalized_sudo_prompt_line(&prompt));
        tail.last_authorized_at = Some(Instant::now());
        clear_sudo_prompt(&app, &id, &mut tail);
        drop(tail);

        writer
            .write_all(secret.as_bytes())
            .and_then(|_| writer.write_all(b"\n"))
            .and_then(|_| writer.flush())
            .map_err(|e| e.to_string())?;
        Ok(())
    })();
    if let Err(error) = write_result {
        if let Some((previous_consumed, previous_line, previous_active, previous_authorized)) =
            reservation
        {
            let credential = sudo_credential_availability(&session);
            let mut tail = session.output_tail.lock().unwrap();
            tail.consumed_before = previous_consumed;
            tail.consumed_line = previous_line;
            tail.active = previous_active;
            tail.last_authorized_at = previous_authorized;
            if let Some(prompt) = tail.visible_sudo_prompt() {
                if credential == "ok" || credential == "unmatched" {
                    let context = sudo_prompt_context(&tail.buf, &prompt);
                    let identity = sudo_prompt_identity(&prompt);
                    let prompt_token = identity.offset;
                    tail.active = Some(identity);
                    emit_sudo_prompt(
                        &app,
                        &id,
                        true,
                        context,
                        credential,
                        Some(prompt_token),
                    );
                }
            }
        }
        return Err(error);
    }

    *session.last_input.lock().unwrap() = Instant::now();
    set_attention(&app, &session, &id, false, None);
    Ok(())
}

fn ensure_sudo_authorize_cooldown(tail: &Tail) -> Result<(), String> {
    if tail
        .last_authorized_at
        .is_some_and(|sent_at| sent_at.elapsed() < SUDO_AUTHORIZE_COOLDOWN)
    {
        Err("aguarde: senha já enviada".into())
    } else {
        Ok(())
    }
}

#[tauri::command]
pub fn dismiss_sudo_prompt(
    app: AppHandle,
    sessions: State<'_, Sessions>,
    id: String,
) -> Result<(), String> {
    let session = get(&sessions, &id)?;
    let mut tail = session.output_tail.lock().unwrap();
    if let Some(prompt) = tail.visible_sudo_prompt() {
        tail.dismissed = Some((prompt.offset, normalized_sudo_prompt_line(&prompt)));
    }
    clear_sudo_prompt(&app, &id, &mut tail);
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
    if data.contains('\r') || data.contains('\n') {
        let mut tail = session.output_tail.lock().unwrap();
        tail.consumed_line = tail
            .active_sudo_prompt()
            .as_ref()
            .map(normalized_sudo_prompt_line);
        tail.consumed_before = tail.stream_len();
        tail.dismissed = None;
        clear_sudo_prompt(&app, &id, &mut tail);
    }
    // tecla do usuário → registra input e limpa atenção
    *session.last_input.lock().unwrap() = Instant::now();
    set_attention(&app, &session, &id, false, None);
    // A escrita segura somente `writer`: close_session ainda alcança o killer
    // via `io`, e output_tail continua livre para o leitor drenar o PTY.
    {
        let mut writer = session.writer.lock().unwrap();
        let writer = writer.as_mut().ok_or("sessão sem PTY ativo")?;
        writer.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())?;
    }
    Ok(())
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
                let unconsumed = {
                    let tail = session.output_tail.lock().unwrap();
                    tail_after_consumed(&tail, tail.consumed_before).to_string()
                };
                // Um Enter consome tudo que já estava visível. Se um TUI não
                // redesenhar seu prompt depois de um Enter acidental, ele não
                // rearma a atenção — trade-off deliberado desta watermark.
                if looks_like_prompt(&unconsumed) {
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
        active_sudo_prompt, agent_binary, authorization_matches_prompt, classify_exit_status,
        decode_utf8_for_tail, ensure_sudo_authorize_cooldown, looks_like_prompt,
        normalized_sudo_prompt_line, should_emit_sudo_prompt, strip_ansi, sudo_prompt_context,
        sudo_prompt_identity, tail_after_consumed, ExitKind, Tail,
    };
    use portable_pty::ExitStatus;
    use std::time::Instant;

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
        assert!(!looks_like_prompt(
            "Documentation example: 1. Yes\nline two\nline three\nline four"
        ));
        assert!(!looks_like_prompt(
            "The options include always allow and allow once.\nnotes\nmore notes\nend"
        ));
        assert!(!looks_like_prompt("Working (12s · esc to interrupt)"));
    }

    #[test]
    fn reconhece_apenas_prompt_sudo_ativo_no_fim_da_cauda() {
        assert!(active_sudo_prompt("[sudo] password for deploy:", 0).is_some());
        assert!(active_sudo_prompt("output\n[sudo] password for deploy:   ", 0).is_some());
        assert!(active_sudo_prompt(
            "[sudo] password for deploy:\x1b[1;1H[0] 0:bash* 12:34",
            0
        )
        .is_some());
        assert!(active_sudo_prompt("[sudo] password for deploy:\nserviço reiniciado", 0).is_none());
        assert!(active_sudo_prompt("[sudo] password for deploy:\rserviço reiniciado", 0).is_none());
        assert!(active_sudo_prompt("Password:", 0).is_none());
        assert!(active_sudo_prompt("status apagado [sudo] password for deploy:", 0).is_some());
    }

    #[test]
    fn reconhece_prompts_sudo_localizados_do_app() {
        assert!(active_sudo_prompt("[sudo] password for deploy:", 0).is_some());
        assert!(active_sudo_prompt("[sudo] senha para deploy:", 0).is_some());
        assert!(active_sudo_prompt("[sudo] contraseña para deploy:", 0).is_some());
        assert!(active_sudo_prompt("[sudo] Mot de passe de deploy :", 0).is_some());
        assert!(active_sudo_prompt("Password:", 0).is_none());
    }

    #[test]
    fn offset_do_prompt_e_o_inicio_do_segmento_cru() {
        let kelvin = "\u{212a} [sudo] password for deploy:";
        let kelvin_prompt = active_sudo_prompt(kelvin, 100).unwrap();
        assert_eq!(kelvin_prompt.start, 0);
        assert_eq!(kelvin_prompt.offset, 100);
        assert_eq!(kelvin_prompt.line, "[sudo] password for deploy:");

        let dotted_i = "prefixo\rİ [sudo] senha para deploy:";
        let dotted_i_prompt = active_sudo_prompt(dotted_i, 200).unwrap();
        assert_eq!(dotted_i_prompt.start, "prefixo\r".len());
        assert_eq!(dotted_i_prompt.offset, 200 + "prefixo\r".len() as u64);
        assert_eq!(dotted_i_prompt.line, "[sudo] senha para deploy:");
    }

    #[test]
    fn prompt_sudo_com_escape_no_meio_do_prefixo_casa() {
        let prompt = active_sudo_prompt("[sudo] pass\x1b[Kword for deploy:", 42).unwrap();
        assert_eq!(prompt.offset, 42);
        assert_eq!(prompt.line, "[sudo] password for deploy:");
    }

    #[test]
    fn mesmo_prompt_com_status_do_tmux_mantem_offset() {
        let mut tail = String::from("sudo systemctl restart api\n[sudo] password for deploy:");
        let first = active_sudo_prompt(&tail, 100).unwrap();

        tail.push_str("\x1b[1;1H[0] 0:bash* 12:34");
        let refreshed = active_sudo_prompt(&tail, 100).unwrap();

        assert_eq!(first.offset, refreshed.offset);
    }

    #[test]
    fn redraw_no_mesmo_offset_com_linha_nova_reemite_e_invalida_autorizacao() {
        let mut tail = Tail::default();
        tail.push("[sudo] password for deploy:");
        let first = tail.visible_sudo_prompt().unwrap();
        let authorized = sudo_prompt_identity(&first);
        tail.active = Some(authorized.clone());

        tail.push("\x1b[2K\x1b[1G[sudo] password for root:");
        let redraw = tail.visible_sudo_prompt().unwrap();

        assert_eq!(redraw.offset, first.offset);
        assert_ne!(sudo_prompt_identity(&redraw), authorized);
        assert!(should_emit_sudo_prompt(tail.active.as_ref(), &redraw));
        assert!(!authorization_matches_prompt(
            &tail,
            &redraw,
            first.offset,
            &authorized,
        ));
    }

    #[test]
    fn prompt_sudo_com_utf8_partido_entre_chunks_ainda_e_detectado() {
        let bytes = "[sudo] contraseña para deploy:".as_bytes();
        let split = bytes.iter().position(|byte| *byte == 0xc3).unwrap() + 1;
        let mut pending = Vec::new();
        let mut tail = Tail::default();

        tail.push(&decode_utf8_for_tail(&mut pending, &bytes[..split]));
        assert_eq!(pending, vec![0xc3]);
        assert!(tail.visible_sudo_prompt().is_none());

        tail.push(&decode_utf8_for_tail(&mut pending, &bytes[split..]));
        let prompt = tail.visible_sudo_prompt().unwrap();
        assert!(pending.is_empty());
        assert_eq!(prompt.line, "[sudo] contraseña para deploy:");
    }

    #[test]
    fn retries_em_ingles_e_portugues_recebem_offsets_novos() {
        let mut tail = Tail::default();
        tail.push("sudo systemctl restart api\n[sudo] password for deploy:");
        let first = tail.visible_sudo_prompt().unwrap();
        tail.consumed_before = tail.stream_len();
        tail.consumed_line = Some(normalized_sudo_prompt_line(&first));
        assert!(tail.visible_sudo_prompt().is_none());

        tail.push("\r\nSorry, try again.\r\n[sudo] password for deploy:");
        let english = tail.visible_sudo_prompt().unwrap();
        assert!(english.offset > first.offset);

        tail.consumed_before = tail.stream_len();
        tail.consumed_line = Some(normalized_sudo_prompt_line(&english));
        tail.push("\r\nDesculpe, tente novamente.\r\n[sudo] senha para deploy:");
        let portuguese = tail.visible_sudo_prompt().unwrap();
        assert!(portuguese.offset > english.offset);
    }

    #[test]
    fn enter_consome_a_linha_e_suprime_repaint_identico() {
        let mut tail = Tail::default();
        tail.push("cmd\n[sudo] password for deploy:");
        let current = tail.visible_sudo_prompt().unwrap();
        tail.consumed_line = Some(normalized_sudo_prompt_line(&current));
        tail.consumed_before = tail.stream_len();

        assert!(tail.visible_sudo_prompt().is_none());
        assert!(!looks_like_prompt(tail_after_consumed(&tail, tail.consumed_before)));

        tail.push("\r\n[sudo] password for deploy:");
        assert!(looks_like_prompt(tail_after_consumed(&tail, tail.consumed_before)));
        assert!(tail.visible_sudo_prompt().is_none());
    }

    #[test]
    fn primeiro_prompt_apos_reset_pode_ter_offset_zero() {
        let mut tail = Tail::default();
        tail.reset();
        tail.push("[sudo] password for deploy:");

        let prompt = tail.visible_sudo_prompt().unwrap();
        assert_eq!(prompt.offset, 0);
    }

    #[test]
    fn watermark_no_meio_de_utf8_avanca_ate_fronteira() {
        let mut tail = Tail::default();
        tail.push("éPassword:");
        let inside_e_acute = tail.start_offset + 1;

        assert_eq!(tail_after_consumed(&tail, inside_e_acute), "Password:");
    }

    #[test]
    fn truncamento_da_cauda_preserva_offset_absoluto_do_prompt() {
        let mut tail = Tail::default();
        tail.push(&format!(
            "{}[sudo] password for deploy:",
            "x".repeat(390)
        ));
        let first = tail.active_sudo_prompt().unwrap();
        tail.active = Some(sudo_prompt_identity(&first));

        tail.push("\x1b[1;1H[0] 0:bash* 12:34");
        let refreshed = tail.active_sudo_prompt().unwrap();

        assert_eq!(tail.stream_len(), tail.start_offset + tail.buf.len() as u64);
        assert_eq!(first.offset, refreshed.offset);
    }

    #[test]
    fn repaint_identico_apos_dismiss_nao_reemite() {
        let mut tail = Tail::default();
        tail.push("cmd\n[sudo] password for deploy:");
        let first = tail.visible_sudo_prompt().unwrap();
        tail.dismissed = Some((first.offset, normalized_sudo_prompt_line(&first)));
        assert!(tail.visible_sudo_prompt().is_none());

        tail.push("\r[sudo] password for deploy:");
        assert!(tail.visible_sudo_prompt().is_none());
    }

    #[test]
    fn repaint_identico_apos_authorize_nao_reemite() {
        let mut tail = Tail::default();
        tail.push("cmd\n[sudo] password for deploy:");
        let first = tail.visible_sudo_prompt().unwrap();
        tail.consumed_before = tail.stream_len();
        tail.consumed_line = Some(normalized_sudo_prompt_line(&first));

        tail.push("\r[sudo] password for deploy:");
        assert!(tail.visible_sudo_prompt().is_none());
    }

    #[test]
    fn retry_apos_authorize_reemite_prompt_identico() {
        let mut tail = Tail::default();
        tail.push("[sudo] password for deploy:");
        let first = tail.visible_sudo_prompt().unwrap();
        tail.consumed_before = tail.stream_len();
        tail.consumed_line = Some(normalized_sudo_prompt_line(&first));

        tail.push("\r\nSorry, try again.\r\n[sudo] password for deploy:");
        assert!(tail.visible_sudo_prompt().is_some());
    }

    #[test]
    fn refreshes_longos_mantem_segmento_do_prompt_ativo() {
        let mut tail = Tail::default();
        tail.push("contexto\n[sudo] password for deploy:");
        let prompt = tail.visible_sudo_prompt().unwrap();
        tail.active = Some(sudo_prompt_identity(&prompt));

        for _ in 0..10 {
            tail.push(&format!("\x1b[1;1H{}", "s".repeat(60)));
        }

        assert!(tail.active_sudo_prompt().is_some());
        assert!(tail.start_offset <= prompt.offset);
    }

    #[test]
    fn cooldown_recusa_segunda_autorizacao() {
        let tail = Tail { last_authorized_at: Some(Instant::now()), ..Default::default() };
        assert_eq!(
            ensure_sudo_authorize_cooldown(&tail),
            Err("aguarde: senha já enviada".into())
        );
    }

    #[test]
    fn contexto_sudo_usa_as_tres_ultimas_linhas_limpas() {
        let tail = "ignorada\n\x1b[32msudo systemctl restart api\x1b[0m\n\nfalha anterior\n[sudo] password for deploy:";
        let prompt = active_sudo_prompt(tail, 0).unwrap();
        assert_eq!(
            sudo_prompt_context(tail, &prompt),
            "sudo systemctl restart api\nfalha anterior\n[sudo] password for deploy:"
        );
    }

    #[test]
    fn contexto_sudo_ignora_status_do_tmux_apos_dois_pontos() {
        let first = concat!(
            "sudo systemctl restart api\n",
            "[sudo] password for deploy:\x1b[1;1H[0] 0:bash* 12:34"
        );
        let refreshed = concat!(
            "sudo systemctl restart api\n",
            "[sudo] password for deploy:\x1b[1;1H[0] 0:bash* 12:49"
        );

        assert_eq!(
            sudo_prompt_context(first, &active_sudo_prompt(first, 0).unwrap()),
            sudo_prompt_context(refreshed, &active_sudo_prompt(refreshed, 0).unwrap())
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
