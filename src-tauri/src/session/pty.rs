use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use base64::Engine;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

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

fn default_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(target_os = "macos") {
            "/bin/zsh".into()
        } else {
            "/bin/bash".into()
        }
    })
}

#[tauri::command]
pub fn open_local_session(
    app: AppHandle,
    sessions: State<'_, Sessions>,
    id: String,
    cols: u16,
    rows: u16,
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

    let mut cmd = CommandBuilder::new(default_shell());
    cmd.arg("-l"); // login shell, como em qualquer app de terminal
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    if let Some(home) = dirs_home() {
        cmd.cwd(home);
    }

    let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    eprintln!("[pty] sessão {id}: shell pid={:?}", child.process_id());
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

    let session = Arc::new(PtySession {
        writer: Mutex::new(writer),
        master: Mutex::new(pair.master),
        child: Mutex::new(child),
    });
    sessions.0.lock().unwrap().insert(id.clone(), session);

    // Leitura bloqueante em thread dedicada; cada chunk vira um evento.
    std::thread::spawn(move || {
        let b64 = base64::engine::general_purpose::STANDARD;
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
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
        eprintln!("[pty] sessão {id}: EOF do leitor — shell encerrou");
        let _ = app.emit("session-exit", ExitPayload { id: &id });
    });

    Ok(())
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

fn dirs_home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(Into::into)
}
