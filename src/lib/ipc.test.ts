import { describe, expect, it, vi } from "vitest";

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(() => Promise.resolve(vi.fn())),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

import { base64ToBytes, detectRemote } from "./ipc";

describe("base64ToBytes", () => {
  it("decodes binary data without treating it as text", () => {
    expect(base64ToBytes("AP9B")).toEqual(new Uint8Array([0, 255, 65]));
  });
});

describe("detectRemote", () => {
  it("evicts rejected probes instead of caching the failure", async () => {
    invokeMock
      .mockRejectedValueOnce(new Error("VPN offline"))
      .mockResolvedValueOnce({ os: "Linux", pkgManager: null, tmux: null, claude: null, codex: null });

    await expect(detectRemote("host-retry-after-failure")).rejects.toThrow("VPN offline");
    await expect(detectRemote("host-retry-after-failure")).resolves.toMatchObject({ os: "Linux" });
    expect(invokeMock).toHaveBeenCalledTimes(2);
  });
});
