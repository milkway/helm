# Helm — notas para o Claude Code

Gerenciador desktop de terminais SSH (macOS + Linux). Tauri 2 + React 19 + xterm.js 6; backend Rust. Site em `site/` (Vite + Tailwind 4). Repo público: `milkway/helm`.

## Comandos

- **Dev:** `npm install` && `npm run tauri dev` (janela nativa; front em `localhost:1420`).
- **Typecheck (SEMPRE antes de relançar):** `npx tsc --noEmit`. O Vite **não** faz checagem de tipos — imports/erros só aparecem em runtime (ex.: `useState` esquecido → tela preta). Rode o `tsc` antes de qualquer relançamento do dev.
- **Rust:** `cd src-tauri && cargo check` / `cargo clippy --all-targets -- -D warnings` (o CI usa `-D warnings`).
- **Testes:** `cd src-tauri && cargo test` (69: strip_ansi/prompt, migrações, askpass one-shot, decisão pós-saída, quoting de `~`, dir privado do ControlMaster, usuário do prompt sudo). Front: `npm test` (Vitest: paridade i18n, stores com Tauri mockado, base64 do ipc). Ambos rodam no CI.
- **Build app:** `npm run tauri build` (.dmg em `src-tauri/target/release/bundle/`).
- **Build .deb:** `scripts/build-deb-remote.sh` (SSH na máquina `prompt`, Ubuntu 22.04) ou `scripts/build-deb.sh` (Docker).
- **Site:** `cd site && npm install && npm run build`.

## Arquitetura

