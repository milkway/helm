# Helm

Gerenciador desktop de terminais SSH (macOS + Linux) construído com Tauri 2 + React 19 + xterm.js 6.

Sessões tmux persistentes por projeto, reconexão e re-attach automáticos, cofre de credenciais nativo (Keychain/Secret Service + Touch ID), integração VPN (Tunnelblick/nmcli) e quick-launch **clmux** — entra na pasta do projeto e abre o Claude dentro do tmux com um clique.

## Status

Em implementação. Veja o plano completo em [PLANO.md](PLANO.md).

## Design

O pacote de design (protótipos HTML hifi, fonte da verdade visual) está em [`design_handoff_helm/`](design_handoff_helm/) — comece pelo [README do handoff](design_handoff_helm/README.md).

## Stack

- **App:** Tauri 2.x · React 19.2 · TypeScript ~6.0 · Vite 8.1 · Zustand 5
- **Terminal:** `@xterm/xterm` 6 + addons fit/webgl
- **Backend (Rust):** `portable-pty` (orquestra o OpenSSH do sistema via `ssh -tt`), `keyring`, `rusqlite`, `tokio`
- **Distribuição:** `.dmg` (macOS) e `.deb` (Linux) via GitHub Actions

## Licença

MIT
