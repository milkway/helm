import { useEffect, useRef, useState } from "react";
import { useVaultStore, type CredMeta } from "../stores/vault";
import { PasswordField, Toggle } from "./fields";
import { useT } from "../i18n";

function relativeTime(iso: string | null, t: (k: string, v?: Record<string, string | number>) => string): string {
  if (!iso) return t("time.never");
  const then = new Date(iso + "Z").getTime();
  const mins = Math.floor((Date.now() - then) / 60_000);
  if (mins < 1) return t("time.now");
  if (mins < 60) return t("time.min", { n: mins });
  const hours = Math.floor(mins / 60);
  if (hours < 24) return t("time.hour", { n: hours });
  const days = Math.floor(hours / 24);
  return days === 1 ? t("time.yesterday") : t("time.days", { n: days });
}

function CredRow({ cred }: { cred: CredMeta }) {
  const t = useT();
  const reveal = useVaultStore((s) => s.reveal);
  const remove = useVaultStore((s) => s.remove);
  const [secret, setSecret] = useState<string | null>(null);
  const [menuOpen, setMenuOpen] = useState(false);
  const hideTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const revealGeneration = useRef(0);

  // Trocar de credencial limpa o segredo anterior; fechar o modal/desmontar
  // também cancela o timer e invalida respostas de reveal ainda pendentes.
  useEffect(() => {
    revealGeneration.current += 1;
    setSecret(null);
    if (hideTimer.current) {
      clearTimeout(hideTimer.current);
      hideTimer.current = null;
    }
    return () => {
      revealGeneration.current += 1;
      if (hideTimer.current) {
        clearTimeout(hideTimer.current);
        hideTimer.current = null;
      }
    };
  }, [cred.id]);

  const isKey = cred.kind === "ssh_key";
  const badge = isKey ? (cred.algo ?? "key") : (cred.scope ?? "senha");
  const noSecretNote = isKey ? t("vault.noPassphrase") : t("vault.nopasswd");

  const doReveal = () => {
    if (hideTimer.current) {
      clearTimeout(hideTimer.current);
      hideTimer.current = null;
    }
    if (secret) {
      revealGeneration.current += 1;
      setSecret(null);
      return;
    }
    const generation = ++revealGeneration.current;
    void reveal(cred.id)
      .then((s) => {
        if (revealGeneration.current !== generation) return;
        setSecret(s);
        hideTimer.current = setTimeout(() => {
          setSecret(null);
          hideTimer.current = null;
        }, 10_000);
      })
      .catch(() => {});
  };

  return (
    <div className="cred-row">
      <span className={`cred-row__badge ${isKey ? "cred-row__badge--key" : "cred-row__badge--pwd"}`}>
        {badge}
      </span>
      <div className="cred-row__body">
        <div className="cred-row__label">{cred.label}</div>
        <div className="cred-row__sub">
          {cred.hasSecret ? relativeTime(cred.lastUsed, t) : noSecretNote}
        </div>
      </div>
      {cred.hasSecret ? (
        <>
          {secret ? (
            <span className="cred-row__secret">{secret}</span>
          ) : (
            <span className="cred-row__mask">••••••</span>
          )}
          <svg
            width="14"
            height="14"
            viewBox="0 0 24 24"
            fill="none"
            stroke={secret ? "var(--accent)" : "var(--st-idle)"}
            strokeWidth="1.8"
            style={{ cursor: "pointer", flex: "none" }}
            onClick={doReveal}
          >
            <path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7z" />
            <circle cx="12" cy="12" r="3" />
          </svg>
        </>
      ) : (
        <span className="cred-row__nosecret">—</span>
      )}
      <span className="cred-row__more" onClick={() => setMenuOpen((v) => !v)}>
        ⋯
        {menuOpen && (
          <div className="cred-row__menu" onClick={(e) => e.stopPropagation()}>
            <div
              className="cred-row__menu-item"
              onClick={() => {
                setMenuOpen(false);
                void remove(cred.id);
              }}
            >
              {t("vault.delete")}
            </div>
          </div>
        )}
      </span>
    </div>
  );
}

type CredType = "key" | "login" | "sudo";

