use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};

pub struct Db(pub Mutex<Connection>);

/// Metadados de um host SSH. Segredos NUNCA entram aqui — `credential_ref`
/// é uma chave do keyring (Fase 5).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Host {
    pub id: String,
    pub name: String,
    pub group: String,
    /// usuário SSH; None = deixar o ~/.ssh/config decidir
    pub user: Option<String>,
    /// endereço ou alias do ~/.ssh/config
    pub host: String,
    /// None = porta do config/22
    pub port: Option<u16>,
    pub credential_ref: Option<String>,
    pub vpn_profile: Option<String>,
    pub auto_reconnect: bool,
    pub auto_install_tmux: bool,
    pub auto_attach: bool,
    pub project_dir: Option<String>,
    /// shell | tmux | clmux
    pub startup_mode: String,
}

const MIGRATIONS: &[&str] = &[
    // v1
    "CREATE TABLE hosts (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        \"group\" TEXT NOT NULL DEFAULT '',
        user TEXT,
        host TEXT NOT NULL,
        port INTEGER,
        credential_ref TEXT,
        vpn_profile TEXT,
        auto_reconnect INTEGER NOT NULL DEFAULT 1,
        auto_install_tmux INTEGER NOT NULL DEFAULT 0,
        auto_attach INTEGER NOT NULL DEFAULT 1,
        project_dir TEXT,
        startup_mode TEXT NOT NULL DEFAULT 'shell'
            CHECK (startup_mode IN ('shell','tmux','clmux')),
        created_at TEXT NOT NULL DEFAULT (datetime('now')),
        updated_at TEXT NOT NULL DEFAULT (datetime('now'))
    );
    CREATE TABLE ui_prefs (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    // v2 — metadados de credenciais; o SEGREDO fica só no Keychain/Secret Service
    "CREATE TABLE credentials_meta (
        id TEXT PRIMARY KEY,
        kind TEXT NOT NULL CHECK (kind IN ('ssh_key','password')),
        label TEXT NOT NULL,
        algo TEXT,
        scope TEXT,
        last_used TEXT
    );",
    // v3 — credenciais só-metadados (NOPASSWD, chave sem passphrase)
    "ALTER TABLE credentials_meta ADD COLUMN has_secret INTEGER NOT NULL DEFAULT 1;",
];

pub fn open(app: &AppHandle) -> Result<Connection, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let conn = Connection::open(dir.join("helm.db")).map_err(|e| e.to_string())?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<(), String> {
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
        // cada migração é atômica: DDL + bump do user_version num único
        // BEGIN/COMMIT. Um crash no meio faz rollback (o user_version também é
        // transacional no SQLite), evitando schema meio-aplicado que travaria o app.
        let batch = format!("BEGIN;\n{sql}\nPRAGMA user_version = {};\nCOMMIT;", i + 1);
        conn.execute_batch(&batch).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn get_host(db: &State<'_, Db>, id: &str) -> Result<Host, String> {
    let conn = db.0.lock().unwrap();
    conn.query_row(
        "SELECT id, name, \"group\", user, host, port, credential_ref, vpn_profile,
                auto_reconnect, auto_install_tmux, auto_attach, project_dir, startup_mode
         FROM hosts WHERE id = ?1",
        [id],
        row_to_host,
    )
    .map_err(|e| format!("host {id}: {e}"))
}

pub(crate) fn has_ssh_password_credential(
    conn: &Connection,
    id: &str,
) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM credentials_meta
            WHERE id = ?1
              AND kind = 'password'
              AND scope LIKE '%ssh%'
              AND has_secret
        )",
        [id],
        |row| row.get(0),
    )
}

pub(crate) type SudoPasswordCredential = (String, String);

