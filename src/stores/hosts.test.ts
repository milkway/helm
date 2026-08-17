import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Host } from "../types";

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(() => Promise.resolve(vi.fn())),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

import { deleteHost, saveHost } from "../lib/ipc";
import { groupHosts, useHostsStore } from "./hosts";

const host: Host = {
  id: "host-1",
  name: "Production",
  group: "Servers",
  user: "deploy",
  host: "10.0.0.10",
  port: 22,
  credentialRef: null,
  vpnProfile: null,
  autoReconnect: true,
  autoInstallTmux: false,
  autoAttach: true,
  projectDir: null,
  startupMode: "tmux",
};

beforeEach(() => {
  invokeMock.mockReset();
  listenMock.mockClear();
  useHostsStore.setState({ hosts: [], loaded: false });
});

describe("hosts store", () => {
  it("loads hosts from Tauri and marks the store as loaded", async () => {
    invokeMock.mockResolvedValueOnce([host]);

    await useHostsStore.getState().load();

    expect(invokeMock).toHaveBeenCalledWith("list_hosts");
    expect(useHostsStore.getState()).toMatchObject({ hosts: [host], loaded: true });
  });

  it("adds a host through IPC and reloads the store", async () => {
    const added = { ...host, id: "host-2", name: "Staging" };
    invokeMock.mockImplementation((command: string) => {
      if (command === "save_host") return Promise.resolve(undefined);
      if (command === "list_hosts") return Promise.resolve([host, added]);
      return Promise.reject(new Error(`Unexpected command: ${command}`));
    });

    await saveHost(added);
    await useHostsStore.getState().load();

    expect(invokeMock).toHaveBeenNthCalledWith(1, "save_host", { host: added });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "list_hosts");
    expect(useHostsStore.getState().hosts).toEqual([host, added]);
  });

  it("removes a host through IPC and reloads the store", async () => {
    useHostsStore.setState({ hosts: [host], loaded: true });
    invokeMock.mockImplementation((command: string) => {
      if (command === "delete_host") return Promise.resolve(undefined);
      if (command === "list_hosts") return Promise.resolve([]);
      return Promise.reject(new Error(`Unexpected command: ${command}`));
    });

    await deleteHost(host.id);
    await useHostsStore.getState().load();

    expect(invokeMock).toHaveBeenNthCalledWith(1, "delete_host", { id: host.id });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "list_hosts");
    expect(useHostsStore.getState().hosts).toEqual([]);
  });
});

describe("groupHosts", () => {
  it("groups hosts in insertion order and uses Hosts for an empty group", () => {
    const ungrouped = { ...host, id: "host-2", name: "Local", group: "" };

    expect(groupHosts([host, ungrouped])).toEqual([
      { name: "Servers", hosts: [host] },
      { name: "Hosts", hosts: [ungrouped] },
    ]);
  });
});
