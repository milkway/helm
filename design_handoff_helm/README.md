# Handoff: Helm — Gerenciador de Terminais SSH

## Overview
Helm é um app desktop (macOS + Linux, Windows depois) para gerenciar conexões SSH persistentes com servidores, com sessões tmux nomeadas por projeto, reconexão/re-attach automáticos, cofre de senhas seguro, integração com VPN (Tunnelblick/nmcli), e atalho "clmux" que entra na pasta do projeto e abre o Claude dentro do tmux.

## About the Design Files
Os arquivos neste pacote são **referências de design criadas em HTML** (protótipos de aparência e comportamento), NÃO código de produção. A tarefa é **recriar estes designs** no ambiente alvo (Tauri 2 + React + TypeScript, ver PROMPT.md) usando os padrões dessa stack. Os `.dc.html` podem ser abertos no navegador para inspecionar valores exatos de estilo (tudo é inline style).

## Fidelity
**High-fidelity (hifi).** Cores, tipografia, espaçamento e copy são finais — recriar pixel-perfect.

## Stack alvo (versões estáveis verificadas em jul/2026)
- Tauri 2.x (2.11 na verificação) — empacota `.dmg` e `.deb`
- React 19.2.x + TypeScript ~6.0.3 + Vite 8.1.x
- xterm.js 6 (`@xterm/xterm` ^6.0.0 + addons fit ^0.11.0, webgl ^0.19.0) para os terminais
- Rust: `portable-pty` orquestrando OpenSSH do sistema (ganha `ssh -tt`, agent e ~/.ssh/config de graça); `keyring` para Keychain/Secret Service; `rusqlite` para metadados de hosts
- Zustand no front; eventos Tauri (emit/listen) para status em tempo real

## Screens / Views
Arquivo `Helm - Terminal Manager.dc.html` (app principal) e `Helm - Telas.dc.html` (canvas com estados; ids nas etiquetas):

