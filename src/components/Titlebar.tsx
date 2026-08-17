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
  const sidebarHidden = useUiStore((s) => s.sidebarHidden);
  const inspectorHidden = useUiStore((s) => s.inspectorHidden);
  const theme = useUiStore((s) => s.theme);
  const toggleSidebar = useUiStore((s) => s.toggleSidebar);
  const toggleInspector = useUiStore((s) => s.toggleInspector);
  const toggleTheme = useUiStore((s) => s.toggleTheme);
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
          <div className="titlebar__light" style={{ background: "var(--st-attention)" }} />
          <div className="titlebar__light" style={{ background: "var(--st-reconnect)" }} />
          <div className="titlebar__light" style={{ background: "var(--st-connected)" }} />
        </div>
      )}
      <div className="titlebar__brand">
        <div className="titlebar__logo">H</div>
        <span className="titlebar__name">Helm</span>
      </div>
      <div className="titlebar__center" data-tauri-drag-region>
        <div className="titlebar__search" style={{ cursor: "text" }} onClick={() => togglePalette(true)}>
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
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
        <button
          type="button"
          className="titlebar__settings"
          title={sidebarHidden ? t("tb.showSidebar") : t("tb.hideSidebar")}
          aria-label={sidebarHidden ? t("tb.showSidebar") : t("tb.hideSidebar")}
          onClick={toggleSidebar}
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
            <rect x="3" y="4" width="18" height="16" rx="2" />
            <path d="M8.5 4v16" />
          </svg>
        </button>
        <button
          type="button"
          className="titlebar__settings"
          title={inspectorHidden ? t("tb.showInspector") : t("tb.hideInspector")}
          aria-label={inspectorHidden ? t("tb.showInspector") : t("tb.hideInspector")}
          onClick={toggleInspector}
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
            <rect x="3" y="4" width="18" height="16" rx="2" />
            <path d="M15.5 4v16" />
          </svg>
        </button>
        <button
          type="button"
          className="titlebar__settings"
          title={theme === "dark" ? t("tb.useLightTheme") : t("tb.useDarkTheme")}
          aria-label={theme === "dark" ? t("tb.useLightTheme") : t("tb.useDarkTheme")}
          onClick={toggleTheme}
        >
          {theme === "dark" ? (
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
              <circle cx="12" cy="12" r="3" />
              <path d="M12 2v3M12 19v3M4.2 4.2l2.1 2.1M17.7 17.7l2.1 2.1M2 12h3M19 12h3M4.2 19.8l2.1-2.1M17.7 6.3l2.1-2.1" />
            </svg>
          ) : (
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
              <path d="M20.5 14.3A8.5 8.5 0 0 1 9.7 3.5 8.5 8.5 0 1 0 20.5 14.3z" />
            </svg>
          )}
        </button>
        <button
          type="button"
          className="titlebar__settings"
          title={vpnConnected ? t("tb.vpnOn") : t("tb.vpns")}
          onClick={() => toggleVpn()}
          aria-label={vpnConnected ? t("tb.vpnOn") : t("tb.vpns")}
          style={{ color: vpnConnected ? "var(--st-info)" : undefined }}
        >
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
            <path d="M12 3l8 4v5c0 5-3.5 8-8 9-4.5-1-8-4-8-9V7z" />
          </svg>
        </button>
        <button
          type="button"
          className="titlebar__settings"
          title={t("tb.about")}
          aria-label={t("tb.about")}
          onClick={() => openModal({ kind: "about" })}
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
            <circle cx="12" cy="12" r="9" />
            <path d="M12 10.5V17" />
            <circle cx="12" cy="7.2" r="0.7" fill="currentColor" stroke="none" />
          </svg>
        </button>
      </div>
    </div>
  );
}
