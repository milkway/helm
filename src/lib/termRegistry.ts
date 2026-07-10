// Posse dos terminais fora do React: um xterm por sessão, vivo enquanto a
// aba existir, reparentado entre as views (terminal ↔ grid) via TermHost.
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import {
  base64ToBytes,
  closeSession,
  detachSession,
  onSessionOutput,
  onSessionStatus,
  openSshSession,
  resizePty,
  writeStdin,
} from "./ipc";
import { useSessionsStore } from "../stores/sessions";
import { useHostsStore } from "../stores/hosts";
import { tmuxSessionName, type SessionStatus } from "../types";

const THEME = {
  background: "#00000000",
  foreground: "#e6e8ea",
  cursor: "#e0a15e",
  cursorAccent: "#1a0f08",
  selectionBackground: "rgba(90,169,224,.35)",
  black: "#0b0d10",
  red: "#f0785a",
  green: "#63d29b",
  yellow: "#e0b15e",
  blue: "#5aa9e0",
  magenta: "#c9a0e8",
  cyan: "#6fd0c8",
  white: "#e6e8ea",
  brightBlack: "#565c64",
  brightRed: "#f5a88f",
  brightGreen: "#8fe3ba",
  brightYellow: "#f0c99a",
  brightBlue: "#9cc8e8",
  brightMagenta: "#dbc0f0",
  brightCyan: "#a0e4de",
  brightWhite: "#f4f6f8",
};

export const TERM_FONT = '"JetBrains Mono", ui-monospace, monospace';

export interface TermEntry {
  uiId: string;
  hostId: string;
  ptyId: string;
  container: HTMLDivElement;
  term: Terminal;
  fit: FitAddon;
  disposers: Array<() => void>;
}

const entries = new Map<string, TermEntry>();
const pending = new Map<string, Promise<TermEntry>>();
/** holder DOM corrente de cada sessão (setado pelo TermHost montado) */
const holders = new Map<string, { el: HTMLElement; fontSize: number }>();

/** Anexa o container da sessão ao holder corrente e ajusta a geometria. */
function attach(entry: TermEntry): void {
  const holder = holders.get(entry.uiId);
  if (!holder) return;
  if (entry.container.parentElement !== holder.el) {
    holder.el.appendChild(entry.container);
  }
  if (entry.term.options.fontSize !== holder.fontSize) {
    entry.term.options.fontSize = holder.fontSize;
  }
  fitEntry(entry.uiId);
}

/**
 * TermHost registra aqui o nó onde o terminal deve viver. Se a sessão ainda
 * não tem terminal, dispara a criação — que só abre o xterm com o container
 * JÁ no DOM (métricas de célula medidas fora do DOM saem erradas).
 */
export function claimHolder(uiId: string, hostId: string, el: HTMLElement, fontSize: number): void {
  holders.set(uiId, { el, fontSize });
  const entry = entries.get(uiId);
  if (entry) attach(entry);
  else void ensureTerm(uiId, hostId);
}

export function releaseHolder(uiId: string, el: HTMLElement): void {
  const holder = holders.get(uiId);
  if (holder?.el === el) holders.delete(uiId);
  const entry = entries.get(uiId);
  if (entry && entry.container.parentElement === el) {
    el.removeChild(entry.container);
  }
}

let fontsReady: Promise<unknown> | null = null;
function ensureFonts(): Promise<unknown> {
  fontsReady ??= Promise.all([
    document.fonts.load(`13px ${TERM_FONT}`),
    document.fonts.load(`700 13px ${TERM_FONT}`),
  ]);
  return fontsReady;
}

// Reload de página (dev) não roda cleanups — fecha todos os PTYs.
window.addEventListener("beforeunload", () => {
  for (const entry of entries.values()) void closeSession(entry.ptyId);
});

export function getEntry(uiId: string): TermEntry | undefined {
  return entries.get(uiId);
}

/** Cria (uma única vez, idempotente sob StrictMode) o terminal + PTY da sessão. */
export function ensureTerm(uiId: string, hostId: string): Promise<TermEntry> {
  const existing = entries.get(uiId);
  if (existing) return Promise.resolve(existing);
  const inflight = pending.get(uiId);
  if (inflight) return inflight;

  const promise = createEntry(uiId, hostId).finally(() => pending.delete(uiId));
  pending.set(uiId, promise);
  return promise;
}

