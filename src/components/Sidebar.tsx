import { useEffect, useState } from "react";
import { deleteHost } from "../lib/ipc";
import { groupHosts, useHostsStore } from "../stores/hosts";
import { useSessionsStore } from "../stores/sessions";
import { useUiStore } from "../stores/ui";
import { useVaultStore } from "../stores/vault";
import { statusColor, type Host, type SessionInfo } from "../types";

function hostSession(sessions: SessionInfo[], hostId: string): SessionInfo | undefined {
  return sessions.find((s) => s.hostId === hostId);
}

function HostRow({ host }: { host: Host }) {
  const sessions = useSessionsStore((s) => s.sessions);
  const activeId = useSessionsStore((s) => s.activeId);
  const open = useSessionsStore((s) => s.open);
  const focus = useSessionsStore((s) => s.focus);
  const openModal = useUiStore((s) => s.openModal);
  const loadHosts = useHostsStore((s) => s.load);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);

  const session = hostSession(sessions, host.id);
  const isActive = session != null && session.id === activeId;
  const attention = session?.attention ?? false;
  const color = attention ? "#f0785a" : statusColor(session?.status);
  const connected = session?.status === "connected";
  const connecting = session?.status === "connecting";
  const nameColor = isActive ? "#f4f6f8" : session ? "#d2d6db" : "#8b9199";

  const dotClasses = ["host-row__dot"];
  if (connected && !attention) dotClasses.push("host-row__dot--glow");
  if (connecting) dotClasses.push("host-row__dot--spin");
  if (attention) dotClasses.push("host-row__dot--pulse");

  return (
    <div
      className={`host-row${isActive ? " host-row--active" : ""}`}
      onClick={() => (session ? focus(session.id) : open(host.id))}
      onContextMenu={(e) => {
        e.preventDefault();
        setMenu({ x: e.clientX, y: e.clientY });
      }}
    >
      <span className={dotClasses.join(" ")} style={{ background: color, color }} />
      <div className="host-row__body">
        <div className="host-row__top">
          <span className="host-row__name" style={{ color: nameColor }}>
            {host.name}
          </span>
          {(host.autoAttach || host.startupMode !== "shell") && (
            <span className="host-row__tmux">tmux</span>
          )}
        </div>
        <div className="host-row__addr">
          {host.user ? `${host.user}@${host.host}` : host.host}
        </div>
      </div>
      {attention && <span className="host-row__badge host-row__badge--attention">!</span>}
      {connecting && !attention && <span className="host-row__badge host-row__badge--reconnect">⟳</span>}
      {menu && (
        <>
          <div
            style={{ position: "fixed", inset: 0, zIndex: 79 }}
            onClick={(e) => {
              e.stopPropagation();
              setMenu(null);
            }}
            onContextMenu={(e) => {
              e.preventDefault();
              setMenu(null);
            }}
          />
          <div className="ctx-menu" style={{ left: menu.x, top: menu.y }} onClick={(e) => e.stopPropagation()}>
            <div
              className="ctx-menu__item"
              onClick={() => {
                setMenu(null);
                openModal({ kind: "newSession", hostId: host.id });
              }}
            >
              Nova sessão…
            </div>
            <div
              className="ctx-menu__item"
              onClick={() => {
                setMenu(null);
                openModal({ kind: "editHost", hostId: host.id });
              }}
            >
              Editar host…
            </div>
            <div
              className="ctx-menu__item"
              onClick={() => {
                setMenu(null);
                openModal({ kind: "installTmux", hostId: host.id });
              }}
            >
              Instalar tmux…
            </div>
            <div
              className="ctx-menu__item ctx-menu__item--danger"
              onClick={() => {
                setMenu(null);
                void deleteHost(host.id).then(loadHosts);
              }}
            >
              Excluir host
            </div>
          </div>
        </>
      )}
    </div>
  );
}

export function Sidebar() {
  const hosts = useHostsStore((s) => s.hosts);
  const load = useHostsStore((s) => s.load);
  const openModal = useUiStore((s) => s.openModal);
  const sessions = useSessionsStore((s) => s.sessions);
  const focus = useSessionsStore((s) => s.focus);

  useEffect(() => {
    void load();
  }, [load]);

  const groups = groupHosts(hosts);
  const attentionSessions = sessions.filter((s) => s.attention);
  const firstAttention = attentionSessions[0];
  const attentionHost = firstAttention
    ? hosts.find((h) => h.id === firstAttention.hostId)
    : undefined;

  return (
    <div className="sidebar">
      <div className="sidebar__header">
        <span className="sidebar__title">Hosts &amp; Sessions</span>
        <div
          className="sidebar__add"
          style={{ cursor: "pointer" }}
          title="Add host"
          onClick={() => openModal({ kind: "addHost" })}
        >
          +
        </div>
      </div>

      {firstAttention && (
        <div className="attention-banner" onClick={() => focus(firstAttention.id)}>
          <span className="attention-banner__dot" />
          <div className="attention-banner__body">
            <div className="attention-banner__title">
              {attentionSessions.length === 1
                ? "1 session needs attention"
                : `${attentionSessions.length} sessions need attention`}
            </div>
            <div className="attention-banner__sub">
              {attentionHost?.name ?? firstAttention.hostId} · aguardando input
            </div>
          </div>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#f0785a" strokeWidth="2">
            <path d="M9 6l6 6-6 6" />
          </svg>
        </div>
      )}

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

      <VaultFooter />
    </div>
  );
}

function VaultFooter() {
  const locked = useVaultStore((s) => s.locked);
  const count = useVaultStore((s) => s.count);
  const openModal = useVaultStore((s) => s.openModal);

  return (
    <div className="vault-footer" onClick={openModal} style={{ cursor: "pointer" }}>
      <div className="vault-footer__icon">
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="#e0a15e" strokeWidth="1.8">
          {locked ? (
            <>
              <rect x="4" y="10" width="16" height="11" rx="2" />
              <path d="M8 10V7a4 4 0 0 1 8 0v3" />
            </>
          ) : (
            <>
              <rect x="4" y="10" width="16" height="11" rx="2" />
              <path d="M8 10V7a4 4 0 0 1 7.5-2" />
            </>
          )}
        </svg>
      </div>
      <div className="vault-footer__body">
        <div className="vault-footer__title">{locked ? "Vault locked" : "Vault unlocked"}</div>
        <div className="vault-footer__sub">
          {count} credentials · Touch&nbsp;ID
        </div>
      </div>
      <div
        className="vault-footer__dot"
        style={{ background: locked ? "#565c64" : "#63d29b" }}
      />
    </div>
  );
}
