import { useEffect } from "react";
import { groupHosts, useHostsStore } from "../stores/hosts";
import { useSessionsStore } from "../stores/sessions";
import { statusColor, type Host, type SessionInfo } from "../types";

function hostSession(sessions: SessionInfo[], hostId: string): SessionInfo | undefined {
  return sessions.find((s) => s.hostId === hostId);
}

function HostRow({ host }: { host: Host }) {
  const sessions = useSessionsStore((s) => s.sessions);
  const activeId = useSessionsStore((s) => s.activeId);
  const open = useSessionsStore((s) => s.open);
  const focus = useSessionsStore((s) => s.focus);

  const session = hostSession(sessions, host.id);
  const isActive = session != null && session.id === activeId;
  const color = statusColor(session?.status);
  const connected = session?.status === "connected";
  const connecting = session?.status === "connecting";
  const nameColor = isActive ? "#f4f6f8" : session ? "#d2d6db" : "#8b9199";

  const dotClasses = ["host-row__dot"];
  if (connected) dotClasses.push("host-row__dot--glow");
  if (connecting) dotClasses.push("host-row__dot--spin");

  return (
    <div
      className={`host-row${isActive ? " host-row--active" : ""}`}
      onClick={() => (session ? focus(session.id) : open(host.id))}
    >
      <span className={dotClasses.join(" ")} style={{ background: color, color }} />
      <div className="host-row__body">
        <div className="host-row__top">
          <span className="host-row__name" style={{ color: nameColor }}>
            {host.name}
          </span>
          {host.startupMode !== "shell" && <span className="host-row__tmux">tmux</span>}
        </div>
        <div className="host-row__addr">
          {host.user ? `${host.user}@${host.host}` : host.host}
        </div>
      </div>
      {connecting && <span className="host-row__badge host-row__badge--reconnect">⟳</span>}
    </div>
  );
}

export function Sidebar() {
  const hosts = useHostsStore((s) => s.hosts);
  const load = useHostsStore((s) => s.load);

  useEffect(() => {
    void load();
  }, [load]);

  const groups = groupHosts(hosts);

  return (
    <div className="sidebar">
      <div className="sidebar__header">
        <span className="sidebar__title">Hosts &amp; Sessions</span>
        <div className="sidebar__add">+</div>
      </div>

      <div className="sidebar__list">
        {groups.length === 0 && (
          <div className="sidebar__empty">Nenhum host ainda</div>
        )}
        {groups.map((group) => (
          <div className="group" key={group.name}>
            <div className="group__header">
              <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4">
                <path d="M6 9l6 6 6-6" />
              </svg>
              <span className="group__name">{group.name}</span>
              <span className="group__count">{group.hosts.length}</span>
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
          <div className="vault-footer__title">Vault</div>
          <div className="vault-footer__sub">disponível na Fase 5</div>
        </div>
        <div className="vault-footer__dot" style={{ background: "#565c64" }} />
      </div>
    </div>
  );
}
