import { useEffect, useState } from "react";
import { deleteHost } from "../lib/ipc";
import { groupHosts, useHostsStore } from "../stores/hosts";
import { useSessionsStore } from "../stores/sessions";
import { useUiStore } from "../stores/ui";
import { useVaultStore } from "../stores/vault";
import { statusColor, type Host, type SessionInfo } from "../types";
import { useT } from "../i18n";

function hostSession(sessions: SessionInfo[], hostId: string): SessionInfo | undefined {
  return sessions.find((s) => s.hostId === hostId);
}

function HostRow({ host }: { host: Host }) {
  const t = useT();
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
  const color = attention ? "var(--st-attention)" : statusColor(session?.status);
  const connected = session?.status === "connected";
  const connecting = session?.status === "connecting";
  const nameColor = isActive ? "var(--text-strong)" : session ? "var(--text)" : "var(--text-3)";

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
          <div
            className="ctx-menu"
            style={{
              // mantém o menu dentro da janela (não corta na borda)
              left: Math.min(menu.x, window.innerWidth - 176),
              top: Math.min(menu.y, window.innerHeight - 176),
            }}
            onClick={(e) => e.stopPropagation()}
          >
            <div
              className="ctx-menu__item"
              onClick={() => {
                setMenu(null);
                openModal({ kind: "newSession", hostId: host.id });
              }}
            >
              {t("sb.newSession")}
            </div>
            <div
              className="ctx-menu__item"
              onClick={() => {
                setMenu(null);
                openModal({ kind: "editHost", hostId: host.id });
              }}
            >
              {t("sb.editHost")}
            </div>
            <div
              className="ctx-menu__item"
              onClick={() => {
                setMenu(null);
                openModal({ kind: "installTmux", hostId: host.id });
              }}
            >
              {t("sb.installTmux")}
            </div>
            <div
              className="ctx-menu__item ctx-menu__item--danger"
              onClick={() => {
                setMenu(null);
                void deleteHost(host.id).then(loadHosts);
              }}
            >
              {t("sb.deleteHost")}
            </div>
          </div>
        </>
      )}
    </div>
  );
}

export function Sidebar() {
  const t = useT();
  const hosts = useHostsStore((s) => s.hosts);
  const load = useHostsStore((s) => s.load);
  const loadError = useHostsStore((s) => s.error);
  const openModal = useUiStore((s) => s.openModal);
  const collapsedGroups = useUiStore((s) => s.collapsedGroups);
  const toggleGroup = useUiStore((s) => s.toggleGroup);
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
        <span className="sidebar__title">{t("sb.title")}</span>
        <div
          className="sidebar__add"
          style={{ cursor: "pointer" }}
          title={t("sb.addHost")}
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
                ? t("sb.attn1")
                : t("sb.attnN", { n: attentionSessions.length })}
            </div>
            <div className="attention-banner__sub">
              {attentionHost?.name ?? firstAttention.hostId} · {t("sb.awaiting")}
            </div>
          </div>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="var(--st-attention)" strokeWidth="2">
            <path d="M9 6l6 6-6 6" />
          </svg>
        </div>
      )}

      <div className="sidebar__list">
        {loadError && (
          <div className="sidebar__empty" style={{ color: "var(--st-attention-text)" }}>
            {t("sb.loadError", { msg: loadError })}
          </div>
        )}
        {groups.length === 0 && !loadError && (
          <div className="sidebar__empty">{t("sb.empty")}</div>
        )}
        {groups.map((group) => {
          const collapsed = collapsedGroups.includes(group.name);
          return (
          <div className="group" key={group.name}>
            <div
              className="group__header"
              style={{ cursor: "pointer" }}
              onClick={() => toggleGroup(group.name)}
            >
              <svg
                width="11"
                height="11"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2.4"
                style={{ transform: collapsed ? "rotate(-90deg)" : "none", transition: "transform .15s" }}
              >
                <path d="M6 9l6 6 6-6" />
              </svg>
              <span className="group__name">{group.name}</span>
              <span className="group__count">{group.hosts.length}</span>
            </div>
            {!collapsed &&
              group.hosts.map((host) => <HostRow key={host.id} host={host} />)}
          </div>
          );
        })}
      </div>

      <VaultFooter />
    </div>
  );
}

function VaultFooter() {
  const t = useT();
  const locked = useVaultStore((s) => s.locked);
  const count = useVaultStore((s) => s.count);
  const openModal = useVaultStore((s) => s.openModal);

  return (
    <div className="vault-footer" onClick={openModal} style={{ cursor: "pointer" }}>
      <div className="vault-footer__icon">
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" strokeWidth="1.8">
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
        <div className="vault-footer__title">{locked ? t("sb.vaultLocked") : t("sb.vaultUnlocked")}</div>
        <div className="vault-footer__sub">
          {t("sb.credentials", { n: count })}
        </div>
      </div>
      <div
        className="vault-footer__dot"
        style={{ background: locked ? "var(--st-idle)" : "var(--st-connected)" }}
      />
    </div>
  );
}
