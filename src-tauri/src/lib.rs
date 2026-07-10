mod db;
mod remote;
mod session;
mod sshconfig;
mod vault;
mod vpn;

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
            let v = vault::Vault::default();
            vault::spawn_auto_lock(app.handle().clone(), v.0.clone());
            app.manage(v);
            manager::spawn_attention_monitor(app.handle().clone());
            app.manage(vpn::Vpn::default());
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
            db::get_pref,
            db::set_pref,
            vault::vault_status,
            vault::vault_unlock,
            vault::vault_lock,
            vault::vault_list,
            vault::vault_save,
            vault::vault_delete,
            vault::vault_reveal,
            remote::test_connection,
            remote::detect_remote,
            remote::install_tmux,
            sshconfig::import_ssh_config,
            sshconfig::app_info,
            vpn::vpn_list,
            vpn::vpn_connect,
            vpn::vpn_disconnect,
            vpn::vpn_set_auto_disconnect,
            vpn::vpn_get_auto_disconnect,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
