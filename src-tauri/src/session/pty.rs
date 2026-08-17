//! Camada baixa: spawn de um processo dentro de um PTY.

use portable_pty::{
    native_pty_system, ChildKiller, CommandBuilder, ExitStatus, MasterPty, PtySize,
};
use std::io::{Read, Write};
use std::sync::mpsc::Receiver;

pub struct PtyHandles {
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
    pub master: Box<dyn MasterPty + Send>,
    pub killer: Box<dyn ChildKiller + Send + Sync>,
    pub exit_status: Receiver<std::io::Result<ExitStatus>>,
}

pub fn spawn(cmd: CommandBuilder, cols: u16, rows: u16) -> Result<PtyHandles, String> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;

    let mut child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    drop(pair.slave);

    let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    let killer = child.clone_killer();
    let (exit_status_tx, exit_status) = std::sync::mpsc::channel();

    // Preserva o status do filho para o manager classificar o EOF.
    std::thread::spawn(move || {
        let _ = exit_status_tx.send(child.wait());
    });

    Ok(PtyHandles {
        reader,
        writer,
        master: pair.master,
        killer,
        exit_status,
    })
}

pub fn default_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(target_os = "macos") {
            "/bin/zsh".into()
        } else {
            "/bin/bash".into()
        }
    })
}
