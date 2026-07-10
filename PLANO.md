# Helm — Plano de Implementação

Gerenciador desktop de terminais SSH (macOS + Linux) com sessões tmux persistentes, reconexão automática, cofre de credenciais nativo, integração VPN e quick-launch `clmux`. Fonte da verdade de design: `design_handoff_helm/` (protótipos hifi `.dc.html`, pixel-perfect).

**Stack** (estáveis verificadas em jul/2026): Tauri 2.x (2.11) · React 19.2 · TypeScript ~6.0.3 · Vite 8.1 · `@xterm/xterm` ^6.0.0 (+ addon-fit ^0.11, addon-webgl ^0.19) · Zustand 5 · Rust: `portable-pty` 0.9, `keyring` 4, `rusqlite` 0.40, `tokio` 1.

---

## 1. Arquitetura

```
┌────────────────────────── Janela Tauri ──────────────────────────┐
│  React 19 + Zustand                                              │
│  ┌─────────┐ ┌──────────────┐ ┌──────────┐ ┌──────────────────┐  │
│  │ Sidebar │ │ TerminalView │ │ GridView │ │ Modais (1b,1c,   │  │
│  │ (grupos,│ │ (xterm 6 +   │ │ (cards   │ │  2a,3a,3b,3c,    │  │
│  │  status)│ │  fit/webgl)  │ │  xterm)  │ │  3d,3e,4a)       │  │
│  └────┬────┘ └──────┬───────┘ └────┬─────┘ └────────┬─────────┘  │
│       └─────────────┴──── invoke / listen ──────────┘            │
├───────────────────────────── IPC Tauri ──────────────────────────┤
│  Rust (tokio)                                                    │
│  ┌────────────────┐ ┌───────────┐ ┌─────────┐ ┌──────────────┐   │
│  │ SessionManager │ │ VaultSvc  │ │ HostDb  │ │ VpnSvc       │   │
│  │ portable-pty → │ │ keyring + │ │ rusqlite│ │ Tunnelblick/ │   │
│  │ ssh -tt / tmux │ │ biometric │ │         │ │ nmcli        │   │
│  └────────────────┘ └───────────┘ └─────────┘ └──────────────┘   │
└──────────────────────────────────────────────────────────────────┘
```

Princípios:
- **SSH pelo OpenSSH do sistema** via `portable-pty` (`ssh -tt user@host -p port`): herda agent, `~/.ssh/config`, known_hosts. Nada de lib SSH em Rust.
- **Um PTY por sessão**, gerido por uma task tokio que lê o output e emite `session-output` (chunks base64/utf8) para o front; o xterm é burro — só renderiza.
- **Segredos nunca saem do keyring**; SQLite guarda apenas metadados (`credencialRef` é uma chave do keyring).
- **Estado vivo no Rust** (status, tentativas de reconexão, latência); o front espelha via eventos.

### Contratos IPC

Commands (`invoke`): `list_hosts`, `save_host`, `delete_host`, `test_connection(hostId)`, `import_ssh_config`, `open_session(hostId, mode)`, `close_session(id)`, `write_stdin(id, data)`, `resize_pty(id, cols, rows)`, `detach(id)`, `install_tmux(id, sudoSource)`, `vault_unlock`, `vault_lock`, `vault_list`, `vault_save(cred)`, `vault_reveal(id)`, `vpn_connect(profile)`, `vpn_disconnect(profile)`, `vpn_status`.

Events (`emit`): `session-output {id, data}`, `session-status {id, status, latency, attempt}`, `attention {id, reason}`, `vpn-status {profile, state}`, `vault-status {locked}`.

### Esquema SQLite (`hosts.db` em `app_data_dir`)

