import { create } from "zustand";
import type { SessionInfo, SessionStatus } from "../types";

interface SessionsState {
  /** sessões abertas, na ordem das abas */
  sessions: SessionInfo[];
  activeId: string | null;
  /** cria uma sessão (aba) para um host e a torna ativa */
  open: (hostId: string) => string;
  focus: (id: string) => void;
  close: (id: string) => void;
  setStatus: (id: string, status: SessionStatus) => void;
}

export const useSessionsStore = create<SessionsState>((set) => ({
  sessions: [],
  activeId: null,

  open: (hostId) => {
    const id = crypto.randomUUID();
    set((s) => ({
      sessions: [...s.sessions, { id, hostId, status: "connecting", connectedAt: null }],
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

  setStatus: (id, status) =>
    set((s) => ({
      sessions: s.sessions.map((x) =>
        x.id === id
          ? {
              ...x,
              status,
              connectedAt:
                status === "connected" ? (x.connectedAt ?? Date.now()) : x.connectedAt,
            }
          : x,
      ),
    })),
}));
