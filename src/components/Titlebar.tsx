import { useEffect } from "react";
import { useSessionsStore } from "../stores/sessions";
import { useUiStore } from "../stores/ui";
import { useVpnStore } from "../stores/vpn";
import { useT } from "../i18n";
import { LanguageSelector } from "./LanguageSelector";

const IS_MAC = navigator.userAgent.includes("Mac");

export function Titlebar() {
  const t = useT();
  const connected = useSessionsStore(
    (s) => s.sessions.filter((x) => x.status === "connected").length,
  );
  const togglePalette = useUiStore((s) => s.togglePalette);
  const openModal = useUiStore((s) => s.openModal);
  const vpnConnected = useVpnStore((s) => s.profiles.some((p) => p.state === "connected"));
  const toggleVpn = useVpnStore((s) => s.togglePanel);
  const loadVpn = useVpnStore((s) => s.load);

  useEffect(() => {
    void loadVpn();
  }, [loadVpn]);

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
        <div className="titlebar__search" style={{ cursor: "text" }} onClick={() => togglePalette(true)}>
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="#7d848d" strokeWidth="2">
            <circle cx="11" cy="11" r="7" />
            <path d="M21 21l-4-4" />
          </svg>
          <span className="titlebar__search-text">{t("tb.search")}</span>
          <span className="titlebar__kbd">⌘K</span>
        </div>
      </div>
      <div className="titlebar__right">
        <div className="titlebar__connected" style={connected === 0 ? { opacity: 0.45 } : undefined}>
          <span className="titlebar__connected-dot" />
          <span className="titlebar__connected-label">{t("tb.connected", { n: connected })}</span>
        </div>
        <LanguageSelector />
        <div
          className="titlebar__settings"
          style={{ cursor: "pointer" }}
          title={vpnConnected ? t("tb.vpnOn") : t("tb.vpns")}
          onClick={() => toggleVpn()}
        >
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke={vpnConnected ? "#5aa9e0" : "#a2a8b0"} strokeWidth="1.8">
            <path d="M12 3l8 4v5c0 5-3.5 8-8 9-4.5-1-8-4-8-9V7z" />
          </svg>
        </div>
        <div
          className="titlebar__settings"
          style={{ cursor: "pointer" }}
          title={t("tb.about")}
          onClick={() => openModal({ kind: "about" })}
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="#a2a8b0" strokeWidth="1.8">
            <circle cx="12" cy="12" r="3" />
            <path d="M12 2v3M12 19v3M4.2 4.2l2.1 2.1M17.7 17.7l2.1 2.1M2 12h3M19 12h3M4.2 19.8l2.1-2.1M17.7 6.3l2.1-2.1" />
          </svg>
        </div>
      </div>
    </div>
  );
}
