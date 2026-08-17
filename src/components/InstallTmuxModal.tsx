import { useEffect, useMemo, useState } from "react";
import { detectRemote, installTmux, saveHost, type RemoteInfo } from "../lib/ipc";
import { useHostsStore } from "../stores/hosts";
import { useSessionsStore } from "../stores/sessions";
import { useUiStore } from "../stores/ui";
import { isSudoCredential, useVaultStore } from "../stores/vault";
import { PasswordField } from "./fields";
import { useT } from "../i18n";

const INSTALL_PREVIEW: Record<string, string> = {
  "apt-get": "sudo apt-get install -y tmux",
  dnf: "sudo dnf install -y tmux",
  yum: "sudo yum install -y tmux",
  pacman: "sudo pacman -S --noconfirm tmux",
  apk: "sudo apk add tmux",
  zypper: "sudo zypper install -y tmux",
};

/** Instalar tmux com sudo — design 2a. */
interface InstallTmuxModalProps {
  hostId: string;
  resumeSessionId?: string;
  initialInfo?: RemoteInfo;
  initialError?: string;
}

export function InstallTmuxModal({
  hostId,
  resumeSessionId,
  initialInfo,
  initialError,
}: InstallTmuxModalProps) {
  const t = useT();
  const closeModal = useUiStore((s) => s.closeModal);
  const hosts = useHostsStore((s) => s.hosts);
  const loadHosts = useHostsStore((s) => s.load);
  const vault = useVaultStore();

  const host = hosts.find((h) => h.id === hostId);
  const [info, setInfo] = useState<RemoteInfo | null>(initialInfo ?? null);
  const [detectErr, setDetectErr] = useState<string | null>(null);
  // "typed" é o padrão: funciona sem depender do Vault destravado
  const [source, setSource] = useState<"vault" | "typed">("typed");
  const [credId, setCredId] = useState<string | null>(null);
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; msg: string } | null>(
    initialError ? { ok: false, msg: initialError } : null,
  );

  useEffect(() => {
    if (initialInfo) return;
    void detectRemote(hostId)
      .then(setInfo)
      .catch((e) => setDetectErr(String(e)));
  }, [hostId, initialInfo]);

  // ao escolher "Usar senha do Vault" com o cofre travado, destrava sozinho
  useEffect(() => {
    if (source === "vault" && vault.locked && !vault.busy) {
      void vault.unlock();
    }
  }, [source, vault.locked, vault.busy]);

  // credenciais de sudo do vault (senha com escopo sudo ou NOPASSWD)
  const sudoCreds = useMemo(() => vault.creds.filter(isSudoCredential), [vault.creds]);
  const selectedCred = credId && sudoCreds.some((cred) => cred.id === credId) ? credId : null;
  const effectiveCred =
    selectedCred ??
    sudoCreds.find((cred) => cred.id === host?.credentialRef)?.id ??
    sudoCreds[0]?.id ??
    null;

  const canInstall =
    !!info?.pkgManager &&
    !busy &&
    !result?.ok &&
    (source === "vault" ? !!effectiveCred && !vault.locked : password.length > 0);

  const install = () => {
    if (!canInstall || !info?.pkgManager) return;
    setBusy(true);
    setResult(null);
    void installTmux(
      hostId,
      info.pkgManager,
      source === "vault" ? { credentialId: effectiveCred! } : { password },
    )
      .then(async (version) => {
        setResult({ ok: true, msg: t("it.installed", { v: version }) });
        // com tmux disponível, liga o re-attach automático do host
        if (host && !host.autoAttach) {
          await saveHost({ ...host, autoAttach: true });
          await loadHosts();
        }
        if (resumeSessionId) {
          useSessionsStore.getState().reattach(resumeSessionId);
          closeModal();
        }
      })
      .catch((e) => setResult({ ok: false, msg: String(e) }))
      .finally(() => setBusy(false));
  };

  return (
    <div className="modal-backdrop" onClick={closeModal}>
      <div className="hxm" style={{ width: 540 }} onClick={(e) => e.stopPropagation()}>
        <div className="hxm__header">
          <div className="hxm__icon hxm__icon--green">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="#63d29b" strokeWidth="1.8">
              <rect x="3" y="3" width="18" height="18" rx="2" />
              <path d="M3 9h18M9 21V9" />
            </svg>
          </div>
          <div className="hxm__titles">
            <div className="hxm__title">
              {t("it.title", { host: "" })}<span className="hxm__title-host">{host?.name ?? hostId}</span>
            </div>
            <div className="hxm__sub">
              {info?.tmux ? t("it.already", { v: info.tmux }) : t("it.notFound")}
            </div>
          </div>
          <span className="hxm__close" onClick={closeModal}>×</span>
        </div>

        <div className="hxm__body">
          <div className="hxm__detect">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#7d848d" strokeWidth="1.8">
              <circle cx="12" cy="12" r="9" />
              <path d="M12 8v4l2.5 2.5" />
            </svg>
            {detectErr ? (
              <span style={{ color: "#f0785a" }}>{detectErr}</span>
            ) : info ? (
              <>
                {t("it.detected")} <span className="hxm__detect-val">{info.os ?? "?"} · {info.pkgManager ?? t("it.noPm")}</span>
              </>
            ) : (
              t("it.detecting")
            )}
            <div className="hxm__spacer" />
            <span className="hxm__badge hxm__badge--green">ssh -tt</span>
          </div>

          <div>
            <div className="hxm__label">{t("it.command")}</div>
            <div className="hxm__cmd">
              <span className="hxm__cmd-prompt">$</span>{" "}
              {info?.pkgManager ? INSTALL_PREVIEW[info.pkgManager] : "…"}
            </div>
          </div>

          <div>
            <div className="hxm__label">{t("it.sudoPwd")}</div>
            <div className="hxm__radios">
              <div
                className={`hxm__radio${source === "vault" ? " hxm__radio--on hxm__radio--clmux" : ""}`}
                onClick={() => setSource("vault")}
              >
                <span className={`hxm__radio-dot${source === "vault" ? " hxm__radio-dot--on" : ""}`} />
                <div className="hxm__radio-body">
                  <div className="hxm__radio-title">{t("it.useVault")}</div>
                  <div className="hxm__radio-sub">
                    {vault.busy ? (
                      t("it.unlocking")
                    ) : vault.locked ? (
                      t("it.vaultLocked")
                    ) : sudoCreds.length === 0 ? (
                      t("it.noSudoCred")
                    ) : (
                      <select
                        className="hxm__inline-select"
                        value={effectiveCred ?? ""}
                        onClick={(e) => e.stopPropagation()}
                        onChange={(e) => setCredId(e.target.value)}
                      >
                        {sudoCreds.map((c) => (
                          <option key={c.id} value={c.id}>
                            {c.label}
                            {c.scope === "NOPASSWD" ? " (NOPASSWD)" : ""}
                          </option>
                        ))}
                      </select>
                    )}
                  </div>
                </div>
                <span className="hxm__badge hxm__badge--amber">
                  <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="#e0a15e" strokeWidth="2">
                    <path d="M12 11a4 4 0 0 1 4 4v0M12 11a4 4 0 0 0-4 4v2a4 4 0 0 0 8 0" />
                    <path d="M12 3a7 7 0 0 1 7 7v1M12 3a7 7 0 0 0-7 7v4" />
                  </svg>
                  TOUCH ID
                </span>
              </div>
              <div
                className={`hxm__radio${source === "typed" ? " hxm__radio--on" : ""}`}
                onClick={() => setSource("typed")}
              >
                <span className={`hxm__radio-dot${source === "typed" ? " hxm__radio-dot--on" : ""}`} />
                <div className="hxm__radio-body">
                  <div className="hxm__radio-title">{t("it.typeNow")}</div>
                  <div className="hxm__radio-sub">{t("it.typeNowSub")}</div>
                </div>
                {source === "typed" && (
                  <div style={{ width: 190 }} onClick={(e) => e.stopPropagation()}>
                    <PasswordField value={password} onChange={setPassword} placeholder={t("it.pwdPh")} autoFocus />
                  </div>
                )}
              </div>
            </div>
          </div>

          <div className="hxm__note">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#63d29b" strokeWidth="1.8" style={{ flex: "none", marginTop: 1 }}>
              <rect x="4" y="10" width="16" height="11" rx="2" />
              <path d="M8 10V7a4 4 0 0 1 8 0v3" />
            </svg>
            <div>{t("it.note")}</div>
          </div>

          {result && (
            <div className={`hxm__test-result${result.ok ? "" : " hxm__test-result--err"}`}>
              {result.msg}
            </div>
          )}
        </div>

        <div className="hxm__footer">
          <div className="hxm__btn hxm__btn--ghost" onClick={closeModal}>
            {result?.ok ? t("it.close") : t("it.later")}
          </div>
          {!result?.ok && (
            <div className={`hxm__btn hxm__btn--primary${canInstall ? "" : " hxm__btn--off"}`} onClick={install}>
              {busy ? t("it.installing") : t("it.install")}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

export function AutoInstallTmuxProgress() {
  const t = useT();
  const autoTmux = useUiStore((state) => state.autoTmux);
  const host = useHostsStore((state) =>
    autoTmux ? state.hosts.find((item) => item.id === autoTmux.hostId) : undefined,
  );
  if (!autoTmux) return null;

  const message =
    autoTmux.phase === "detecting"
      ? t("it.autoDetecting")
      : autoTmux.phase === "unlocking"
        ? t("it.autoUnlocking")
        : t("it.autoInstalling");

  return (
    <div className="modal-backdrop">
      <div className="hxm" style={{ width: 460 }}>
        <div className="hxm__header">
          <div className="hxm__icon hxm__icon--green">
            <span className="connect-overlay__spinner" />
          </div>
          <div className="hxm__titles">
            <div className="hxm__title">
              {t("it.autoTitle", { host: host?.name ?? autoTmux.hostId })}
            </div>
            <div className="hxm__sub">{message}</div>
          </div>
        </div>
      </div>
    </div>
  );
}
