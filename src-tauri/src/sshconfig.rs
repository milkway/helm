//! Parser de ~/.ssh/config para importar hosts (design 3a).
//! Extrai Host/HostName/User/Port; ignora wildcards e o Host "*".

use serde::Serialize;
use tauri::State;

use crate::db::{Db, Host};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedHost {
    pub alias: String,
    pub host_name: String,
    pub user: Option<String>,
    pub port: Option<u16>,
}

fn parse(content: &str) -> Vec<ParsedHost> {
    let mut hosts: Vec<ParsedHost> = Vec::new();
    let mut current: Option<ParsedHost> = None;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = match line.split_once(|c: char| c.is_whitespace() || c == '=') {
            Some((k, v)) => (k.trim().to_lowercase(), v.trim().trim_start_matches('=').trim()),
            None => continue,
        };
        match key.as_str() {
            "host" => {
                if let Some(h) = current.take() {
                    if !h.host_name.is_empty() {
                        hosts.push(h);
                    }
                }
                // primeiro alias não-wildcard do bloco
                let alias = value
                    .split_whitespace()
                    .find(|a| !a.contains('*') && !a.contains('?'))
                    .unwrap_or("")
                    .to_string();
                current = if alias.is_empty() {
                    None
                } else {
                    Some(ParsedHost {
                        alias: alias.clone(),
                        host_name: alias, // default: HostName = alias se não vier
                        user: None,
                        port: None,
                    })
                };
            }
            "hostname" => {
                if let Some(h) = current.as_mut() {
                    h.host_name = value.to_string();
                }
            }
            "user" => {
                if let Some(h) = current.as_mut() {
                    h.user = Some(value.to_string());
                }
            }
            "port" => {
                if let Some(h) = current.as_mut() {
                    h.port = value.parse().ok();
                }
            }
            _ => {}
        }
    }
    if let Some(h) = current.take() {
        if !h.host_name.is_empty() {
            hosts.push(h);
        }
    }
    hosts
}

/// Lê o ~/.ssh/config e insere no banco os hosts ainda não cadastrados
/// (evita duplicar pelo endereço). Retorna quantos foram importados.
#[tauri::command]
pub fn import_ssh_config(db: State<'_, Db>) -> Result<u32, String> {
    let home = std::env::var_os("HOME").ok_or("HOME indefinido")?;
    let path = std::path::Path::new(&home).join(".ssh/config");
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("não foi possível ler {}: {e}", path.display()))?;

    let parsed = parse(&content);
    let conn = db.0.lock().unwrap();

    // endereços já cadastrados (para não duplicar)
    let existing: std::collections::HashSet<String> = {
        let mut stmt = conn.prepare("SELECT host FROM hosts").map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        rows.filter_map(Result::ok).collect()
    };

    let mut imported = 0u32;
    for p in parsed {
        if p.host_name.starts_with('-') || existing.contains(&p.host_name) {
            continue;
        }
        let host = Host {
            id: uuid_like(&p.alias, imported),
            name: p.alias.clone(),
            group: "SSH config".into(),
            user: p.user,
            host: p.host_name,
            port: p.port,
            credential_ref: None,
            vpn_profile: None,
            auto_reconnect: true,
            auto_install_tmux: false,
            auto_attach: false,
            project_dir: None,
            startup_mode: "shell".into(),
        };
        conn.execute(
            "INSERT INTO hosts (id, name, \"group\", user, host, port, credential_ref, vpn_profile,
                                auto_reconnect, auto_install_tmux, auto_attach, project_dir, startup_mode)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            rusqlite::params![
                host.id, host.name, host.group, host.user, host.host, host.port,
                host.credential_ref, host.vpn_profile, host.auto_reconnect,
                host.auto_install_tmux, host.auto_attach, host.project_dir, host.startup_mode,
            ],
        )
        .map_err(|e| e.to_string())?;
        imported += 1;
    }
    Ok(imported)
}

/// id estável e único o suficiente sem depender de crate de uuid/random.
fn uuid_like(alias: &str, seq: u32) -> String {
    format!("sshcfg-{}-{seq}", alias.replace(|c: char| !c.is_ascii_alphanumeric(), "-"))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub build: String,
    pub arch: String,
    pub os: String,
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        build: option_env!("HELM_BUILD").unwrap_or("dev").to_string(),
        arch: std::env::consts::ARCH.to_string(),
        os: std::env::consts::OS.to_string(),
    }
}
