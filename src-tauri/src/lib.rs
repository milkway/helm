pub mod askpass;
mod db;
mod external;
mod remote;
mod session;
mod sshconfig;
mod vault;
mod vpn;

use session::manager;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
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
            manager::open_ssh_session,
            manager::write_stdin,
            manager::authorize_sudo,
            manager::resize_pty,
            manager::close_session,
            manager::detach_session,
            manager::retry_session,
            db::list_hosts,
            db::save_host,
            db::delete_host,
            db::host_has_ssh_credential,
            db::get_pref,
            db::set_pref,
            external::open_external,
            external::check_updates,
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
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    let mut shutdown_started = false;
    app.run(move |app, event| {
        if !shutdown_started
            && matches!(event, tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit)
        {
            shutdown_started = true;
            if let Some(sessions) = app.try_state::<manager::Sessions>() {
                manager::shutdown_sessions(&sessions, std::time::Duration::from_secs(2));
            }
        }
    });
}