```sql
CREATE TABLE hosts (
  id TEXT PRIMARY KEY, name TEXT, "group" TEXT, user TEXT, host TEXT,
  port INTEGER DEFAULT 22, credential_ref TEXT, vpn_profile TEXT,
  auto_reconnect INTEGER DEFAULT 1, auto_install_tmux INTEGER DEFAULT 0,
  auto_attach INTEGER DEFAULT 1, project_dir TEXT,
  startup_mode TEXT CHECK(startup_mode IN ('shell','tmux','clmux')) DEFAULT 'clmux',
  created_at TEXT, updated_at TEXT
);
CREATE TABLE credentials_meta (          -- só metadados; segredo no keyring
  ref TEXT PRIMARY KEY, kind TEXT CHECK(kind IN ('ssh_key','password')),
  label TEXT, algo TEXT, scope TEXT, last_used TEXT
);
CREATE TABLE ui_prefs (key TEXT PRIMARY KEY, value TEXT);  -- gridCols, grupos abertos…
```

---

## 2. Fases de implementação

Uma fase por PR/commit-série; cada fase termina com o critério de aceite verificado manualmente (app abre, conecta, reconecta) antes de avançar.

### Fase 0 — Scaffold e fundação visual
- `npm create tauri-app@latest` (template react-ts); ajustar para React 19.2/Vite 8.1/TS ~6.0.3; adicionar Zustand.
- Fontes **Inter Tight** e **JetBrains Mono** embutidas (arquivos woff2 no bundle, `@font-face` local — sem rede).
- Design tokens em CSS custom properties (fundo `#0b0d10`, painéis `#0e1014`, acento `#e0a15e`, status `#63d29b/#e0b15e/#f0785a/#565c64/#5aa9e0`, radii, sombras — seção "Design Tokens" do README do handoff).
- Layout da janela 1440×920: titlebar custom 44px (decorations off, drag region, traffic lights), sidebar 296px, área central, inspetor 264px, status bar 30px. Conteúdo estático placeholder fiel ao `Helm - Terminal Manager.dc.html`.
- `tauri.conf.json`: alvos `dmg` + `deb`; ícones gerados de `assets/helm-logo.svg` (`tauri icon`), squircle conforme `Helm - Logo.dc.html`.
- **Aceite:** `tauri dev` abre a janela pixel-perfect vs. screenshot do modo terminal.

### Fase 1 — Terminal local (PTY sem SSH)
- Rust: `PtySession` com `portable-pty` (shell local), task tokio de leitura → `session-output`; commands `write_stdin`/`resize_pty`.
- Front: componente `<Term>` com xterm 6 + fit + webgl (fallback canvas se WebGL indisponível), tema do terminal (fundo radial, cursor âmbar blink 1.1s), wire de stdin/stdout/resize.
- **Aceite:** shell local digitável, resize correto, sem flicker.

### Fase 2 — SSH + modelo de dados + sidebar/abas
- `HostDb` (rusqlite, migrações embutidas); CRUD de hosts; commands `list/save/delete_host`.
- `SessionManager`: `open_session` faz spawn `ssh -tt user@host -p port`; máquina de estados `connecting→connected→reconnecting→error/idle`; latência via medição periódica (`ssh -O check` ou eco de keepalive); emite `session-status`.
- Sidebar: grupos colapsáveis, dots de status ao vivo, item ativo âmbar; toolbar de abas 40px com dot + nome + fechar.
- Inspetor: metadados do host, latência, uptime.
- **Aceite:** conectar a um host real, abrir 2+ sessões em abas, status na sidebar reflete a realidade.

### Fase 3 — Auto-reconnect + auto-attach tmux + Detach + tela de erro
- Reconexão: backoff exponencial 1→30s, 5 tentativas; ao reconectar, `tmux new -As <projeto>` (attach-or-create) e linha verde "reconnected · auto-attached tmux session \"…\"".
- Pós-falha: tela de erro (canvas 3d) com log das tentativas, botões Tentar agora/Editar host/Ver log; retry automático a cada 60s. Toast de auth recusada com Abrir Vault/Usar senha.
- **Detach** na toolbar (e depois no grid): roda `tmux detach` — sessão vive no servidor; badge `tmux: <sessão>` acoplado ao botão.
- Simulação de queda para teste: matar o processo ssh e derrubar rede.
- **Aceite:** derrubar a rede → app reconecta e re-atacha sozinho; esgotar tentativas → tela 3d; Detach mantém tmux vivo no servidor.

