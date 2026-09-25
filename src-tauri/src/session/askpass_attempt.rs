//! Tentativa one-shot de askpass: arquivo 0600 com a senha SSH do Vault,
//! consumido (e apagado) pelo próprio binário do Helm em modo helper
//! (`HELM_ASKPASS_MODE=1`, ver `main.rs` e `askpass::read_secret_file`).
//!
//! Compartilhado entre o PTY das sessões (`manager.rs`) e os comandos ssh
//! auxiliares (`remote.rs`: teste, detecção, instalação do tmux).
//!
//! Tempo de vida do segredo em disco, em ordem de preferência:
//! 1. o helper apaga o arquivo ao ler (fluxo normal);
//! 2. um timer apaga o arquivo `ASKPASS_EXPIRY` após o spawn (fim da fase de
//!    autenticação — o ssh nunca pede a senha depois disso);
//! 3. o `Drop` apaga ao fim da tentativa;
//! 4. o `setup` do app apaga sobras de execuções que morreram (crash/kill).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Manager};

/// Janela da fase de autenticação: após isso o arquivo é apagado mesmo que o
/// helper nunca tenha sido chamado (ex.: autenticou por chave).
pub(crate) const ASKPASS_EXPIRY: Duration = Duration::from_secs(20);
/// Sobras mais velhas que isso são de execuções anteriores (o timer acima já
/// as teria apagado). A idade mínima protege uma 2ª instância do app (dev +
/// bundle compartilham o app_data_dir) com autenticação em andamento.
const STALE_AFTER: Duration = Duration::from_secs(60);
const SECRET_PREFIX: &str = "secret-";

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) struct AskpassAttempt {
    pub(crate) helper: PathBuf,
    pub(crate) secret_file: PathBuf,
    /// true quando o timer (não o helper) apagou o arquivo.
    expired: Arc<AtomicBool>,
}

impl AskpassAttempt {
    /// O helper foi invocado e leu o segredo? O helper apaga o arquivo ao
    /// ler; um arquivo removido pelo timer de expiração não conta.
    pub(crate) fn was_consumed(&self) -> bool {
        !self.expired.load(Ordering::SeqCst) && !self.secret_file.exists()
    }

    /// Agenda a remoção do arquivo após `after` — chamar logo após o spawn.
    pub(crate) fn schedule_expiry(&self, after: Duration) {
        let path = self.secret_file.clone();
        let expired = self.expired.clone();
        std::thread::spawn(move || {
            std::thread::sleep(after);
            if path.exists() {
                expired.store(true, Ordering::SeqCst);
                remove_ignoring_not_found(&path);
            }
        });
    }
}

impl Drop for AskpassAttempt {
    fn drop(&mut self) {
        remove_ignoring_not_found(&self.secret_file);
    }
}

/// Variáveis de askpass que nunca devem ser herdadas do processo pai (inclui a
/// variável antiga que carregava o segredo diretamente no ambiente).
pub(crate) const INHERITED_ASKPASS_ENV: &[&str] = &[
    "SSH_ASKPASS",
    "SSH_ASKPASS_REQUIRE",
    "HELM_ASKPASS_MODE",
    "HELM_ASKPASS_SECRET",
    "HELM_ASKPASS_SECRET_FILE",
];

fn remove_ignoring_not_found(path: &Path) {
    if let Err(e) = std::fs::remove_file(path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            eprintln!("[askpass] falha ao remover {}: {e}", path.display());
        }
    }
}

fn session_tag(id: &str) -> u64 {
    let mut hash = 1469598103934665603u64;
    for byte in id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1099511628211);
    }
    hash
}

#[cfg(unix)]
fn ensure_private_askpass_dir(dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    match std::fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = std::fs::symlink_metadata(dir).map_err(|e| e.to_string())?;
            if metadata.file_type().is_dir() && metadata.permissions().mode() & 0o777 == 0o700 {
                Ok(())
            } else {
                Err(format!("diretório askpass inseguro: {}", dir.display()))
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(not(unix))]
fn ensure_private_askpass_dir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir(dir)
        .map_err(|e| e.to_string())
        .or_else(|e| if dir.is_dir() { Ok(()) } else { Err(e) })
}

