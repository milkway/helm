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
    /// releases recebidos enquanto outra operação bloqueante está em andamento
    pending_releases: u32,
    /// a conexão atual foi iniciada automaticamente pelo Helm
    connected_by_helm: bool,
    /// identifica a transição bloqueante atual; releases são acumulados
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
    if let Err(error) = app.emit("vpn-status", profiles) {
        eprintln!("[vpn] falha ao emitir status: {error}");
    }
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
    let names = run_osascript(
        "tell application \"Tunnelblick\"\nset configurationNames to name of configurations\nend tell\nset AppleScript's text item delimiters to linefeed\nreturn configurationNames as text",
    )?;
    let mut result = Vec::new();
    for name in names.lines() {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let state = backend_state(name).unwrap_or_else(|error| {
            eprintln!("[vpn] falha ao consultar estado de '{name}': {error}");
            "disconnected".into()
        });
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
        .map(|(name, usage)| {
            (
                name.clone(),
                usage.refs.saturating_sub(usage.pending_releases),
            )
        })
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
    let profiles = snapshot(vpn).unwrap_or_else(|error| {
        eprintln!("[vpn] falha ao listar perfis: {error}");
        vpn.profiles
            .lock()
            .unwrap()
            .iter()
            .map(|(name, usage)| VpnProfile {
                name: name.clone(),
                state: "disconnected".into(),
                hosts_using: usage.refs.saturating_sub(usage.pending_releases),
            })
            .collect()
    });
    emit_status(app, &profiles);
}

#[derive(Clone, Copy)]
enum TransitionBehavior {
    Wait,
    DeferRelease,
}

enum TransitionOutcome {
    Completed(Result<(), String>),
    Deferred,
}

struct TransitionUpdate {
    result: Result<(), String>,
    refs_added: u32,
    refs_released: u32,
    connected_by_helm: Option<bool>,
    check_auto_disconnect: bool,
}

/// Serializa uma mutação por perfil. O mutex fica solto durante o trabalho
/// bloqueante; a geração protege a finalização contra resultados obsoletos.
/// Releases concorrentes nunca esperam: o dono atual os aplica ao finalizar.
fn with_transition(
    vpn: &Vpn,
    profile: &str,
    behavior: TransitionBehavior,
    operation: impl FnOnce(&ProfileUsage) -> TransitionUpdate,
) -> TransitionOutcome {
    let (generation, initial) = {
        let mut profiles = vpn.profiles.lock().unwrap();
        loop {
            let transitioning = profiles
                .get(profile)
                .is_some_and(|usage| usage.transitioning.is_some());
            if !transitioning {
                break;
            }
            match behavior {
                TransitionBehavior::Wait => {
                    profiles = vpn.transition_done.wait(profiles).unwrap();
                }
                TransitionBehavior::DeferRelease => {
                    let usage = profiles.entry(profile.to_string()).or_default();
                    usage.pending_releases = usage.pending_releases.saturating_add(1);
                    return TransitionOutcome::Deferred;
                }
            }
        }

        let usage = profiles.entry(profile.to_string()).or_default();
        usage.generation = usage.generation.wrapping_add(1);
        usage.transitioning = Some(usage.generation);
        (usage.generation, usage.clone())
    };

    let update = operation(&initial);
    let TransitionUpdate {
        result,
        refs_added,
        refs_released,
        connected_by_helm,
        check_auto_disconnect,
    } = update;

    let should_disconnect = {
        let mut profiles = vpn.profiles.lock().unwrap();
        let usage = profiles.entry(profile.to_string()).or_default();
        if usage.transitioning != Some(generation) {
            eprintln!("[vpn] resultado obsoleto da transição de '{profile}' ignorado");
            false
        } else {
            usage.refs = usage.refs.saturating_add(refs_added);
            usage.refs = usage.refs.saturating_sub(refs_released);
            if let Some(connected_by_helm) = connected_by_helm {
                usage.connected_by_helm = connected_by_helm;
            }

            let pending_releases = std::mem::take(&mut usage.pending_releases);
            usage.refs = usage.refs.saturating_sub(pending_releases);
            let should_disconnect = (check_auto_disconnect || pending_releases > 0)
                && usage.refs == 0
                && usage.connected_by_helm
                && vpn.auto_disconnect.load(Ordering::Relaxed);
            if !should_disconnect {
                usage.transitioning = None;
            }
            should_disconnect
        }
    };

    if should_disconnect {
        let disconnect_result = backend_disconnect(profile);
        {
            let mut profiles = vpn.profiles.lock().unwrap();
            let usage = profiles.entry(profile.to_string()).or_default();
            if usage.transitioning == Some(generation) {
                let pending_releases = std::mem::take(&mut usage.pending_releases);
                usage.refs = usage.refs.saturating_sub(pending_releases);
                if disconnect_result.is_ok() {
                    usage.connected_by_helm = false;
                }
                usage.transitioning = None;
            }
        }
        if let Err(error) = disconnect_result {
            eprintln!("[vpn] falha ao desconectar '{profile}': {error}");
        }
    }

    vpn.transition_done.notify_all();
    TransitionOutcome::Completed(result)
}

