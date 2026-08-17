import { create } from "zustand";
import {
  onVpnStatus,
  vpnConnect,
  vpnDisconnect,
  vpnGetAutoDisconnect,
  vpnList,
  vpnSetAutoDisconnect,
  type VpnProfile,
} from "../lib/ipc";

interface VpnState {
  profiles: VpnProfile[];
  autoDisconnect: boolean;
  panelOpen: boolean;
  load: () => Promise<void>;
  connect: (name: string) => Promise<void>;
  disconnect: (name: string) => Promise<void>;
  setAutoDisconnect: (v: boolean) => void;
  togglePanel: (open?: boolean) => void;
}

export const useVpnStore = create<VpnState>((set, get) => ({
  profiles: [],
  autoDisconnect: true,
  panelOpen: false,

  load: async () => {
    try {
      const [profiles, autoDisconnect] = await Promise.all([vpnList(), vpnGetAutoDisconnect()]);
      set({ profiles, autoDisconnect });
    } catch (error) {
      console.error("[vpn] Failed to load VPN profiles", error);
    }
  },

  connect: async (name) => {
    // otimista: marca connecting até o evento vpn-status atualizar
    set((s) => ({
      profiles: s.profiles.map((p) => (p.name === name ? { ...p, state: "connecting" } : p)),
    }));
    try {
      await vpnConnect(name);
    } catch (error) {
      console.error(`[vpn] Failed to connect "${name}"`, error);
    }
    await get().load();
  },

  disconnect: async (name) => {
    try {
      await vpnDisconnect(name);
    } catch (error) {
      console.error(`[vpn] Failed to disconnect "${name}"`, error);
    }
    await get().load();
  },

  setAutoDisconnect: (v) => {
    set({ autoDisconnect: v });
    void vpnSetAutoDisconnect(v);
  },

  togglePanel: (open) => set((s) => ({ panelOpen: open ?? !s.panelOpen })),
}));

// estado ao vivo (o Rust emite ao conectar/desconectar)
const unlistenStatus = onVpnStatus((profiles) => useVpnStore.setState({ profiles }));

// HMR: sem isto, cada reload do módulo empilha um listener duplicado
if (import.meta.hot) {
  import.meta.hot.dispose(() => void unlistenStatus.then((un) => un()));
}
