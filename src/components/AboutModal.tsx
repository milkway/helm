import { useEffect, useState } from "react";
import helmLogo from "../assets/helm-logo.svg";
import { appInfo, type AppInfo } from "../lib/ipc";
import { useUiStore } from "../stores/ui";
import { useT } from "../i18n";

/** Sobre (design 3e) — versão/arch reais em runtime. */
export function AboutModal() {
  const t = useT();
  const closeModal = useUiStore((s) => s.closeModal);
  const [info, setInfo] = useState<AppInfo | null>(null);

  useEffect(() => {
    void appInfo().then(setInfo).catch(() => {});
  }, []);

  const line = info
    ? `${info.version} · build ${info.build} · ${info.arch}`
    : t("ab.loading");

  return (
    <div className="modal-backdrop" onClick={closeModal}>
      <div className="about" onClick={(e) => e.stopPropagation()}>
        <img src={helmLogo} className="about__logo" alt="Helm" />
        <div className="about__name">Helm</div>
        <div className="about__tag">{t("ab.tagline")}</div>
        <div className="about__version">{line}</div>
        <div className="about__badges">
          <span>Tauri 2.11</span>
          <span>React 19.2</span>
          <span>xterm.js 6</span>
        </div>
        <div className="about__actions">
          <div className="about__btn about__btn--primary">{t("ab.checkUpdates")}</div>
          <div className="about__btn about__btn--ghost">{t("ab.licenses")}</div>
        </div>
        <div className="about__copy">© 2026 · MIT License</div>
      </div>
    </div>
  );
}
