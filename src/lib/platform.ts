// Plataforma e atalhos globais do app. No macOS os atalhos usam ⌘; no Linux
// usam Ctrl+Shift, deixando livres o Ctrl+B do tmux e o Ctrl+K/Ctrl+D do readline.

export const IS_MAC = typeof navigator !== "undefined" && navigator.userAgent.includes("Mac");

/** Rótulo da tecla modificadora primária ("⌘" no macOS, "Ctrl" nos demais). */
export function modKey(): string {
  return IS_MAC ? "⌘" : "Ctrl";
}

/** Rótulo de um atalho global do app: "⌘D" no macOS, "Ctrl+Shift+D" nos demais. */
export function shortcutLabel(key: string, alt = false): string {
  if (IS_MAC) return `⌘${alt ? "⌥" : ""}${key}`;
  return `Ctrl+Shift+${alt ? "Alt+" : ""}${key}`;
}

export type AppShortcut = "palette" | "sidebar" | "inspector" | "detach";

/**
 * Classifica um keydown como atalho do app (ou null). Compara por `e.code`
 * (KeyK/KeyB/KeyD) porque o Option do macOS altera `e.key`.
 */
export function isAppShortcut(e: KeyboardEvent): AppShortcut | null {
  if (e.type !== "keydown") return null;
  const mod = IS_MAC
    ? e.metaKey && !e.ctrlKey && !e.shiftKey
    : e.ctrlKey && e.shiftKey && !e.metaKey;
  if (!mod) return null;
  switch (e.code) {
    case "KeyK":
      return e.altKey ? null : "palette";
    case "KeyB":
      return e.altKey ? "inspector" : "sidebar";
    case "KeyD":
      return e.altKey ? null : "detach";
    default:
      return null;
  }
}
