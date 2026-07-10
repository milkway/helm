import { useState } from "react";
import helmLogo from "../assets/helm-logo.svg";
import { importSshConfig } from "../lib/ipc";
import { useHostsStore } from "../stores/hosts";
import { useUiStore } from "../stores/ui";

/** Primeira execução — sem hosts (design 3a). */
export function EmptyState() {
  const openModal = useUiStore((s) => s.openModal);
  const load = useHostsStore((s) => s.load);
  const [msg, setMsg] = useState<string | null>(null);

  const doImport = () => {
    setMsg("Importando…");
    void importSshConfig()
      .then(async (n) => {
        await load();
        setMsg(n > 0 ? `${n} host(s) importado(s)` : "nenhum host novo no ~/.ssh/config");
      })
      .catch((e) => setMsg(String(e)));
  };

  return (
    <div className="empty-state">
      <img src={helmLogo} className="empty-state__logo" alt="Helm" />
      <div className="empty-state__title">Nenhum host ainda</div>
      <div className="empty-state__sub">
        Adicione um servidor ou importe do seu <span className="empty-state__mono">~/.ssh/config</span>
      </div>
      <div className="empty-state__actions">
        <div className="empty-state__btn empty-state__btn--primary" onClick={() => openModal({ kind: "addHost" })}>
          + Add host
        </div>
        <div className="empty-state__btn empty-state__btn--ghost" onClick={doImport}>
          Importar de ~/.ssh/config
        </div>
      </div>
      {msg && <div className="empty-state__msg">{msg}</div>}
      <div className="empty-state__hint">
        <span className="empty-state__kbd">⌘K</span> para buscar e comandos
      </div>
    </div>
  );
}
