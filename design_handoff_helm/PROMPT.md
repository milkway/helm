# Prompt inicial para o Claude Code

Cole isto como primeira mensagem no Claude Code, na pasta onde o projeto será criado:

---

Crie um app desktop chamado **Helm** — um gerenciador de terminais SSH — seguindo fielmente o pacote de design em `./design_handoff_helm/` (leia `README.md` primeiro; os `.dc.html` são protótipos HTML hifi que servem de fonte da verdade para estilos; screenshots em `screenshots/`).

**Stack (use exatamente estas versões majors, estáveis):**
- Tauri 2.x (`npm create tauri-app@latest` com template react-ts)
- React 19.2.x, TypeScript ~6.0.3 (linha 6.x; migrar para o 7 nativo quando o tooling amadurecer), Vite 8.1.x
- xterm.js 6 (`@xterm/xterm` ^6.0.0) + addons `@xterm/addon-fit` ^0.11.0 e `@xterm/addon-webgl` ^0.19.0
- Zustand para estado do front
- Rust: `portable-pty` (orquestrar OpenSSH do sistema com `ssh -tt`), `keyring` (Keychain/Secret Service), `rusqlite` (metadados de hosts), `tokio`
- Bundling: alvos `dmg` (macOS) e `deb` (Linux) no `tauri.conf.json`; ícone a partir de `assets/helm-logo.svg`

**Ordem de implementação (uma etapa por vez, commits pequenos):**
1. Scaffold Tauri + React + tema base (tokens do README: fundo #0b0d10, acento #e0a15e, Inter Tight + JetBrains Mono embutidas) e layout da janela: titlebar custom, sidebar 296px, área de terminal, inspetor 264px, status bar.
2. Terminal xterm conectado a um PTY local (sem SSH ainda) via comandos/eventos Tauri.
3. SSH: spawn `ssh -tt user@host` no PTY; modelo de hosts/sessões no SQLite; sidebar com grupos e dots de status; abas.
4. Auto-reconnect (backoff 1–30s, 5 tentativas) + auto-attach tmux (`tmux new -As <projeto>`); botão **Detach** na toolbar; tela de erro pós-falha (design 3d) com retry a cada 60s.
5. Modo grid (design 1a): densidade 2×/3×/4×, cards com xterm vivos, modo compacto em 4×, copiar saída/screenshot PNG por card, chip "Copiar seleção · PNG".
6. Vault (design 3c): crate `keyring`; só metadados no SQLite; destravar com Touch ID no macOS (LocalAuthentication via plugin biometric do Tauri); auto-lock 15 min.
7. Add host (1b) + Nova sessão (1c) + instalação remota de tmux com sudo via stdin (2a) — nunca expor senha em argv/history/logs.
8. clmux quick-launch: `ssh -tt <host> 'cd <pasta> && tmux new -As <sessão> && claude'`.
9. Detecção de "precisa de atenção" (prompt interativo aguardando + inatividade) → dot pulsante, banner na sidebar e toast "Jump →".
10. ⌘K command palette (3b), empty state com import de ~/.ssh/config (3a), Sobre (3e).
11. VPN (4a): campo "requer VPN" no host; macOS via AppleScript do Tunnelblick, Linux via `nmcli`; auto-conectar antes do SSH e auto-desconectar quando o último host detachar.
12. CI: GitHub Actions com `tauri-action`, matriz macos-latest + ubuntu-22.04, artefatos .dmg e .deb.

**Regras:**
- Recrie o visual pixel-perfect a partir dos protótipos (todos os estilos estão inline nos .dc.html — inspecione-os).
- Não invente telas nem features fora do pacote; pergunte antes.
- Teste cada etapa antes de seguir (app abre, conecta, reconecta).

---
