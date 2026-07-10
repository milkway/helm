mod session;

use session::pty;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(pty::Sessions::default())
        .invoke_handler(tauri::generate_handler![
            pty::open_local_session,
            pty::write_stdin,
            pty::resize_pty,
            pty::close_session,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
