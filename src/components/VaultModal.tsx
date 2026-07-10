import { useState } from "react";
import { useVaultStore, type CredMeta } from "../stores/vault";

function relativeTime(iso: string | null): string {
  if (!iso) return "nunca usada";
  const then = new Date(iso + "Z").getTime();
  const mins = Math.floor((Date.now() - then) / 60_000);
  if (mins < 1) return "usada agora";
  if (mins < 60) return `usada há ${mins} min`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `usada há ${hours}h`;
  const days = Math.floor(hours / 24);
  return days === 1 ? "usada ontem" : `usada há ${days} dias`;
}

function CredRow({ cred }: { cred: CredMeta }) {
  const reveal = useVaultStore((s) => s.reveal);
  const remove = useVaultStore((s) => s.remove);
  const [secret, setSecret] = useState<string | null>(null);
  const [menuOpen, setMenuOpen] = useState(false);

  const isKey = cred.kind === "ssh_key";
  const badge = isKey ? (cred.algo ?? "key") : (cred.scope ?? "senha");
  const noSecretNote = isKey ? "sem passphrase" : "NOPASSWD — sudo sem senha";

  const doReveal = () => {
    if (secret) {
      setSecret(null);
      return;
    }
    void reveal(cred.id)
      .then((s) => {
        setSecret(s);
        setTimeout(() => setSecret(null), 10_000);
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
          {cred.hasSecret ? relativeTime(cred.lastUsed) : noSecretNote}
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
            stroke={secret ? "#e0a15e" : "#565c64"}
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
              Excluir
            </div>
          </div>
        )}
      </span>
    </div>
  );
}

function Toggle({ on, onChange, label }: { on: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <div className="hx-toggle" onClick={() => onChange(!on)}>
      <div className={`hx-toggle__track${on ? " hx-toggle__track--on" : ""}`}>
        <div className="hx-toggle__knob" />
      </div>
      <span className="hx-toggle__label">{label}</span>
    </div>
  );
}

function EyeIcon({ off }: { off: boolean }) {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
      <path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7z" />
      <circle cx="12" cy="12" r="3" />
      {off && <path d="M3 3l18 18" />}
    </svg>
  );
}

/** Campo de senha com botão de mostrar/ocultar. */
function PasswordField({
  value,
  onChange,
  placeholder,
  onEnter,
  autoFocus,
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder: string;
  onEnter?: () => void;
  autoFocus?: boolean;
}) {
  const [show, setShow] = useState(false);
  return (
    <div className="pwd-field">
      <input
        className="add-cred__input pwd-field__input"
        type={show ? "text" : "password"}
        placeholder={placeholder}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => e.key === "Enter" && onEnter?.()}
        autoFocus={autoFocus}
      />
      <span
        className="pwd-field__eye"
        title={show ? "Ocultar" : "Mostrar"}
        onClick={() => setShow((v) => !v)}
      >
        <EyeIcon off={show} />
      </span>
    </div>
  );
}

type CredType = "key" | "login" | "sudo";

const CRED_TYPES: { id: CredType; icon: string; title: string; desc: string }[] = [
  { id: "key", icon: "🔑", title: "Chave SSH", desc: "passphrase da chave privada" },
  { id: "login", icon: "👤", title: "Login + senha", desc: "autenticação SSH por senha" },
  { id: "sudo", icon: "#", title: "Senha de sudo", desc: "quando o login é por chave" },
];

