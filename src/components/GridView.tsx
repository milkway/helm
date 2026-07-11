import { useEffect, useState } from "react";
import { cardToPng, copySelection, copyVisible } from "../lib/captures";
import { detachTab, ensureTerm } from "../lib/termRegistry";
import { useHostsStore } from "../stores/hosts";
import { useSessionsStore } from "../stores/sessions";
import { useUiStore } from "../stores/ui";
import { statusColor, type Host, type SessionInfo, type SessionStatus } from "../types";
import { TermHost } from "./TermHost";
import { useT } from "../i18n";

const CARD_BODY_HEIGHT: Record<number, number> = { 2: 150, 3: 120, 4: 96 };

function tagFor(status: SessionStatus): { label: string; color: string; bg: string } {
  switch (status) {
    case "connected":
      return { label: "live", color: "#63d29b", bg: "rgba(99,210,155,.12)" };
    case "connecting":
    case "reconnecting":
      return { label: "reconnecting", color: "#e0b15e", bg: "rgba(224,177,94,.16)" };
    case "error":
      return { label: "error", color: "#f0785a", bg: "rgba(240,120,90,.16)" };
    default:
      return { label: "idle", color: "#565c64", bg: "rgba(255,255,255,.05)" };
  }
}

function GridCard({ session, host, dense }: { session: SessionInfo; host?: Host; dense: boolean }) {
  const t = useT();
  const focus = useSessionsStore((s) => s.focus);
  const activeId = useSessionsStore((s) => s.activeId);
  const setView = useUiStore((s) => s.setView);
  const gridCols = useUiStore((s) => s.gridCols);
  const [hasSelection, setHasSelection] = useState(false);
  const [copied, setCopied] = useState<"out" | "png" | "sel" | null>(null);

  useEffect(() => {
    let dispose: (() => void) | undefined;
    let cancelled = false;
    void ensureTerm(session.id, session.hostId).then((entry) => {
      if (cancelled) return;
      const off = entry.term.onSelectionChange(() =>
        setHasSelection(entry.term.getSelection().length > 0),
      );
      dispose = () => off.dispose();
    });
    return () => {
      cancelled = true;
      dispose?.();
    };
  }, [session.id, session.hostId]);

  const flash = (kind: "out" | "png" | "sel") => {
    setCopied(kind);
    setTimeout(() => setCopied(null), 1200);
  };

  const name = host?.name ?? session.hostId;
  const addr = host ? (host.user ? `${host.user}@${host.host}` : host.host) : "";
  const tag = tagFor(session.status);
  const color = statusColor(session.status);
  const openInTerm = () => {
    focus(session.id);
    setView("term");
  };

  return (
    <div className={`grid-card${session.status === "error" ? " grid-card--attention" : ""}`}>
      <div className="grid-card__header" onClick={openInTerm} title={t("grid.openTerm")}>
        <span
          className={`grid-card__dot${session.status === "connecting" || session.status === "reconnecting" ? " host-row__dot--spin" : ""}`}
          style={{ background: color }}
        />
        <div className="grid-card__names">
          <div className="grid-card__name">{name}</div>
          {!dense && <div className="grid-card__addr">{addr}</div>}
        </div>
        {!dense && (
          <span className="grid-card__tag" style={{ color: tag.color, background: tag.bg }}>
            {tag.label}
          </span>
        )}
        {!dense && (
          <>
            <div
              className="grid-card__icon"
              title={t("grid.copyOut")}
              onClick={(e) => {
                e.stopPropagation();
                void copyVisible(session.id).then((ok) => ok && flash("out"));
              }}
            >
              {copied === "out" ? (
                "✓"
              ) : (
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <rect x="9" y="9" width="11" height="11" rx="2" />
                  <path d="M5 15V5a2 2 0 0 1 2-2h10" />
                </svg>
              )}
            </div>
            <div
              className="grid-card__icon"
              title={t("grid.png")}
              onClick={(e) => {
                e.stopPropagation();
                void cardToPng(session.id, name, addr).then((ok) => ok && flash("png"));
              }}
            >
              {copied === "png" ? (
                "✓"
              ) : (
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <path d="M4 7h3l2-3h6l2 3h3v12H4z" />
                  <circle cx="12" cy="13" r="3.5" />
                </svg>
              )}
            </div>
            <div
              className="grid-card__icon grid-card__icon--detach"
              title="Detach"
              onClick={(e) => {
                e.stopPropagation();
                detachTab(session.id);
              }}
            >
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <path d="M12 4l-6 6h4v6h4v-6h4z" fill="currentColor" stroke="none" />
                <path d="M5 20h14" />
              </svg>
            </div>
          </>
        )}
      </div>
      <div className="grid-card__body" style={{ height: CARD_BODY_HEIGHT[gridCols] }}>
        <TermHost
          uiId={session.id}
          hostId={session.hostId}
          active={session.id === activeId}
          fontSize={11.5}
        />
        {hasSelection && (
          <div className="sel-chip">
            <div
              className="sel-chip__btn"
              onClick={() => void copySelection(session.id).then((ok) => ok && flash("sel"))}
            >
              <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <rect x="9" y="9" width="11" height="11" rx="2" />
                <path d="M5 15V5a2 2 0 0 1 2-2h10" />
              </svg>
              {copied === "sel" ? t("grid.copied") : t("grid.copySel")}
            </div>
            <div className="sel-chip__div" />
            <div
              className="sel-chip__btn"
              onClick={() => void cardToPng(session.id, name, addr).then((ok) => ok && flash("png"))}
            >
              <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <path d="M4 7h3l2-3h6l2 3h3v12H4z" />
                <circle cx="12" cy="13" r="3.5" />
              </svg>
              PNG
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

export function GridView() {
  const t = useT();
  const sessions = useSessionsStore((s) => s.sessions);
  const hosts = useHostsStore((s) => s.hosts);
  const gridCols = useUiStore((s) => s.gridCols);

  if (sessions.length === 0) {
    return (
      <div className="term term--empty">
        <span className="term__hint">{t("term.gridEmpty")}</span>
      </div>
    );
  }

  return (
    <div className="grid-view" style={{ gridTemplateColumns: `repeat(${gridCols}, 1fr)` }}>
      {sessions.map((session) => (
        <GridCard
          key={`${session.id}:${session.generation}`}
          session={session}
          host={hosts.find((h) => h.id === session.hostId)}
          dense={gridCols >= 4}
        />
      ))}
    </div>
  );
}
