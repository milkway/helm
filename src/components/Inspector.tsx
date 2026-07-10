import { META, ACTIONS } from "../mock";

export function Inspector() {
  return (
    <div className="inspector">
      <div className="inspector__header">
        <div className="inspector__host">
          <span className="inspector__host-dot" />
          <span className="inspector__host-name">atlas-api</span>
        </div>
        <div className="inspector__host-addr">deploy@10.4.2.18</div>
      </div>

      <div className="inspector__scroll">
        <div className="inspector__section">Connection</div>
        {META.map((m) => (
          <div className="meta-row" key={m.k}>
            <span className="meta-row__k">{m.k}</span>
            <span className="meta-row__v">{m.v}</span>
          </div>
        ))}

        <div className="inspector__section inspector__section--gap">Quick launch</div>
        <div className="quick-list">
          <div className="clmux-card">
            <div className="clmux-card__icon">cl</div>
            <div className="card-body">
              <div className="clmux-card__title">clmux → claude</div>
              <div className="clmux-card__sub">cd atlas-api · tmux · claude</div>
            </div>
          </div>
          {ACTIONS.map((a) => (
            <div className="action-card" key={a.title}>
              <div className="action-card__icon">{a.icon}</div>
              <div className="card-body">
                <div className="action-card__title">{a.title}</div>
                <div className="action-card__sub">{a.sub}</div>
              </div>
            </div>
          ))}
        </div>

        <div className="inspector__section inspector__section--gap">Credential</div>
        <div className="cred-card">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="#e0a15e" strokeWidth="1.8">
            <rect x="4" y="10" width="16" height="11" rx="2" />
            <path d="M8 10V7a4 4 0 0 1 8 0v3" />
          </svg>
          <div className="card-body">
            <div className="cred-card__title">id_ed25519 · atlas</div>
            <div className="cred-card__sub">key + passphrase</div>
          </div>
          <span className="cred-card__mask">••••</span>
        </div>
      </div>
    </div>
  );
}
