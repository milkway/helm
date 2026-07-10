import { create } from "zustand";

export type View = "term" | "grid";

interface UiState {
  view: View;
  gridCols: 2 | 3 | 4;
  activeHostId: string;
  setView: (view: View) => void;
  setGridCols: (cols: 2 | 3 | 4) => void;
  selectHost: (id: string) => void;
}

export const useUiStore = create<UiState>((set) => ({
  view: "term",
  gridCols: 2,
  activeHostId: "atlas",
  setView: (view) => set({ view }),
  setGridCols: (gridCols) => set({ gridCols }),
  selectHost: (activeHostId) => set({ activeHostId }),
}));
