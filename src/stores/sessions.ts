import { create } from "zustand";
import type { SessionParams } from "../lib/ipc";
import type { SessionInfo, SessionStatus } from "../types";
import { translate } from "../i18n";
import { useLangStore } from "../i18n/lang";

/** contador monotônico p/ dar id estável a cada linha de log (key do React) */
let logSeq = 0;

function timestamp(): string {
  return new Date().toLocaleTimeString("pt-BR", { hour12: false });
}

function logLine(status: SessionStatus, attempt: number | null, delaySecs: number | null): string {
  const ts = timestamp();
  const lang = useLangStore.getState().lang;
  const tr = (k: string, v?: Record<string, string | number>) => translate(lang, k, v);
  switch (status) {
    case "vpn":
    case "connecting":
      return `${ts} ${tr("log.connecting")}`;
    case "connected":
      return `${ts} ${tr("log.connected")}`;
    case "reconnecting":
      return `${ts} ${tr("log.reconnecting", { n: attempt ?? 1, d: delaySecs ?? 0 })}`;
    case "error":
      return `${ts} ${tr("log.error")}`;
    case "detached":
      return `${ts} ${tr("log.detached")}`;
    default:
      return `${ts} ${tr("log.exited")}`;
  }
}

interface SessionsState {
  /** sessões abertas, na ordem das abas */
  sessions: SessionInfo[];
  activeId: string | null;
  /** cria uma sessão (aba) para um host e a torna ativa */
  open: (hostId: string, params?: SessionParams) => string;
  focus: (id: string) => void;
  close: (id: string) => void;
  setStatus: (id: string, status: SessionStatus, attempt?: number | null, delaySecs?: number | null) => void;
  setPtyId: (id: string, ptyId: string | null) => void;
  setAttention: (id: string, active: boolean) => void;
  setSudoPrompt: (id: string, active: boolean) => void;
  /** força remontagem do Term (re-attach após detach/erro terminal) */
  reattach: (id: string) => void;
}

export const useSessionsStore = create<SessionsState>((set) => ({
  sessions: [],
  activeId: null,

  open: (hostId, params) => {
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
          params: params ?? null,
          attention: false,
          sudoPrompt: false,
        },
      ],
      activeId: id,
    }));
    return id;
  },

  // focar a sessão limpa a atenção (o usuário está olhando para ela)
  focus: (id) =>
    set((s) => ({
      activeId: id,
      sessions: s.sessions.map((x) => (x.id === id ? { ...x, attention: false } : x)),
    })),

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
              log: [
                ...x.log.slice(-30),
                { id: logSeq++, text: logLine(status, attempt, delaySecs) },
              ],
            }
          : x,
      ),
    })),

  setPtyId: (id, ptyId) =>
    set((s) => ({
      sessions: s.sessions.map((x) => (x.id === id ? { ...x, ptyId } : x)),
    })),

  setAttention: (id, active) =>
    set((s) => ({
      // não marca atenção na aba que já está ativa (usuário está olhando)
      sessions: s.sessions.map((x) =>
        x.id === id ? { ...x, attention: active && x.id !== s.activeId } : x,
      ),
    })),

  setSudoPrompt: (id, active) =>
    set((s) => ({
      sessions: s.sessions.map((x) => (x.id === id ? { ...x, sudoPrompt: active } : x)),
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
              attention: false,
              sudoPrompt: false,
            }
          : x,
      ),
    })),
}));
