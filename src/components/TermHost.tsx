import { useEffect, useRef } from "react";
import "@xterm/xterm/css/xterm.css";
import { claimHolder, fitEntry, getEntry, releaseHolder } from "../lib/termRegistry";

interface TermHostProps {
  uiId: string;
  hostId: string;
  active: boolean;
  /** 13 na view terminal, 11.5 nos cards do grid */
  fontSize: number;
}

/**
 * Hospeda o terminal da sessão (posse é do termRegistry): registra este nó
 * como holder e o registry anexa/cria o xterm já dentro do DOM.
 */
export function TermHost({ uiId, hostId, active, fontSize }: TermHostProps) {
  const holderRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const holder = holderRef.current;
    if (!holder) return;

    claimHolder(uiId, hostId, holder, fontSize);

    const observer = new ResizeObserver(() => fitEntry(uiId));
    observer.observe(holder);

    return () => {
      observer.disconnect();
      releaseHolder(uiId, holder);
    };
  }, [uiId, hostId, fontSize]);

  useEffect(() => {
    if (active) getEntry(uiId)?.term.focus();
  }, [active, uiId]);

  return <div ref={holderRef} className="term__holder" />;
}
