import { useEffect, useMemo, useState } from "react";
import { saveHost, testConnection, vpnList, type TestResult, type VpnProfile } from "../lib/ipc";
import { useHostsStore } from "../stores/hosts";
import { useUiStore } from "../stores/ui";
import { useVaultStore } from "../stores/vault";
import type { Host } from "../types";
import { Toggle } from "./fields";
import { useT } from "../i18n";

/** Add/editar host — design 1b. */
export function HostModal({ editHostId }: { editHostId?: string }) {
  const t = useT();
  const closeModal = useUiStore((s) => s.closeModal);
  const hosts = useHostsStore((s) => s.hosts);
  const load = useHostsStore((s) => s.load);
  const vault = useVaultStore();

  const editing = editHostId ? hosts.find((h) => h.id === editHostId) : undefined;

  const [name, setName] = useState(editing?.name ?? "");
  const [group, setGroup] = useState(editing?.group ?? "");
  const [addr, setAddr] = useState(
    editing ? (editing.user ? `${editing.user}@${editing.host}` : editing.host) : "",
  );
  const [port, setPort] = useState(editing?.port ? String(editing.port) : "");
  const [credentialRef, setCredentialRef] = useState<string | null>(editing?.credentialRef ?? null);
  const [autoReconnect, setAutoReconnect] = useState(editing?.autoReconnect ?? true);
  const [autoInstallTmux, setAutoInstallTmux] = useState(editing?.autoInstallTmux ?? false);
  const [autoAttach, setAutoAttach] = useState(editing?.autoAttach ?? true);
  const [vpnProfile, setVpnProfile] = useState<string | null>(editing?.vpnProfile ?? null);
  const [vpnProfiles, setVpnProfiles] = useState<VpnProfile[]>([]);

  useEffect(() => {
    void vpnList().then(setVpnProfiles).catch(() => setVpnProfiles([]));
  }, []);
  const [testing, setTesting] = useState(false);
  const [test, setTest] = useState<TestResult | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const groups = useMemo(() => [...new Set(hosts.map((h) => h.group).filter(Boolean))], [hosts]);

  const parsed = useMemo(() => {
    const at = addr.indexOf("@");
    return at > 0
      ? { user: addr.slice(0, at).trim(), host: addr.slice(at + 1).trim() }
      : { user: null as string | null, host: addr.trim() };
  }, [addr]);

  const valid = name.trim().length > 0 && parsed.host.length > 0 && !parsed.host.startsWith("-");

  useEffect(() => setTest(null), [addr, port]);

  const runTest = () => {
    if (!parsed.host || testing) return;
    setTesting(true);
    void testConnection({ user: parsed.user, host: parsed.host, port: port ? Number(port) : null })
      .then(setTest)
      .catch((e) => setTest({ ok: false, latencyMs: 0, tmux: null, message: String(e) }))
      .finally(() => setTesting(false));
  };

  const submit = () => {
    if (!valid || saving) return;
    setSaving(true);
    setError(null);
    const host: Host = {
      id: editing?.id ?? crypto.randomUUID(),
      name: name.trim(),
      group: group.trim(),
      user: parsed.user,
      host: parsed.host,
      port: port ? Number(port) : null,
      credentialRef,
      vpnProfile,
      autoReconnect,
      autoInstallTmux,
      autoAttach,
      projectDir: editing?.projectDir ?? null,
      startupMode: editing?.startupMode ?? "shell",
    };
    void saveHost(host)
      .then(async () => {
        await load();
        closeModal();
      })
      .catch((e) => setError(String(e)))
      .finally(() => setSaving(false));
  };

  return (
    <div className="modal-backdrop" onClick={closeModal}>
      <div className="hxm" onClick={(e) => e.stopPropagation()}>
        <div className="hxm__header">
          <div className="hxm__icon">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" strokeWidth="1.8">
              <rect x="3" y="4" width="18" height="7" rx="2" />
              <rect x="3" y="13" width="18" height="7" rx="2" />
              <circle cx="7" cy="7.5" r="1" fill="var(--accent)" stroke="none" />
              <circle cx="7" cy="16.5" r="1" fill="var(--accent)" stroke="none" />
            </svg>
          </div>
          <div className="hxm__titles">
            <div className="hxm__title">{editing ? t("hm.edit") : t("hm.add")}</div>
            <div className="hxm__sub">{t("hm.sub")}</div>
          </div>
          <span className="hxm__close" onClick={closeModal}>×</span>
        </div>

        <div className="hxm__body">
          <div className="hxm__grid" style={{ gridTemplateColumns: "1fr 130px" }}>
            <div>
              <div className="hxm__label">{t("hm.name")}</div>
              <input className="hxm__input" value={name} onChange={(e) => setName(e.target.value)} autoFocus placeholder="atlas" />
            </div>
            <div>
              <div className="hxm__label">{t("hm.group")}</div>
              <input
                className="hxm__input"
                value={group}
                onChange={(e) => setGroup(e.target.value)}
                placeholder="Production"
                list="host-groups"
              />
              <datalist id="host-groups">
                {groups.map((g) => (
                  <option key={g} value={g} />
                ))}
              </datalist>
            </div>
          </div>
          <div className="hxm__grid" style={{ gridTemplateColumns: "1fr 90px" }}>
            <div>
              <div className="hxm__label">{t("hm.addr")}</div>
              <input
                className="hxm__input hxm__input--mono"
                value={addr}
                onChange={(e) => setAddr(e.target.value)}
                placeholder={t("hm.addrPh")}
              />
            </div>
            <div>
              <div className="hxm__label">{t("in.port")}</div>
              <input
                className="hxm__input hxm__input--mono"
                value={port}
                onChange={(e) => setPort(e.target.value.replace(/\D/g, ""))}
                placeholder="22"
              />
            </div>
          </div>

          <div>
            <div className="hxm__label">{t("hm.auth")}</div>
            {vault.locked ? (
              <div className="hxm__cred hxm__cred--locked" onClick={() => void vault.unlock()}>
                <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" strokeWidth="1.8">
                  <rect x="4" y="10" width="16" height="11" rx="2" />
                  <path d="M8 10V7a4 4 0 0 1 8 0v3" />
                </svg>
                <div className="hxm__cred-body">
                  <div className="hxm__cred-title">ssh-agent / ~/.ssh/config</div>
                  <div className="hxm__cred-sub">{t("hm.authLockedSub")}</div>
                </div>
                <span className="hxm__badge hxm__badge--amber">VAULT</span>
              </div>
            ) : (
              <select
                className="hxm__input hxm__select"
                value={credentialRef ?? ""}
                onChange={(e) => setCredentialRef(e.target.value || null)}
              >
                <option value="">{t("hm.authDefault")}</option>
                {vault.creds.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.label} — {c.kind === "ssh_key" ? (c.algo ?? "chave") : (c.scope ?? "senha")}
                  </option>
                ))}
              </select>
            )}
            <div className="hxm__cred-sub" style={{ marginTop: 6 }}>
              {t("hm.authHint")}
            </div>
          </div>

          <div className="hxm__toggles">
            <Toggle on={autoReconnect} onChange={setAutoReconnect} label={t("hm.toggleReconnect")} />
            <Toggle on={autoInstallTmux} onChange={setAutoInstallTmux} label={t("hm.toggleTmux")} />
            <Toggle on={autoAttach} onChange={setAutoAttach} label={t("hm.toggleAttach")} />
          </div>

          <div>
            <div className="hxm__label">{t("hm.vpn")}</div>
            <select
              className="hxm__input hxm__select"
              value={vpnProfile ?? ""}
              onChange={(e) => setVpnProfile(e.target.value || null)}
            >
              <option value="">{t("hm.vpnNone")}</option>
              {vpnProfiles.map((p) => (
                <option key={p.name} value={p.name}>
                  {p.name}
                </option>
              ))}
              {vpnProfile && !vpnProfiles.some((p) => p.name === vpnProfile) && (
                <option value={vpnProfile}>{vpnProfile}</option>
              )}
            </select>
          </div>

          <div className="hxm__test">
            <div
              className={`hxm__test-btn${testing ? " hxm__test-btn--busy" : ""}`}
              onClick={runTest}
            >
              {testing ? t("hm.testing") : t("hm.test")}
            </div>
            {test && (
              <div className={`hxm__test-result${test.ok ? "" : " hxm__test-result--err"}`}>
                {test.ok
                  ? t("hm.testOk", { ms: test.latencyMs, tmux: test.tmux ? t("hm.tmuxFound", { v: test.tmux }) : t("hm.noTmux") })
                  : t("hm.testErr", { msg: test.message ?? "" })}
              </div>
            )}
          </div>
          {error && <div className="hxm__error">{error}</div>}
        </div>

        <div className="hxm__footer">
          <div className="hxm__btn hxm__btn--ghost" onClick={closeModal}>{t("hm.cancel")}</div>
          <div className={`hxm__btn hxm__btn--primary${valid ? "" : " hxm__btn--off"}`} onClick={submit}>
            {saving ? t("vault.saving") : editing ? t("hm.save") : t("hm.create")}
          </div>
        </div>
      </div>
    </div>
  );
}