1. **App principal — modo terminal** (janela 1440×920)
   - Titlebar 44px: traffic lights, logo + "Helm", busca central 380px ("Search hosts, sessions, commands… ⌘K"), pill "6 connected" (verde), ícone settings.
   - Sidebar esquerda 296px (#0e1014): header "HOSTS & SESSIONS" + botão "+"; banner de atenção (borda #f0785a, dot pulsante); grupos colapsáveis (Production/Staging/Personal); cada sessão = dot de status + nome do projeto (ex.: `atlas-api`) + badge `tmux` + `user@host` em mono 11px. Item ativo: fundo rgba(224,161,94,.1) + borda rgba(224,161,94,.24). Rodapé: card "Vault unlocked · 12 credentials · Touch ID".
   - Toolbar de abas 40px: abas por sessão (dot de status + nome + ×), controle de densidade 2×/3×/4× (só no grid), toggle terminal/grid, badge verde `tmux: atlas-api` acoplado ao botão âmbar **Detach** (ícone eject, tooltip "keeps running on server").
   - Terminal: JetBrains Mono 13px/1.7, fundo radial #0e1114→#0a0c0f, cursor âmbar piscante; toast de atenção no canto inferior direito ("gpu-train v3 waiting · Jump →").
   - Status bar 30px mono 11px: `● SSH · atlas · deploy@10.4.2.18:22 · ↔ 24ms · reconnect: auto · attach: auto | tmux 3.4 · clmux ready · utf-8`.
   - Inspetor direito 264px: host + status, metadados (Host/User/Port/Latency/Uptime/Auto-reconnect/Auto-attach), Quick launch (card destacado **clmux → claude**: `cd atlas-api · tmux · claude`; Detach; Install/attach tmux via ssh -tt; Open project shell; Port forward), credencial usada.

2. **Modo grid** (toggle na toolbar; canvas id 1a)
   - Grid `repeat(cols, 1fr)` gap 14px, cols ∈ {2,3,4}. Card: header (dot status, nome projeto, `user@host`, tag `live/reconnecting/needs input/idle`, ícones copiar saída + screenshot PNG + detach) + mini-terminal (mono 11.5px, min-height 150/120/96px por densidade). Card em atenção: borda rgba(240,120,90,.45) + glow. Em 4×: esconder ícones de captura, tag e endereço (só dot + nome). Clicar no card abre a sessão no modo terminal. Seleção de linhas mostra chip flutuante "Copiar seleção · PNG" (borda azul rgba(90,169,224,.4)).

3. **Add host** (canvas 1b) — modal 520px: Nome, Grupo, Endereço SSH (mono), Porta; Autenticação via Vault (badge VAULT verde); toggles âmbar: Reconectar automaticamente (backoff 1–30s), Instalar tmux se não existir (via ssh -tt), Re-atachar última sessão; strip de teste "✓ Conectado · 24 ms · tmux 3.4 encontrado"; Cancelar / Adicionar host.

4. **Nova sessão** (canvas 1c) — modal 520px: Host picker, Projeto/sessão (vira nome da sessão tmux), Pasta do projeto, radio "Ao conectar": Shell simples / tmux (`tmux new -As <projeto>`) / **clmux** (`cd <pasta> && tmux new -As <projeto> && claude`, marcado PADRÃO); toggle re-attach após queda.

5. **Instalar tmux + sudo** (canvas 2a) — modal 540px: detecção "Ubuntu 22.04 · apt" (badge ssh -tt), comando `$ sudo apt-get install -y tmux`, senha do sudo: radio "Usar senha do Vault" (badge TOUCH ID, padrão) / "Digitar agora (não será salva)"; nota: senha via `sudo -S` por stdin, nunca em terminal/history/logs; Agora não / Autorizar e instalar.

6. **Primeira execução** (canvas 3a) — sidebar vazia, Vault bloqueado; hero central: logo 88px, "Nenhum host ainda", botões "+ Add host" (âmbar) e "Importar de ~/.ssh/config", dica ⌘K.

7. **⌘K Command palette** (canvas 3b) — 580px, top 150px: input mono; seção Sessões (com status ao vivo), seção Comandos (clmux ⌘⏎, Detach ⌘D, Instalar tmux); rodapé ↑↓ ↵.

8. **Vault** (canvas 3c) — modal 640px: header "12 credenciais · destravado com Touch ID · Keychain (macOS) / Secret Service (Linux)" + botão Bloquear; busca; seções Chaves SSH (badge ed25519 verde, último uso) e Senhas (badge sudo/ssh âmbar, ••••••, olho para revelar); "+ Adicionar credencial"; "bloqueio automático: 15 min inativo".

9. **Erros** (canvas 3d) — card central 480px borda vermelha: "Reconexão falhou — atlas-api · stg", "5 tentativas em 2 min · timeout", log mono das tentativas, nota "sessão tmux preservada no servidor", botões Tentar agora / Editar host / Ver log completo, "retry auto: 60s". Toast canto sup. direito: "Autenticação recusada — root@stg-db · chave rejeitada" com Abrir Vault / Usar senha.

10. **Sobre** (canvas 3e) — modal 420px: logo 76px, "Helm", "Gerenciador de terminais SSH", "1.0.0 · build 2026.07 · aarch64", badges com as versões reais em runtime (ex.: Tauri 2.11 / React 19.2 / xterm.js 6), Verificar atualizações / Licenças open-source, "© 2026 · MIT License".

11. **VPN** (canvas 4a) — sequência ao conectar host que requer VPN: ✓ Tunnelblick perfil "office" conectado (1.8s) → ⟳ ssh handshake → tmux attach; nota "VPN desconecta sozinha quando o último host que a usa for detachado (configurável)". Painel VPNs: perfis com status + nº de hosts que usam, Conectar/Desconectar, toggle "Auto-conectar quando o host precisar".

## Interactions & Behavior
- **Auto-reconnect**: backoff exponencial 1–30s, 5 tentativas; depois estado de erro (tela 3d) com retry automático a cada 60s. Ao reconectar: `tmux attach` automático na sessão do projeto; mostrar linha verde "reconnected · auto-attached tmux session \"<projeto>\"".
- **Detach**: botão na toolbar e no card do grid; roda `tmux detach` — nunca exigir Ctrl+b d.
- **Atenção**: sessão pede atenção quando Claude/processo aguarda input (heurística: prompt interativo detectado + sem tecla do usuário por N s). Trio de sinais: dot vermelho pulsante na sidebar, banner no topo da sidebar, toast no terminal com botão "Jump →".
- **Instalar tmux**: detectar via `ssh -tt 'command -v tmux'`; se ausente, detectar package manager, mostrar modal 2a; enviar senha via stdin (`sudo -S`).
- **clmux**: `ssh -tt <host> 'cd <pasta> && tmux new -As <sessão> && claude'`.
- **VPN**: macOS → AppleScript Tunnelblick (`connect "perfil"`, poll de estado); Linux → `nmcli connection up <perfil>`. Conectar antes do SSH quando o host exigir; desconectar quando o último host que usa o perfil detachar (configurável).
- **Grid**: densidade 2×/3×/4× persiste; terminais do grid são xterm vivos (digitáveis); seleção de texto mostra chip Copiar/PNG; ícone câmera exporta card como PNG.
- **Animações**: pulse de atenção `box-shadow 0 0 0 0→5px rgba(240,120,90,.55/0)` 1.8s; spinner reconexão 1.4s linear; cursor blink 1.1s step-end; toasts `fade+translateY(4px)` .4s ease.

## State Management
- `hosts[]` (id, nome-projeto, user, host, porta, grupo, credencialRef, vpnProfile?, autoReconnect, autoInstallTmux, autoAttach, pastaProjeto, startupMode: shell|tmux|clmux)
- `sessions[]` (hostId, status: connected|reconnecting|attention|idle|error, latency, tmuxSession, buffer)
- `ui` (view: term|grid, gridCols: 2|3|4, activeSessionId, paletteOpen, vaultOpen)
- `vault` (locked, credenciais — só metadados; segredos ficam no Keychain/Secret Service)
- `vpn[]` (perfil, status, hostsUsing)
- Backend Rust emite eventos: `session-status`, `session-output`, `attention`, `vpn-status`.

## Design Tokens
Cores (dark, única): fundo app #0b0d10; painéis #0e1014; terminal radial #0e1114→#0a0c0f; modais #111318; cards grid #101317; bordas rgba(255,255,255,.06–.12); texto #e6e8ea; secundário #a2a8b0; muted #7d848d; faint #565c64.
Acento âmbar #e0a15e (hover #f0c99a, texto sobre âmbar #1a0f08); gradiente logo #F0C27B→#C9683F.
Status: conectado #63d29b; reconectando #e0b15e; atenção #f0785a; idle #565c64; VPN/seleção #5aa9e0.
Tipografia: UI **Inter Tight** (400–700); mono/terminal **JetBrains Mono** (400–700). Terminal 13px/1.7; UI 12–16px; labels uppercase 10.5px ls 1.1–1.4px.
Radius: janelas 12px, modais 14–16px, cards 9–12px, botões/inputs 7–9px. Sombras: modais `0 40px 120px rgba(0,0,0,.7)`; toasts `0 12px 40px rgba(0,0,0,.5)`.
Botões 34–40px de altura; toggles 34×20 (âmbar = on).

## Segurança
- Segredos SÓ no Keychain (macOS, com Touch ID) / Secret Service (Linux) via crate `keyring`; nunca em arquivo/SQLite.
- Senha sudo via stdin (`sudo -S`) dentro do canal SSH; nunca em argv, history ou logs.
- Vault auto-bloqueia após 15 min inativo.

## Assets
- `assets/helm-logo.svg` — logo (leme, gradiente âmbar). Usar como base do ícone .icns/.ico/.png (ver `Helm - Logo.dc.html` para o tratamento do ícone: squircle #1a1d23→#0e1013, borda rgba(255,255,255,.1)).
- Fontes: Google Fonts (Inter Tight, JetBrains Mono) — embutir no app, não carregar por rede.
- Ícones: SVG stroke inline (lucide-like, stroke-width 1.8–2).

## Files
- `Helm - Terminal Manager.dc.html` — app principal (terminal + grid). Fonte da verdade de estilos.
- `Helm - Telas.dc.html` — canvas com modais e estados (ids 1a–4a).
- `Helm - Logo.dc.html` — logo, ícone do app, lockups.
- `assets/helm-logo.svg` — logo.
- `screenshots/` — capturas de referência.
- `PROMPT.md` — prompt inicial para o Claude Code.