### Fase 4 — Modo grid (canvas 1a)
- Toggle terminal/grid na toolbar; densidade 2×/3×/4× persistida em `ui_prefs`.
- Cards com xterm **vivos** (digitáveis), header com dot/nome/endereço/tag; modo compacto em 4× (só dot + nome); card em atenção com borda + glow.
- Copiar saída e exportar screenshot PNG por card; seleção de texto → chip flutuante "Copiar seleção · PNG".
- Clique no card → abre a sessão no modo terminal.
- **Aceite:** 4+ sessões no grid, todas digitáveis, export PNG funciona, densidades batem com o protótipo.

### Fase 5 — Vault (canvas 3c)
- `VaultSvc` com crate `keyring` (Keychain/Secret Service); metadados em `credentials_meta`.
- Destravar com **Touch ID** no macOS (plugin biometric do Tauri / LocalAuthentication); fallback senha; Linux: Secret Service destrava com a sessão.
- Modal do vault: seções Chaves SSH e Senhas, revelar com olho (re-exige biometria), adicionar credencial, botão Bloquear; **auto-lock 15 min** de inatividade (timer no Rust).
- **Aceite:** segredo salvo não aparece em nenhum arquivo/DB; Touch ID exigido para destravar/revelar; auto-lock dispara.

### Fase 6 — Add host (1b) + Nova sessão (1c) + instalar tmux com sudo (2a)
- Modal Add host: campos do canvas, seleção de credencial via Vault, toggles âmbar, strip "Testar conexão" (`✓ Conectado · Xms · tmux Y encontrado`).
- Modal Nova sessão: host picker, nome do projeto (= sessão tmux), pasta, radio shell/tmux/clmux (clmux padrão), toggle re-attach.
- Instalar tmux: detectar via `ssh -tt 'command -v tmux'`; detectar package manager; modal 2a; senha via `sudo -S` **por stdin dentro do canal SSH** — nunca em argv, history ou logs (redigir dos logs internos também).
- **Aceite:** host novo criado e conectado fim-a-fim; tmux instalado num host sem tmux; grep nos logs não encontra a senha.

### Fase 7 — clmux quick-launch
- Card destacado no inspetor (**clmux → claude**) e modo de startup: `ssh -tt <host> 'cd <pasta> && tmux new -As <sessão> && claude'`.
- Escapar pasta/sessão com shell-quoting correto.
- **Aceite:** um clique abre Claude dentro do tmux na pasta do projeto.

### Fase 8 — Detecção de "precisa de atenção"
- Heurística no Rust por sessão: output termina em prompt interativo (regex: `? `, `(y/N)`, `password:`, prompt do Claude aguardando…) **e** sem tecla do usuário por N segundos → evento `attention`.
- Trio de sinais: dot vermelho pulsante (animação pulse 1.8s), banner no topo da sidebar, toast no terminal com "Jump →" (foca a sessão). Limpar ao digitar/focar.
- **Aceite:** processo aguardando input em sessão em background gera os 3 sinais; Jump leva à sessão certa.

### Fase 9 — ⌘K palette (3b) + empty state (3a) + Sobre (3e)
- Command palette 580px: busca fuzzy de sessões (status ao vivo) e comandos (clmux ⌘⏎, Detach ⌘D, Instalar tmux); navegação ↑↓ ↵.
- Primeira execução: hero com logo 88px, "+ Add host", "Importar de ~/.ssh/config" (parser de `Host/HostName/User/Port/IdentityFile` → hosts em lote).
- Sobre: versão/build/arch reais em runtime + badges (Tauri/React/xterm), Verificar atualizações, Licenças.
- **Aceite:** import do ssh_config real do usuário cria hosts corretos; palette acha e executa tudo.

