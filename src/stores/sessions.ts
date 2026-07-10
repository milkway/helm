import { create } from "zustand";
import type { SessionInfo, SessionStatus } from "../types";

function timestamp(): string {
  return new Date().toLocaleTimeString("pt-BR", { hour12: false });
}

function logLine(status: SessionStatus, attempt: number | null, delaySecs: number | null): string {
  const t = timestamp();
  switch (status) {
    case "connecting":
      return `${t} conectando…`;
    case "connected":
      return `${t} conectado`;
    case "reconnecting":
      return `${t} retry ${attempt}/5 · reconectando em ${delaySecs}s`;
    case "error":
      return `${t} giving up · session preserved on server (tmux)`;
    case "detached":
      return `${t} detached · tmux segue no servidor`;
    default:
      return `${t} sessão encerrada`;
  }
}

interface SessionsState {
  /** sessões abertas, na ordem das abas */
  sessions: SessionInfo[];
  activeId: string | null;
  /** cria uma sessão (aba) para um host e a torna ativa */
  open: (hostId: string) => string;
  focus: (id: string) => void;
  close: (id: string) => void;
  setStatus: (id: string, status: SessionStatus, attempt?: number | null, delaySecs?: number | null) => void;
  setPtyId: (id: string, ptyId: string | null) => void;
  /** força remontagem do Term (re-attach após detach/erro terminal) */
  reattach: (id: string) => void;
}

export const useSessionsStore = create<SessionsState>((set) => ({
  sessions: [],
  activeId: null,

  open: (hostId) => {
    const id = crypto.randomUUID();
    set((s) => ({
      sessions: [
        ...s.sessions,
        {
          id,
          hostId,
          status: "connecting",
          attempt: null,
          connectedAt: null,
          ptyId: null,
          generation: 0,
          log: [],
        },
      ],
      activeId: id,
    }));
    return id;
  },

  focus: (id) => set({ activeId: id }),

  close: (id) =>
    set((s) => {
      const sessions = s.sessions.filter((x) => x.id !== id);
      let activeId = s.activeId;
      if (activeId === id) {
        const idx = s.sessions.findIndex((x) => x.id === id);
        activeId = sessions[Math.min(idx, sessions.length - 1)]?.id ?? null;
      }
      return { sessions, activeId };
    }),

  setStatus: (id, status, attempt = null, delaySecs = null) =>
    set((s) => ({
      sessions: s.sessions.map((x) =>
        x.id === id
          ? {
              ...x,
              status,
              attempt,
              connectedAt:
                status === "connected" ? (x.connectedAt ?? Date.now()) : x.connectedAt,
              log: [...x.log.slice(-30), logLine(status, attempt, delaySecs)],
            }
          : x,
      ),
    })),

  setPtyId: (id, ptyId) =>
    set((s) => ({
      sessions: s.sessions.map((x) => (x.id === id ? { ...x, ptyId } : x)),
    })),

  reattach: (id) =>
    set((s) => ({
      sessions: s.sessions.map((x) =>
        x.id === id
          ? {
              ...x,
              generation: x.generation + 1,
              status: "connecting",
              attempt: null,
              connectedAt: null,
              log: x.log,
            }
          : x,
      ),
    })),
}));
