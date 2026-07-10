import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface CredMeta {
  id: string;
  kind: "ssh_key" | "password";
  label: string;
  algo: string | null;
  scope: string | null;
  lastUsed: string | null;
  /** false = só metadados (NOPASSWD, chave sem passphrase) */
  hasSecret: boolean;
}

interface VaultStatus {
  locked: boolean;
  count: number;
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
    const status = await invoke<VaultStatus>("vault_status");
    set({ locked: status.locked, count: status.count });
    if (!status.locked) {
      const creds = await invoke<CredMeta[]>("vault_list");
      set({ creds });
    }
  },

  unlock: async () => {
    if (get().busy) return;
    set({ busy: true, error: null });
    try {
      const status = await invoke<VaultStatus>("vault_unlock");
      set({ locked: status.locked, count: status.count });
      const creds = await invoke<CredMeta[]>("vault_list");
      set({ creds });
    } catch (e) {
      set({ error: String(e) });
    } finally {
      set({ busy: false });
    }
  },

  lock: async () => {
    const status = await invoke<VaultStatus>("vault_lock");
    set({ locked: status.locked, count: status.count, creds: [] });
  },

  save: async (meta, secret) => {
    await invoke("vault_save", { meta, secret });
    await get().refresh();
  },

  remove: async (id) => {
    await invoke("vault_delete", { id });
    await get().refresh();
  },

  reveal: (id) => invoke<string>("vault_reveal", { id }),
}));

// estado ao vivo (auto-lock dispara no Rust)
void listen<VaultStatus>("vault-status", (event) => {
  const { locked, count } = event.payload;
  useVaultStore.setState((s) => ({
    locked,
    count: count >= 0 ? count : s.count,
    creds: locked ? [] : s.creds,
  }));
});

// estado inicial
void useVaultStore.getState().refresh();
