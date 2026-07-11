import { useState } from "react";
import helmLogo from "../assets/helm-logo.svg";
import { importSshConfig } from "../lib/ipc";
import { useHostsStore } from "../stores/hosts";
import { useUiStore } from "../stores/ui";
import { useT } from "../i18n";

/** Primeira execução — sem hosts (design 3a). */
export function EmptyState() {
  const t = useT();
  const openModal = useUiStore((s) => s.openModal);
  const load = useHostsStore((s) => s.load);
  const [msg, setMsg] = useState<string | null>(null);

  const doImport = () => {
    setMsg(t("es.importing"));
    void importSshConfig()
      .then(async (n) => {
        await load();
        setMsg(n > 0 ? t("es.imported", { n }) : t("es.importNone"));
      })
      .catch((e) => setMsg(String(e)));
  };

  return (
    <div className="empty-state">
      <img src={helmLogo} className="empty-state__logo" alt="Helm" />
      <div className="empty-state__title">{t("es.title")}</div>
      <div className="empty-state__sub">
        {t("es.sub", { config: "" })}<span className="empty-state__mono">~/.ssh/config</span>
      </div>
      <div className="empty-state__actions">
        <div className="empty-state__btn empty-state__btn--primary" onClick={() => openModal({ kind: "addHost" })}>
          {t("es.add")}
        </div>
        <div className="empty-state__btn empty-state__btn--ghost" onClick={doImport}>
          {t("es.import")}
        </div>
      </div>
      {msg && <div className="empty-state__msg">{msg}</div>}
      <div className="empty-state__hint">
        <span className="empty-state__kbd">⌘K</span> {t("es.hint")}
      </div>
    </div>
  );
}
