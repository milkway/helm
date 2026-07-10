import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

export type View = "term" | "grid";
export type GridCols = 2 | 3 | 4;

export type Modal =
  | { kind: "addHost" }
  | { kind: "editHost"; hostId: string }
  | { kind: "newSession"; hostId?: string }
  | { kind: "installTmux"; hostId: string }
  | { kind: "about" }
  | null;

interface UiState {
  view: View;
  gridCols: GridCols;
  modal: Modal;
  paletteOpen: boolean;
  collapsedGroups: string[];
  setView: (view: View) => void;
  setGridCols: (cols: GridCols) => void;
  openModal: (modal: Modal) => void;
  closeModal: () => void;
  togglePalette: (open?: boolean) => void;
  toggleGroup: (name: string) => void;
  /** carrega preferências persistidas (ui_prefs no SQLite) */
  loadPrefs: () => Promise<void>;
}

export const useUiStore = create<UiState>((set, get) => ({
  view: "term",
  gridCols: 2,
  modal: null,
  paletteOpen: false,
  collapsedGroups: [],
  openModal: (modal) => set({ modal, paletteOpen: false }),
  closeModal: () => set({ modal: null }),
  togglePalette: (open) => set((s) => ({ paletteOpen: open ?? !s.paletteOpen })),

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
    const [view, cols, collapsed] = await Promise.all([
      invoke<string | null>("get_pref", { key: "view" }),
      invoke<string | null>("get_pref", { key: "gridCols" }),
      invoke<string | null>("get_pref", { key: "collapsedGroups" }),
    ]);
    let collapsedGroups: string[] = [];
    try {
      if (collapsed) collapsedGroups = JSON.parse(collapsed);
    } catch {
      collapsedGroups = [];
    }
    set({
      view: view === "grid" ? "grid" : "term",
      gridCols: cols === "3" ? 3 : cols === "4" ? 4 : 2,
      collapsedGroups,
    });
  },
}));
