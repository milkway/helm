import { useHostsStore } from "../stores/hosts";
import { useSessionsStore } from "../stores/sessions";
import { useT } from "../i18n";
import { hostAddr } from "../types";

export function StatusBar() {
  const t = useT();
  const sessions = useSessionsStore((s) => s.sessions);
  const activeId = useSessionsStore((s) => s.activeId);
  const hosts = useHostsStore((s) => s.hosts);

  const session = sessions.find((s) => s.id === activeId);
  const host = session ? hosts.find((h) => h.id === session.hostId) : undefined;

  if (!session || !host) {
    return (
      <div className="statusbar">
        <span className="statusbar__dim">{t("st.none")}</span>
        <div className="statusbar__spacer" />
        <span className="statusbar__dim">utf-8</span>
      </div>
    );
  }

  const stColor = session.status === "connected" ? "statusbar__ssh" : "statusbar__dim";

  return (
    <div className="statusbar">
      <span className={stColor}>● SSH</span>
      <span className="statusbar__dim statusbar__gap">
        {host.name} · {hostAddr(host)}
      </span>
      <span className="statusbar__dim">{session.status}</span>
      <span className="statusbar__sep">·</span>
      <span className="statusbar__mid">
        {t("st.reconnect", { r: host.autoReconnect ? "auto" : "off", a: host.autoAttach ? "auto" : "off" })}
      </span>
      <div className="statusbar__spacer" />
      <span className="statusbar__accent">{t("st.clmux")}</span>
      <span className="statusbar__sep">·</span>
      <span className="statusbar__dim">utf-8</span>
    </div>
  );
}
