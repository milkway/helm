import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import "@xterm/xterm/css/xterm.css";
import {
  base64ToBytes,
  closeSession,
  onSessionExit,
  onSessionOutput,
  openLocalSession,
  resizePty,
  writeStdin,
} from "../lib/ipc";

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

export function Term({ sessionId }: { sessionId: string }) {
  const hostRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    let disposed = false;
    // Id único por montagem: o StrictMode monta o efeito duas vezes em dev e,
    // com id fixo, a limpeza da 1ª montagem mataria a sessão da 2ª.
    const ptyId = `${sessionId}-${crypto.randomUUID().slice(0, 8)}`;

    const term = new Terminal({
      fontFamily: '"JetBrains Mono", ui-monospace, monospace',
      fontSize: 13,
      lineHeight: 1.7,
      cursorBlink: true,
      cursorStyle: "block",
      allowTransparency: true,
      scrollback: 10_000,
      theme: THEME,
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

    const unlisteners: Array<() => void> = [];

    let opened = false;
    openLocalSession(ptyId, term.cols, term.rows)
      .then(async () => {
        if (disposed) {
          closeSession(ptyId);
          return;
        }
        opened = true;
        const offData = term.onData((data) => void writeStdin(ptyId, data));
        unlisteners.push(() => offData.dispose());

        const offOutput = await onSessionOutput((payload) => {
          if (payload.id === ptyId) term.write(base64ToBytes(payload.data));
        });
        unlisteners.push(offOutput);

        const offExit = await onSessionExit((id) => {
          if (id === ptyId) term.write("\r\n\x1b[38;2;86;92;100m[sessão encerrada]\x1b[0m\r\n");
        });
        unlisteners.push(offExit);
      })
      .catch((err) => {
        term.write(`\r\n\x1b[38;2;240;120;90mfalha ao abrir PTY: ${err}\x1b[0m\r\n`);
        // repropaga para o vite logar como unhandled rejection durante o dev
        return Promise.reject(err);
      });

    const observer = new ResizeObserver(() => {
      fit.fit();
      if (opened) void resizePty(ptyId, term.cols, term.rows);
    });
    observer.observe(host);

    return () => {
      disposed = true;
      observer.disconnect();
      for (const off of unlisteners) off();
      term.dispose();
      void closeSession(ptyId);
    };
  }, [sessionId]);

  return <div ref={hostRef} className="term__host" />;
}
