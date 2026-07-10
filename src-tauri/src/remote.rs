//! Operações remotas fora do PTY: teste de conexão, detecção de ambiente e
//! instalação de tmux com sudo.
//!
//! SEGURANÇA: a senha do sudo trafega EXCLUSIVAMENTE pelo stdin do ssh
//! (`sudo -S -p ''`) — nunca em argv (visível no ps), history ou logs.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Instant;
use tauri::State;

use crate::db::{self, Db};
use crate::vault::{self, Vault};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostDraft {
    pub user: Option<String>,
    pub host: String,
    pub port: Option<u16>,
}

fn target_of(user: &Option<String>, host: &str) -> Result<String, String> {
    if host.is_empty() || host.starts_with('-') {
        return Err(format!("host inválido: {host:?}"));
    }
    if let Some(user) = user {
        if user.starts_with('-') {
            return Err(format!("user inválido: {user:?}"));
        }
    }
    Ok(match user {
        Some(user) if !user.is_empty() => format!("{user}@{host}"),
        _ => host.to_string(),
    })
}

/// Caminho do socket de multiplexação para um alvo. Curto (limite ~104
/// chars para sockets unix no macOS) e estável por alvo+porta.
fn control_path(target: &str, port: Option<u16>) -> std::path::PathBuf {
    let mut hash: u64 = 1469598103934665603; // FNV-1a
    for b in target.bytes().chain(port.unwrap_or(22).to_le_bytes()) {
        hash ^= b as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    std::env::temp_dir().join(format!("helm-ssh-{hash:016x}"))
}

/// ssh não-interativo (sem PTY): chaves/agent/config apenas.
///
/// Usa ControlMaster: a 1ª conexão a um host abre um master persistente e as
/// seguintes (detectar → instalar → verificar) o reaproveitam — a
/// autenticação (chave/Touch ID) acontece UMA vez por fluxo, não por comando.
fn ssh_command(target: &str, port: Option<u16>, remote_cmd: &str) -> Command {
    let mut cmd = Command::new("ssh");
    cmd.arg("-T")
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("ConnectTimeout=10")
        .arg("-o")
        .arg("ControlMaster=auto")
        .arg("-o")
        .arg(format!("ControlPath={}", control_path(target, port).display()))
        .arg("-o")
        .arg("ControlPersist=45");
    if let Some(port) = port {
        cmd.arg("-p").arg(port.to_string());
    }
    cmd.arg("--").arg(target).arg(remote_cmd);
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResult {
    pub ok: bool,
    pub latency_ms: u64,
    pub tmux: Option<String>,
    pub message: Option<String>,
}

#[tauri::command]
pub async fn test_connection(draft: HostDraft) -> Result<TestResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let target = target_of(&draft.user, &draft.host)?;
        let start = Instant::now();
        let output = ssh_command(&target, draft.port, "echo __HELM_OK__; tmux -V 2>/dev/null || true")
            .output()
            .map_err(|e| e.to_string())?;
        let latency_ms = start.elapsed().as_millis() as u64;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let ok = stdout.contains("__HELM_OK__");
        let tmux = stdout
            .lines()
            .find(|l| l.starts_with("tmux "))
            .map(|l| l.trim().to_string());
        let message = if ok {
            None
        } else {
            let err = String::from_utf8_lossy(&output.stderr);
            Some(err.lines().last().unwrap_or("conexão falhou").trim().to_string())
        };
        Ok(TestResult { ok, latency_ms, tmux, message })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteInfo {
    pub os: Option<String>,
    pub pkg_manager: Option<String>,
    pub tmux: Option<String>,
}

const DETECT_SCRIPT: &str = r#". /etc/os-release 2>/dev/null; echo "OS=${PRETTY_NAME:-$(uname -s)}"; tmux -V 2>/dev/null | sed 's/^/TMUX=/'; for pm in apt-get dnf yum pacman apk zypper; do command -v $pm >/dev/null 2>&1 && { echo "PM=$pm"; break; }; done"#;

#[tauri::command]
pub async fn detect_remote(db: State<'_, Db>, host_id: String) -> Result<RemoteInfo, String> {
    let host = db::get_host(&db, &host_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let target = target_of(&host.user, &host.host)?;
        let output = ssh_command(&target, host.port, DETECT_SCRIPT)
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() && output.stdout.is_empty() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let field = |k: &str| {
            stdout
                .lines()
                .find_map(|l| l.strip_prefix(k).map(|v| v.trim().to_string()))
                .filter(|v| !v.is_empty())
        };
        Ok(RemoteInfo {
            os: field("OS="),
            pkg_manager: field("PM="),
            tmux: field("TMUX="),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

fn install_command(pkg_manager: &str) -> Option<&'static str> {
    Some(match pkg_manager {
        "apt-get" => "apt-get install -y tmux",
        "dnf" => "dnf install -y tmux",
        "yum" => "yum install -y tmux",
        "pacman" => "pacman -S --noconfirm tmux",
        "apk" => "apk add tmux",
        "zypper" => "zypper --non-interactive install tmux",
        _ => return None,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallAuth {
    /// credencial do Vault (senha ou NOPASSWD)
    pub credential_id: Option<String>,
    /// senha digitada agora — usada uma vez, nunca salva
    pub password: Option<String>,
}

#[tauri::command]
pub async fn install_tmux(
    db: State<'_, Db>,
    vault: State<'_, Vault>,
    host_id: String,
    pkg_manager: String,
    auth: InstallAuth,
) -> Result<String, String> {
    let host = db::get_host(&db, &host_id)?;
    let install = install_command(&pkg_manager)
        .ok_or_else(|| format!("package manager não suportado: {pkg_manager}"))?;

    // resolve a senha ANTES do spawn_blocking (State não atravessa threads)
    let password: Option<String> = match (&auth.credential_id, &auth.password) {
        (Some(cred_id), _) => {
            // NOPASSWD (só-metadados) → sem senha; senão busca no keyring
            match vault::get_secret(&db, &vault, cred_id) {
                Ok(secret) => Some(secret),
                Err(e) if e.contains("No matching entry") => None,
                Err(e) => return Err(e),
            }
        }
        (None, Some(pwd)) if !pwd.is_empty() => Some(pwd.clone()),
        _ => None,
    };

    tauri::async_runtime::spawn_blocking(move || {
        let target = target_of(&host.user, &host.host)?;
        // -S: senha via stdin; -p '': sem prompt; NOPASSWD usa -n (nunca pede).
        // instala E confirma no MESMO comando (uma conexão só) — a versão vai
        // numa linha marcada para separar do ruído do apt.
        let sudo = if password.is_some() {
            format!("sudo -S -p '' {install}")
        } else {
            format!("sudo -n {install}")
        };
        let remote_cmd = format!("{sudo} 2>&1 && printf '__HELM_TMUX__'; tmux -V 2>/dev/null");
        let mut child = ssh_command(&target, host.port, &remote_cmd)
            .spawn()
            .map_err(|e| e.to_string())?;

        if let Some(pwd) = &password {
            let mut stdin = child.stdin.take().ok_or("stdin indisponível")?;
            // senha SÓ aqui: pipe direto ao sudo -S remoto
            stdin.write_all(pwd.as_bytes()).map_err(|e| e.to_string())?;
            stdin.write_all(b"\n").map_err(|e| e.to_string())?;
            drop(stdin);
        }

        let output = child.wait_with_output().map_err(|e| e.to_string())?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let version = stdout
            .split("__HELM_TMUX__")
            .nth(1)
            .map(|v| v.trim().to_string())
            .filter(|v| v.starts_with("tmux"));
        match version {
            Some(v) => Ok(v),
            None => {
                let last = stdout.lines().rev().find(|l| !l.trim().is_empty());
                Err(last.unwrap_or("instalação falhou").trim().to_string())
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
