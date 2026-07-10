mod db;
mod session;

use session::pty;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(pty::Sessions::default())
        .setup(|app| {
            let conn = db::open(app.handle()).map_err(std::io::Error::other)?;
            app.manage(db::Db(std::sync::Mutex::new(conn)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            pty::open_local_session,
            pty::open_ssh_session,
            pty::write_stdin,
            pty::resize_pty,
            pty::close_session,
            db::list_hosts,
            db::save_host,
            db::delete_host,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