fn askpass_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("askpass")
}

/// Cria o arquivo de segredo de uma tentativa. `tag` identifica a origem
/// (id da sessão, "remote-<host>") só para diagnóstico; a unicidade vem do
/// pid + contador global.
pub(crate) fn create(app: &AppHandle, tag: &str, secret: &str) -> Result<AskpassAttempt, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    create_in(&app_data_dir, tag, secret)
}

pub(crate) fn create_in(
    app_data_dir: &Path,
    tag: &str,
    secret: &str,
) -> Result<AskpassAttempt, String> {
    let helper = std::env::current_exe().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(app_data_dir).map_err(|e| e.to_string())?;
    let dir = askpass_dir(app_data_dir);
    ensure_private_askpass_dir(&dir)?;
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let secret_file = dir.join(format!(
        "{SECRET_PREFIX}{:016x}-{}-{counter}",
        session_tag(tag),
        std::process::id()
    ));

    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&secret_file)
            .map_err(|e| e.to_string())?
    };
    #[cfg(not(unix))]
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&secret_file)
        .map_err(|e| e.to_string())?;

    #[cfg(unix)]
    if let Err(e) = file.set_permissions({
        use std::os::unix::fs::PermissionsExt;
        std::fs::Permissions::from_mode(0o600)
    }) {
        drop(file);
        let _ = std::fs::remove_file(&secret_file);
        return Err(e.to_string());
    }

    if let Err(e) = file.write_all(secret.as_bytes()).and_then(|_| file.flush()) {
        drop(file);
        let _ = std::fs::remove_file(&secret_file);
        return Err(e.to_string());
    }

    Ok(AskpassAttempt {
        helper,
        secret_file,
        expired: Arc::new(AtomicBool::new(false)),
    })
}

/// Apaga arquivos `secret-*` deixados por execuções anteriores (crash/kill
/// antes do Drop). Chamado no `setup` do app. Retorna quantos removeu.
pub(crate) fn wipe_stale(app_data_dir: &Path) -> usize {
    wipe_stale_older_than(&askpass_dir(app_data_dir), STALE_AFTER)
}

fn wipe_stale_older_than(dir: &Path, min_age: Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(SECRET_PREFIX)
        {
            continue;
        }
        let old_enough = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_none_or(|age| age >= min_age);
        if old_enough && std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::{create_in, wipe_stale_older_than};
    use std::time::Duration;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "helm-askpass-attempt-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn consumo_pelo_helper_e_detectado() {
        let base = scratch("consumo");
        let attempt = create_in(&base, "s1", "s3cr3t").unwrap();
        assert!(!attempt.was_consumed());
        let secret = crate::askpass::read_secret_file(&attempt.secret_file).unwrap();
        assert_eq!(secret.as_str(), "s3cr3t");
        assert!(attempt.was_consumed());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn expiracao_pelo_timer_nao_conta_como_consumo() {
        let base = scratch("expira");
        let attempt = create_in(&base, "s1", "s3cr3t").unwrap();
        attempt.schedule_expiry(Duration::from_millis(10));
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while attempt.secret_file.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!attempt.secret_file.exists());
        assert!(!attempt.was_consumed());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn drop_apaga_o_arquivo() {
        let base = scratch("drop");
        let attempt = create_in(&base, "s1", "s3cr3t").unwrap();
        let path = attempt.secret_file.clone();
        assert!(path.exists());
        drop(attempt);
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn wipe_remove_apenas_segredos() {
        let dir = scratch("wipe");
        std::fs::write(dir.join("secret-abc-1-0"), "x").unwrap();
        std::fs::write(dir.join("outro"), "x").unwrap();
        assert_eq!(wipe_stale_older_than(&dir, Duration::ZERO), 1);
        assert!(!dir.join("secret-abc-1-0").exists());
        assert!(dir.join("outro").exists());
        // arquivo recente de outra instância é preservado
        std::fs::write(dir.join("secret-def-2-0"), "x").unwrap();
        assert_eq!(wipe_stale_older_than(&dir, Duration::from_secs(3600)), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
