import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

export type View = "term" | "grid";
export type GridCols = 2 | 3 | 4;

interface UiState {
  view: View;
  gridCols: GridCols;
  setView: (view: View) => void;
  setGridCols: (cols: GridCols) => void;
  /** carrega preferências persistidas (ui_prefs no SQLite) */
  loadPrefs: () => Promise<void>;
}

export const useUiStore = create<UiState>((set) => ({
  view: "term",
  gridCols: 2,

  setView: (view) => {
    set({ view });
    void invoke("set_pref", { key: "view", value: view });
  },

  setGridCols: (gridCols) => {
    set({ gridCols });
    void invoke("set_pref", { key: "gridCols", value: String(gridCols) });
  },

  loadPrefs: async () => {
    const [view, cols] = await Promise.all([
      invoke<string | null>("get_pref", { key: "view" }),
      invoke<string | null>("get_pref", { key: "gridCols" }),
    ]);
    set({
      view: view === "grid" ? "grid" : "term",
      gridCols: cols === "3" ? 3 : cols === "4" ? 4 : 2,
    });
  },
}));
