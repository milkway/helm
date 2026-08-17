use serde::Serialize;
use std::process::Command;

const RELEASES_API_URL: &str = "https://api.github.com/repos/milkway/helm/releases/latest";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub update_available: bool,
}

fn validate_external_url(url: &str) -> Result<(), String> {
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| "URL rejeitada: apenas URLs https:// são permitidas".to_string())?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if !matches!(host, "github.com" | "milkway.github.io" | "doi.org") {
        return Err(format!(
            "URL rejeitada: host {host:?} não está na lista permitida"
        ));
    }
    Ok(())
}

#[tauri::command]
pub fn open_external(url: String) -> Result<(), String> {
    validate_external_url(&url)?;

    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(target_os = "linux")]
    let program = "xdg-open";
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    return Err("abrir URLs externas não é suportado nesta plataforma".to_string());

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let status = Command::new(program)
            .arg(&url)
            .status()
            .map_err(|e| format!("não foi possível executar {program}: {e}"))?;
        if !status.success() {
            return Err(format!(
                "{program} terminou com status {}",
                status.code().map_or_else(|| "desconhecido".to_string(), |c| c.to_string())
            ));
        }
        Ok(())
    }
}

fn semver_tuple(version: &str) -> Result<(u64, u64, u64), String> {
    let stripped = version
        .strip_prefix('v')
        .or_else(|| version.strip_prefix('V'))
        .unwrap_or(version);
    let clean = stripped.split_once('-').map_or(stripped, |(core, _)| core);
    let mut parts = clean.split('.');
    let major = parts
        .next()
        .ok_or_else(|| format!("versão inválida: {version}"))?
        .parse::<u64>()
        .map_err(|_| format!("versão inválida: {version}"))?;
    let minor = parts
        .next()
        .ok_or_else(|| format!("versão inválida: {version}"))?
        .parse::<u64>()
        .map_err(|_| format!("versão inválida: {version}"))?;
    let patch = parts
        .next()
        .ok_or_else(|| format!("versão inválida: {version}"))?
        .parse::<u64>()
        .map_err(|_| format!("versão inválida: {version}"))?;
    if parts.next().is_some() {
        return Err(format!("versão inválida: {version}"));
    }
    Ok((major, minor, patch))
}

fn check_updates_blocking() -> Result<UpdateInfo, String> {
    let output = Command::new("curl")
        .args(["-fsSL", "--max-time", "10", RELEASES_API_URL])
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "não foi possível verificar atualizações: curl não está instalado".to_string()
            } else {
                format!("não foi possível executar curl: {e}")
            }
        })?;

    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let message = match output.status.code() {
            Some(28) => "a verificação de atualizações excedeu 10 segundos".to_string(),
            Some(22) if detail.contains("404") => {
                "o GitHub não encontrou uma release publicada (HTTP 404)".to_string()
            }
            Some(22) => "o GitHub respondeu com erro HTTP (release ausente ou indisponível)".to_string(),
            Some(code) if detail.is_empty() => format!("curl terminou com status {code}"),
            Some(code) => format!("curl terminou com status {code}: {detail}"),
            None => "curl foi interrompido antes de concluir".to_string(),
        };
        return Err(message);
    }

    let payload: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("resposta inválida do GitHub: {e}"))?;
    let latest = payload
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .filter(|tag| !tag.is_empty())
        .ok_or_else(|| "resposta do GitHub sem tag_name".to_string())?
        .to_string();
    let current = env!("CARGO_PKG_VERSION").to_string();
    let update_available = semver_tuple(&latest)? > semver_tuple(&current)?;

    Ok(UpdateInfo {
        current,
        latest,
        update_available,
    })
}

#[tauri::command]
pub async fn check_updates() -> Result<UpdateInfo, String> {
    tauri::async_runtime::spawn_blocking(check_updates_blocking)
        .await
        .map_err(|e| format!("falha interna ao verificar atualizações: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::{semver_tuple, validate_external_url};

    #[test]
    fn valida_allowlist_estrita() {
        assert!(validate_external_url("https://github.com/milkway/helm").is_ok());
        assert!(validate_external_url("http://github.com/milkway/helm").is_err());
        assert!(validate_external_url("https://github.com.evil.test/").is_err());
        assert!(validate_external_url("https://github.com:443/").is_err());
    }

    #[test]
    fn compara_semver_simples() {
        assert_eq!(semver_tuple("v1.2.3").unwrap(), (1, 2, 3));
        assert!(semver_tuple("1.2").is_err());
    }
}
