import { create } from "zustand";
import {
  onVaultStatus,
  vaultDelete,
  vaultList,
  vaultLock,
  vaultReveal,
  vaultSave,
  vaultStatus,
  vaultUnlock,
  type VaultCredentialMeta,
} from "../lib/ipc";

export type CredMeta = VaultCredentialMeta;

export function isSudoCredential(cred: CredMeta): boolean {
  return (
    cred.kind === "password" &&
    ((cred.scope ?? "").toLowerCase().includes("sudo") || cred.scope === "NOPASSWD")
  );
}

export function isSshPasswordCredential(cred: CredMeta): boolean {
  return (
    cred.kind === "password" &&
    cred.hasSecret &&
    (cred.scope ?? "").toLowerCase().includes("ssh")
  );
}

interface VaultState {
  locked: boolean;
  count: number;
  creds: CredMeta[];
  modalOpen: boolean;
  busy: boolean;
  error: string | null;
  openModal: () => void;
  closeModal: () => void;
  refresh: () => Promise<void>;
  unlock: () => Promise<void>;
  lock: () => Promise<void>;
  save: (meta: CredMeta, secret: string) => Promise<void>;
  remove: (id: string) => Promise<void>;
  reveal: (id: string) => Promise<string>;
}

let unlockRequest: Promise<void> | null = null;

export const useVaultStore = create<VaultState>((set, get) => ({
  locked: true,
  count: 0,
  creds: [],
  modalOpen: false,
  busy: false,
  error: null,

  openModal: () => set({ modalOpen: true, error: null, busy: false }),
  closeModal: () => set({ modalOpen: false }),

  refresh: async () => {
    try {
      const status = await vaultStatus();
      const creds = status.locked ? [] : await vaultList();
      set({ locked: status.locked, count: status.count, creds, error: null });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  unlock: async () => {
    if (unlockRequest) return unlockRequest;
    unlockRequest = (async () => {
      set({ busy: true, error: null });
      try {
        const status = await vaultUnlock();
        const creds = await vaultList();
        set({ locked: status.locked, count: status.count, creds });
      } catch (e) {
        set({ error: String(e) });
      } finally {
        set({ busy: false });
        unlockRequest = null;
      }
    })();
    return unlockRequest;
  },

  lock: async () => {
    const status = await vaultLock();
    set({ locked: status.locked, count: status.count, creds: [] });
  },

  save: async (meta, secret) => {
    await vaultSave(meta, secret);
    await get().refresh();
  },

  remove: async (id) => {
    await vaultDelete(id);
    await get().refresh();
  },

  reveal: vaultReveal,
}));

// estado ao vivo (auto-lock dispara no Rust)
const unlistenStatus = onVaultStatus(({ locked, count }) => {
  useVaultStore.setState((s) => ({
    locked,
    count: count >= 0 ? count : s.count,
    creds: locked ? [] : s.creds,
  }));
});

// HMR: sem isto, cada reload do módulo empilha um listener duplicado
if (import.meta.hot) {
  import.meta.hot.dispose(() => void unlistenStatus.then((un) => un()));
}

// estado inicial
void useVaultStore.getState().refresh();
