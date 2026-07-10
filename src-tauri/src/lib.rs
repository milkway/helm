mod db;
mod session;

use session::manager;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(manager::Sessions::default())
        .setup(|app| {
            let conn = db::open(app.handle()).map_err(std::io::Error::other)?;
            app.manage(db::Db(std::sync::Mutex::new(conn)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            manager::open_local_session,
            manager::open_ssh_session,
            manager::write_stdin,
            manager::resize_pty,
            manager::close_session,
            manager::detach_session,
            manager::retry_session,
            db::list_hosts,
            db::save_host,
            db::delete_host,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
