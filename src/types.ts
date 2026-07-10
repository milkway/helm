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

/** Estados da máquina de sessão (Fase 2; a Fase 3 adiciona reconexão). */
export type SessionStatus = "connecting" | "connected" | "exited";

export interface SessionInfo {
  /** id estável da sessão na UI (aba) — o PTY tem id próprio por montagem */
  id: string;
  hostId: string;
  status: SessionStatus;
  connectedAt: number | null;
}

export const STATUS_COLORS = {
  connected: "#63d29b",
  reconnect: "#e0b15e",
  attention: "#f0785a",
  idle: "#565c64",
} as const;

/** Cor de dot para um status de sessão (mapeia para a paleta do design). */
export function statusColor(status: SessionStatus | undefined): string {
  switch (status) {
    case "connected":
      return STATUS_COLORS.connected;
    case "connecting":
      return STATUS_COLORS.reconnect;
    default:
      return STATUS_COLORS.idle;
  }
}

export function hostAddr(host: Host): string {
  const base = host.user ? `${host.user}@${host.host}` : host.host;
  return host.port ? `${base}:${host.port}` : base;
}
