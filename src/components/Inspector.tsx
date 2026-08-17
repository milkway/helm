import { useEffect, useState } from "react";
import { detachTab } from "../lib/termRegistry";
import { useHostsStore } from "../stores/hosts";
import { useSessionsStore } from "../stores/sessions";
import { useUiStore } from "../stores/ui";
import { sessionUsesTmux, statusColor, tmuxSessionName } from "../types";
import { useT } from "../i18n";

function uptime(connectedAt: number | null): string {
  if (!connectedAt) return "—";
  const s = Math.floor((Date.now() - connectedAt) / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${s % 60}s`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ${String(m % 60).padStart(2, "0")}m`;
  return `${Math.floor(h / 24)}d ${String(h % 24).padStart(2, "0")}h`;
}

export function Inspector() {
  const sessions = useSessionsStore((s) => s.sessions);
  const activeId = useSessionsStore((s) => s.activeId);
  const open = useSessionsStore((s) => s.open);
  const hosts = useHostsStore((s) => s.hosts);
  const openModal = useUiStore((s) => s.openModal);
  const defaultAgent = useUiStore((s) => s.defaultAgent);
  const t = useT();

  // rerender periódico para o uptime andar
  const [, setTick] = useState(0);
  useEffect(() => {
    const t = setInterval(() => setTick((n) => n + 1), 10_000);
    return () => clearInterval(t);
  }, []);

  const session = sessions.find((s) => s.id === activeId);
  const host = session ? hosts.find((h) => h.id === session.hostId) : undefined;

  if (!session || !host) {
    return (
      <div className="inspector">
        <div className="inspector__header">
          <div className="inspector__host">
            <span className="inspector__host-dot" style={{ background: "var(--st-idle)", boxShadow: "none" }} />
            <span className="inspector__host-name">—</span>
          </div>
          <div className="inspector__host-addr">{t("in.none")}</div>
        </div>
      </div>
    );
  }

  const color = statusColor(session.status);
  const agentLabel = defaultAgent === "codex" ? t("ns.codex") : t("ns.claude");
  const meta: { k: string; v: string }[] = [
    { k: t("in.host"), v: host.host },
    { k: t("in.user"), v: host.user ?? t("in.config") },
    { k: t("in.port"), v: host.port ? String(host.port) : t("in.config") },
    { k: t("in.latency"), v: "—" },
    { k: t("in.uptime"), v: uptime(session.connectedAt) },
    { k: t("in.autoReconnect"), v: host.autoReconnect ? t("in.on") : t("in.off") },
    { k: t("in.autoAttach"), v: host.autoAttach ? t("in.on") : t("in.off") },
  ];

  return (
    <div className="inspector">
      <div className="inspector__header">
        <div className="inspector__host">
          <span
            className="inspector__host-dot"
            style={{ background: color, boxShadow: session.status === "connected" ? `0 0 8px ${color}` : "none" }}
          />
          <span className="inspector__host-name">{host.name}</span>
        </div>
        <div className="inspector__host-addr">
          {host.user ? `${host.user}@${host.host}` : host.host}
        </div>
      </div>

      <div className="inspector__scroll">
        <div className="inspector__section">{t("in.connection")}</div>
        {meta.map((m) => (
          <div className="meta-row" key={m.k}>
            <span className="meta-row__k">{m.k}</span>
            <span className="meta-row__v">{m.v}</span>
          </div>
        ))}

        <div className="inspector__section inspector__section--gap">{t("in.quickLaunch")}</div>
        <div className="quick-list">
          <div
            className="clmux-card"
            style={{ cursor: "pointer" }}
            title="clmux"
            onClick={() =>
              open(host.id, {
                mode: "clmux",
                sessionName: tmuxSessionName(host.name),
                projectDir: host.projectDir ?? undefined,
                agent: defaultAgent,
              })
            }
          >
            <div className="clmux-card__icon">cl</div>
            <div className="card-body">
              <div className="clmux-card__title">{t("in.clmuxTitle", { agent: agentLabel })}</div>
              <div className="clmux-card__sub">
                {t("in.clmuxSub", { dir: host.projectDir ?? "~", agent: agentLabel })}
              </div>
            </div>
          </div>
          {(() => {
            const canDetach = session.status === "connected" && sessionUsesTmux(session, host);
            return (
              <div
                className="action-card"
                style={{ cursor: canDetach ? "pointer" : "default", opacity: canDetach ? 1 : 0.5 }}
                onClick={() => canDetach && detachTab(session.id)}
              >
                <div className="action-card__icon">⏏</div>
                <div className="card-body">
                  <div className="action-card__title">{t("in.detach")}</div>
                  <div className="action-card__sub">
                    {sessionUsesTmux(session, host) ? t("in.detachOn") : t("in.detachOff")}
                  </div>
                </div>
              </div>
            );
          })()}
          <div
            className="action-card"
            style={{ cursor: "pointer" }}
            onClick={() => openModal({ kind: "installTmux", hostId: host.id })}
          >
            <div className="action-card__icon">⟳</div>
            <div className="card-body">
              <div className="action-card__title">{t("in.installTmux")}</div>
              <div className="action-card__sub">ssh -tt · auto</div>
            </div>
          </div>
          <div
            className="action-card"
            style={{ cursor: "pointer" }}
            title="shell"
            onClick={() =>
              open(host.id, { mode: "shell", projectDir: host.projectDir ?? undefined })
            }
          >
            <div className="action-card__icon">⌘</div>
            <div className="card-body">
              <div className="action-card__title">{t("in.openShell")}</div>
              <div className="action-card__sub">cd {host.projectDir ?? "~"}</div>
            </div>
          </div>
        </div>

        <div className="inspector__section inspector__section--gap">{t("in.credential")}</div>
        <div className="cred-card">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" strokeWidth="1.8">
            <rect x="4" y="10" width="16" height="11" rx="2" />
            <path d="M8 10V7a4 4 0 0 1 8 0v3" />
          </svg>
          <div className="card-body">
            <div className="cred-card__title">{host.credentialRef ?? "ssh-agent / config"}</div>
            <div className="cred-card__sub">{t("in.credSub")}</div>
          </div>
          <span className="cred-card__mask">••••</span>
        </div>
      </div>
    </div>
  );
}