- **Front (`src/`):** componentes React; estado em Zustand (`src/stores/`: hosts, sessions, ui, vault, vpn). IPC tipado em `src/lib/ipc.ts`. Terminais vivem FORA do React em `src/lib/termRegistry.ts` (um xterm por sessão, reparentado entre views via `TermHost` — sobrevive à troca terminal↔grid). i18n em `src/i18n/` (dict + `useT` + persistência via ui_prefs; default por `navigator.language`). Atalhos globais e rótulos por plataforma em `src/lib/platform.ts` (⌘ no macOS, **Ctrl+Shift** no Linux para não roubar Ctrl+B do tmux e Ctrl+K/D do shell; o xterm deixa esses chords subirem via `attachCustomKeyEventHandler`).
- **Rust (`src-tauri/src/`):** `session/manager.rs` (ciclo de vida das sessões: spawn ssh -tt, reconexão com backoff, detach, atenção; a decisão pós-saída é a função pura `decide_after_exit`, testada; "connected" só é emitido 1,5 s após o 1º output com o processo vivo; stdin passa por uma fila com thread escritora por sessão), `session/askpass_attempt.rs` (segredo one-shot do askpass, compartilhado por sessões e helpers: expira 20 s após o spawn, sobras varridas no setup), `session/pty.rs` (portable-pty), `db.rs` (rusqlite: hosts, ui_prefs, credentials_meta), `vault.rs` (keyring + Touch ID via LocalAuthentication), `remote.rs` (test_connection, detect_remote, install_tmux com sudo -S por stdin; usam askpass quando o host tem senha no cofre, `BatchMode` só como fallback; socket de ControlMaster só em dir privado 0700 do próprio uid), `sshconfig.rs` (import de ~/.ssh/config, app_info), `vpn.rs` (Tunnelblick/nmcli, refcount).
- **Estilos:** `src/styles/tokens.css` (design tokens: dark #0b0d10, âmbar #e0a15e), `app.css`. Fontes Inter Tight + JetBrains Mono embutidas (woff2 locais, sem rede).

## Gotchas importantes (aprendidos na prática)

- **Touch ID / Keychain só funcionam no bundle assinado** (`tauri build`), não no binário solto do `tauri dev`. Em debug o vault destrava sem biometria (`cfg!(debug_assertions)` em `vault.rs::authenticate_owner`).
- **Import do ssh_config guarda o ALIAS**, não o HostName resolvido — conectar pelo IP cru perde IdentityFile/ProxyCommand do config e o SSH pendura.
- **z-index nos overlays** (connect/error/detached/attn-toast): o canvas WebGL do xterm pinta por cima e captura cliques; overlays precisam de `z-index`.
- **Id de PTY único por montagem** no termRegistry: StrictMode monta efeitos 2× em dev; id fixo mataria a sessão da 2ª montagem.
- **Segurança:** host/user validados contra injeção de argv (começar com `-`) no SSH; perfil de VPN passado por argv do osascript (não interpolado) contra injeção de AppleScript. Senha de sudo só por stdin (`sudo -S`), nunca em argv/logs.
- **Atalhos com o terminal focado:** o xterm chama `stopPropagation` em Ctrl+letra, então um handler no `window` nunca vê Ctrl+K/B/D vindo do terminal. Só chords liberados em `isAppShortcut` chegam ao App. Com modal aberto os atalhos são ignorados; Esc fecha só o que está no topo e nunca um modal com `modalBusy` (instalação de tmux, salvar host).
- **Senha do cofre recusada é detectada pelo consumo do arquivo askpass**, não por "nenhum output": o próprio `Permission denied` do ssh chega pelo PTY. Depois disso o askpass fica desligado para a sessão (nada de reenviar a senha na reconexão).
- **Remontagem do TermHost não recria sessão em erro/detached** (`ensureTerm` rejeita `NotRecreating`); retry só via `reattachTab`, que incrementa `generation`.
- **CI clippy roda no Linux:** código só-macOS (ex.: enum do vault) precisa de `#[cfg(target_os = "macos")]` senão vira dead_code e quebra o `-D warnings`.

## Release / distribuição

- Bump de versão em 4 lugares: `package.json`, `src-tauri/Cargo.toml` (+ `Cargo.lock` — rode `cargo check` p/ atualizar), `src-tauri/tauri.conf.json`, `CITATION.cff` (+ `date-released`). Faça o bump numa branch/PR (não commite direto no `main`).
- Tag `vX.Y.Z` → `release.yml` gera `.dmg` (macOS aarch64 + x64) e `.deb` (Ubuntu 22.04) num release **draft**. Publicar é manual (e dispara o DOI do Zenodo).
- **DOI (Zenodo):** o badge e a citação do README usam o **concept DOI** `10.5281/zenodo.21303522` (segue sempre a última versão — NÃO mudar a cada release). O `CITATION.cff` usa o DOI **da versão** no campo `doi:` (ex.: v0.2.1 = `21380404`), pego em `https://zenodo.org/api/records/21303522/versions/latest`. Zenodo cunha sozinho via webhook ao publicar o release — **a latência varia de 5 s (v0.4.0) a ~1h20 (v0.5.0)**; reentregar o webhook responde 409 "already received" e o evento `released` duplicado pode dar 500, ambos normais. Espere antes de recriar a release. O endpoint `versions/latest` responde 301: use `curl -L`. (Até a v0.2.0 o repo citava por engano o DOI da v0.1.0 em todo lugar — badge preso; corrigido na v0.2.1.)
- Site (`milkway.github.io/helm`) faz deploy via `pages.yml` ao empurrar em `site/`.

## Instalação do bundle (macOS)

- O `.dmg` é **assinado ad-hoc**, não notarizado pela Apple → o Gatekeeper bloqueia no 1º abrir. Solução: **clique-direito no Helm.app → Abrir**, ou `xattr -dr com.apple.quarantine /Applications/Helm.app`. Notarização exigiria certificado Apple Developer (US$ 99/ano) + passo no `release.yml`.
- Dados do app (banco SQLite, hosts, ui_prefs): `~/Library/Application Support/io.github.milkway.helm/` — compartilhado entre dev e o bundle.

## Revisão cruzada (receita que funcionou na r9, set/2026)

Agentes Opus por área (Rust; front/estado; UX/hierarquia) em paralelo + Codex CLI (`codex exec -m gpt-5.6-sol -s read-only -o out.md - < prompt.md`, prompt por stdin senão trava em background) → validar cada achado no código antes de corrigir (o Codex errou ~2 de 25) → correções por agentes em arquivos disjuntos (dois agentes no mesmo `dict.ts` colidem) → `/code-review high` no PR antes do merge.

## Convenções

- Textos de UI passam por `t("chave")` do i18n (EN/PT/FR/ES). Ao adicionar strings, inclua a chave nos 4 idiomas em `src/i18n/dict.ts`.
- Screenshots reais do app ficam em `assets/screenshots/` (README) e `site/public/shots/` (site).
- README em **inglês** (público internacional); site e UI têm seletor de idioma.
