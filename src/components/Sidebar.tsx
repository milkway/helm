import { GROUPS, STATUS_COLORS, type MockHost } from "../mock";
import { useUiStore } from "../stores/ui";

function HostRow({ host }: { host: MockHost }) {
  const activeHostId = useUiStore((s) => s.activeHostId);
  const selectHost = useUiStore((s) => s.selectHost);
  const isActive = host.id === activeHostId;
  const color = STATUS_COLORS[host.status];
  const nameColor = isActive ? "#f4f6f8" : host.status === "idle" ? "#8b9199" : "#d2d6db";

  const dotClasses = ["host-row__dot"];
  if (host.status === "connected" || host.status === "attention") dotClasses.push("host-row__dot--glow");
  if (host.status === "attention") dotClasses.push("host-row__dot--pulse");
  if (host.status === "reconnect") dotClasses.push("host-row__dot--spin");

  return (
    <div
      className={`host-row${isActive ? " host-row--active" : ""}`}
      onClick={() => selectHost(host.id)}
    >
      <span className={dotClasses.join(" ")} style={{ background: color, color }} />
      <div className="host-row__body">
        <div className="host-row__top">
          <span className="host-row__name" style={{ color: nameColor }}>
            {host.name}
          </span>
          {host.tmux && <span className="host-row__tmux">tmux</span>}
        </div>
        <div className="host-row__addr">{host.addr}</div>
      </div>
      {(host.status === "attention" || host.status === "reconnect") && (
        <span className={`host-row__badge host-row__badge--${host.status}`}>{host.badge}</span>
      )}
    </div>
  );
}

export function Sidebar() {
  return (
    <div className="sidebar">
      <div className="sidebar__header">
        <span className="sidebar__title">Hosts &amp; Sessions</span>
        <div className="sidebar__add">+</div>
      </div>

      <div className="attention-banner">
        <span className="attention-banner__dot" />
        <div className="attention-banner__body">
          <div className="attention-banner__title">1 session needs attention</div>
          <div className="attention-banner__sub">gpu-train v3 · claude awaiting input</div>
        </div>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#f0785a" strokeWidth="2">
          <path d="M9 6l6 6-6 6" />
        </svg>
      </div>

      <div className="sidebar__list">
        {GROUPS.map((group) => (
          <div className="group" key={group.name}>
            <div className="group__header">
              <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4">
                <path d="M6 9l6 6 6-6" />
              </svg>
              <span className="group__name">{group.name}</span>
              <span className="group__count">{group.count}</span>
            </div>
            {group.hosts.map((host) => (
              <HostRow key={host.id} host={host} />
            ))}
          </div>
        ))}
      </div>

      <div className="vault-footer">
        <div className="vault-footer__icon">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="#e0a15e" strokeWidth="1.8">
            <rect x="4" y="10" width="16" height="11" rx="2" />
            <path d="M8 10V7a4 4 0 0 1 8 0v3" />
          </svg>
        </div>
        <div className="vault-footer__body">
          <div className="vault-footer__title">Vault unlocked</div>
          <div className="vault-footer__sub">12 credentials · Touch&nbsp;ID</div>
        </div>
        <div className="vault-footer__dot" />
      </div>
    </div>
  );
}
