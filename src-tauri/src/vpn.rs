//! Integração VPN (design 4a): macOS via Tunnelblick (AppleScript), Linux via
//! nmcli. Auto-conecta antes do SSH quando o host exige um perfil e
//! auto-desconecta quando o último host que o usa detacha (refcount).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

pub struct Vpn {
    /// quantos hosts vivos usam cada perfil
    refs: Mutex<HashMap<String, u32>>,
    /// auto-desconectar quando o refcount zera
    auto_disconnect: AtomicBool,
}

impl Default for Vpn {
    fn default() -> Self {
        Vpn {
            refs: Mutex::new(HashMap::new()),
            auto_disconnect: AtomicBool::new(true),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VpnProfile {
    pub name: String,
    /// connected | disconnected | connecting
    pub state: String,
    pub hosts_using: u32,
}

fn emit_status(app: &AppHandle, profiles: &[VpnProfile]) {
    let _ = app.emit("vpn-status", profiles);
}

// ── Backend macOS: Tunnelblick via osascript ─────────────────────────────

#[cfg(target_os = "macos")]
fn run_osascript(script: &str) -> Result<String, String> {
    let out = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(target_os = "macos")]
fn backend_list() -> Result<Vec<(String, String)>, String> {
    let names = run_osascript("tell application \"Tunnelblick\" to get name of configurations")?;
    let mut result = Vec::new();
    for name in names.split(", ") {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let state = backend_state(name).unwrap_or_else(|_| "disconnected".into());
        result.push((name.to_string(), state));
    }
    Ok(result)
}

#[cfg(target_os = "macos")]
fn backend_state(profile: &str) -> Result<String, String> {
    // Tunnelblick: EXITING = desconectado; CONNECTED = conectado; resto = em progresso
    let script = format!(
        "tell application \"Tunnelblick\" to get state of first configuration whose name is \"{}\"",
        profile.replace('"', "")
    );
    let raw = run_osascript(&script)?;
    Ok(normalize_state(&raw))
}

#[cfg(target_os = "macos")]
fn backend_connect(profile: &str) -> Result<(), String> {
    run_osascript(&format!(
        "tell application \"Tunnelblick\" to connect \"{}\"",
        profile.replace('"', "")
    ))
    .map(|_| ())
}

#[cfg(target_os = "macos")]
fn backend_disconnect(profile: &str) -> Result<(), String> {
    run_osascript(&format!(
        "tell application \"Tunnelblick\" to disconnect \"{}\"",
        profile.replace('"', "")
    ))
    .map(|_| ())
}

#[cfg(target_os = "macos")]
fn normalize_state(raw: &str) -> String {
    match raw.trim().to_uppercase().as_str() {
        "CONNECTED" => "connected".into(),
        "EXITING" => "disconnected".into(),
        _ => "connecting".into(),
    }
}

// ── Backend Linux: nmcli ─────────────────────────────────────────────────

#[cfg(not(target_os = "macos"))]
fn backend_list() -> Result<Vec<(String, String)>, String> {
    let out = std::process::Command::new("nmcli")
        .args(["-t", "-f", "NAME,TYPE,STATE", "connection", "show"])
        .output()
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut result = Vec::new();
    for line in text.lines() {
        let cols: Vec<&str> = line.split(':').collect();
        if cols.len() >= 2 && cols[1] == "vpn" {
            let state = if cols.get(2).is_some_and(|s| *s == "activated") {
                "connected"
            } else {
                "disconnected"
            };
            result.push((cols[0].to_string(), state.to_string()));
        }
    }
    Ok(result)
}

#[cfg(not(target_os = "macos"))]
fn backend_state(profile: &str) -> Result<String, String> {
    Ok(backend_list()?
        .into_iter()
        .find(|(n, _)| n == profile)
        .map(|(_, s)| s)
        .unwrap_or_else(|| "disconnected".into()))
}

#[cfg(not(target_os = "macos"))]
fn backend_connect(profile: &str) -> Result<(), String> {
    let out = std::process::Command::new("nmcli")
        .args(["connection", "up", profile])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[cfg(not(target_os = "macos"))]
fn backend_disconnect(profile: &str) -> Result<(), String> {
    let out = std::process::Command::new("nmcli")
        .args(["connection", "down", profile])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

// ── Lógica comum ─────────────────────────────────────────────────────────

fn snapshot(vpn: &Vpn) -> Vec<VpnProfile> {
    let refs = vpn.refs.lock().unwrap().clone();
    backend_list()
        .unwrap_or_default()
        .into_iter()
        .map(|(name, state)| VpnProfile {
            hosts_using: *refs.get(&name).unwrap_or(&0),
            name,
            state,
        })
        .collect()
}

/// Garante que o perfil esteja conectado (bloqueante, com poll até CONNECTED).
/// Chamado pelo manager antes do SSH. Registra o uso (refcount++).
pub fn acquire(app: &AppHandle, vpn: &Vpn, profile: &str) -> Result<(), String> {
    *vpn.refs.lock().unwrap().entry(profile.to_string()).or_insert(0) += 1;

    if backend_state(profile)? == "connected" {
        emit_status(app, &snapshot(vpn));
        return Ok(());
    }
    backend_connect(profile)?;
    emit_status(app, &snapshot(vpn));

    let deadline = Instant::now() + Duration::from_secs(45);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(600));
        if backend_state(profile)? == "connected" {
            emit_status(app, &snapshot(vpn));
            return Ok(());
        }
    }
    Err(format!("VPN '{profile}' não conectou em 45s"))
}

/// Libera o uso do perfil (refcount--); desconecta se zerou e auto está ligado.
pub fn release(app: &AppHandle, vpn: &Vpn, profile: &str) {
    let now = {
        let mut refs = vpn.refs.lock().unwrap();
        if let Some(c) = refs.get_mut(profile) {
            *c = c.saturating_sub(1);
            *c
        } else {
            0
        }
    };
    if now == 0 && vpn.auto_disconnect.load(Ordering::Relaxed) {
        // último host que usava o perfil fechou → desconecta
        let _ = backend_disconnect(profile);
    }
    emit_status(app, &snapshot(vpn));
}

// ── Commands ─────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn vpn_list(vpn: State<'_, Vpn>) -> Result<Vec<VpnProfile>, String> {
    let refs = vpn.refs.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        backend_list()
            .unwrap_or_default()
            .into_iter()
            .map(|(name, state)| VpnProfile {
                hosts_using: *refs.get(&name).unwrap_or(&0),
                name,
                state,
            })
            .collect()
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn vpn_connect(app: AppHandle, profile: String) -> Result<(), String> {
    let p = profile.clone();
    tauri::async_runtime::spawn_blocking(move || backend_connect(&p))
        .await
        .map_err(|e| e.to_string())??;
    if let Some(vpn) = app.try_state::<Vpn>() {
        emit_status(&app, &snapshot(&vpn));
    }
    Ok(())
}

#[tauri::command]
pub async fn vpn_disconnect(app: AppHandle, profile: String) -> Result<(), String> {
    let p = profile.clone();
    tauri::async_runtime::spawn_blocking(move || backend_disconnect(&p))
        .await
        .map_err(|e| e.to_string())??;
    if let Some(vpn) = app.try_state::<Vpn>() {
        emit_status(&app, &snapshot(&vpn));
    }
    Ok(())
}

#[tauri::command]
pub fn vpn_set_auto_disconnect(vpn: State<'_, Vpn>, enabled: bool) {
    vpn.auto_disconnect.store(enabled, Ordering::Relaxed);
}

#[tauri::command]
pub fn vpn_get_auto_disconnect(vpn: State<'_, Vpn>) -> bool {
    vpn.auto_disconnect.load(Ordering::Relaxed)
}
