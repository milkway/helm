import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { VpnProfile } from "../lib/ipc";

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(() => Promise.resolve(vi.fn())),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

import { useVpnStore } from "./vpn";

const profile: VpnProfile = {
  name: "office",
  state: "disconnected",
  hostsUsing: 2,
};

beforeEach(() => {
  invokeMock.mockReset();
  listenMock.mockClear();
  useVpnStore.setState({ profiles: [profile], autoDisconnect: true, panelOpen: false });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("VPN store", () => {
  it("captures a connect error and reloads the VPN state", async () => {
    const error = new Error("connect failed");
    const refreshed = [{ ...profile, state: "disconnected" as const, hostsUsing: 3 }];
    invokeMock.mockImplementation((command: string) => {
      if (command === "vpn_connect") return Promise.reject(error);
      if (command === "vpn_list") return Promise.resolve(refreshed);
      if (command === "vpn_get_auto_disconnect") return Promise.resolve(false);
      return Promise.reject(new Error(`Unexpected command: ${command}`));
    });
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);

    await expect(useVpnStore.getState().connect(profile.name)).resolves.toBeUndefined();

    expect(consoleError).toHaveBeenCalledWith(
      `[vpn] Failed to connect "${profile.name}"`,
      error,
    );
    expect(invokeMock).toHaveBeenCalledWith("vpn_connect", { profile: profile.name });
    expect(invokeMock).toHaveBeenCalledWith("vpn_list");
    expect(invokeMock).toHaveBeenCalledWith("vpn_get_auto_disconnect");
    expect(useVpnStore.getState()).toMatchObject({
      profiles: refreshed,
      autoDisconnect: false,
    });
  });

  it("captures a disconnect error and reloads the VPN state", async () => {
    const error = new Error("disconnect failed");
    const connected = { ...profile, state: "connected" as const };
    useVpnStore.setState({ profiles: [connected] });
    invokeMock.mockImplementation((command: string) => {
      if (command === "vpn_disconnect") return Promise.reject(error);
      if (command === "vpn_list") return Promise.resolve([connected]);
      if (command === "vpn_get_auto_disconnect") return Promise.resolve(true);
      return Promise.reject(new Error(`Unexpected command: ${command}`));
    });
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);

    await expect(useVpnStore.getState().disconnect(profile.name)).resolves.toBeUndefined();

    expect(consoleError).toHaveBeenCalledWith(
      `[vpn] Failed to disconnect "${profile.name}"`,
      error,
    );
    expect(invokeMock).toHaveBeenCalledWith("vpn_disconnect", { profile: profile.name });
    expect(invokeMock).toHaveBeenCalledWith("vpn_list");
    expect(invokeMock).toHaveBeenCalledWith("vpn_get_auto_disconnect");
    expect(useVpnStore.getState()).toMatchObject({
      profiles: [connected],
      autoDisconnect: true,
    });
  });
});