async function createEntry(uiId: string, hostId: string): Promise<TermEntry> {
  // A fonte PRECISA estar carregada antes do xterm medir a célula — métricas
  // da fonte fallback deixam espaçamento e geometria errados.
  await ensureFonts();

  const ptyId = `${uiId}-${crypto.randomUUID().slice(0, 8)}`;
  const setStatus = useSessionsStore.getState().setStatus;
  const disposers: Array<() => void> = [];

  const container = document.createElement("div");
  container.className = "term__host";
  // container no DOM ANTES do term.open — métricas de célula corretas
  const holder = holders.get(uiId);
  if (holder) holder.el.appendChild(container);

  const term = new Terminal({
    fontFamily: TERM_FONT,
    fontSize: 13,
    // o protótipo usa 1.7 em linhas fake de HTML; num terminal real isso
    // quebra TUIs — 1.35 preserva o ar do design
    lineHeight: 1.35,
    cursorBlink: true,
    cursorStyle: "block",
    allowTransparency: true,
    scrollback: 10_000,
    theme: THEME,
  });
  if (holder) term.options.fontSize = holder.fontSize;
  const fit = new FitAddon();
  term.loadAddon(fit);
  term.open(container);
  try {
    term.loadAddon(new WebglAddon());
  } catch {
    // WebGL indisponível — o renderer padrão do xterm assume.
  }
  fit.fit();
  disposers.push(() => term.dispose());

  // Listeners ANTES do spawn: os primeiros bytes chegam no arranque.
  const offOutput = await onSessionOutput((payload) => {
    if (payload.id === ptyId) term.write(base64ToBytes(payload.data));
  });
  disposers.push(offOutput);

  let lastStatus: SessionStatus | null = null;
  const offStatus = await onSessionStatus((payload) => {
    if (payload.id !== ptyId) return;
    const prev = lastStatus;
    lastStatus = payload.status;
    setStatus(uiId, payload.status, payload.attempt ?? null, payload.delaySecs ?? null);

    if (payload.status === "connected" && (prev === "reconnecting" || prev === "error")) {
      const host = useHostsStore.getState().hosts.find((h) => h.id === hostId);
      const msg = host?.autoAttach
        ? `reconnected · auto-attached tmux session "${tmuxSessionName(host.name)}"`
        : "reconnected";
      term.write(`\r\n\x1b[38;2;99;210;155m${msg}\x1b[0m\r\n`);
    }
    if (payload.status === "exited") {
      term.write("\r\n\x1b[38;2;86;92;100m[sessão encerrada]\x1b[0m\r\n");
    }
  });
  disposers.push(offStatus);

  const params = useSessionsStore.getState().sessions.find((s) => s.id === uiId)?.params;
  try {
    await openSshSession(ptyId, hostId, term.cols || 80, term.rows || 24, params ?? undefined);
  } catch (err) {
    term.write(`\r\n\x1b[38;2;240;120;90mfalha ao abrir sessão: ${err}\x1b[0m\r\n`);
    setStatus(uiId, "exited");
    throw err;
  }
  disposers.push(() => void closeSession(ptyId));
  useSessionsStore.getState().setPtyId(uiId, ptyId);

  const offData = term.onData((data) => void writeStdin(ptyId, data));
  disposers.push(() => offData.dispose());

  const entry: TermEntry = { uiId, hostId, ptyId, container, term, fit, disposers };
  entries.set(uiId, entry);
  // o holder pode ter mudado durante o connect — reanexa e ressincroniza
  attach(entry);
  return entry;
}

/** Ajusta o xterm ao container atual e sincroniza a geometria do PTY. */
export function fitEntry(uiId: string): void {
  const entry = entries.get(uiId);
  if (!entry) return;
  if (entry.container.offsetWidth === 0 || entry.container.offsetHeight === 0) return;
  entry.fit.fit();
  void resizePty(entry.ptyId, entry.term.cols, entry.term.rows);
}

/** Fecha a sessão por completo: PTY, terminal e aba. */
export function closeTab(uiId: string): void {
  disposeEntry(uiId);
  useSessionsStore.getState().close(uiId);
}

/** Descarta terminal+PTY e remonta do zero (re-attach pós-detach/erro). */
export function reattachTab(uiId: string): void {
  disposeEntry(uiId);
  useSessionsStore.getState().reattach(uiId);
}

function disposeEntry(uiId: string): void {
  const entry = entries.get(uiId);
  if (!entry) return;
  entries.delete(uiId);
  for (const dispose of entry.disposers.reverse()) dispose();
  entry.container.remove();
}

export function detachTab(uiId: string): void {
  const entry = entries.get(uiId);
  if (entry) void detachSession(entry.ptyId);
}
