# Helm — notas para o Claude Code

Gerenciador desktop de terminais SSH (macOS + Linux). Tauri 2 + React 19 + xterm.js 6; backend Rust. Site em `site/` (Vite + Tailwind 4). Repo público: `milkway/helm`.

## Comandos

- **Dev:** `npm install` && `npm run tauri dev` (janela nativa; front em `localhost:1420`).
- **Typecheck (SEMPRE antes de relançar):** `npx tsc --noEmit`. O Vite **não** faz checagem de tipos — imports/erros só aparecem em runtime (ex.: `useState` esquecido → tela preta). Rode o `tsc` antes de qualquer relançamento do dev.
- **Rust:** `cd src-tauri && cargo check` / `cargo clippy --all-targets -- -D warnings` (o CI usa `-D warnings`).
- **Testes:** `cd src-tauri && cargo test` (existem desde a v0.2.1: `strip_ansi`/`looks_like_prompt` em `manager.rs`, migrações em `db.rs`). Ainda não há testes no front.
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

- Bump de versão em 4 lugares: `package.json`, `src-tauri/Cargo.toml` (+ `Cargo.lock` — rode `cargo check` p/ atualizar), `src-tauri/tauri.conf.json`, `CITATION.cff` (+ `date-released`). Faça o bump numa branch/PR (não commite direto no `main`).
- Tag `vX.Y.Z` → `release.yml` gera `.dmg` (macOS aarch64 + x64) e `.deb` (Ubuntu 22.04) num release **draft**. Publicar é manual (e dispara o DOI do Zenodo).
- **DOI (Zenodo):** o badge e a citação do README usam o **concept DOI** `10.5281/zenodo.21303522` (segue sempre a última versão — NÃO mudar a cada release). O `CITATION.cff` usa o DOI **da versão** no campo `doi:` (ex.: v0.2.1 = `21380404`), pego em `https://zenodo.org/api/records/21303522/versions/latest`. Zenodo cunha sozinho via webhook ao publicar o release. (Até a v0.2.0 o repo citava por engano o DOI da v0.1.0 em todo lugar — badge preso; corrigido na v0.2.1.)
- Site (`milkway.github.io/helm`) faz deploy via `pages.yml` ao empurrar em `site/`.

## Instalação do bundle (macOS)

- O `.dmg` é **assinado ad-hoc**, não notarizado pela Apple → o Gatekeeper bloqueia no 1º abrir. Solução: **clique-direito no Helm.app → Abrir**, ou `xattr -dr com.apple.quarantine /Applications/Helm.app`. Notarização exigiria certificado Apple Developer (US$ 99/ano) + passo no `release.yml`.
- Dados do app (banco SQLite, hosts, ui_prefs): `~/Library/Application Support/io.github.milkway.helm/` — compartilhado entre dev e o bundle.

## Convenções

- Textos de UI passam por `t("chave")` do i18n (EN/PT/FR/ES). Ao adicionar strings, inclua a chave nos 4 idiomas em `src/i18n/dict.ts`.
- Screenshots reais do app ficam em `assets/screenshots/` (README) e `site/public/shots/` (site).
- README em **inglês** (público internacional); site e UI têm seletor de idioma.
