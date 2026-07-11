# Helm — notas para o Claude Code

Gerenciador desktop de terminais SSH (macOS + Linux). Tauri 2 + React 19 + xterm.js 6; backend Rust. Site em `site/` (Vite + Tailwind 4). Repo público: `milkway/helm`.

## Comandos

- **Dev:** `npm install` && `npm run tauri dev` (janela nativa; front em `localhost:1420`).
- **Typecheck (SEMPRE antes de relançar):** `npx tsc --noEmit`. O Vite **não** faz checagem de tipos — imports/erros só aparecem em runtime (ex.: `useState` esquecido → tela preta). Rode o `tsc` antes de qualquer relançamento do dev.
- **Rust:** `cd src-tauri && cargo check` / `cargo clippy --all-targets -- -D warnings` (o CI usa `-D warnings`).
- **Build app:** `npm run tauri build` (.dmg em `src-tauri/target/release/bundle/`).
- **Build .deb:** `scripts/build-deb-remote.sh` (SSH na máquina `prompt`, Ubuntu 22.04) ou `scripts/build-deb.sh` (Docker).
- **Site:** `cd site && npm install && npm run build`.

## Arquitetura

- **Front (`src/`):** componentes React; estado em Zustand (`src/stores/`: hosts, sessions, ui, vault, vpn). IPC tipado em `src/lib/ipc.ts`. Terminais vivem FORA do React em `src/lib/termRegistry.ts` (um xterm por sessão, reparentado entre views via `TermHost` — sobrevive à troca terminal↔grid). i18n em `src/i18n/` (dict + `useT` + persistência via ui_prefs; default por `navigator.language`).
- **Rust (`src-tauri/src/`):** `session/manager.rs` (ciclo de vida das sessões: spawn ssh -tt, reconexão com backoff, detach, atenção), `session/pty.rs` (portable-pty), `db.rs` (rusqlite: hosts, ui_prefs, credentials_meta), `vault.rs` (keyring + Touch ID via LocalAuthentication), `remote.rs` (test_connection, detect_remote, install_tmux com sudo -S por stdin), `sshconfig.rs` (import de ~/.ssh/config, app_info), `vpn.rs` (Tunnelblick/nmcli, refcount).
- **Estilos:** `src/styles/tokens.css` (design tokens: dark #0b0d10, âmbar #e0a15e), `app.css`. Fontes Inter Tight + JetBrains Mono embutidas (woff2 locais, sem rede).

## Gotchas importantes (aprendidos na prática)

- **Touch ID / Keychain só funcionam no bundle assinado** (`tauri build`), não no binário solto do `tauri dev`. Em debug o vault destrava sem biometria (`cfg!(debug_assertions)` em `vault.rs::authenticate_owner`).
- **Import do ssh_config guarda o ALIAS**, não o HostName resolvido — conectar pelo IP cru perde IdentityFile/ProxyCommand do config e o SSH pendura.
- **z-index nos overlays** (connect/error/detached/attn-toast): o canvas WebGL do xterm pinta por cima e captura cliques; overlays precisam de `z-index`.
- **Id de PTY único por montagem** no termRegistry: StrictMode monta efeitos 2× em dev; id fixo mataria a sessão da 2ª montagem.
- **Segurança:** host/user validados contra injeção de argv (começar com `-`) no SSH; perfil de VPN passado por argv do osascript (não interpolado) contra injeção de AppleScript. Senha de sudo só por stdin (`sudo -S`), nunca em argv/logs.
- **CI clippy roda no Linux:** código só-macOS (ex.: enum do vault) precisa de `#[cfg(target_os = "macos")]` senão vira dead_code e quebra o `-D warnings`.

## Release / distribuição

- Bump de versão em 4 lugares: `package.json`, `src-tauri/Cargo.toml` (+ `Cargo.lock`), `src-tauri/tauri.conf.json`, `CITATION.cff`.
- Tag `vX.Y.Z` → `release.yml` gera `.dmg` (macOS aarch64 + x64) e `.deb` (Ubuntu 22.04) num release **draft**. Publicar é manual (e dispara o DOI do Zenodo).
- DOI (Zenodo) e site (`milkway.github.io/helm`, deploy via `pages.yml`) documentados no README.

## Convenções

- Textos de UI passam por `t("chave")` do i18n (EN/PT/FR/ES). Ao adicionar strings, inclua a chave nos 4 idiomas em `src/i18n/dict.ts`.
- Screenshots reais do app ficam em `assets/screenshots/` (README) e `site/public/shots/` (site).