### Fase 10 — VPN (4a)
- Campo `vpn_profile` no host. macOS: AppleScript Tunnelblick (`connect "perfil"` + poll de estado); Linux: `nmcli connection up/down <perfil>`.
- Auto-conectar VPN antes do SSH quando o host exigir (sequência visual do canvas 4a); auto-desconectar quando o **último** host que usa o perfil detachar (configurável). Refcount de perfis no `VpnSvc`.
- Painel VPNs: perfis, status, nº de hosts que usam, Conectar/Desconectar, toggle auto-conectar.
- **Aceite:** host com VPN conecta na ordem VPN→SSH→tmux; detach do último host derruba a VPN.

### Fase 11 — CI/CD
- GitHub Actions com `tauri-action`: matriz `macos-latest` + `ubuntu-22.04` (glibc antiga = .deb mais compatível; se o runner for aposentado, migrar para container ou 24.04), artefatos `.dmg` e `.deb` por tag; job de lint (`cargo clippy`, `tsc --noEmit`, eslint) em todo push.
- **Aceite:** tag `v0.1.0` produz .dmg e .deb instaláveis.

---

## 3. Estrutura de diretórios prevista

```
helm/
├── design_handoff_helm/        # handoff (fonte da verdade de design)
├── src/                        # React
│   ├── components/  (Titlebar, Sidebar, Tabs, Term, GridCard, Inspector, StatusBar, modals/…)
│   ├── stores/      (useSessions, useHosts, useUi, useVault, useVpn)
│   ├── lib/         (ipc.ts — wrappers tipados de invoke/listen)
│   └── styles/      (tokens.css, fonts/)
├── src-tauri/
│   ├── src/         (main.rs, session/{manager,pty,reconnect,attention}.rs,
│   │                 vault.rs, db.rs, vpn.rs, ssh_config.rs, commands.rs)
│   └── tauri.conf.json
└── .github/workflows/ (ci.yml, release.yml)
```

## 4. Segurança (invariantes, valem em todas as fases)

1. Segredos **só** no Keychain/Secret Service; nunca em SQLite, arquivos, logs ou estado do front.
2. Senha sudo via `sudo -S` por stdin no canal SSH; redigida de qualquer log/telemetria; zeroize no Rust após uso.
3. Vault auto-bloqueia em 15 min; revelar segredo re-exige biometria.
4. Nenhum segredo em argv (visível em `ps`) nem em variáveis de ambiente de processos filhos.

## 5. Riscos e mitigação

| Risco | Mitigação |
|---|---|
| WebGL indisponível em alguns Linux (WebKitGTK) | fallback automático para renderer DOM/canvas do xterm |
| Heurística de atenção com falsos positivos | regexes configuráveis + debounce por N s + limpar ao digitar |
| Touch ID indisponível (Mac sem sensor / Linux) | fallback senha de sessão; Secret Service segue o lock da sessão |
| `ssh -tt` interativo pedindo host key/senha fora do fluxo | detectar prompts conhecidos no output e roteá-los para UI |
| Runner `ubuntu-22.04` aposentado | build em container Ubuntu 22.04 sobre runner atual |
| xterm 6 recém-major | versões pinadas (^6.0.0/0.11/0.19, lançadas juntas em dez/2025) |

## 6. Regras de trabalho

- Recriar visual pixel-perfect a partir dos `.dc.html` (estilos inline — inspecionar no navegador).
- Não inventar telas/features fora do handoff; dúvidas → perguntar antes.
- Commits pequenos, uma etapa por vez; testar cada fase (abre, conecta, reconecta) antes de seguir.
