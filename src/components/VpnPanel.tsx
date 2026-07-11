import { useEffect } from "react";
import { Toggle } from "./fields";
import { useVpnStore } from "../stores/vpn";
import { useT } from "../i18n";

/** Painel de VPNs (design 4a) — flutuante no canto superior direito. */
export function VpnPanel() {
  const t = useT();
  const open = useVpnStore((s) => s.panelOpen);
  const profiles = useVpnStore((s) => s.profiles);
  const autoDisconnect = useVpnStore((s) => s.autoDisconnect);
  const load = useVpnStore((s) => s.load);
  const connect = useVpnStore((s) => s.connect);
  const disconnect = useVpnStore((s) => s.disconnect);
  const setAutoDisconnect = useVpnStore((s) => s.setAutoDisconnect);
  const togglePanel = useVpnStore((s) => s.togglePanel);

  // enquanto o painel está aberto, atualiza o estado periodicamente —
  // conectar/desconectar passa por estados transitórios do Tunnelblick.
  useEffect(() => {
    if (!open) return;
    void load();
    const t = setInterval(() => void load(), 2000);
    return () => clearInterval(t);
  }, [open, load]);

  if (!open) return null;

  return (
    <>
      <div className="vpn-panel__scrim" onClick={() => togglePanel(false)} />
      <div className="vpn-panel">
        <div className="vpn-panel__header">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#5aa9e0" strokeWidth="1.8">
            <path d="M12 3l8 4v5c0 5-3.5 8-8 9-4.5-1-8-4-8-9V7z" />
          </svg>
          <span className="vpn-panel__title">{t("vp.title")}</span>
          <span className="vpn-panel__via">{t("vp.via")}</span>
        </div>

        <div className="vpn-panel__list">
          {profiles.length === 0 && <div className="vpn-panel__empty">{t("vp.empty")}</div>}
          {profiles.map((p) => {
            const connected = p.state === "connected";
            const connecting = p.state === "connecting";
            return (
              <div
                key={p.name}
                className={`vpn-row${connected ? " vpn-row--on" : ""}`}
              >
                <span
                  className="vpn-row__dot"
                  style={{
                    background: connected ? "#5aa9e0" : "#565c64",
                    boxShadow: connected ? "0 0 7px #5aa9e0" : "none",
                  }}
                />
                <div className="vpn-row__body">
                  <div className="vpn-row__name">{p.name}</div>
                  <div className="vpn-row__sub">
                    {connecting ? t("vp.connecting") : connected ? t("vp.connected") : t("vp.disconnected")} ·{" "}
                    {t("vp.hosts", { n: p.hostsUsing })}
                  </div>
                </div>
                {connected ? (
                  <span className="vpn-row__action" onClick={() => void disconnect(p.name)}>
                    {t("vp.disconnect")}
                  </span>
                ) : (
                  <span className="vpn-row__action" onClick={() => void connect(p.name)}>
                    {connecting ? "…" : t("vp.connect")}
                  </span>
                )}
              </div>
            );
          })}
        </div>

        <div className="vpn-panel__footer">
          <Toggle on={autoDisconnect} onChange={setAutoDisconnect} label={t("vp.auto")} />
        </div>
      </div>
    </>
  );
}