const CRED_TYPES: { id: CredType; icon: string; titleKey: string; descKey: string }[] = [
  { id: "key", icon: "🔑", titleKey: "vault.typeKey", descKey: "vault.typeKeyD" },
  { id: "login", icon: "👤", titleKey: "vault.typeLogin", descKey: "vault.typeLoginD" },
  { id: "sudo", icon: "#", titleKey: "vault.typeSudo", descKey: "vault.typeSudoD" },
];

function AddCredForm({ onDone }: { onDone: () => void }) {
  const t = useT();
  const save = useVaultStore((s) => s.save);
  const [type, setType] = useState<CredType>("login");
  const [keyName, setKeyName] = useState("");
  const [algo, setAlgo] = useState("ed25519");
  const [noPassphrase, setNoPassphrase] = useState(false);
  const [user, setUser] = useState("");
  const [host, setHost] = useState("");
  const [alsoSudo, setAlsoSudo] = useState(false);
  const [noPasswd, setNoPasswd] = useState(false);
  const [secret, setSecret] = useState("");
  const [saving, setSaving] = useState(false);

  const secretless = (type === "key" && noPassphrase) || (type === "sudo" && noPasswd);
  const target = user.trim() && host.trim() ? `${user.trim()}@${host.trim()}` : "";

  const valid = (() => {
    if (type === "key") return keyName.trim().length > 0 && (noPassphrase || secret.length > 0);
    if (!target) return false;
    return secretless || secret.length > 0;
  })();

  const submit = () => {
    if (!valid || saving) return;
    setSaving(true);
    const meta: CredMeta =
      type === "key"
        ? {
            id: crypto.randomUUID(),
            kind: "ssh_key",
            label: keyName.trim(),
            algo,
            scope: null,
            lastUsed: null,
            hasSecret: !noPassphrase,
          }
        : {
            id: crypto.randomUUID(),
            kind: "password",
            label: type === "sudo" ? `sudo · ${target}` : target,
            algo: null,
            scope:
              type === "sudo"
                ? noPasswd
                  ? "NOPASSWD"
                  : "sudo"
                : alsoSudo
                  ? "ssh · sudo"
                  : "ssh",
            lastUsed: null,
            hasSecret: !secretless,
          };
    void save(meta, secretless ? "" : secret)
      .then(onDone)
      .finally(() => setSaving(false));
  };

  return (
    <div className="add-cred">
      <div className="add-cred__types">
        {CRED_TYPES.map((ty) => (
          <div
            key={ty.id}
            className={`add-cred__type${type === ty.id ? " add-cred__type--on" : ""}`}
            onClick={() => setType(ty.id)}
          >
            <span className="add-cred__type-icon">{ty.icon}</span>
            <div>
              <div className="add-cred__type-title">{t(ty.titleKey)}</div>
              <div className="add-cred__type-desc">{t(ty.descKey)}</div>
            </div>
          </div>
        ))}
      </div>

      {type === "key" ? (
        <div className="add-cred__row">
          <input
            className="add-cred__input"
            placeholder={t("vault.keyName")}
            value={keyName}
            onChange={(e) => setKeyName(e.target.value)}
            style={{ flex: 1 }}
            autoFocus
          />
          <select className="add-cred__input add-cred__select" value={algo} onChange={(e) => setAlgo(e.target.value)}>
            <option value="ed25519">ed25519</option>
            <option value="rsa">rsa</option>
            <option value="ecdsa">ecdsa</option>
          </select>
          <Toggle on={noPassphrase} onChange={setNoPassphrase} label={t("vault.noPassToggle")} />
        </div>
      ) : (
        <div className="add-cred__row">
          <input
            className="add-cred__input"
            placeholder={t("vault.user")}
            value={user}
            onChange={(e) => setUser(e.target.value)}
            style={{ width: 130 }}
            autoFocus
          />
          <span className="add-cred__at">@</span>
          <input
            className="add-cred__input"
            placeholder={t("vault.hostPh")}
            value={host}
            onChange={(e) => setHost(e.target.value)}
            style={{ flex: 1 }}
          />
          {type === "login" ? (
            <Toggle on={alsoSudo} onChange={setAlsoSudo} label={t("vault.sameSudo")} />
          ) : (
            <Toggle on={noPasswd} onChange={setNoPasswd} label="NOPASSWD" />
          )}
        </div>
      )}

      <div className="add-cred__row">
        {secretless ? (
          <div className="add-cred__note">
            {type === "key"
              ? t("vault.noteKey")
              : t("vault.noteSudo")}
          </div>
        ) : (
          <PasswordField
            value={secret}
            onChange={setSecret}
            placeholder={type === "key" ? t("vault.passphrasePh") : t("vault.passwordPh")}
            onEnter={submit}
          />
        )}
        <div className={`add-cred__save${valid ? "" : " add-cred__save--off"}`} onClick={submit}>
          {saving ? t("vault.saving") : t("vault.save")}
        </div>
        <div className="add-cred__cancel" onClick={onDone}>
          {t("vault.cancel")}
        </div>
      </div>
    </div>
  );
}

