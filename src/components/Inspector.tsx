import { useEffect, useState } from "react";
import { useHostsStore } from "../stores/hosts";
import { useSessionsStore } from "../stores/sessions";
import { statusColor } from "../types";

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
  const hosts = useHostsStore((s) => s.hosts);

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
            <span className="inspector__host-dot" style={{ background: "#565c64", boxShadow: "none" }} />
            <span className="inspector__host-name">—</span>
          </div>
          <div className="inspector__host-addr">sem sessão ativa</div>
        </div>
      </div>
    );
  }

  const color = statusColor(session.status);
  const meta: { k: string; v: string }[] = [
    { k: "Host", v: host.host },
    { k: "User", v: host.user ?? "config" },
    { k: "Port", v: host.port ? String(host.port) : "config" },
    { k: "Latency", v: "—" },
    { k: "Uptime", v: uptime(session.connectedAt) },
    { k: "Auto-reconnect", v: host.autoReconnect ? "on" : "off" },
    { k: "Auto-attach", v: host.autoAttach ? "on" : "off" },
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
        <div className="inspector__section">Connection</div>
        {meta.map((m) => (
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
              <div className="clmux-card__sub">
                cd {host.projectDir ?? "~"} · tmux · claude
              </div>
            </div>
          </div>
          <div className="action-card">
            <div className="action-card__icon">⏏</div>
            <div className="card-body">
              <div className="action-card__title">Detach session</div>
              <div className="action-card__sub">keeps running on server</div>
            </div>
          </div>
          <div className="action-card">
            <div className="action-card__icon">⟳</div>
            <div className="card-body">
              <div className="action-card__title">Install / attach tmux</div>
              <div className="action-card__sub">ssh -tt · auto</div>
            </div>
          </div>
          <div className="action-card">
            <div className="action-card__icon">⌘</div>
            <div className="card-body">
              <div className="action-card__title">Open project shell</div>
              <div className="action-card__sub">cd {host.projectDir ?? "~"}</div>
            </div>
          </div>
        </div>

        <div className="inspector__section inspector__section--gap">Credential</div>
        <div className="cred-card">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="#e0a15e" strokeWidth="1.8">
            <rect x="4" y="10" width="16" height="11" rx="2" />
            <path d="M8 10V7a4 4 0 0 1 8 0v3" />
          </svg>
          <div className="card-body">
            <div className="cred-card__title">{host.credentialRef ?? "ssh-agent / config"}</div>
            <div className="cred-card__sub">via OpenSSH do sistema</div>
          </div>
          <span className="cred-card__mask">••••</span>
        </div>
      </div>
    </div>
  );
}
