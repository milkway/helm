import { closeTab, detachTab } from "../lib/termRegistry";
import { useHostsStore } from "../stores/hosts";
import { useSessionsStore } from "../stores/sessions";
import { useUiStore, type GridCols } from "../stores/ui";
import { statusColor, tmuxSessionName } from "../types";

const DENSITIES: GridCols[] = [2, 3, 4];

export function TabsToolbar() {
  const sessions = useSessionsStore((s) => s.sessions);
  const activeId = useSessionsStore((s) => s.activeId);
  const focus = useSessionsStore((s) => s.focus);
  const hosts = useHostsStore((s) => s.hosts);
  const view = useUiStore((s) => s.view);
  const openModal = useUiStore((s) => s.openModal);
  const setView = useUiStore((s) => s.setView);
  const gridCols = useUiStore((s) => s.gridCols);
  const setGridCols = useUiStore((s) => s.setGridCols);

  const hostName = (hostId: string) => hosts.find((h) => h.id === hostId)?.name ?? hostId;
  const activeSession = sessions.find((s) => s.id === activeId);
  const activeHost = activeSession ? hosts.find((h) => h.id === activeSession.hostId) : undefined;
  const activeUsesTmux =
    activeSession?.params != null
      ? activeSession.params.mode !== "shell"
      : (activeHost?.autoAttach ?? false) || (activeHost && activeHost.startupMode !== "shell");
  const tmuxActive = activeSession?.status === "connected" && activeUsesTmux ? activeHost : undefined;
  const tmuxName = activeSession?.params?.sessionName
    ? tmuxSessionName(activeSession.params.sessionName)
    : tmuxActive
      ? tmuxSessionName(tmuxActive.name)
      : null;

  return (
    <div className="tabsbar">
      {sessions.map((session) => {
        const active = session.id === activeId;
        return (
          <div
            key={session.id}
            className={`tab${active ? " tab--active" : ""}`}
            onClick={() => focus(session.id)}
          >
            <span className="tab__dot" style={{ background: statusColor(session.status) }} />
            <span className="tab__label">
              {session.params?.sessionName ?? hostName(session.hostId)}
            </span>
            <span
              className="tab__close"
              onClick={(e) => {
                e.stopPropagation();
                closeTab(session.id);
              }}
            >
              ×
            </span>
          </div>
        );
      })}
      <div
        className="tabsbar__new"
        title="Nova sessão…"
        onClick={() => openModal({ kind: "newSession", hostId: activeSession?.hostId })}
      >
        +
      </div>
      <div className="tabsbar__spacer" />
      <div className="tabsbar__right">
        <div className="tmux-badge" style={tmuxActive ? undefined : { opacity: 0.45 }}>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="#63d29b" strokeWidth="2">
            <rect x="3" y="3" width="18" height="18" rx="2" />
            <path d="M3 9h18M9 21V9" />
          </svg>
          <span className="tmux-badge__label">tmux: {tmuxName ?? "—"}</span>
        </div>
        <div
          className="detach-btn"
          title="Detach — keeps running on server"
          style={tmuxActive ? undefined : { opacity: 0.45, cursor: "default" }}
          onClick={() => {
            if (tmuxActive && activeSession) detachTab(activeSession.id);
          }}
        >
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="#e0a15e" strokeWidth="2">
            <path d="M12 4l-6 6h4v6h4v-6h4z" fill="#e0a15e" stroke="none" />
            <path d="M5 20h14" />
          </svg>
          <span className="detach-btn__label">Detach</span>
        </div>
        {view === "grid" && (
          <div className="seg-control seg-control--mono">
            {DENSITIES.map((n) => (
              <div
                key={n}
                title={`${n} colunas`}
                className={`seg-control__btn seg-control__btn--wide${gridCols === n ? " seg-control__btn--on" : ""}`}
                onClick={() => setGridCols(n)}
              >
                {n}×
              </div>
            ))}
          </div>
        )}
        <div className="seg-control">
          <div
            className={`seg-control__btn${view === "term" ? " seg-control__btn--on" : ""}`}
            title="Terminal view"
            onClick={() => setView("term")}
          >
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <rect x="3" y="4" width="18" height="16" rx="2" />
              <path d="M7 9l3 3-3 3M12 15h5" />
            </svg>
          </div>
          <div
            className={`seg-control__btn${view === "grid" ? " seg-control__btn--on" : ""}`}
            title="Grid view — all sessions"
            onClick={() => setView("grid")}
          >
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <rect x="3" y="3" width="7" height="7" rx="1" />
              <rect x="14" y="3" width="7" height="7" rx="1" />
              <rect x="3" y="14" width="7" height="7" rx="1" />
              <rect x="14" y="14" width="7" height="7" rx="1" />
            </svg>
          </div>
        </div>
      </div>
    </div>
  );
}
