import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { RemoteInfo } from "../lib/ipc";

export type View = "term" | "grid";
export type GridCols = 2 | 3 | 4;
export type Theme = "dark" | "light";

export type Modal =
  | { kind: "addHost" }
  | { kind: "editHost"; hostId: string }
  | { kind: "newSession"; hostId?: string }
  | {
      kind: "installTmux";
      hostId: string;
      resumeSessionId?: string;
      initialInfo?: RemoteInfo;
      initialError?: string;
    }
  | { kind: "about" }
  | null;

interface UiState {
  view: View;
  gridCols: GridCols;
  modal: Modal;
  paletteOpen: boolean;
  collapsedGroups: string[];
  sidebarHidden: boolean;
  inspectorHidden: boolean;
  theme: Theme;
  autoTmux: { hostId: string; phase: "detecting" | "unlocking" | "installing" } | null;
  setView: (view: View) => void;
  setGridCols: (cols: GridCols) => void;
  openModal: (modal: Modal) => void;
  closeModal: () => void;
  togglePalette: (open?: boolean) => void;
  toggleGroup: (name: string) => void;
  toggleSidebar: () => void;
  toggleInspector: () => void;
  toggleTheme: () => void;
  setTheme: (theme: Theme) => void;
  setAutoTmux: (state: UiState["autoTmux"]) => void;
  /** carrega preferências persistidas (ui_prefs no SQLite) */
  loadPrefs: () => Promise<void>;
}

function applyTheme(theme: Theme): void {
  if (typeof document === "undefined") return;
  document.documentElement.dataset.theme = theme;
  void import("../lib/termRegistry").then(({ applyTermTheme }) => applyTermTheme(theme));
}

if (typeof document !== "undefined") document.documentElement.dataset.theme ||= "dark";

export const useUiStore = create<UiState>((set, get) => ({
  view: "term",
  gridCols: 2,
  modal: null,
  paletteOpen: false,
  collapsedGroups: [],
  sidebarHidden: false,
  inspectorHidden: false,
  theme: "dark",
  autoTmux: null,
  openModal: (modal) => set({ modal, paletteOpen: false }),
  closeModal: () => set({ modal: null }),
  togglePalette: (open) => set((s) => ({ paletteOpen: open ?? !s.paletteOpen })),
  setAutoTmux: (autoTmux) => set({ autoTmux }),

  toggleSidebar: () => {
    const sidebarHidden = !get().sidebarHidden;
    set({ sidebarHidden });
    void invoke("set_pref", { key: "ui.sidebarHidden", value: sidebarHidden ? "1" : "0" });
  },

  toggleInspector: () => {
    const inspectorHidden = !get().inspectorHidden;
    set({ inspectorHidden });
    void invoke("set_pref", { key: "ui.inspectorHidden", value: inspectorHidden ? "1" : "0" });
  },

  setTheme: (theme) => {
    set({ theme });
    applyTheme(theme);
    void invoke("set_pref", { key: "ui.theme", value: theme });
  },

  toggleTheme: () => get().setTheme(get().theme === "dark" ? "light" : "dark"),

  toggleGroup: (name) => {
    const collapsedGroups = get().collapsedGroups.includes(name)
      ? get().collapsedGroups.filter((g) => g !== name)
      : [...get().collapsedGroups, name];
    set({ collapsedGroups });
    void invoke("set_pref", { key: "collapsedGroups", value: JSON.stringify(collapsedGroups) });
  },

  setView: (view) => {
    set({ view });
    void invoke("set_pref", { key: "view", value: view });
  },

  setGridCols: (gridCols) => {
    set({ gridCols });
    void invoke("set_pref", { key: "gridCols", value: String(gridCols) });
  },

  loadPrefs: async () => {
    const getPref = (key: string) =>
      invoke<string | null>("get_pref", { key }).catch(() => null);
    const [view, cols, collapsed, savedSidebar, savedInspector, savedTheme] = await Promise.all([
      getPref("view"),
      getPref("gridCols"),
      getPref("collapsedGroups"),
      getPref("ui.sidebarHidden"),
      getPref("ui.inspectorHidden"),
      getPref("ui.theme"),
    ]);
    let collapsedGroups: string[] = [];
    try {
      if (collapsed) collapsedGroups = JSON.parse(collapsed);
    } catch {
      collapsedGroups = [];
    }
    const theme: Theme = savedTheme === "light" ? "light" : "dark";
    set({
      view: view === "grid" ? "grid" : "term",
      gridCols: cols === "3" ? 3 : cols === "4" ? 4 : 2,
      collapsedGroups,
      sidebarHidden: savedSidebar === "1",
      inspectorHidden: savedInspector === "1",
      theme,
    });
    applyTheme(theme);
  },
}));
