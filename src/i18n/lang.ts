import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

export type Lang = "en" | "pt" | "fr" | "es";

export const LANGS: { code: Lang; label: string; flag: string }[] = [
  { code: "en", label: "English", flag: "🇬🇧" },
  { code: "pt", label: "Português", flag: "🇧🇷" },
  { code: "fr", label: "Français", flag: "🇫🇷" },
  { code: "es", label: "Español", flag: "🇪🇸" },
];

/** Idioma default a partir do sistema (navigator reflete o locale do OS). */
function detectLang(): Lang {
  const nav = (navigator.languages?.[0] ?? navigator.language ?? "en").toLowerCase();
  if (nav.startsWith("pt")) return "pt";
  if (nav.startsWith("fr")) return "fr";
  if (nav.startsWith("es")) return "es";
  return "en";
}

interface LangState {
  lang: Lang;
  setLang: (lang: Lang) => void;
  /** carrega a escolha persistida (ui_prefs); senão usa o idioma do sistema */
  load: () => Promise<void>;
}

export const useLangStore = create<LangState>((set) => ({
  lang: detectLang(),
  setLang: (lang) => {
    set({ lang });
    void invoke("set_pref", { key: "lang", value: lang });
  },
  load: async () => {
    try {
      const saved = await invoke<string | null>("get_pref", { key: "lang" });
      if (saved && ["en", "pt", "fr", "es"].includes(saved)) {
        set({ lang: saved as Lang });
      }
    } catch {
      /* mantém o idioma detectado */
    }
  },
}));
