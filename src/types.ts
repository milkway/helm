/** Espelho do Host do Rust (serde camelCase). */
export interface Host {
  id: string;
  name: string;
  group: string;
  user: string | null;
  host: string;
  port: number | null;
  credentialRef: string | null;
  vpnProfile: string | null;
  autoReconnect: boolean;
  autoInstallTmux: boolean;
  autoAttach: boolean;
  projectDir: string | null;
  startupMode: "shell" | "tmux" | "clmux";
}

/** Estados da máquina de sessão (manager.rs). */
export type SessionStatus =
  | "vpn"
  | "connecting"
  | "connected"
  | "reconnecting"
  | "error"
  | "detached"
  | "exited";

/** uma linha do log de eventos; `id` monotônico dá key estável na janela deslizante */
export interface LogEntry {
  id: number;
  text: string;
}

export interface SessionInfo {
  /** id estável da sessão na UI (aba) — o PTY tem id próprio por montagem */
  id: string;
  hostId: string;
  status: SessionStatus;
  /** tentativa de reconexão corrente (1–5) */
  attempt: number | null;
  connectedAt: number | null;
  /** id da sessão no Rust (registrado pelo Term ao montar) */
  ptyId: string | null;
  /** incrementa para forçar remontagem do Term (re-attach pós-detach) */
  generation: number;
  /** log de eventos para a tela de erro (design 3d); id estável p/ key do React */
  log: LogEntry[];
  /** overrides da Nova sessão (1c); null = defaults do host */
  params: import("./lib/ipc").SessionParams | null;
  /** sessão aguardando input do usuário (Fase 8) */
  attention: boolean;
}

export const STATUS_COLORS = {
  connected: "var(--st-connected)",
  reconnect: "var(--st-reconnect)",
  attention: "var(--st-attention)",
  idle: "var(--st-idle)",
} as const;

/** Cor de dot para um status de sessão (mapeia para a paleta do design). */
export function statusColor(status: SessionStatus | undefined): string {
  switch (status) {
    case "connected":
      return STATUS_COLORS.connected;
    case "connecting":
    case "reconnecting":
    case "vpn":
      return STATUS_COLORS.reconnect;
    case "error":
      return STATUS_COLORS.attention;
    default:
      return STATUS_COLORS.idle;
  }
}

/** Nome da sessão tmux derivado do nome do host (espelha manager.rs). */
export function tmuxSessionName(name: string): string {
  const s = name.replace(/[^A-Za-z0-9_-]/g, "-").replace(/^-+|-+$/g, "");
  return s || "helm";
}

export function hostAddr(host: Host): string {
  const base = host.user ? `${host.user}@${host.host}` : host.host;
  return host.port ? `${base}:${host.port}` : base;
}

/** A sessão roda dentro de um tmux? (Detach só faz sentido nesse caso.) */
export function sessionUsesTmux(session: SessionInfo, host: Host | undefined): boolean {
  if (session.params) return session.params.mode !== "shell";
  if (!host) return false;
  return host.autoAttach || host.startupMode !== "shell";
}