fn sudo_password_credentials(
    conn: &Connection,
) -> rusqlite::Result<Vec<SudoPasswordCredential>> {
    let mut stmt = conn.prepare(
        "SELECT id, label FROM credentials_meta
         WHERE kind = 'password'
           AND LOWER(COALESCE(scope, '')) LIKE '%sudo%'
           AND has_secret = 1",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn matching_sudo_password_credentials(
    host: &Host,
    eligible: &[SudoPasswordCredential],
) -> Vec<String> {
    eligible
        .iter()
        .filter(|(_, label)| {
            let Some(target) = label.trim().strip_prefix("sudo · ") else {
                return false;
            };
            let target = target.trim();
            let (label_user, label_host) = match target.rsplit_once('@') {
                Some((user, target_host)) if !user.trim().is_empty() => {
                    (Some(user.trim()), target_host.trim())
                }
                _ => (None, target),
            };
            let label_host_folded = label_host.to_lowercase();
            let label_hostname_folded = label_hostname_without_port(label_host).to_lowercase();
            let host_address_folded = host.host.trim().to_lowercase();
            let host_name_folded = host.name.trim().to_lowercase();
            let host_matches = label_host_folded == host_address_folded
                || label_host_folded == host_name_folded
                || label_hostname_folded == host_address_folded
                || label_hostname_folded == host_name_folded;
            if label_host.is_empty()
                || !host_matches
            {
                return false;
            }
            match (label_user, host.user.as_deref()) {
                (Some(label_user), Some(host_user)) if !host_user.trim().is_empty() => {
                    label_user == host_user.trim()
                }
                (Some(_), _) => false,
                (None, _) => true,
            }
        })
        .map(|(id, _)| id.clone())
        .collect()
}

/// Recarrega host e credenciais sudo elegíveis sob uma única aquisição do DB.
/// O chamador deriva tanto a credencial única quanto o caso de label divergente
/// desta mesma fotografia, sem repetir o SELECT de `credentials_meta`.
pub(crate) fn get_host_and_sudo_password_credentials(
    db: &State<'_, Db>,
    id: &str,
) -> Result<(Host, Vec<SudoPasswordCredential>), String> {
    let conn = db.0.lock().unwrap();
    let host = conn
        .query_row(
            "SELECT id, name, \"group\", user, host, port, credential_ref, vpn_profile,
                    auto_reconnect, auto_install_tmux, auto_attach, project_dir, startup_mode
             FROM hosts WHERE id = ?1",
            [id],
            row_to_host,
        )
        .map_err(|e| format!("host {id}: {e}"))?;
    let candidates = sudo_password_credentials(&conn).map_err(|e| e.to_string())?;
    Ok((host, candidates))
}

pub(crate) fn resolve_sudo_password_credential(
    host: &Host,
    eligible: &[SudoPasswordCredential],
) -> (Option<String>, bool) {
    if let Some(credential_id) = host.credential_ref.as_deref() {
        if eligible.iter().any(|(id, _)| id == credential_id) {
            return (Some(credential_id.to_string()), false);
        }
    }

    let candidates = matching_sudo_password_credentials(host, eligible);
    if candidates.len() > 1 {
        eprintln!(
            "[db] {} credenciais sudo correspondem exatamente ao host {}",
            candidates.len(),
            host.id
        );
    }
    let credential_id = match candidates.as_slice() {
        [id] => Some(id.clone()),
        _ => None,
    };
    let unmatched_exists = !eligible.is_empty() && candidates.is_empty();

    (credential_id, unmatched_exists)
}

fn label_hostname_without_port(host: &str) -> &str {
    if let Some(bracketed) = host.strip_prefix('[') {
        if let Some((hostname, port)) = bracketed.rsplit_once("]:") {
            if port.parse::<u16>().is_ok() {
                return hostname;
            }
        }
    }
    match host.rsplit_once(':') {
        Some((hostname, port))
            if !hostname.contains(':') && port.parse::<u16>().is_ok() => hostname,
        _ => host,
    }
}

#[tauri::command]
pub fn host_has_ssh_credential(db: State<'_, Db>, host_id: String) -> Result<bool, String> {
    let conn = db.0.lock().unwrap();
    let credential_id: Option<String> = conn
        .query_row(
            "SELECT credential_ref FROM hosts WHERE id = ?1",
            [&host_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("host {host_id}: {e}"))?;
    let Some(credential_id) = credential_id else {
        return Ok(false);
    };
    has_ssh_password_credential(&conn, &credential_id).map_err(|e| e.to_string())
}

fn row_to_host(row: &rusqlite::Row) -> rusqlite::Result<Host> {
    Ok(Host {
        id: row.get(0)?,
        name: row.get(1)?,
        group: row.get(2)?,
        user: row.get(3)?,
        host: row.get(4)?,
        port: row.get(5)?,
        credential_ref: row.get(6)?,
        vpn_profile: row.get(7)?,
        auto_reconnect: row.get(8)?,
        auto_install_tmux: row.get(9)?,
        auto_attach: row.get(10)?,
        project_dir: row.get(11)?,
        startup_mode: row.get(12)?,
    })
}

#[tauri::command]
pub fn list_hosts(db: State<'_, Db>) -> Result<Vec<Host>, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT id, name, \"group\", user, host, port, credential_ref, vpn_profile,
                    auto_reconnect, auto_install_tmux, auto_attach, project_dir, startup_mode
             FROM hosts ORDER BY \"group\", name",
        )
        .map_err(|e| e.to_string())?;
    let hosts = stmt
        .query_map([], row_to_host)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(hosts)
}

#[tauri::command]
pub fn save_host(db: State<'_, Db>, host: Host) -> Result<(), String> {
    // Valores que começam com '-' virariam flags do ssh no spawn — rejeitar
    // já na persistência (defesa em profundidade com open_ssh_session).
    if host.host.is_empty() || host.host.starts_with('-') {
        return Err(format!("endereço de host inválido: {:?}", host.host));
    }
    if let Some(user) = &host.user {
        if user.starts_with('-') {
            return Err(format!("usuário inválido: {user:?}"));
        }
    }
    let conn = db.0.lock().unwrap();
    conn.execute(
        "INSERT INTO hosts (id, name, \"group\", user, host, port, credential_ref, vpn_profile,
                            auto_reconnect, auto_install_tmux, auto_attach, project_dir, startup_mode)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
         ON CONFLICT(id) DO UPDATE SET
            name=?2, \"group\"=?3, user=?4, host=?5, port=?6, credential_ref=?7,
            vpn_profile=?8, auto_reconnect=?9, auto_install_tmux=?10, auto_attach=?11,
            project_dir=?12, startup_mode=?13, updated_at=datetime('now')",
        rusqlite::params![
            host.id,
            host.name,
            host.group,
            host.user,
            host.host,
            host.port,
            host.credential_ref,
            host.vpn_profile,
            host.auto_reconnect,
            host.auto_install_tmux,
            host.auto_attach,
            host.project_dir,
            host.startup_mode,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_pref(db: State<'_, Db>, key: String) -> Result<Option<String>, String> {
    let conn = db.0.lock().unwrap();
    conn.query_row("SELECT value FROM ui_prefs WHERE key = ?1", [key], |r| r.get(0))
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            e => Err(e.to_string()),
        })
}

#[tauri::command]
pub fn set_pref(db: State<'_, Db>, key: String, value: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute(
        "INSERT INTO ui_prefs (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = ?2",
        rusqlite::params![key, value],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn delete_host(db: State<'_, Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM hosts WHERE id = ?1", [id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        has_ssh_password_credential, matching_sudo_password_credentials, migrate,
        resolve_sudo_password_credential, sudo_password_credentials, Host, MIGRATIONS,
    };
    use rusqlite::Connection;

    #[test]
    fn migrate_aplica_tudo_e_e_idempotente() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        // user_version chega ao total de migrações
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v as usize, MIGRATIONS.len());

        // rodar de novo é no-op (skip) e não falha
        migrate(&conn).unwrap();

        // schema aplicado (a coluna da v3 existe → migração transacional rodou)
        let has_col: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('credentials_meta') WHERE name='has_secret'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(has_col, 1);
    }

    #[test]
    fn reconhece_apenas_senha_com_escopo_ssh_e_segredo() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO credentials_meta (id, kind, label, scope, has_secret) VALUES
                ('ssh', 'password', 'SSH', 'ssh', 1),
                ('ssh-sudo', 'password', 'SSH sudo', 'ssh · sudo', 1),
                ('sudo', 'password', 'sudo', 'sudo', 1),
                ('nopasswd', 'password', 'NOPASSWD', 'NOPASSWD', 0),
                ('empty', 'password', 'SSH sem segredo', 'ssh', 0),
                ('key', 'ssh_key', 'Chave', 'ssh', 1);",
        )
        .unwrap();

        assert!(has_ssh_password_credential(&conn, "ssh").unwrap());
        assert!(has_ssh_password_credential(&conn, "ssh-sudo").unwrap());
        assert!(!has_ssh_password_credential(&conn, "sudo").unwrap());
        assert!(!has_ssh_password_credential(&conn, "nopasswd").unwrap());
        assert!(!has_ssh_password_credential(&conn, "empty").unwrap());
        assert!(!has_ssh_password_credential(&conn, "key").unwrap());
        assert!(!has_ssh_password_credential(&conn, "missing").unwrap());
    }

    #[test]
    fn reconhece_apenas_senha_com_escopo_sudo_e_segredo() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO credentials_meta (id, kind, label, scope, has_secret) VALUES
                ('sudo', 'password', 'sudo', 'sudo', 1),
                ('ssh-sudo', 'password', 'SSH sudo', 'ssh · sudo', 1),
                ('upper', 'password', 'SUDO', 'SUDO', 1),
                ('ssh', 'password', 'SSH', 'ssh', 1),
                ('nopasswd', 'password', 'NOPASSWD', 'NOPASSWD', 0),
                ('empty', 'password', 'sudo sem segredo', 'sudo', 0),
                ('key', 'ssh_key', 'Chave', 'ssh · sudo', 1);",
        )
        .unwrap();

        let ids: Vec<String> = sudo_password_credentials(&conn)
            .unwrap()
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert!(ids.contains(&"sudo".into()));
        assert!(ids.contains(&"ssh-sudo".into()));
        assert!(ids.contains(&"upper".into()));
        assert!(!ids.contains(&"ssh".into()));
        assert!(!ids.contains(&"nopasswd".into()));
        assert!(!ids.contains(&"empty".into()));
        assert!(!ids.contains(&"key".into()));
        assert!(!ids.contains(&"missing".into()));
    }

    #[test]
    fn encontra_credencial_sudo_dedicada_sem_adivinhar_ambiguidades() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO credentials_meta (id, kind, label, scope, has_secret) VALUES
                ('atlas', 'password', 'sudo · deploy@10.4.2.18', 'sudo', 1),
                ('other', 'password', 'sudo · deploy@10.4.2.19', 'sudo', 1),
                ('wrong-label', 'password', 'deploy@10.4.2.18', 'sudo', 1),
                ('no-secret', 'password', 'sudo · atlas-api', 'sudo', 0),
                ('ssh-only', 'password', 'sudo · atlas-api', 'ssh', 1);",
        )
        .unwrap();
        let host = Host {
            id: "host-atlas".into(),
            name: "Atlas API".into(),
            group: String::new(),
            user: Some("deploy".into()),
            host: "10.4.2.18".into(),
            port: None,
            credential_ref: None,
            vpn_profile: None,
            auto_reconnect: true,
            auto_install_tmux: false,
            auto_attach: true,
            project_dir: None,
            startup_mode: "shell".into(),
        };

        let eligible = sudo_password_credentials(&conn).unwrap();
        assert_eq!(
            resolve_sudo_password_credential(&host, &eligible),
            (Some("atlas".into()), false)
        );

        conn.execute(
            "INSERT INTO credentials_meta (id, kind, label, scope, has_secret)
             VALUES ('atlas-duplicate', 'password', 'sudo · Atlas API', 'sudo', 1)",
            [],
        )
        .unwrap();
        let eligible = sudo_password_credentials(&conn).unwrap();
        assert_eq!(resolve_sudo_password_credential(&host, &eligible).0, None);
    }

    #[test]
    fn credencial_sudo_exige_host_exato_e_usuario_compativel() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO credentials_meta (id, kind, label, scope, has_secret) VALUES
                ('ip-long', 'password', 'sudo · deploy@10.4.2.18', 'sudo', 1),
                ('api-prod', 'password', 'sudo · deploy@api-prod', 'sudo', 1),
                ('wrong-user', 'password', 'sudo · root@api', 'sudo', 1);",
        )
        .unwrap();
        let mut host = Host {
            id: "host-api".into(),
            name: "api".into(),
            group: String::new(),
            user: Some("deploy".into()),
            host: "10.4.2.1".into(),
            port: None,
            credential_ref: None,
            vpn_profile: None,
            auto_reconnect: true,
            auto_install_tmux: false,
            auto_attach: true,
            project_dir: None,
            startup_mode: "shell".into(),
        };

        let eligible = sudo_password_credentials(&conn).unwrap();
        assert_eq!(resolve_sudo_password_credential(&host, &eligible).0, None);

        host.host = "api".into();
        host.name = "API".into();
        assert_eq!(resolve_sudo_password_credential(&host, &eligible).0, None);

        conn.execute(
            "INSERT INTO credentials_meta (id, kind, label, scope, has_secret)
             VALUES ('api-exact', 'password', 'sudo · deploy@ApI', 'sudo', 1)",
            [],
        )
        .unwrap();
        let eligible = sudo_password_credentials(&conn).unwrap();
        assert_eq!(
            resolve_sudo_password_credential(&host, &eligible).0,
            Some("api-exact".into())
        );
    }

    #[test]
    fn credencial_sudo_aceita_hostname_do_label_sem_porta() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO credentials_meta (id, kind, label, scope, has_secret) VALUES
                ('api-port', 'password', 'sudo · deploy@api:22', 'sudo', 1);",
        )
        .unwrap();
        let mut host = Host {
            id: "host-api".into(),
            name: "produção".into(),
            group: String::new(),
            user: Some("deploy".into()),
            host: "api".into(),
            port: Some(22),
            credential_ref: None,
            vpn_profile: None,
            auto_reconnect: true,
            auto_install_tmux: false,
            auto_attach: true,
            project_dir: None,
            startup_mode: "shell".into(),
        };

        let eligible = sudo_password_credentials(&conn).unwrap();
        assert_eq!(
            resolve_sudo_password_credential(&host, &eligible),
            (Some("api-port".into()), false)
        );

        host.host = "other".into();
        assert_eq!(resolve_sudo_password_credential(&host, &eligible), (None, true));
    }

    #[test]
    fn alias_sem_usuario_nao_casa_com_label_que_especifica_usuario() {
        let eligible = vec![
            ("root".into(), "sudo · root@produção".into()),
            ("host-only".into(), "sudo · produção".into()),
        ];
        let mut host = Host {
            id: "host-production".into(),
            name: "Produção".into(),
            group: String::new(),
            user: None,
            host: "prod-alias".into(),
            port: None,
            credential_ref: None,
            vpn_profile: None,
            auto_reconnect: true,
            auto_install_tmux: false,
            auto_attach: true,
            project_dir: None,
            startup_mode: "shell".into(),
        };

        assert_eq!(
            matching_sudo_password_credentials(&host, &eligible),
            vec!["host-only".to_string()]
        );

        host.user = Some("  ".into());
        assert_eq!(
            matching_sudo_password_credentials(&host, &eligible),
            vec!["host-only".to_string()]
        );
    }

    #[test]
    fn credencial_ref_sudo_e_o_fast_path_mesmo_com_label_nao_correspondente() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO credentials_meta (id, kind, label, scope, has_secret)
             VALUES ('linked', 'password', 'rótulo legado', 'ssh · sudo', 1)",
            [],
        )
        .unwrap();
        let host = Host {
            id: "host-linked".into(),
            name: "API".into(),
            group: String::new(),
            user: Some("deploy".into()),
            host: "api.internal".into(),
            port: None,
            credential_ref: Some("linked".into()),
            vpn_profile: None,
            auto_reconnect: true,
            auto_install_tmux: false,
            auto_attach: true,
            project_dir: None,
            startup_mode: "shell".into(),
        };

        let eligible = sudo_password_credentials(&conn).unwrap();
        assert_eq!(
            resolve_sudo_password_credential(&host, &eligible),
            (Some("linked".into()), false)
        );
    }
}
