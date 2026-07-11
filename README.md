# Helm

[![DOI](https://zenodo.org/badge/DOI/10.5281/zenodo.21303523.svg)](https://doi.org/10.5281/zenodo.21303523)
[![Release](https://img.shields.io/github/v/release/milkway/helm)](https://github.com/milkway/helm/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-e0a15e.svg)](LICENSE)

**Gerenciador desktop de terminais SSH para macOS e Linux.** Sessões tmux persistentes por projeto, reconexão e re-attach automáticos, cofre de credenciais nativo (Keychain/Secret Service com Touch ID), integração VPN e quick-launch **clmux** — entra na pasta do projeto e abre o Claude dentro do tmux com um clique.

**🌐 Site:** [milkway.github.io/helm](https://milkway.github.io/helm/) · **⬇ Download:** [Releases](https://github.com/milkway/helm/releases)

## Interface

| Modo terminal | Modo grid |
|---|---|
| ![Terminal](assets/screenshots/terminal.png) | ![Grid](assets/screenshots/grid.png) |

| Vault (Touch ID) | Add host | VPN |
|---|---|---|
| ![Vault](assets/screenshots/vault.png) | ![Add host](assets/screenshots/add-host.png) | ![VPN](assets/screenshots/vpn.png) |

## Recursos

- **Sessões SSH persistentes** via OpenSSH do sistema (`ssh -tt`), herdando agent, `~/.ssh/config` e ProxyCommand.
- **tmux por projeto** com auto-attach e **Detach** que preserva a sessão no servidor.
- **Auto-reconnect** com backoff exponencial (1–30s, 5 tentativas) e tela de erro com retry automático.
- **Modo grid** com terminais vivos, densidade 2×/3×/4× e captura (copiar saída / PNG por card).
- **Vault** de credenciais no Keychain (macOS, com Touch ID) / Secret Service (Linux); apenas metadados no SQLite, o segredo nunca toca o disco.
- **Detecção de atenção** — destaca sessões em segundo plano aguardando input do usuário.
- **⌘K command palette**, importação de `~/.ssh/config` e **clmux** (abre o Claude dentro do tmux).
- **Integração VPN** (Tunnelblick no macOS, `nmcli` no Linux): conecta antes do SSH e desconecta quando o último host que a usa fecha.

## Instalação

Baixe o instalador da sua plataforma em **[Releases](https://github.com/milkway/helm/releases/latest)**:

| Plataforma | Arquivo |
|---|---|
| macOS · Apple Silicon | `Helm_x.y.z_aarch64.dmg` |
| macOS · Intel | `Helm_x.y.z_x64.dmg` |
| Linux · Debian/Ubuntu | `Helm_x.y.z_amd64.deb` |

No macOS, o cofre de credenciais usa Touch ID; no Linux, o Secret Service da sessão.

## Desenvolvimento

```bash
npm install
npm run tauri dev
```

**Compilar instaladores:**
- **macOS (`.dmg`):** `npm run tauri build`
- **Linux (`.deb`):** `scripts/build-deb-remote.sh` (via SSH numa máquina Ubuntu) ou `scripts/build-deb.sh` (Docker)

**Release:** empurrar uma tag `vX.Y.Z` dispara o workflow `release.yml` (`.dmg` macOS aarch64 + x86_64 e `.deb` Ubuntu 22.04 via `tauri-action`). O CI (`ci.yml`) roda typecheck, build e `cargo clippy` em cada push/PR.

## Stack

- **App:** Tauri 2 · React 19 · TypeScript 6 · Vite 8 · Zustand 5
- **Terminal:** `@xterm/xterm` 6 (+ addons fit/webgl)
- **Backend (Rust):** `portable-pty` (orquestra o OpenSSH do sistema), `keyring`, `rusqlite`, `tokio`

## Citação

Se este software for útil no seu trabalho, cite-o via [`CITATION.cff`](CITATION.cff) ou pelo DOI **[10.5281/zenodo.21303523](https://doi.org/10.5281/zenodo.21303523)**.

## Licença

[MIT](LICENSE) © 2026 André Leite
