//! Cofre de credenciais.
//!
//! Segredos vivem SÓ no Keychain (macOS) / Secret Service (Linux) via crate
//! `keyring`; o SQLite guarda apenas metadados (credentials_meta). Destravar
//! e revelar exigem autenticação do dono (Touch ID com fallback de senha no
//! macOS via LocalAuthentication). Auto-lock após 15 min sem atividade.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, State};

use crate::db::Db;

const KEYRING_SERVICE: &str = if cfg!(debug_assertions) {
    "io.github.milkway.helm.dev"
} else {
    "io.github.milkway.helm"
};
const AUTO_LOCK: Duration = Duration::from_secs(15 * 60);

pub struct Vault(pub Arc<VaultInner>);

pub struct VaultInner {
    unlocked: AtomicBool,
    last_activity: Mutex<Instant>,
}

impl Default for Vault {
    fn default() -> Self {
        Vault(Arc::new(VaultInner {
            unlocked: AtomicBool::new(false),
            last_activity: Mutex::new(Instant::now()),
        }))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredMeta {
    pub id: String,
    /// ssh_key (passphrase de chave) | password (ssh/sudo)
    pub kind: String,
    pub label: String,
    pub algo: Option<String>,
    pub scope: Option<String>,
    pub last_used: Option<String>,
    /// false = só metadados (NOPASSWD, chave sem passphrase) — nada no keyring
    #[serde(default = "default_true")]
    pub has_secret: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    pub locked: bool,
    pub count: i64,
}

fn emit_vault_status(app: &AppHandle, locked: bool, count: i64) {
    let _ = app.emit("vault-status", VaultStatus { locked, count });
}

/// Distingue "biometria indisponível" (típico no binário de dev não
/// empacotado) de "usuário recusou/cancelou". Só o caminho macOS usa.
#[cfg(target_os = "macos")]
enum AuthError {
    Unavailable(String),
    Denied(String),
}

/// Autenticação do dono do dispositivo (Touch ID com fallback de senha).
///
/// A UI do LocalAuthentication PRECISA ser apresentada pela main thread do
/// macOS — chamar `evaluatePolicy` de uma thread de background não mostra o
/// prompt. Agendamos a chamada na main thread e esperamos o reply por canal.
#[cfg(target_os = "macos")]
fn evaluate_biometrics(app: &AppHandle, reason: &str) -> Result<(), AuthError> {
    use block2::RcBlock;
    use objc2_foundation::{NSError, NSString};
    use objc2_local_authentication::{LAContext, LAPolicy};
    use std::sync::mpsc;

    let (tx, rx) = mpsc::channel::<Result<(), AuthError>>();
    let reason = reason.to_string();

    app.run_on_main_thread(move || {
        let policy = LAPolicy::DeviceOwnerAuthentication;
        let ctx = unsafe { LAContext::new() };

        if let Err(err) = unsafe { ctx.canEvaluatePolicy_error(policy) } {
            let _ = tx.send(Err(AuthError::Unavailable(err.localizedDescription().to_string())));
            return;
        }

        // clone do ctx capturado no reply → mantém o objeto vivo até o prompt
        // resolver (sem isto o ctx é liberado e o macOS cancela o prompt).
        let ctx_keep = ctx.clone();
        let block = RcBlock::new(move |success: objc2::runtime::Bool, error: *mut NSError| {
            let _ = &ctx_keep;
            let result = if success.as_bool() {
                Ok(())
            } else {
                let msg = if error.is_null() {
                    "autenticação recusada".to_string()
                } else {
                    unsafe { (*error).localizedDescription() }.to_string()
                };
                Err(AuthError::Denied(msg))
            };
            let _ = tx.send(result);
        });

        unsafe {
            ctx.evaluatePolicy_localizedReason_reply(policy, &NSString::from_str(&reason), &block);
        }
    })
    .map_err(|e| AuthError::Unavailable(format!("main thread: {e}")))?;

    rx.recv_timeout(Duration::from_secs(60))
        .map_err(|_| AuthError::Unavailable("sem resposta do sistema".into()))?
}

#[cfg(target_os = "macos")]
fn authenticate_owner(app: &AppHandle, reason: &str) -> Result<(), String> {
    // No binário de `tauri dev` (não empacotado, sem Info.plist) o prompt de
    // Touch ID não sobe e a avaliação trava até dar timeout. Pulamos direto —
    // o gate biométrico real vale no bundle de release (.app assinado).
    if cfg!(debug_assertions) {
        eprintln!("[vault] dev: destravando sem Touch ID (gate real só no bundle de release)");
        return Ok(());
    }
    match evaluate_biometrics(app, reason) {
        Ok(()) => Ok(()),
        Err(AuthError::Unavailable(msg)) => Err(format!("autenticação indisponível: {msg}")),
        Err(AuthError::Denied(msg)) => Err(msg),
    }
}

/// Linux: o Secret Service destrava junto com a sessão do usuário.
#[cfg(not(target_os = "macos"))]
fn authenticate_owner(_app: &AppHandle, _reason: &str) -> Result<(), String> {
    Ok(())
}

fn require_unlocked(vault: &VaultInner) -> Result<(), String> {
    if !vault.unlocked.load(Ordering::Relaxed) {
        return Err("vault bloqueado".into());
    }
    *vault.last_activity.lock().unwrap() = Instant::now();
    Ok(())
}

fn count_creds(db: &State<'_, Db>) -> i64 {
    let conn = db.0.lock().unwrap();
    conn.query_row("SELECT COUNT(*) FROM credentials_meta", [], |r| r.get(0))
        .unwrap_or(0)
}

/// Thread de auto-lock: verifica a cada 30s e bloqueia após 15 min parado.
pub fn spawn_auto_lock(app: AppHandle, vault: Arc<VaultInner>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(30));
        if vault.unlocked.load(Ordering::Relaxed) {
            let idle = vault.last_activity.lock().unwrap().elapsed();
            if idle >= AUTO_LOCK {
                vault.unlocked.store(false, Ordering::Relaxed);
                eprintln!("[vault] auto-lock após {} min inativo", idle.as_secs() / 60);
                let _ = app.emit(
                    "vault-status",
                    VaultStatus { locked: true, count: -1 },
                );
            }
        }
    });
}

