import { useEffect, useState } from "react";
import helmLogo from "../assets/helm-logo.svg";
import {
  appInfo,
  checkUpdates,
  openExternal,
  type AppInfo,
  type UpdateInfo,
} from "../lib/ipc";
import { useUiStore } from "../stores/ui";
import { useT } from "../i18n";

const RELEASES_URL = "https://github.com/milkway/helm/releases";
const REPOSITORY_URL = "https://github.com/milkway/helm";

const LICENSES = [
  ["Tauri", "MIT / Apache-2.0"],
  ["React", "MIT"],
  ["xterm.js", "MIT"],
  ["Zustand", "MIT"],
  ["portable-pty", "MIT"],
  ["rusqlite", "MIT"],
  ["keyring", "MIT / Apache-2.0"],
  ["serde", "MIT / Apache-2.0"],
  ["zeroize", "MIT / Apache-2.0"],
  ["Vite", "MIT"],
] as const;

type UpdateState =
  | { status: "idle" }
  | { status: "checking" }
  | { status: "current"; info: UpdateInfo }
  | { status: "available"; info: UpdateInfo }
  | { status: "error"; message: string };

/** Sobre (design 3e) — versão/arch reais em runtime. */
export function AboutModal() {
  const t = useT();
  const closeModal = useUiStore((s) => s.closeModal);
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [update, setUpdate] = useState<UpdateState>({ status: "idle" });
  const [licensesOpen, setLicensesOpen] = useState(false);
  const [externalError, setExternalError] = useState<string | null>(null);

  useEffect(() => {
    void appInfo().then(setInfo).catch(() => {});
  }, []);

  const line = info
    ? `${info.version} · build ${info.build} · ${info.arch}`
    : t("ab.loading");

  const runUpdateCheck = async () => {
    setExternalError(null);
    setUpdate({ status: "checking" });
    try {
      const result = await checkUpdates();
      setUpdate({ status: result.updateAvailable ? "available" : "current", info: result });
    } catch (error) {
      setUpdate({ status: "error", message: String(error) });
    }
  };

  const openUrl = (url: string) => {
    setExternalError(null);
    void openExternal(url).catch((error) => setExternalError(String(error)));
  };

  const updateLabel = (() => {
    if (update.status === "checking") return t("ab.checking");
    if (update.status === "current") return t("ab.upToDate");
    if (update.status === "available") {
      return t("ab.updateAvailable", { version: update.info.latest.replace(/^v/i, "") });
    }
    if (update.status === "error") return t("ab.tryAgain");
    return t("ab.checkUpdates");
  })();

  const handleUpdateClick = () => {
    if (update.status === "available") openUrl(RELEASES_URL);
    else void runUpdateCheck();
  };

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
          <button
            type="button"
            className="about__btn about__btn--primary"
            disabled={update.status === "checking"}
            onClick={handleUpdateClick}
          >
            {updateLabel}
          </button>
          <button
            type="button"
            className="about__btn about__btn--ghost"
            aria-expanded={licensesOpen}
            onClick={() => setLicensesOpen((open) => !open)}
          >
            {licensesOpen ? t("ab.hideLicenses") : t("ab.licenses")}
          </button>
        </div>
        {update.status === "error" && (
          <div className="about__error">{t("ab.updateError", { message: update.message })}</div>
        )}
        {externalError && (
          <div className="about__error">{t("ab.externalError", { message: externalError })}</div>
        )}
        {licensesOpen && (
          <div className="about__licenses">
            <div className="about__licenses-title">{t("ab.dependencies")}</div>
            <div className="about__licenses-list">
              {LICENSES.map(([name, license]) => (
                <div className="about__license" key={name}>
                  <span>{name}</span>
                  <span>{license}</span>
                </div>
              ))}
            </div>
            <button
              type="button"
              className="about__github"
              onClick={() => openUrl(REPOSITORY_URL)}
            >
              {t("ab.github")}
            </button>
          </div>
        )}
        <div className="about__copy">© 2026 · MIT License</div>
      </div>
    </div>
  );
}
