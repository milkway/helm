//! Operações remotas fora do PTY: teste de conexão, detecção de ambiente e
//! instalação de tmux com sudo.
//!
//! SEGURANÇA: a senha do sudo trafega EXCLUSIVAMENTE pelo stdin do ssh
//! (`sudo -S -p ''`) — nunca em argv (visível no ps), history ou logs.

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, State};
use zeroize::Zeroizing;

use crate::db::{self, Db};
use crate::session::askpass_attempt::{self, fnv1a, AskpassAttempt};
use crate::session::manager::{lookup_ssh_password, SshPassword};
use crate::vault::{self, Vault};

/// Erro dos comandos auxiliares quando a credencial tem senha SSH mas o Vault
/// está bloqueado (em vez de um "Permission denied" sem contexto do BatchMode).
const VAULT_LOCKED_ERROR: &str =
    "cofre bloqueado: destrave o Vault para usar a senha SSH desta credencial";

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

/// Diretório privado (0700, nosso) para os sockets de ControlMaster. Nunca o
/// `temp_dir()` em si: no Linux ele é o `/tmp` compartilhado, onde outro
/// usuário poderia plantar/observar sockets. Ordem de preferência:
/// 1. `$XDG_RUNTIME_DIR/helm` (Linux; o runtime dir já é 0700 do usuário);
/// 2. `temp_dir()/helm-<hash(HOME)>` (no macOS o `$TMPDIR` já é por usuário).
///
/// Sem diretório privado → `None` e o ssh roda sem multiplexação.
fn control_dir() -> Option<std::path::PathBuf> {
    let uid = current_uid()?;
    control_dir_candidates()
        .into_iter()
        .find(|dir| ensure_private_dir(dir, uid))
}

fn control_dir_candidates() -> Vec<std::path::PathBuf> {
    let mut candidates = Vec::new();
    #[cfg(target_os = "linux")]
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()) {
        let runtime = std::path::PathBuf::from(runtime);
        // só caminho absoluto: relativo dependeria do cwd do app
        if runtime.is_absolute() {
            candidates.push(runtime.join("helm"));
        }
    }
    let home = std::env::var_os("HOME").unwrap_or_default();
    // hash curto: o socket precisa caber no limite de ~104 chars do macOS
    // ($TMPDIR já tem ~49), incluindo o sufixo temporário que o ssh anexa.
    let hash = fnv1a(home.as_encoded_bytes()) as u32;
    candidates.push(std::env::temp_dir().join(format!("helm-{hash:08x}")));
    candidates
}

/// uid efetivo do processo. Sem `libc` nas dependências, usa o dono do
/// `$HOME` — o app sempre roda como o dono do próprio HOME; se não rodar
/// (HOME alheio), os diretórios criados por nós não casam e a multiplexação é
/// desligada, que é o lado seguro.
#[cfg(unix)]
fn current_uid() -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty())?;
    std::fs::metadata(home).ok().map(|m| m.uid())
}

#[cfg(not(unix))]
fn current_uid() -> Option<u32> {
    Some(0)
}