#[tauri::command]
pub fn vault_status(db: State<'_, Db>, vault: State<'_, Vault>) -> VaultStatus {
    VaultStatus {
        locked: !vault.0.unlocked.load(Ordering::Relaxed),
        count: count_creds(&db),
    }
}

#[tauri::command]
pub async fn vault_unlock(
    app: AppHandle,
    db: State<'_, Db>,
    vault: State<'_, Vault>,
) -> Result<VaultStatus, String> {
    let inner = vault.0.clone();
    let app_auth = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        authenticate_owner(&app_auth, "destravar o cofre do Helm")?;
        inner.unlocked.store(true, Ordering::Relaxed);
        *inner.last_activity.lock().unwrap() = Instant::now();
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| e.to_string())??;

    let status = VaultStatus {
        locked: false,
        count: count_creds(&db),
    };
    emit_vault_status(&app, false, status.count);
    Ok(status)
}

#[tauri::command]
pub fn vault_lock(app: AppHandle, db: State<'_, Db>, vault: State<'_, Vault>) -> VaultStatus {
    vault.0.unlocked.store(false, Ordering::Relaxed);
    let count = count_creds(&db);
    emit_vault_status(&app, true, count);
    VaultStatus { locked: true, count }
}

#[tauri::command]
pub fn vault_list(db: State<'_, Db>, vault: State<'_, Vault>) -> Result<Vec<CredMeta>, String> {
    require_unlocked(&vault.0)?;
    let conn = db.0.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT id, kind, label, algo, scope, last_used, has_secret FROM credentials_meta ORDER BY kind, label")
        .map_err(|e| e.to_string())?;
    let creds = stmt
        .query_map([], |row| {
            Ok(CredMeta {
                id: row.get(0)?,
                kind: row.get(1)?,
                label: row.get(2)?,
                algo: row.get(3)?,
                scope: row.get(4)?,
                last_used: row.get(5)?,
                has_secret: row.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(creds)
}

#[tauri::command]
pub fn vault_save(
    app: AppHandle,
    db: State<'_, Db>,
    vault: State<'_, Vault>,
    meta: CredMeta,
    secret: String,
) -> Result<(), String> {
    require_unlocked(&vault.0)?;
    if meta.id.is_empty() || meta.label.is_empty() {
        return Err("credencial incompleta".into());
    }
    let has_secret = !secret.is_empty();
    let entry = keyring::Entry::new(KEYRING_SERVICE, &meta.id).map_err(|e| e.to_string())?;
    if has_secret {
        // segredo SÓ no keyring
        entry.set_password(&secret).map_err(|e| e.to_string())?;
    } else {
        // credencial só-metadados (NOPASSWD / chave sem passphrase)
        let _ = entry.delete_credential();
    }

    let conn = db.0.lock().unwrap();
    conn.execute(
        "INSERT INTO credentials_meta (id, kind, label, algo, scope, last_used, has_secret)
         VALUES (?1,?2,?3,?4,?5,NULL,?6)
         ON CONFLICT(id) DO UPDATE SET kind=?2, label=?3, algo=?4, scope=?5, has_secret=?6",
        rusqlite::params![meta.id, meta.kind, meta.label, meta.algo, meta.scope, has_secret],
    )
    .map_err(|e| e.to_string())?;
    drop(conn);

    emit_vault_status(&app, false, count_creds(&db));
    Ok(())
}

#[tauri::command]
pub fn vault_delete(
    app: AppHandle,
    db: State<'_, Db>,
    vault: State<'_, Vault>,
    id: String,
) -> Result<(), String> {
    require_unlocked(&vault.0)?;
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, &id) {
        let _ = entry.delete_credential();
    }
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM credentials_meta WHERE id = ?1", [id])
        .map_err(|e| e.to_string())?;
    drop(conn);
    emit_vault_status(&app, false, count_creds(&db));
    Ok(())
}

/// Revela um segredo — re-exige autenticação do dono (design 3c).
#[tauri::command]
pub async fn vault_reveal(
    app: AppHandle,
    db: State<'_, Db>,
    vault: State<'_, Vault>,
    id: String,
) -> Result<String, String> {
    require_unlocked(&vault.0)?;
    let reveal_id = id.clone();
    let secret = tauri::async_runtime::spawn_blocking(move || {
        authenticate_owner(&app, "revelar uma credencial do cofre")?;
        keyring::Entry::new(KEYRING_SERVICE, &reveal_id)
            .map_err(|e| e.to_string())?
            .get_password()
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    let conn = db.0.lock().unwrap();
    let _ = conn.execute(
        "UPDATE credentials_meta SET last_used = datetime('now') WHERE id = ?1",
        [id],
    );
    Ok(secret)
}

/// Busca interna de segredo para uso pelo app (ssh/sudo) — sem prompt extra,
/// mas exige vault destravado e marca o last_used.
#[allow(dead_code)] // usado nas Fases 6/7 (auth de host e sudo por stdin)
pub fn get_secret(db: &State<'_, Db>, vault: &State<'_, Vault>, id: &str) -> Result<String, String> {
    require_unlocked(&vault.0)?;
    let secret = keyring::Entry::new(KEYRING_SERVICE, id)
        .map_err(|e| e.to_string())?
        .get_password()
        .map_err(|e| e.to_string())?;
    let conn = db.0.lock().unwrap();
    let _ = conn.execute(
        "UPDATE credentials_meta SET last_used = datetime('now') WHERE id = ?1",
        [id],
    );
    Ok(secret)
}
