import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

export type View = "term" | "grid";
export type GridCols = 2 | 3 | 4;

export type Modal =
  | { kind: "addHost" }
  | { kind: "editHost"; hostId: string }
  | { kind: "newSession"; hostId?: string }
  | { kind: "installTmux"; hostId: string }
  | null;

interface UiState {
  view: View;
  gridCols: GridCols;
  modal: Modal;
  setView: (view: View) => void;
  setGridCols: (cols: GridCols) => void;
  openModal: (modal: Modal) => void;
  closeModal: () => void;
  /** carrega preferências persistidas (ui_prefs no SQLite) */
  loadPrefs: () => Promise<void>;
}

export const useUiStore = create<UiState>((set) => ({
  view: "term",
  gridCols: 2,
  modal: null,
  openModal: (modal) => set({ modal }),
  closeModal: () => set({ modal: null }),

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
