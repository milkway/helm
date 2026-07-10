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

use crate::db::{self, Db, Host};
use crate::session::pty;

const MAX_ATTEMPTS: u32 = 5;
const ERROR_RETRY_SECS: u64 = 60;
/// viveu menos que isto com output = tentativa falhada, não conexão real
const MIN_STABLE_SECS: u64 = 5;

pub struct Session {
    #[allow(dead_code)] // usado nas fases de attention/latência
    host_id: Option<String>,
    closed: AtomicBool,
    detached: AtomicBool,
    retry_now: AtomicBool,
    size: Mutex<(u16, u16)>,
    io: Mutex<Option<Io>>,
}

struct Io {
    writer: Box<dyn Write + Send>,
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
}

fn emit_status(app: &AppHandle, id: &str, status: &str, attempt: Option<u32>, delay: Option<u64>) {
    let _ = app.emit(
        "session-status",
        StatusPayload {
            id,
            status,
            attempt,
            delay_secs: delay,
        },
    );
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

fn build_command(host: Option<&Host>, params: Option<&SessionParams>) -> Result<CommandBuilder, String> {
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
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut attempt: u32 = 0;

    loop {
        if session.closed.load(Ordering::Relaxed) {
            break;
        }

        if attempt == 0 {
            emit_status(&app, &id, "connecting", None, None);
        }

        let (cols, rows) = *session.size.lock().unwrap();
        let cmd = match build_command(host.as_ref(), params.as_ref()) {
            Ok(cmd) => cmd,
            Err(e) => {
                eprintln!("[session {id}] comando inválido: {e}");
                emit_status(&app, &id, "error", Some(attempt), None);
                break;
            }
        };

        let spawn_result = pty::spawn(cmd, cols, rows);
        let (lived, got_output) = match spawn_result {
            Err(e) => {
                eprintln!("[session {id}] spawn falhou: {e}");
                (Duration::ZERO, false)
            }
            Ok(mut handles) => {
                *session.io.lock().unwrap() = Some(Io {
                    writer: handles.writer,
                    master: handles.master,
                    killer: handles.killer,
                });
                let started = Instant::now();
                let mut got_output = false;
                let mut buf = [0u8; 8192];
                loop {
                    match handles.reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if !got_output {
                                got_output = true;
                                emit_status(&app, &id, "connected", None, None);
                            }
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
                *session.io.lock().unwrap() = None;
                (started.elapsed(), got_output)
            }
        };

        if session.closed.load(Ordering::Relaxed) {
            emit_status(&app, &id, "exited", None, None);
            break;
        }
        if session.detached.load(Ordering::Relaxed) {
            eprintln!("[session {id}] detach — tmux preservado no servidor");
            emit_status(&app, &id, "detached", None, None);
            break;
        }
        if !auto_reconnect {
            emit_status(&app, &id, "exited", None, None);
            break;
        }

        // conexão estável zera o contador de tentativas
        if got_output && lived >= Duration::from_secs(MIN_STABLE_SECS) {
            attempt = 0;
        }
        attempt += 1;

        if attempt > MAX_ATTEMPTS {
            eprintln!("[session {id}] {MAX_ATTEMPTS} tentativas falharam — retry em {ERROR_RETRY_SECS}s");
            emit_status(&app, &id, "error", Some(MAX_ATTEMPTS), Some(ERROR_RETRY_SECS));
            interruptible_sleep(&session, ERROR_RETRY_SECS);
            if session.closed.load(Ordering::Relaxed) {
                break;
            }
            attempt = 0; // novo ciclo completo
            continue;
        }

        let delay = (1u64 << (attempt - 1)).min(30);
        eprintln!("[session {id}] reconectando — tentativa {attempt}/{MAX_ATTEMPTS} em {delay}s");
        emit_status(&app, &id, "reconnecting", Some(attempt), Some(delay));
        interruptible_sleep(&session, delay);
    }

    // remove do registro ao terminar de vez
    if let Some(sessions) = app.try_state::<Sessions>() {
        sessions.0.lock().unwrap().remove(&id);
    }
}

fn start_session(
    app: AppHandle,
    sessions: &State<'_, Sessions>,
    id: String,
    host: Option<Host>,
    cols: u16,
    rows: u16,
    params: Option<SessionParams>,
) {
    let session = Arc::new(Session {
        host_id: host.as_ref().map(|h| h.id.clone()),
        closed: AtomicBool::new(false),
        detached: AtomicBool::new(false),
        retry_now: AtomicBool::new(false),
        size: Mutex::new((cols, rows)),
        io: Mutex::new(None),
    });
    sessions.0.lock().unwrap().insert(id.clone(), session.clone());
    std::thread::spawn(move || manager_loop(app, id, session, host, params));
}

#[tauri::command]
pub fn open_local_session(
    app: AppHandle,
    sessions: State<'_, Sessions>,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    start_session(app, &sessions, id, None, cols, rows, None);
    Ok(())
}

#[tauri::command]
pub fn open_ssh_session(
    app: AppHandle,
    sessions: State<'_, Sessions>,
    db: State<'_, Db>,
    id: String,
    host_id: String,
    cols: u16,
    rows: u16,
    params: Option<SessionParams>,
) -> Result<(), String> {
    let host = db::get_host(&db, &host_id)?;
    start_session(app, &sessions, id, Some(host), cols, rows, params);
    Ok(())
}

#[tauri::command]
pub fn write_stdin(sessions: State<'_, Sessions>, id: String, data: String) -> Result<(), String> {
    let session = get(&sessions, &id)?;
    let mut io = session.io.lock().unwrap();
    let io = io.as_mut().ok_or("sessão sem PTY ativo")?;
    io.writer.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
    io.writer.flush().map_err(|e| e.to_string())
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
        session.closed.store(true, Ordering::Relaxed);
        if let Some(io) = session.io.lock().unwrap().as_mut() {
            let _ = io.killer.kill();
        }
    }
    Ok(())
}

/// Detach do tmux: injeta prefixo Ctrl+B + d — o comando remoto termina,
/// o ssh sai limpo e a sessão tmux continua viva no servidor.
#[tauri::command]
pub fn detach_session(sessions: State<'_, Sessions>, id: String) -> Result<(), String> {
    let session = get(&sessions, &id)?;
    session.detached.store(true, Ordering::Relaxed);
    let mut io = session.io.lock().unwrap();
    let io = io.as_mut().ok_or("sessão sem PTY ativo")?;
    io.writer.write_all(b"\x02d").map_err(|e| e.to_string())?;
    io.writer.flush().map_err(|e| e.to_string())
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
