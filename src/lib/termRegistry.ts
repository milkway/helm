// Posse dos terminais fora do React: um xterm por sessão, vivo enquanto a
// aba existir, reparentado entre as views (terminal ↔ grid) via TermHost.
import { Terminal, type ITheme } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import {
  type AttentionPayload,
  base64ToBytes,
  closeSession,
  detectRemote,
  detachSession,
  hostHasSshCredential,
  installTmux,
  onAttention,
  onSessionOutput,
  onSessionStatus,
  openSshSession,
  resizePty,
  saveHost,
  type SessionOutput,
  type SessionStatusPayload,
  writeStdin,
} from "./ipc";
import { useSessionsStore } from "../stores/sessions";
import { useHostsStore } from "../stores/hosts";
import { useUiStore } from "../stores/ui";
import { isSudoCredential, useVaultStore } from "../stores/vault";
import { sessionUsesTmux, tmuxSessionName, type Host, type SessionStatus } from "../types";

const THEME_DARK: ITheme = {
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

export const THEME_LIGHT: ITheme = {
  background: "#00000000",
  foreground: "#1a1d21",
  cursor: "#92571f",
  cursorAccent: "#faf9f6",
  selectionBackground: "rgba(47,127,192,.24)",
  black: "#2a2d31",
  red: "#b23a35",
  green: "#18794e",
  yellow: "#8a6500",
  blue: "#2563a6",
  magenta: "#7a4ba0",
  cyan: "#147d78",
  white: "#4f565e",
  brightBlack: "#6b727b",
  brightRed: "#b74228",
  brightGreen: "#18794e",
  brightYellow: "#8a6500",
  brightBlue: "#2563a6",
  brightMagenta: "#7a4ba0",
  brightCyan: "#147d78",
  brightWhite: "#3a3f45",
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
let currentTheme: "dark" | "light" = "dark";

export function applyTermTheme(theme: "dark" | "light"): void {
  currentTheme = theme;
  const xtermTheme = theme === "light" ? THEME_LIGHT : THEME_DARK;
  for (const entry of entries.values()) entry.term.options.theme = xtermTheme;
}
const lastPtySizes = new WeakMap<TermEntry, { cols: number; rows: number }>();
const pending = new Map<string, Promise<TermEntry>>();
type ConnectAbort = { aborted: boolean };
/** sinal de cancelamento p/ connects em voo (aba fechada durante o connect) */
const pendingAbort = new Map<string, ConnectAbort>();
/** uma única preparação/instalação automática de tmux por host */
const tmuxInstallations = new Map<string, Promise<boolean>>();
/** holder DOM corrente de cada sessão (setado pelo TermHost montado) */
const holders = new Map<string, { el: HTMLElement; fontSize: number }>();

type SessionHandler<T> = (payload: T) => void;

const outputHandlers = new Map<string, SessionHandler<SessionOutput>>();
const statusHandlers = new Map<string, SessionHandler<SessionStatusPayload>>();
const attentionHandlers = new Map<string, SessionHandler<AttentionPayload>>();
let sessionListenersReady: Promise<void> | null = null;

/** Instala uma única vez os listeners globais e roteia cada evento pelo PTY. */
function ensureSessionListeners(): Promise<void> {
  sessionListenersReady ??= (async () => {
    const unlisteners: Array<() => void> = [];
    try {
      unlisteners.push(
        await onSessionOutput((payload) => outputHandlers.get(payload.id)?.(payload)),
      );
      unlisteners.push(
        await onSessionStatus((payload) => statusHandlers.get(payload.id)?.(payload)),
      );
      unlisteners.push(
        await onAttention((payload) => attentionHandlers.get(payload.id)?.(payload)),
      );
    } catch (error) {
      for (const unlisten of unlisteners.reverse()) unlisten();
      sessionListenersReady = null;
      throw error;
    }
  })();
  return sessionListenersReady;
}

function addSessionHandler<T>(
  handlers: Map<string, SessionHandler<T>>,
  ptyId: string,
  handler: SessionHandler<T>,
): () => void {
  handlers.set(ptyId, handler);
  return () => {
    if (handlers.get(ptyId) === handler) handlers.delete(ptyId);
  };
}

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
  else void ensureTerm(uiId, hostId).catch(() => undefined);
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

  const abort = { aborted: false };
  pendingAbort.set(uiId, abort);
  const promise = createEntry(uiId, hostId, abort).finally(() => {
    pending.delete(uiId);
    pendingAbort.delete(uiId);
  });
  pending.set(uiId, promise);
  return promise;
}

function throwIfAborted(abort: ConnectAbort): void {
  if (abort.aborted) throw new Error("sessão cancelada durante a conexão");
}

async function prepareSshPassword(hostId: string, abort: ConnectAbort): Promise<void> {
  const host = useHostsStore.getState().hosts.find((item) => item.id === hostId);
  if (!host?.credentialRef) return;

  if (useVaultStore.getState().locked) {
    let eligible: boolean;
    try {
      eligible = await hostHasSshCredential(hostId);
    } catch {
      throwIfAborted(abort);
      // Falha ao consultar metadata não bloqueia o SSH: segue interativo.
      return;
    }
    throwIfAborted(abort);
    if (!eligible) return;

    const vault = useVaultStore.getState();
    if (!vault.locked) return;
    // unlock trata recusa/cancelamento sem lançar; a conexão segue e o ssh
    // oferece digitação interativa quando o cofre continuar bloqueado.
    try {
      await vault.unlock();
    } finally {
      throwIfAborted(abort);
    }
  }
}

async function runTmuxInstallation(
  uiId: string,
  hostId: string,
  host: Host,
  abort: ConnectAbort,
): Promise<boolean> {
  const setAutoTmux = useUiStore.getState().setAutoTmux;
  const openManual = (initialInfo?: Awaited<ReturnType<typeof detectRemote>>, initialError?: string) => {
    useUiStore.getState().openModal({
      kind: "installTmux",
      hostId,
      resumeSessionId: uiId,
      initialInfo,
      initialError,
    });
    return false;
  };

  try {
    setAutoTmux({ hostId, phase: "detecting" });
    let info: Awaited<ReturnType<typeof detectRemote>>;
    try {
      info = await detectRemote(hostId);
    } catch (e) {
      throwIfAborted(abort);
      return openManual(undefined, String(e));
    }
    throwIfAborted(abort);

    if (info.tmux) return true;
    if (!info.pkgManager) return openManual(info);

    let vault = useVaultStore.getState();
    if (vault.locked) {
      setAutoTmux({ hostId, phase: "unlocking" });
      try {
        await vault.unlock();
      } finally {
        throwIfAborted(abort);
      }
      vault = useVaultStore.getState();
    }

    const sudoCreds = vault.creds.filter(isSudoCredential);
    const credential =
      sudoCreds.find((cred) => cred.id === host.credentialRef) ??
      sudoCreds.find((cred) => cred.scope === "NOPASSWD");
    if (vault.locked || !credential) return openManual(info);

    setAutoTmux({ hostId, phase: "installing" });
    try {
      await installTmux(hostId, info.pkgManager, { credentialId: credential.id });
    } catch (e) {
      throwIfAborted(abort);
      return openManual(info, String(e));
    }
    throwIfAborted(abort);

    if (!host.autoAttach) {
      try {
        await saveHost({ ...host, autoAttach: true });
        throwIfAborted(abort);
        await useHostsStore.getState().load();
        throwIfAborted(abort);
      } catch {
        throwIfAborted(abort);
        // tmux já foi instalado; falha ao persistir auto-attach não bloqueia a sessão pedida.
      }
    }
    return true;
  } finally {
    const autoTmux = useUiStore.getState().autoTmux;
    if (autoTmux?.hostId === hostId) setAutoTmux(null);
  }
}

async function prepareTmux(
  uiId: string,
  hostId: string,
  abort: ConnectAbort,
): Promise<boolean> {
  const host = useHostsStore.getState().hosts.find((item) => item.id === hostId);
  const session = useSessionsStore.getState().sessions.find((item) => item.id === uiId);
  if (!host || !session || !sessionUsesTmux(session, host) || !host.autoInstallTmux) {
    return true;
  }

  const inflight = tmuxInstallations.get(hostId);
  if (inflight) {
    let ready: boolean;
    try {
      ready = await inflight;
    } catch {
      // Preserva o fallback existente para rejeições da instalação em voo.
      throwIfAborted(abort);
      return true;
    }
    throwIfAborted(abort);
    return ready;
  }

  const installation = runTmuxInstallation(uiId, hostId, host, abort);
  tmuxInstallations.set(hostId, installation);
  try {
    return await installation;
  } finally {
    if (tmuxInstallations.get(hostId) === installation) tmuxInstallations.delete(hostId);
  }
}

async function createEntry(
  uiId: string,
  hostId: string,
  abort: ConnectAbort,
): Promise<TermEntry> {
  const setStatus = useSessionsStore.getState().setStatus;
  await prepareSshPassword(hostId, abort);
  throwIfAborted(abort);
  const tmuxReady = await prepareTmux(uiId, hostId, abort);
  throwIfAborted(abort);
  if (!tmuxReady) {
    setStatus(uiId, "error");
    throw new Error("tmux installation requires user action");
  }

  // A fonte PRECISA estar carregada antes do xterm medir a célula — métricas
  // da fonte fallback deixam espaçamento e geometria errados.
  await ensureFonts();
  throwIfAborted(abort);

  const ptyId = `${uiId}-${crypto.randomUUID().slice(0, 8)}`;
  const disposers: Array<() => void> = [];
  // desfaz tudo o que já foi alocado (listeners, PTY, terminal, nó DOM) — usado
  // quando a conexão falha ou a aba é fechada durante o connect, antes de a
  // entry entrar em `entries` (senão nada mais chamaria disposeEntry).
  const cleanup = () => {
    for (const dispose of disposers.reverse()) dispose();
    container.remove();
  };

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
    theme: currentTheme === "light" ? THEME_LIGHT : THEME_DARK,
  });
  if (holder) term.options.fontSize = holder.fontSize;
  const fit = new FitAddon();
  term.loadAddon(fit);
  term.open(container);
  let webgl: WebglAddon | null = null;
  try {
    webgl = new WebglAddon();
    webgl.onContextLoss(() => {
      const lostAddon = webgl;
      webgl = null;
      lostAddon?.dispose();
    });
    term.loadAddon(webgl);
  } catch {
    webgl?.dispose();
    webgl = null;
    // WebGL indisponível — o renderer padrão do xterm assume.
  }
  fit.fit();
  disposers.push(() => term.dispose());

  // Listeners ANTES do spawn: os primeiros bytes chegam no arranque.
  try {
    await ensureSessionListeners();
    throwIfAborted(abort);
  } catch (error) {
    cleanup();
    throw error;
  }
  disposers.push(
    addSessionHandler(outputHandlers, ptyId, (payload) => {
      term.write(base64ToBytes(payload.data));
    }),
  );

  let lastStatus: SessionStatus | null = null;
  disposers.push(
    addSessionHandler(statusHandlers, ptyId, (payload) => {
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
    }),
  );

  disposers.push(
    addSessionHandler(attentionHandlers, ptyId, (payload) => {
      useSessionsStore.getState().setAttention(uiId, payload.active);
    }),
  );

  const params = useSessionsStore.getState().sessions.find((s) => s.id === uiId)?.params;
  const initialCols = term.cols || 80;
  const initialRows = term.rows || 24;
  try {
    await openSshSession(ptyId, hostId, initialCols, initialRows, params ?? undefined);
  } catch (err) {
    // falha ao abrir (ex.: host sumiu do DB): libera listeners/terminal/DOM em
    // vez de vazá-los; status "error" mostra o overlay com retry (o terminal,
    // que antes exibia o texto do erro, é descartado aqui)
    cleanup();
    throwIfAborted(abort);
    setStatus(uiId, "error");
    throw err;
  }
  disposers.push(() => void closeSession(ptyId));

  if (abort.aborted) {
    // a aba foi fechada durante o connect: desfaz tudo, incl. o PTY já aberto
    cleanup();
    throw new Error("sessão cancelada durante a conexão");
  }

  useSessionsStore.getState().setPtyId(uiId, ptyId);

  const offData = term.onData((data) => void writeStdin(ptyId, data));
  disposers.push(() => offData.dispose());

  const entry: TermEntry = { uiId, hostId, ptyId, container, term, fit, disposers };
  lastPtySizes.set(entry, { cols: initialCols, rows: initialRows });
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
  const { cols, rows } = entry.term;
  const lastPtySize = lastPtySizes.get(entry);
  if (lastPtySize && cols === lastPtySize.cols && rows === lastPtySize.rows) return;
  lastPtySizes.set(entry, { cols, rows });
  void resizePty(entry.ptyId, cols, rows);
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
  // cancela um connect em voo (a entry ainda não está em `entries`): o
  // createEntry desfaz tudo ao resolver, evitando PTY/listeners órfãos
  const abort = pendingAbort.get(uiId);
  if (abort) abort.aborted = true;
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