export function VaultModal() {
  const t = useT();
  const { modalOpen, locked, count, creds, busy, error } = useVaultStore();
  const closeModal = useVaultStore((s) => s.closeModal);
  const unlock = useVaultStore((s) => s.unlock);
  const lock = useVaultStore((s) => s.lock);
  const [query, setQuery] = useState("");
  const [adding, setAdding] = useState(false);

  if (!modalOpen) return null;

  const q = query.toLowerCase();
  const filtered = creds.filter(
    (c) => c.label.toLowerCase().includes(q) || (c.scope ?? "").includes(q) || (c.algo ?? "").includes(q),
  );
  const keys = filtered.filter((c) => c.kind === "ssh_key");
  const pwds = filtered.filter((c) => c.kind === "password");
  const backend = navigator.userAgent.includes("Mac") ? "Keychain (macOS)" : "Secret Service (Linux)";

  return (
    <div className="modal-backdrop" onClick={closeModal}>
      <div className="vault-modal" onClick={(e) => e.stopPropagation()}>
        <div className="vault-modal__header">
          <div className="vault-modal__icon">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" strokeWidth="1.8">
              <rect x="4" y="10" width="16" height="11" rx="2" />
              <path d="M8 10V7a4 4 0 0 1 8 0v3" />
            </svg>
          </div>
          <div className="vault-modal__titles">
            <div className="vault-modal__title">Vault</div>
            <div className="vault-modal__sub">
              {locked ? t("vault.subLocked", { n: count, backend }) : t("vault.subUnlocked", { n: count, backend })}
            </div>
          </div>
          {!locked && (
            <div className="vault-modal__lock" onClick={() => void lock()}>
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="var(--text-2)" strokeWidth="2">
                <rect x="4" y="10" width="16" height="11" rx="2" />
                <path d="M8 10V7a4 4 0 0 1 8 0v3" />
              </svg>
              {t("vault.lock")}
            </div>
          )}
        </div>

        {locked ? (
          <div className="vault-modal__locked">
            <div className="vault-modal__locked-msg">{t("vault.lockedMsg", { backend })}</div>
            <div
              className={`vault-modal__unlock${busy ? " vault-modal__unlock--busy" : ""}`}
              onClick={() => void unlock()}
            >
              {busy ? t("vault.unlocking") : t("vault.unlock")}
            </div>
            {error && <div className="vault-modal__error">{error}</div>}
          </div>
        ) : (
          <>
            <div className="vault-modal__search-wrap">
              <div className="vault-modal__search">
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="var(--st-idle)" strokeWidth="2">
                  <circle cx="11" cy="11" r="7" />
                  <path d="M21 21l-4-4" />
                </svg>
                <input
                  placeholder={t("vault.search")}
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                />
              </div>
            </div>

            <div className="vault-modal__list">
              <div className="vault-modal__section">{t("vault.keys")}</div>
              {keys.length === 0 && <div className="vault-modal__empty">{t("vault.noKeys")}</div>}
              {keys.map((c) => (
                <CredRow key={c.id} cred={c} />
              ))}
              <div className="vault-modal__section vault-modal__section--gap">{t("vault.passwords")}</div>
              {pwds.length === 0 && <div className="vault-modal__empty">{t("vault.noPwd")}</div>}
              {pwds.map((c) => (
                <CredRow key={c.id} cred={c} />
              ))}
            </div>

            <div className="vault-modal__footer">
              {adding ? (
                <AddCredForm onDone={() => setAdding(false)} />
              ) : (
                <>
                  <div className="vault-modal__add" onClick={() => setAdding(true)}>
                    {t("vault.add")}
                  </div>
                  <div className="vault-modal__spacer" />
                  <span className="vault-modal__autolock">{t("vault.autolock")}</span>
                </>
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
