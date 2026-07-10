import { TABS, STATUS_COLORS } from "../mock";
import { useUiStore } from "../stores/ui";

export function TabsToolbar() {
  const activeHostId = useUiStore((s) => s.activeHostId);
  const selectHost = useUiStore((s) => s.selectHost);
  const view = useUiStore((s) => s.view);

  return (
    <div className="tabsbar">
      {TABS.map((tab) => {
        const active = tab.id === activeHostId;
        return (
          <div
            key={tab.id}
            className={`tab${active ? " tab--active" : ""}`}
            onClick={() => selectHost(tab.id)}
          >
            <span
              className={`tab__dot${tab.status === "attention" ? " tab__dot--pulse" : ""}`}
              style={{ background: STATUS_COLORS[tab.status] }}
            />
            <span className="tab__label">{tab.label}</span>
            <span className="tab__close">×</span>
          </div>
        );
      })}
      <div className="tabsbar__new">+</div>
      <div className="tabsbar__spacer" />
      <div className="tabsbar__right">
        <div className="tmux-badge">
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="#63d29b" strokeWidth="2">
            <rect x="3" y="3" width="18" height="18" rx="2" />
            <path d="M3 9h18M9 21V9" />
          </svg>
          <span className="tmux-badge__label">tmux: atlas-api</span>
        </div>
        <div className="detach-btn" title="Detach — keeps running on server">
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="#e0a15e" strokeWidth="2">
            <path d="M12 4l-6 6h4v6h4v-6h4z" fill="#e0a15e" stroke="none" />
            <path d="M5 20h14" />
          </svg>
          <span className="detach-btn__label">Detach</span>
        </div>
        <div className="seg-control">
          <div className={`seg-control__btn${view === "term" ? " seg-control__btn--on" : ""}`} title="Terminal view">
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <rect x="3" y="4" width="18" height="16" rx="2" />
              <path d="M7 9l3 3-3 3M12 15h5" />
            </svg>
          </div>
          <div className={`seg-control__btn${view === "grid" ? " seg-control__btn--on" : ""}`} title="Grid view — all sessions">
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