/// Garante que `dir` é um diretório (não symlink) 0700 cujo dono é `uid`.
/// Cria com 0700 se não existir; um diretório pré-existente só é aceito se já
/// estiver exatamente assim — nunca "conserta" permissões de um diretório que
/// outro usuário pode ter plantado.
#[cfg(unix)]
fn ensure_private_dir(dir: &std::path::Path, uid: u32) -> bool {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    match std::fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => {
            // o umask pode ter tirado bits; fixa 0700 explicitamente
            if std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).is_err() {
                return false;
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return false,
    }
    // symlink_metadata: um symlink (mesmo para um diretório nosso) é recusado
    std::fs::symlink_metadata(dir)
        .map(|m| {
            m.file_type().is_dir() && m.permissions().mode() & 0o777 == 0o700 && m.uid() == uid
        })
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn ensure_private_dir(dir: &std::path::Path, _uid: u32) -> bool {
    std::fs::create_dir_all(dir).is_ok()
}

/// Caminho do socket de multiplexação para um alvo, ou `None` se não há
/// diretório privado (ssh então roda com `ControlMaster=no`). Curto (limite
/// ~104 chars para sockets unix no macOS) e estável por alvo+porta. O
/// nome-base é só o hash (o diretório privado já separa por usuário).
fn control_path(target: &str, port: Option<u16>) -> Option<std::path::PathBuf> {
    let mut seed = target.as_bytes().to_vec();
    seed.extend_from_slice(&port.unwrap_or(22).to_le_bytes());
    Some(control_dir()?.join(format!("{:016x}", fnv1a(&seed))))
}

/// ssh não-interativo (sem PTY). Sem askpass: chaves/agent/config apenas
/// (`BatchMode=yes`). Com askpass (host com senha SSH no Vault): o helper
/// one-shot responde UM prompt de senha; prompts de host key/passphrase são
/// recusados pelo helper, então nada fica esperando input.
///
/// Com `Multiplex::Shared` usa ControlMaster: a 1ª conexão a um host abre um
/// master persistente e as seguintes (detectar → instalar → verificar) o
/// reaproveitam — a autenticação (chave/Touch ID/senha) acontece UMA vez por
/// fluxo, não por comando. `Multiplex::Off` sempre autentica de novo.
fn ssh_command(
    target: &str,
    port: Option<u16>,
    remote_cmd: &str,
    askpass: Option<&AskpassAttempt>,
    multiplex: Multiplex,
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
    cmd.arg("-o").arg("ConnectTimeout=10");
    let control = match multiplex {
        Multiplex::Shared => control_path(target, port),
        Multiplex::Off => None,
    };
    match control {
        Some(path) => {
            cmd.arg("-o")
                .arg("ControlMaster=auto")
                .arg("-o")
                .arg(format!("ControlPath={}", path.display()))
                .arg("-o")
                .arg("ControlPersist=45");
        }
        None => {
            // sem multiplexação (pedida ou sem diretório privado): nada de
            // socket em lugar compartilhado. ControlPath=none também ignora
            // um ControlPath do ~/.ssh/config.
            cmd.arg("-o")
                .arg("ControlMaster=no")
                .arg("-o")
                .arg("ControlPath=none");
        }
    }
    if let Some(port) = port {
        cmd.arg("-p").arg(port.to_string());
    }
    cmd.arg("--").arg(target).arg(remote_cmd);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

/// Multiplexação (ControlMaster) de um comando ssh auxiliar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Multiplex {
    /// Reaproveita/abre o master persistente do alvo (detecção, instalação).
    Shared,
    /// Conexão própria: o teste de conexão precisa autenticar DE VERDADE com
    /// a credencial do formulário — um master vivo (ControlPersist) de outro
    /// comando daria "OK" mesmo com a senha trocada/errada.
    Off,
}

/// Prepara o askpass one-shot de um comando auxiliar, se a credencial tiver
/// senha SSH no Vault. Vault bloqueado com credencial SSH elegível →
/// `Err(VAULT_LOCKED_ERROR)`; demais falhas caem para BatchMode (`Ok(None)`).
///
/// A leitura do keychain (pode abrir o diálogo de ACL) roda fora das threads
/// do runtime async; os `State` são obtidos lá dentro pelo `AppHandle`.
async fn helper_askpass(
    app: &AppHandle,
    credential_ref: Option<String>,
    tag: &str,
) -> Result<Option<AskpassAttempt>, String> {
    let lookup_app = app.clone();
    let password = tauri::async_runtime::spawn_blocking(move || {
        let db = lookup_app.state::<Db>();
        let vault = lookup_app.state::<Vault>();
        lookup_ssh_password(&db, &vault, credential_ref.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?;
    let secret = match password {
        SshPassword::Secret(secret) => secret,
        SshPassword::Locked => return Err(VAULT_LOCKED_ERROR.into()),
        SshPassword::Unavailable => return Ok(None),
    };
    match askpass_attempt::create(app, tag, &secret) {
        Ok(askpass) => {
            // Comandos longos (instalação: até 180s) não mantêm o segredo em
            // disco além da fase de autenticação.
            askpass.schedule_expiry(askpass_attempt::ASKPASS_EXPIRY);
            Ok(Some(askpass))
        }
        Err(e) => {
            eprintln!("[remote] preparação do askpass falhou ({e}); usando BatchMode");
            Ok(None)
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
pub async fn test_connection(app: AppHandle, draft: HostDraft) -> Result<TestResult, String> {
    let askpass = helper_askpass(
        &app,
        draft.credential_ref.clone(),
        &format!("test-{}", draft.host),
    )
    .await?;
    tauri::async_runtime::spawn_blocking(move || {
        let target = target_of(&draft.user, &draft.host)?;
        let start = Instant::now();
        let output = run_with_timeout(
            ssh_command(
                &target,
                draft.port,
                "echo __HELM_OK__; tmux -V 2>/dev/null || true",
                askpass.as_ref(),
                Multiplex::Off,
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
    host_id: String,
) -> Result<RemoteInfo, String> {
    let host = db::get_host(&db, &host_id)?;
    let askpass = helper_askpass(
        &app,
        host.credential_ref.clone(),
        &format!("detect-{host_id}"),
    )
    .await?;
    tauri::async_runtime::spawn_blocking(move || {
        let target = target_of(&host.user, &host.host)?;
        let output = run_with_timeout(
            ssh_command(
                &target,
                host.port,
                DETECT_SCRIPT,
                askpass.as_ref(),
                Multiplex::Shared,
            ),
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
        host.credential_ref.clone(),
        &format!("install-{host_id}"),
    )
    .await?;

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
            ssh_command(
                &target,
                host.port,
                &remote_cmd,
                askpass.as_ref(),
                Multiplex::Shared,
            ),
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
    use super::{ssh_command, HostDraft, Multiplex};
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
        let cmd = ssh_command(
            "deploy@example.com",
            Some(2222),
            "true",
            None,
            Multiplex::Shared,
        );
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
        let cmd = ssh_command(
            "deploy@example.com",
            None,
            "true",
            Some(&askpass),
            Multiplex::Shared,
        );
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

    #[cfg(unix)]
    mod private_dir {
        use super::super::ensure_private_dir;
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        use std::path::PathBuf;

        fn scratch(name: &str) -> (PathBuf, u32) {
            let base =
                std::env::temp_dir().join(format!("helm-ctl-test-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&base);
            std::fs::create_dir_all(&base).unwrap();
            // dono real dos arquivos que este processo cria
            let uid = std::fs::metadata(&base).unwrap().uid();
            (base, uid)
        }

        #[test]
        fn cria_diretorio_novo_0700() {
            let (base, uid) = scratch("novo");
            let dir = base.join("helm");
            assert!(ensure_private_dir(&dir, uid));
            let meta = std::fs::symlink_metadata(&dir).unwrap();
            assert!(meta.is_dir());
            assert_eq!(meta.permissions().mode() & 0o777, 0o700);
            // de outro dono: recusado
            assert!(!ensure_private_dir(&dir, uid.wrapping_add(1)));
            let _ = std::fs::remove_dir_all(&base);
        }

        #[test]
        fn aceita_diretorio_existente_0700_nosso() {
            let (base, uid) = scratch("existente");
            let dir = base.join("helm");
            std::fs::create_dir(&dir).unwrap();
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
            assert!(ensure_private_dir(&dir, uid));
            let _ = std::fs::remove_dir_all(&base);
        }

        #[test]
        fn recusa_diretorio_existente_0755() {
            let (base, uid) = scratch("aberto");
            let dir = base.join("helm");
            std::fs::create_dir(&dir).unwrap();
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert!(!ensure_private_dir(&dir, uid));
            // e não "conserta" as permissões de um diretório pré-existente
            let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o755);
            let _ = std::fs::remove_dir_all(&base);
        }

        #[test]
        fn recusa_symlink_mesmo_para_diretorio_privado() {
            let (base, uid) = scratch("symlink");
            let real = base.join("real");
            std::fs::create_dir(&real).unwrap();
            std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o700)).unwrap();
            let link = base.join("helm");
            std::os::unix::fs::symlink(&real, &link).unwrap();
            assert!(!ensure_private_dir(&link, uid));
            let _ = std::fs::remove_dir_all(&base);
        }
    }

    #[test]
    fn socket_de_controle_so_em_diretorio_privado() {
        let cmd = ssh_command(
            "deploy@example.com",
            Some(22),
            "true",
            None,
            Multiplex::Shared,
        );
        let args = args(&cmd);
        if let Some(path) = args.iter().find_map(|a| a.strip_prefix("ControlPath=")) {
            if path == "none" {
                assert!(args.contains(&"ControlMaster=no".to_string()));
            } else {
                assert!(args.contains(&"ControlMaster=auto".to_string()));
                // nunca direto no temp_dir compartilhado
                let parent = std::path::Path::new(path).parent().unwrap();
                assert_ne!(parent, std::env::temp_dir().as_path());
                assert!(path.len() < 90, "socket longo demais: {path}");
            }
        } else {
            panic!("ControlPath ausente");
        }
    }

    #[test]
    fn teste_de_conexao_nunca_reusa_master() {
        // um ControlMaster vivo autenticaria no lugar da credencial testada
        let cmd = ssh_command("deploy@example.com", Some(22), "true", None, Multiplex::Off);
        let args = args(&cmd);
        assert!(args.contains(&"ControlMaster=no".to_string()));
        assert!(args.contains(&"ControlPath=none".to_string()));
        assert!(!args.iter().any(|a| a.starts_with("ControlPersist")));
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
