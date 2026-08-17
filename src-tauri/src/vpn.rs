//! Integração VPN (design 4a): macOS via Tunnelblick (AppleScript), Linux via
//! nmcli. Auto-conecta antes do SSH quando o host exige um perfil e
//! auto-desconecta quando o último host que o usa detacha (refcount).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

pub struct Vpn {
    profiles: Mutex<HashMap<String, ProfileUsage>>,
    transition_done: Condvar,
    /// auto-desconectar quando o refcount zera
    auto_disconnect: AtomicBool,
}

#[derive(Clone, Default)]
struct ProfileUsage {
    /// quantos hosts vivos usam o perfil
    refs: u32,
    /// a conexão atual foi iniciada automaticamente pelo Helm
    connected_by_helm: bool,
    /// identifica a transição bloqueante atual; outras operações aguardam
    transitioning: Option<u64>,
    generation: u64,
}

impl Default for Vpn {
    fn default() -> Self {
        Vpn {
            profiles: Mutex::new(HashMap::new()),
            transition_done: Condvar::new(),
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

/// Roda um handler AppleScript passando o perfil como ARGUMENTO (argv), sem
/// interpolar no código-fonte — imune a injeção de AppleScript. `{}` no corpo
/// é substituído por `(item 1 of argv)`.
#[cfg(target_os = "macos")]
fn run_osascript_with_profile(body: &str, profile: &str) -> Result<String, String> {
    let handler = format!(
        "on run argv\n{}\nend run",
        body.replace("{}", "(item 1 of argv)")
    );
    let out = std::process::Command::new("osascript")
        .arg("-e")
        .arg(&handler)
        .arg("--")
        .arg(profile) // valor passa por argv, nunca pelo código
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
        let state = backend_state(name)?;
        result.push((name.to_string(), state));
    }
    Ok(result)
}

#[cfg(target_os = "macos")]
fn backend_state(profile: &str) -> Result<String, String> {
    // Tunnelblick: EXITING = desconectado; CONNECTED = conectado; resto = em progresso
    let raw = run_osascript_with_profile(
        "tell application \"Tunnelblick\" to get state of first configuration whose name is {}",
        profile,
    )?;
    Ok(normalize_state(&raw))
}

#[cfg(target_os = "macos")]
fn backend_connect(profile: &str) -> Result<(), String> {
    run_osascript_with_profile("tell application \"Tunnelblick\" to connect {}", profile).map(|_| ())
}

#[cfg(target_os = "macos")]
fn backend_disconnect(profile: &str) -> Result<(), String> {
    run_osascript_with_profile("tell application \"Tunnelblick\" to disconnect {}", profile)
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
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
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

fn snapshot(vpn: &Vpn) -> Result<Vec<VpnProfile>, String> {
    let refs: HashMap<String, u32> = vpn
        .profiles
        .lock()
        .unwrap()
        .iter()
        .map(|(name, usage)| (name.clone(), usage.refs))
        .collect();
    Ok(backend_list()?
        .into_iter()
        .map(|(name, state)| VpnProfile {
            hosts_using: *refs.get(&name).unwrap_or(&0),
            name,
            state,
        })
        .collect())
}

fn emit_snapshot(app: &AppHandle, vpn: &Vpn) {
    match snapshot(vpn) {
        Ok(profiles) => emit_status(app, &profiles),
        Err(error) => eprintln!("[vpn] falha ao listar perfis: {error}"),
    }
}

/// Garante que o perfil esteja conectado (bloqueante, com poll até CONNECTED).
/// Chamado pelo manager antes do SSH. Registra o uso (refcount++).
pub fn acquire(app: &AppHandle, vpn: &Vpn, profile: &str) -> Result<(), String> {
    let generation = {
        let mut profiles = vpn.profiles.lock().unwrap();
        while profiles
            .get(profile)
            .is_some_and(|usage| usage.transitioning.is_some())
        {
            profiles = vpn.transition_done.wait(profiles).unwrap();
        }
        let usage = profiles.entry(profile.to_string()).or_default();
        usage.generation = usage.generation.wrapping_add(1);
        usage.transitioning = Some(usage.generation);
        usage.generation
    };

    // NÃO registra o uso ainda: se a conexão falhar, o manager faz `return` sem
    // chamar `release`, então o refcount ficaria preso em 1 para sempre.
    // Incrementa só depois de confirmar CONNECTED.
    let mut initiated_connection = false;
    let result = (|| {
        if backend_state(profile)? != "connected" {
            backend_connect(profile)?;
            initiated_connection = true;
            emit_snapshot(app, vpn);

            let deadline = Instant::now() + Duration::from_secs(45);
            loop {
                std::thread::sleep(Duration::from_millis(600));
                if backend_state(profile)? == "connected" {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err(format!("VPN '{profile}' não conectou em 45s"));
                }
            }
        }
        Ok(())
    })();

    {
        let mut profiles = vpn.profiles.lock().unwrap();
        let usage = profiles.entry(profile.to_string()).or_default();
        if usage.transitioning == Some(generation) {
            if initiated_connection {
                usage.connected_by_helm = true;
            }
            if result.is_ok() {
                usage.refs += 1;
            }
            usage.transitioning = None;
        }
    }
    vpn.transition_done.notify_all();
    emit_snapshot(app, vpn);
    result
}

/// Libera o uso do perfil (refcount--); desconecta se zerou e auto está ligado.
pub fn release(app: &AppHandle, vpn: &Vpn, profile: &str) {
    let disconnect_generation = {
        let mut profiles = vpn.profiles.lock().unwrap();
        while profiles
            .get(profile)
            .is_some_and(|usage| usage.transitioning.is_some())
        {
            profiles = vpn.transition_done.wait(profiles).unwrap();
        }

        let usage = profiles.entry(profile.to_string()).or_default();
        let was_in_use = usage.refs > 0;
        usage.refs = usage.refs.saturating_sub(1);
        if was_in_use
            && usage.refs == 0
            && usage.connected_by_helm
            && vpn.auto_disconnect.load(Ordering::Relaxed)
        {
            usage.generation = usage.generation.wrapping_add(1);
            usage.transitioning = Some(usage.generation);
            Some(usage.generation)
        } else {
            None
        }
    };

    if let Some(generation) = disconnect_generation {
        // último host que usava o perfil fechou → desconecta
        let result = backend_disconnect(profile);
        {
            let mut profiles = vpn.profiles.lock().unwrap();
            let usage = profiles.entry(profile.to_string()).or_default();
            if usage.transitioning == Some(generation) {
                if result.is_ok() {
                    usage.connected_by_helm = false;
                }
                usage.transitioning = None;
            }
        }
        vpn.transition_done.notify_all();
        if let Err(error) = result {
            eprintln!("[vpn] falha ao desconectar '{profile}': {error}");
        }
    }
    emit_snapshot(app, vpn);
}

// ── Commands ─────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn vpn_list(vpn: State<'_, Vpn>) -> Result<Vec<VpnProfile>, String> {
    let refs: HashMap<String, u32> = vpn
        .profiles
        .lock()
        .unwrap()
        .iter()
        .map(|(name, usage)| (name.clone(), usage.refs))
        .collect();
    tauri::async_runtime::spawn_blocking(move || -> Result<Vec<VpnProfile>, String> {
        Ok(backend_list()?
            .into_iter()
            .map(|(name, state)| VpnProfile {
                hosts_using: *refs.get(&name).unwrap_or(&0),
                name,
                state,
            })
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn vpn_connect(app: AppHandle, profile: String) -> Result<(), String> {
    let p = profile.clone();
    tauri::async_runtime::spawn_blocking(move || backend_connect(&p))
        .await
        .map_err(|e| e.to_string())??;
    if let Some(vpn) = app.try_state::<Vpn>() {
        emit_snapshot(&app, &vpn);
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
        if let Some(usage) = vpn.profiles.lock().unwrap().get_mut(&profile) {
            usage.connected_by_helm = false;
        }
        emit_snapshot(&app, &vpn);
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
