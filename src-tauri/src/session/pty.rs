use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use base64::Engine;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::db::{self, Db};

/// Uma sessão de terminal viva: PTY + processo filho.
pub struct PtySession {
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
}

#[derive(Default)]
pub struct Sessions(pub Mutex<HashMap<String, Arc<PtySession>>>);

#[derive(Clone, Serialize)]
struct OutputPayload<'a> {
    id: &'a str,
    /// Chunk de bytes do PTY em base64 — preserva sequências UTF-8/ANSI
    /// partidas na fronteira do chunk (o xterm decodifica byte a byte).
    data: String,
}

#[derive(Clone, Serialize)]
struct ExitPayload<'a> {
    id: &'a str,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusPayload<'a> {
    id: &'a str,
    status: &'a str,
}

fn emit_status(app: &AppHandle, id: &str, status: &str) {
    let _ = app.emit("session-status", StatusPayload { id, status });
}

fn default_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(target_os = "macos") {
            "/bin/zsh".into()
        } else {
            "/bin/bash".into()
        }
    })
}

/// Faz o spawn de `cmd` num PTY novo, registra a sessão e inicia a thread de
/// leitura. `track_status` liga a máquina de estados (sessões SSH).
fn spawn_in_pty(
    app: AppHandle,
    sessions: &State<'_, Sessions>,
    id: String,
    cols: u16,
    rows: u16,
    cmd: CommandBuilder,
    track_status: bool,
) -> Result<(), String> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;

    let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    eprintln!("[pty] sessão {id}: processo pid={:?}", child.process_id());
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

    let session = Arc::new(PtySession {
        writer: Mutex::new(writer),
        master: Mutex::new(pair.master),
        child: Mutex::new(child),
    });
    sessions.0.lock().unwrap().insert(id.clone(), session);

    if track_status {
        emit_status(&app, &id, "connecting");
    }

    // Leitura bloqueante em thread dedicada; cada chunk vira um evento.
    std::thread::spawn(move || {
        let b64 = base64::engine::general_purpose::STANDARD;
        let mut buf = [0u8; 8192];
        let mut first_output = true;
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if track_status && first_output {
                        // Heurística da Fase 2: primeiro output = canal vivo.
                        // A Fase 3 refina (reconexão, auth recusada, timeout).
                        emit_status(&app, &id, "connected");
                        first_output = false;
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
        eprintln!("[pty] sessão {id}: EOF do leitor — processo encerrou");
        if track_status {
            emit_status(&app, &id, "exited");
        }
        let _ = app.emit("session-exit", ExitPayload { id: &id });
    });

    Ok(())
}

#[tauri::command]
pub fn open_local_session(
    app: AppHandle,
    sessions: State<'_, Sessions>,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let mut cmd = CommandBuilder::new(default_shell());
    cmd.arg("-l"); // login shell, como em qualquer app de terminal
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    if let Some(home) = std::env::var_os("HOME") {
        cmd.cwd(home);
    }
    spawn_in_pty(app, &sessions, id, cols, rows, cmd, false)
}

/// Abre uma sessão SSH para um host cadastrado, orquestrando o OpenSSH do
/// sistema (`ssh -tt`) — herda agent, ~/.ssh/config e ProxyCommand de graça.
#[tauri::command]
pub fn open_ssh_session(
    app: AppHandle,
    sessions: State<'_, Sessions>,
    db: State<'_, Db>,
    id: String,
    host_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let host = db::get_host(&db, &host_id)?;

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
    cmd.env("TERM", "xterm-256color");

    spawn_in_pty(app, &sessions, id, cols, rows, cmd, true)
}

#[tauri::command]
pub fn write_stdin(sessions: State<'_, Sessions>, id: String, data: String) -> Result<(), String> {
    let session = get(&sessions, &id)?;
    let mut writer = session.writer.lock().unwrap();
    writer.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn resize_pty(
    sessions: State<'_, Sessions>,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let session = get(&sessions, &id)?;
    let master = session.master.lock().unwrap();
    let result = master.resize(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    });
    result.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn close_session(sessions: State<'_, Sessions>, id: String) -> Result<(), String> {
    if let Some(session) = sessions.0.lock().unwrap().remove(&id) {
        let _ = session.child.lock().unwrap().kill();
    }
    Ok(())
}

fn get(sessions: &State<'_, Sessions>, id: &str) -> Result<Arc<PtySession>, String> {
    sessions
        .0
        .lock()
        .unwrap()
        .get(id)
        .cloned()
        .ok_or_else(|| format!("sessão desconhecida: {id}"))
}
