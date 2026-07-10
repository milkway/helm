import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { Host } from "../types";

interface HostsState {
  hosts: Host[];
  loaded: boolean;
  load: () => Promise<void>;
}

export const useHostsStore = create<HostsState>((set) => ({
  hosts: [],
  loaded: false,
  load: async () => {
    const hosts = await invoke<Host[]>("list_hosts");
    set({ hosts, loaded: true });
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
