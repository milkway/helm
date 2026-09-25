//! Operações remotas fora do PTY: teste de conexão, detecção de ambiente e
//! instalação de tmux com sudo.
//!
//! SEGURANÇA: a senha do sudo trafega EXCLUSIVAMENTE pelo stdin do ssh
//! (`sudo -S -p ''`) — nunca em argv (visível no ps), history ou logs.

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use tauri::{AppHandle, State};
use zeroize::Zeroizing;

use crate::db::{self, Db};
use crate::session::askpass_attempt::{self, AskpassAttempt};
use crate::session::manager::resolve_ssh_password;
use crate::vault::{self, Vault};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostDraft {
    pub user: Option<String>,
    pub host: String,
    pub port: Option<u16>,
    /// Credencial do Vault do formulário (opcional): com senha SSH, o teste
    /// autentica via askpass em vez de exigir chave.
    #[serde(default)]
    pub credential_ref: Option<String>,
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

fn fnv1a(seed: &[u8]) -> u64 {
    let mut hash: u64 = 1469598103934665603; // FNV-1a
    for b in seed {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    hash
}

/// Diretório 0700 para os sockets de ControlMaster, fora do `/tmp` mundialmente
/// acessível. Namespaced por usuário (hash do HOME). Se não der para garantir um
/// diretório privado nosso, cai no `temp_dir` (comportamento antigo, sem regressão).
fn control_dir() -> std::path::PathBuf {
    let base = std::env::temp_dir();
    let home = std::env::var_os("HOME").unwrap_or_default();
    let dir = base.join(format!("helm-{:016x}", fnv1a(home.as_encoded_bytes())));
    if ensure_private_dir(&dir) {
        dir
    } else {
        base
    }
}

#[cfg(unix)]
fn ensure_private_dir(dir: &std::path::Path) -> bool {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    match std::fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => true, // criado por nós com 0700
        // já existe: só confia se for um DIRETÓRIO 0700 (não symlink, não
        // afrouxado) — senão outro usuário pode tê-lo plantado
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => std::fs::symlink_metadata(dir)
            .map(|m| m.is_dir() && (m.permissions().mode() & 0o777) == 0o700)
            .unwrap_or(false),
        Err(_) => false,
    }
}

#[cfg(not(unix))]
fn ensure_private_dir(dir: &std::path::Path) -> bool {
    std::fs::create_dir_all(dir).is_ok()
}

/// Caminho do socket de multiplexação para um alvo. Curto (limite ~104
/// chars para sockets unix no macOS) e estável por alvo+porta. O nome-base é só
/// o hash (o diretório privado já separa por usuário).
fn control_path(target: &str, port: Option<u16>) -> std::path::PathBuf {
    let mut seed = target.as_bytes().to_vec();
    seed.extend_from_slice(&port.unwrap_or(22).to_le_bytes());
    control_dir().join(format!("{:016x}", fnv1a(&seed)))
}

/// ssh não-interativo (sem PTY). Sem askpass: chaves/agent/config apenas
/// (`BatchMode=yes`). Com askpass (host com senha SSH no Vault): o helper
/// one-shot responde UM prompt de senha; prompts de host key/passphrase são
/// recusados pelo helper, então nada fica esperando input.
///
/// Usa ControlMaster: a 1ª conexão a um host abre um master persistente e as
/// seguintes (detectar → instalar → verificar) o reaproveitam — a
/// autenticação (chave/Touch ID/senha) acontece UMA vez por fluxo, não por comando.
fn ssh_command(
    target: &str,
    port: Option<u16>,
    remote_cmd: &str,
    askpass: Option<&AskpassAttempt>,
) -> Command {
    let mut cmd = Command::new("ssh");
    for key in askpass_attempt::INHERITED_ASKPASS_ENV {
        cmd.env_remove(key);
    }
    cmd.arg("-T");
    match askpass {
        Some(askpass) => {
            cmd.env("SSH_ASKPASS", &askpass.helper)
                .env("SSH_ASKPASS_REQUIRE", "force")
                .env("HELM_ASKPASS_MODE", "1")
                .env("HELM_ASKPASS_SECRET_FILE", &askpass.secret_file)
                .arg("-o")
                .arg("NumberOfPasswordPrompts=1");
        }
        None => {
            cmd.arg("-o").arg("BatchMode=yes");
        }
    }
    cmd.arg("-o")
        .arg("ConnectTimeout=10")
        .arg("-o")
        .arg("ControlMaster=auto")
        .arg("-o")
        .arg(format!(
            "ControlPath={}",
            control_path(target, port).display()
        ))
        .arg("-o")
        .arg("ControlPersist=45");
    if let Some(port) = port {
        cmd.arg("-p").arg(port.to_string());
    }
    cmd.arg("--").arg(target).arg(remote_cmd);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

/// Prepara o askpass one-shot de um comando auxiliar, se a credencial tiver
/// senha SSH no Vault (destrancado). Falhas caem para BatchMode (None).
fn helper_askpass(
    app: &AppHandle,
    db: &State<'_, Db>,
    vault: &State<'_, Vault>,
    credential_ref: Option<&str>,
    tag: &str,
) -> Option<AskpassAttempt> {
    let secret = resolve_ssh_password(db, vault, credential_ref)?;
    match askpass_attempt::create(app, tag, &secret) {
        Ok(askpass) => {
            // Comandos longos (instalação: até 180s) não mantêm o segredo em
            // disco além da fase de autenticação.
            askpass.schedule_expiry(askpass_attempt::ASKPASS_EXPIRY);
            Some(askpass)
        }
        Err(e) => {
            eprintln!("[remote] preparação do askpass falhou ({e}); usando BatchMode");
            None
        }
    }
}

const REMOTE_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const INSTALL_TIMEOUT: Duration = Duration::from_secs(180);

fn run_with_timeout(
    mut cmd: Command,
    timeout: Duration,
    stdin_input: Option<&[u8]>,
    operation: &str,
) -> Result<Output, String> {
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("falha ao iniciar {operation}: {e}"))?;
    let deadline = Instant::now() + timeout;

    if let Some(input) = stdin_input {
        let write_result = (|| -> std::io::Result<()> {
            let mut stdin = child.stdin.take().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::BrokenPipe, "stdin indisponível")
            })?;
            stdin.write_all(input)?;
            stdin.write_all(b"\n")
        })();
        if let Err(err) = write_result {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("falha ao enviar stdin para {operation}: {err}"));
        }
    }
    drop(child.stdin.take());

    // Drena os pipes em paralelo para que output volumoso não bloqueie o filho.
    let mut stdout = child.stdout.take().ok_or("stdout indisponível")?;
    let mut stderr = child.stderr.take().ok_or("stderr indisponível")?;
    let stdout_reader = std::thread::spawn(move || {
        let mut output = Vec::new();
        stdout.read_to_end(&mut output).map(|_| output)
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut output = Vec::new();
        stderr.read_to_end(&mut output).map(|_| output)
    });

    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if Instant::now() >= deadline {
            let kill_error = child.kill().err();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(match kill_error {
                Some(err) => format!(
                    "{operation} excedeu o timeout de {}s; falha ao encerrar ssh: {err}",
                    timeout.as_secs()
                ),
                None => format!(
                    "{operation} excedeu o timeout de {}s; processo ssh encerrado",
                    timeout.as_secs()
                ),
            });
        }
        std::thread::sleep(Duration::from_millis(100));
    };

    let stdout = stdout_reader
        .join()
        .map_err(|_| format!("falha ao coletar stdout de {operation}"))?
        .map_err(|e| e.to_string())?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| format!("falha ao coletar stderr de {operation}"))?
        .map_err(|e| e.to_string())?;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
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
pub async fn test_connection(
    app: AppHandle,
    db: State<'_, Db>,
    vault: State<'_, Vault>,
    draft: HostDraft,
) -> Result<TestResult, String> {
    let askpass = helper_askpass(
        &app,
        &db,
        &vault,
        draft.credential_ref.as_deref(),
        &format!("test-{}", draft.host),
    );
    tauri::async_runtime::spawn_blocking(move || {
        let target = target_of(&draft.user, &draft.host)?;
        let start = Instant::now();
        let output = run_with_timeout(
            ssh_command(
                &target,
                draft.port,
                "echo __HELM_OK__; tmux -V 2>/dev/null || true",
                askpass.as_ref(),
            ),
            REMOTE_COMMAND_TIMEOUT,
            None,
            "teste de conexão",
        );
        drop(askpass); // apaga o segredo assim que o ssh retorna
        let output = output?;
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
            Some(
                err.lines()
                    .last()
                    .unwrap_or("conexão falhou")
                    .trim()
                    .to_string(),
            )
        };
        Ok(TestResult {
            ok,
            latency_ms,
            tmux,
            message,
        })
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
    pub claude: Option<String>,
    pub codex: Option<String>,
}

// Em zsh, `nonomatch` deixa um glob sem resultados passar como literal para
// que `[ -d ]` o filtre; em bash, `setopt` falha silenciosamente e é um no-op.
const DETECT_SCRIPT: &str = r#"setopt nonomatch >/dev/null 2>&1; export PATH="$HOME/.local/bin:$HOME/.npm-global/bin:$HOME/.cargo/bin:$PATH"; nvm_alias="$HOME/.nvm/alias/default"; nvm_default=$(cat "$nvm_alias" 2>/dev/null); nvm_default_bin="$HOME/.nvm/versions/node/$nvm_default/bin"; if [ -f "$nvm_alias" ] && printf '%s\n' "$nvm_default" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+$' && [ -d "$nvm_default_bin" ]; then PATH="$nvm_default_bin:$PATH"; else for d in "$HOME"/.nvm/versions/node/*/bin; do [ -d "$d" ] && PATH="$d:$PATH"; done; fi; export PATH; . /etc/os-release 2>/dev/null; echo "OS=${PRETTY_NAME:-$(uname -s)}"; tmux -V 2>/dev/null | sed 's/^/TMUX=/'; command -v claude 2>/dev/null | sed 's/^/CLAUDE=/'; command -v codex 2>/dev/null | sed 's/^/CODEX=/'; for pm in apt-get dnf yum pacman apk zypper; do command -v $pm >/dev/null 2>&1 && { echo "PM=$pm"; break; }; done"#;

#[tauri::command]
pub async fn detect_remote(
    app: AppHandle,
    db: State<'_, Db>,
    vault: State<'_, Vault>,
    host_id: String,
) -> Result<RemoteInfo, String> {
    let host = db::get_host(&db, &host_id)?;
    let askpass = helper_askpass(
        &app,
        &db,
        &vault,
        host.credential_ref.as_deref(),
        &format!("detect-{host_id}"),
    );
    tauri::async_runtime::spawn_blocking(move || {
        let target = target_of(&host.user, &host.host)?;
        let output = run_with_timeout(
            ssh_command(&target, host.port, DETECT_SCRIPT, askpass.as_ref()),
            REMOTE_COMMAND_TIMEOUT,
            None,
            "detecção remota",
        );
        drop(askpass); // apaga o segredo assim que o ssh retorna
        let output = output?;
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
            claude: field("CLAUDE="),
            codex: field("CODEX="),
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
    app: AppHandle,
    db: State<'_, Db>,
    vault: State<'_, Vault>,
    host_id: String,
    pkg_manager: String,
    auth: InstallAuth,
) -> Result<String, String> {
    let supplied_password = auth.password.map(Zeroizing::new);
    let host = db::get_host(&db, &host_id)?;
    let install = install_command(&pkg_manager)
        .ok_or_else(|| format!("package manager não suportado: {pkg_manager}"))?;

    // resolve a senha ANTES do spawn_blocking (State não atravessa threads).
    // Zeroizing zera a memória da senha ao sair do escopo (fim do closure).
    let password: Option<Zeroizing<String>> = match (&auth.credential_id, supplied_password) {
        (Some(cred_id), _) => {
            let (kind, scope, has_secret): (String, Option<String>, bool) = {
                let conn = db.0.lock().unwrap();
                conn.query_row(
                    "SELECT kind, scope, has_secret FROM credentials_meta WHERE id = ?1",
                    [cred_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .map_err(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => {
                        format!("credencial não encontrada no Vault: {cred_id}")
                    }
                    e => format!("falha ao consultar credencial do Vault: {e}"),
                })?
            };
            if kind != "password" {
                return Err("credencial de sudo deve ser do tipo password".into());
            }
            let sudo_scope = scope
                .as_deref()
                .is_some_and(|scope| scope.to_lowercase().contains("sudo") || scope == "NOPASSWD");
            if !sudo_scope {
                return Err("credencial sem escopo compatível com sudo".into());
            }
            if has_secret {
                match vault::get_secret(&db, &vault, cred_id) {
                    Ok(secret) => Some(Zeroizing::new(secret)),
                    Err(err) if err.contains("No matching entry") => {
                        eprintln!(
                            "[remote] segredo da credencial {cred_id} ausente no keyring; tentando sudo sem senha: {err}"
                        );
                        None
                    }
                    Err(err) => return Err(err),
                }
            } else {
                None
            }
        }
        (None, Some(pwd)) if !pwd.is_empty() => Some(pwd),
        _ => None,
    };
    // Senha SSH (login) via askpass; a senha do SUDO continua só no stdin.
    let askpass = helper_askpass(
        &app,
        &db,
        &vault,
        host.credential_ref.as_deref(),
        &format!("install-{host_id}"),
    );

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
        let output = run_with_timeout(
            ssh_command(&target, host.port, &remote_cmd, askpass.as_ref()),
            INSTALL_TIMEOUT,
            password.as_ref().map(|pwd| pwd.as_bytes()),
            "instalação remota",
        );
        drop(askpass); // apaga o segredo assim que o ssh retorna
        let output = output?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let version = stdout
            .split("__HELM_TMUX__")
            .nth(1)
            .map(|v| v.trim().to_string())
            .filter(|v| v.starts_with("tmux"));
        match version {
            Some(v) => Ok(v),
            None => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let last = stdout
                    .lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .or_else(|| stderr.lines().rev().find(|l| !l.trim().is_empty()));
                Err(last.unwrap_or("instalação falhou").trim().to_string())
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::{ssh_command, HostDraft};
    use crate::session::askpass_attempt::create_in;
    use std::ffi::OsStr;

    fn args(cmd: &std::process::Command) -> Vec<String> {
        cmd.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    fn env_of<'a>(cmd: &'a std::process::Command, key: &str) -> Option<Option<&'a OsStr>> {
        cmd.get_envs().find(|(k, _)| *k == key).map(|(_, v)| v)
    }

    #[test]
    fn sem_askpass_mantem_batchmode_e_limpa_ambiente() {
        let cmd = ssh_command("deploy@example.com", Some(2222), "true", None);
        let args = args(&cmd);
        assert!(args.contains(&"BatchMode=yes".to_string()));
        assert!(!args
            .iter()
            .any(|a| a.starts_with("NumberOfPasswordPrompts")));
        // variáveis herdadas são removidas (Some(None) = env_remove)
        assert_eq!(env_of(&cmd, "SSH_ASKPASS"), Some(None));
        assert_eq!(env_of(&cmd, "HELM_ASKPASS_SECRET"), Some(None));
        let sep = args.iter().position(|a| a == "--").unwrap();
        assert_eq!(args[sep + 1], "deploy@example.com");
    }

    #[test]
    fn com_askpass_troca_batchmode_pelo_helper_one_shot() {
        let base = std::env::temp_dir().join(format!("helm-remote-askpass-{}", std::process::id()));
        let askpass = create_in(&base, "t", "s3cr3t").unwrap();
        let cmd = ssh_command("deploy@example.com", None, "true", Some(&askpass));
        let args = args(&cmd);
        assert!(!args.contains(&"BatchMode=yes".to_string()));
        assert!(args.contains(&"NumberOfPasswordPrompts=1".to_string()));
        assert_eq!(
            env_of(&cmd, "SSH_ASKPASS_REQUIRE"),
            Some(Some(OsStr::new("force")))
        );
        assert_eq!(
            env_of(&cmd, "HELM_ASKPASS_MODE"),
            Some(Some(OsStr::new("1")))
        );
        assert_eq!(
            env_of(&cmd, "HELM_ASKPASS_SECRET_FILE"),
            Some(Some(askpass.secret_file.as_os_str()))
        );
        // o segredo nunca vai para argv nem para o ambiente
        assert!(!args.iter().any(|a| a.contains("s3cr3t")));
        assert!(!cmd
            .get_envs()
            .any(|(_, v)| v.is_some_and(|v| v.to_string_lossy().contains("s3cr3t"))));
        let path = askpass.secret_file.clone();
        drop(askpass);
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn rascunho_sem_credential_ref_continua_valido() {
        let draft: HostDraft =
            serde_json::from_str(r#"{"user":null,"host":"h","port":null}"#).unwrap();
        assert!(draft.credential_ref.is_none());
        let draft: HostDraft =
            serde_json::from_str(r#"{"user":"u","host":"h","port":22,"credentialRef":"c1"}"#)
                .unwrap();
        assert_eq!(draft.credential_ref.as_deref(), Some("c1"));
    }
}
