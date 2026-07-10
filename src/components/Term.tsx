import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import "@xterm/xterm/css/xterm.css";
import {
  base64ToBytes,
  closeSession,
  onSessionOutput,
  onSessionStatus,
  openSshSession,
  resizePty,
  writeStdin,
} from "../lib/ipc";
import { useSessionsStore } from "../stores/sessions";
import { useHostsStore } from "../stores/hosts";
import { tmuxSessionName, type SessionStatus } from "../types";

// Tema do terminal (tokens do handoff). Fundo transparente: o gradiente
// radial fica no container (.term), como no protótipo.
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

const FONT = '"JetBrains Mono", ui-monospace, monospace';

interface TermProps {
  /** id estável da sessão (aba) na UI */
  uiId: string;
  hostId: string;
  active: boolean;
}

export function Term({ uiId, hostId, active }: TermProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);

  useEffect(() => {
    let disposed = false;
    const disposers: Array<() => void> = [];
    // Id de PTY único por montagem: o StrictMode monta o efeito duas vezes em
    // dev e, com id fixo, a limpeza da 1ª montagem mataria a sessão da 2ª.
    const ptyId = `${uiId}-${crypto.randomUUID().slice(0, 8)}`;
    const setStatus = useSessionsStore.getState().setStatus;

    (async () => {
      // A fonte PRECISA estar carregada antes do xterm medir a célula —
      // métricas da fonte fallback deixam espaçamento e geometria errados
      // (e o PTY abriria com cols/rows falsos, banner fora de posição).
      await Promise.all([
        document.fonts.load(`13px ${FONT}`),
        document.fonts.load(`700 13px ${FONT}`),
      ]);
      const host = hostRef.current;
      if (disposed || !host) return;

      const term = new Terminal({
        fontFamily: FONT,
        fontSize: 13,
        // o protótipo usa 1.7 em linhas fake de HTML; num terminal real
        // isso quebra TUIs — 1.35 preserva o ar do design
        lineHeight: 1.35,
        cursorBlink: true,
        cursorStyle: "block",
        allowTransparency: true,
        scrollback: 10_000,
        theme: THEME,
      });
      termRef.current = term;
      disposers.push(() => {
        termRef.current = null;
        term.dispose();
      });

      const fit = new FitAddon();
      term.loadAddon(fit);
      term.open(host);
      try {
        term.loadAddon(new WebglAddon());
      } catch {
        // WebGL indisponível — o renderer padrão do xterm assume.
      }
      fit.fit();
      term.focus();

      // Listeners ANTES do spawn: os primeiros bytes do processo chegam já
      // no arranque e seriam perdidos se registrados depois.
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

      if (disposed) return;
      await openSshSession(ptyId, hostId, term.cols, term.rows);
      disposers.push(() => void closeSession(ptyId));
      if (disposed) return;
      useSessionsStore.getState().setPtyId(uiId, ptyId);

      let opened = true;
      // o pane pode ter mudado de tamanho durante o connect — garante que a
      // geometria do PTY casa com a do xterm antes do primeiro output pesado
      fit.fit();
      void resizePty(ptyId, term.cols, term.rows);

      const offData = term.onData((data) => {
        if (opened) void writeStdin(ptyId, data);
      });
      disposers.push(() => {
        opened = false;
        offData.dispose();
      });

      const observer = new ResizeObserver(() => {
        if (host.offsetWidth === 0 || host.offsetHeight === 0) return;
        fit.fit();
        void resizePty(ptyId, term.cols, term.rows);
      });
      observer.observe(host);
      disposers.push(() => observer.disconnect());
    })().catch((err) => {
      termRef.current?.write(
        `\r\n\x1b[38;2;240;120;90mfalha ao abrir sessão: ${err}\x1b[0m\r\n`,
      );
      setStatus(uiId, "exited");
      // repropaga para o vite logar como unhandled rejection durante o dev
      return Promise.reject(err);
    });

    // Reload de página (dev) não roda o cleanup do efeito — sem isto o PTY vaza.
    const onUnload = () => void closeSession(ptyId);
    window.addEventListener("beforeunload", onUnload);

    return () => {
      disposed = true;
      window.removeEventListener("beforeunload", onUnload);
      for (const dispose of disposers.reverse()) dispose();
    };
  }, [uiId, hostId]);

  useEffect(() => {
    if (active) termRef.current?.focus();
  }, [active]);

  return <div ref={hostRef} className="term__host" />;
}
