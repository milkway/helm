import { create } from "zustand";
import { listHosts } from "../lib/ipc";
import type { Host } from "../types";

interface HostsState {
  hosts: Host[];
  loaded: boolean;
  error: string | null;
  load: () => Promise<void>;
}

export const useHostsStore = create<HostsState>((set) => ({
  hosts: [],
  loaded: false,
  error: null,
  load: async () => {
    set({ error: null });
    try {
      const hosts = await listHosts();
      set({ hosts, loaded: true });
    } catch (e) {
      set({ error: String(e), loaded: true });
    }
  },
}));

export function groupHosts(hosts: Host[]): { name: string; hosts: Host[] }[] {
  const groups = new Map<string, Host[]>();
  for (const host of hosts) {
    const key = host.group || "Hosts";
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key)!.push(host);
  }
  return [...groups.entries()].map(([name, hosts]) => ({ name, hosts }));
}