/// Garante que o perfil esteja conectado (bloqueante, com poll até CONNECTED).
/// Chamado pelo manager antes do SSH. Registra o uso (refcount++).
pub fn acquire(app: &AppHandle, vpn: &Vpn, profile: &str) -> Result<(), String> {
    // NÃO registra o uso ainda: se a conexão falhar, o manager faz `return` sem
    // chamar `release`, então o refcount ficaria preso em 1 para sempre.
    // Incrementa só depois de confirmar CONNECTED.
    let outcome = with_transition(
        vpn,
        profile,
        TransitionBehavior::Wait,
        |_| {
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

            if initiated_connection && result.is_err() {
                if let Err(error) = backend_disconnect(profile) {
                    eprintln!(
                        "[vpn] falha ao desfazer conexão incompleta de '{profile}': {error}"
                    );
                }
            }

            let connected = result.is_ok();
            TransitionUpdate {
                result,
                refs_added: if connected { 1 } else { 0 },
                refs_released: 0,
                connected_by_helm: initiated_connection.then_some(connected),
                check_auto_disconnect: false,
            }
        },
    );
    emit_snapshot(app, vpn);
    match outcome {
        TransitionOutcome::Completed(result) => result,
        TransitionOutcome::Deferred => unreachable!("acquire nunca adia sua transição"),
    }
}

/// Libera o uso do perfil (refcount--); desconecta se zerou e auto está ligado.
pub fn release(app: &AppHandle, vpn: &Vpn, profile: &str) {
    let _ = with_transition(
        vpn,
        profile,
        TransitionBehavior::DeferRelease,
        |initial| TransitionUpdate {
            result: Ok(()),
            refs_added: 0,
            refs_released: 1,
            connected_by_helm: None,
            check_auto_disconnect: initial.refs > 0,
        },
    );
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
        .map(|(name, usage)| {
            (
                name.clone(),
                usage.refs.saturating_sub(usage.pending_releases),
            )
        })
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
    let worker_app = app.clone();
    let p = profile.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let vpn = worker_app
            .try_state::<Vpn>()
            .ok_or_else(|| "estado de VPN indisponível".to_string())?;
        match with_transition(&vpn, &p, TransitionBehavior::Wait, |_| {
            TransitionUpdate {
                result: backend_connect(&p),
                refs_added: 0,
                refs_released: 0,
                connected_by_helm: None,
                check_auto_disconnect: false,
            }
        }) {
            TransitionOutcome::Completed(result) => result,
            TransitionOutcome::Deferred => unreachable!("vpn_connect nunca adia sua transição"),
        }
    })
    .await
    .map_err(|e| e.to_string())??;
    if let Some(vpn) = app.try_state::<Vpn>() {
        emit_snapshot(&app, &vpn);
    }
    Ok(())
}

#[tauri::command]
pub async fn vpn_disconnect(app: AppHandle, profile: String) -> Result<(), String> {
    let worker_app = app.clone();
    let p = profile.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let vpn = worker_app
            .try_state::<Vpn>()
            .ok_or_else(|| "estado de VPN indisponível".to_string())?;
        match with_transition(&vpn, &p, TransitionBehavior::Wait, |_| {
            let result = backend_disconnect(&p);
            let disconnected = result.is_ok();
            TransitionUpdate {
                result,
                refs_added: 0,
                refs_released: 0,
                connected_by_helm: disconnected.then_some(false),
                check_auto_disconnect: false,
            }
        }) {
            TransitionOutcome::Completed(result) => result,
            TransitionOutcome::Deferred => {
                unreachable!("vpn_disconnect nunca adia sua transição")
            }
        }
    })
    .await
    .map_err(|e| e.to_string())??;
    if let Some(vpn) = app.try_state::<Vpn>() {
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
