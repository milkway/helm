const IS_MAC = navigator.userAgent.includes("Mac");

export function Titlebar() {
  return (
    <div className="titlebar" data-tauri-drag-region>
      {IS_MAC ? (
        <div className="titlebar__lights titlebar__lights--spacer" />
      ) : (
        <div className="titlebar__lights">
          <div className="titlebar__light" style={{ background: "#f0785a" }} />
          <div className="titlebar__light" style={{ background: "#e0b15e" }} />
          <div className="titlebar__light" style={{ background: "#63d29b" }} />
        </div>
      )}
      <div className="titlebar__brand">
        <div className="titlebar__logo">H</div>
        <span className="titlebar__name">Helm</span>
      </div>
      <div className="titlebar__center" data-tauri-drag-region>
        <div className="titlebar__search">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="#7d848d" strokeWidth="2">
            <circle cx="11" cy="11" r="7" />
            <path d="M21 21l-4-4" />
          </svg>
          <span className="titlebar__search-text">Search hosts, sessions, commands…</span>
          <span className="titlebar__kbd">⌘K</span>
        </div>
      </div>
      <div className="titlebar__right">
        <div className="titlebar__connected">
          <span className="titlebar__connected-dot" />
          <span className="titlebar__connected-label">6 connected</span>
        </div>
        <div className="titlebar__settings">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="#a2a8b0" strokeWidth="1.8">
            <circle cx="12" cy="12" r="3" />
            <path d="M12 2v3M12 19v3M4.2 4.2l2.1 2.1M17.7 17.7l2.1 2.1M2 12h3M19 12h3M4.2 19.8l2.1-2.1M17.7 6.3l2.1-2.1" />
          </svg>
        </div>
      </div>
    </div>
  );
}