function AddCredForm({ onDone }: { onDone: () => void }) {
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
        {CRED_TYPES.map((t) => (
          <div
            key={t.id}
            className={`add-cred__type${type === t.id ? " add-cred__type--on" : ""}`}
            onClick={() => setType(t.id)}
          >
            <span className="add-cred__type-icon">{t.icon}</span>
            <div>
              <div className="add-cred__type-title">{t.title}</div>
              <div className="add-cred__type-desc">{t.desc}</div>
            </div>
          </div>
        ))}
      </div>

      {type === "key" ? (
        <div className="add-cred__row">
          <input
            className="add-cred__input"
            placeholder="nome da chave — ex.: id_ed25519"
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
          <Toggle on={noPassphrase} onChange={setNoPassphrase} label="sem passphrase" />
        </div>
      ) : (
        <div className="add-cred__row">
          <input
            className="add-cred__input"
            placeholder="usuário"
            value={user}
            onChange={(e) => setUser(e.target.value)}
            style={{ width: 130 }}
            autoFocus
          />
          <span className="add-cred__at">@</span>
          <input
            className="add-cred__input"
            placeholder="host — ex.: 10.4.2.18 ou alias"
            value={host}
            onChange={(e) => setHost(e.target.value)}
            style={{ flex: 1 }}
          />
          {type === "login" ? (
            <Toggle on={alsoSudo} onChange={setAlsoSudo} label="mesma senha no sudo" />
          ) : (
            <Toggle on={noPasswd} onChange={setNoPasswd} label="NOPASSWD" />
          )}
        </div>
      )}

      <div className="add-cred__row">
        {secretless ? (
          <div className="add-cred__note">
            {type === "key"
              ? "Chave sem passphrase — nada será salvo no Keychain, só o registro."
              : "sudo configurado com NOPASSWD — nenhuma senha é necessária nem salva."}
          </div>
        ) : (
          <PasswordField
            value={secret}
            onChange={setSecret}
            placeholder={type === "key" ? "passphrase (vai só para o Keychain)" : "senha (vai só para o Keychain)"}
            onEnter={submit}
          />
        )}
        <div className={`add-cred__save${valid ? "" : " add-cred__save--off"}`} onClick={submit}>
          {saving ? "Salvando…" : "Salvar"}
        </div>
        <div className="add-cred__cancel" onClick={onDone}>
          Cancelar
        </div>
      </div>
    </div>
  );
}

export function VaultModal() {
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
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="#e0a15e" strokeWidth="1.8">
              <rect x="4" y="10" width="16" height="11" rx="2" />
              <path d="M8 10V7a4 4 0 0 1 8 0v3" />
            </svg>
          </div>
          <div className="vault-modal__titles">
            <div className="vault-modal__title">Vault</div>
            <div className="vault-modal__sub">
              {count} credenciais · {locked ? "bloqueado" : "destravado com Touch ID"} · {backend}
            </div>
          </div>
          {!locked && (
            <div className="vault-modal__lock" onClick={() => void lock()}>
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="#a2a8b0" strokeWidth="2">
                <rect x="4" y="10" width="16" height="11" rx="2" />
                <path d="M8 10V7a4 4 0 0 1 8 0v3" />
              </svg>
              Bloquear
            </div>
          )}
        </div>

        {locked ? (
          <div className="vault-modal__locked">
            <div className="vault-modal__locked-msg">
              Os segredos ficam no {backend}. Destrave com a autenticação do sistema para ver e
              gerenciar credenciais.
            </div>
            <div
              className={`vault-modal__unlock${busy ? " vault-modal__unlock--busy" : ""}`}
              onClick={() => !busy && void unlock()}
            >
              {busy ? "Aguardando autenticação…" : "Destravar com Touch ID"}
            </div>
            {error && <div className="vault-modal__error">{error}</div>}
          </div>
        ) : (
          <>
            <div className="vault-modal__search-wrap">
              <div className="vault-modal__search">
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="#565c64" strokeWidth="2">
                  <circle cx="11" cy="11" r="7" />
                  <path d="M21 21l-4-4" />
                </svg>
                <input
                  placeholder="Buscar credenciais…"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                />
              </div>
            </div>

            <div className="vault-modal__list">
              <div className="vault-modal__section">Chaves SSH</div>
              {keys.length === 0 && <div className="vault-modal__empty">nenhuma chave</div>}
              {keys.map((c) => (
                <CredRow key={c.id} cred={c} />
              ))}
              <div className="vault-modal__section vault-modal__section--gap">Senhas</div>
              {pwds.length === 0 && <div className="vault-modal__empty">nenhuma senha</div>}
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
                    + Adicionar credencial
                  </div>
                  <div className="vault-modal__spacer" />
                  <span className="vault-modal__autolock">bloqueio automático: 15 min inativo</span>
                </>
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
