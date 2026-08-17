import { useEffect, useState } from "react";
import { authorizeSudo, dismissSudoPrompt, retrySession } from "../lib/ipc";
import { reattachTab } from "../lib/termRegistry";
import { useHostsStore } from "../stores/hosts";
import { useSessionsStore } from "../stores/sessions";
import { useUiStore } from "../stores/ui";
import { useVaultStore } from "../stores/vault";
import { hostAddr, type Host, type SessionInfo } from "../types";
import { useT } from "../i18n";
import { TermHost } from "./TermHost";

function ConnectingOverlay({
  host,
  reconnect,
  vpn,
  attempt,
}: {
  host?: Host;
  reconnect: boolean;
  vpn: boolean;
  attempt: number | null;
}) {
  const t = useT();
  const addr = host ? (host.user ? `${host.user}@${host.host}` : host.host) : "";
  const title = vpn
    ? t("ov.vpn", { profile: host?.vpnProfile ?? "" })
    : reconnect
      ? t("ov.reconnecting", { n: attempt ?? 1 })
      : t("ov.connecting", { host: host?.name ?? "host" });
  const sub = vpn ? t("ov.vpnSub") : t("ov.connSub", { addr });
  return (
    <div className="connect-overlay">
      <span className="connect-overlay__spinner" />
      <div>
        <div className="connect-overlay__title">{title}</div>
        <div className="connect-overlay__sub">{sub}</div>
      </div>
    </div>
  );
}

/** Tela de erro pós-falha — design 3d. */
function ErrorOverlay({ session, host }: { session: SessionInfo; host?: Host }) {
  const t = useT();
  const openModal = useUiStore((s) => s.openModal);
  const retryNow = () => {
    if (session.ptyId) {
      // sessão ainda viva no Rust aguardando o ciclo de 60s
      void retrySession(session.ptyId).catch(() => reattachTab(session.id));
    } else {
      reattachTab(session.id);
    }
  };

  return (
    <div className="error-overlay">
      <div className="error-card">
        <div className="error-card__head">
          <div className="error-card__badge">!</div>
          <div className="error-card__titles">
            <div className="error-card__title">
              {t("err.title", { host: host?.name ?? session.hostId })}
            </div>
            <div className="error-card__sub">
              {t("err.sub", { addr: host ? hostAddr(host) : "" })}
            </div>
          </div>
        </div>
        <div className="error-card__log">
          {session.log.slice(-6).map((entry, i, arr) => (
            <div
              key={entry.id}
              className={i === arr.length - 1 ? "error-card__log-last" : undefined}
            >
              {entry.text}
            </div>
          ))}
        </div>
        <div className="error-card__note">{t("err.note")}</div>
        <div className="error-card__actions">
          <div className="error-card__btn error-card__btn--primary" onClick={retryNow}>
            {t("err.retryNow")}
          </div>
          <div
            className="error-card__btn error-card__btn--secondary"
            onClick={() => host && openModal({ kind: "editHost", hostId: host.id })}
          >
            {t("err.editHost")}
          </div>
          <div className="error-card__btn error-card__btn--ghost">{t("err.viewLog")}</div>
          <div className="error-card__spacer" />
          <div className="error-card__auto">{t("err.autoRetry")}</div>
        </div>
      </div>
    </div>
  );
}

function DetachedOverlay({ session, host }: { session: SessionInfo; host?: Host }) {
  const t = useT();
  const [clicked, setClicked] = useState(false);
  return (
    <div className="error-overlay">
      <div className="error-card error-card--detached">
        <div className="error-card__head">
          <div className="error-card__badge error-card__badge--amber">⏏</div>
          <div className="error-card__titles">
            <div className="error-card__title error-card__title--amber">
              {t("det.title", { host: host?.name ?? session.hostId })}
            </div>
            <div className="error-card__sub">{t("det.sub")}</div>
          </div>
        </div>
        <div className="error-card__actions">
          <button
            type="button"
            className="error-card__btn error-card__btn--primary"
            style={{ border: "none", font: "inherit" }}
            onClick={() => {
              setClicked(true);
              reattachTab(session.id);
            }}
          >
            {clicked ? "…" : t("det.reattach")}
          </button>
        </div>
      </div>
    </div>
  );
}

function SudoToast({ session }: { session: SessionInfo }) {
  const t = useT();
  const unlock = useVaultStore((s) => s.unlock);
  const setSudoPrompt = useSessionsStore((s) => s.setSudoPrompt);
  const [busy, setBusy] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setConfirming(false);
    setError(null);
  }, [session.sudoContext]);

  const authorize = async () => {
    if (!session.ptyId || busy) return;
    if (!confirming) {
      setConfirming(true);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      if (useVaultStore.getState().locked) await unlock();
      if (useVaultStore.getState().locked) {
        setError(t("sudo.unlockError"));
        return;
      }
      await authorizeSudo(session.ptyId);
      setSudoPrompt(session.id, false);
    } catch {
      setError(t("sudo.error"));
    } finally {
      setBusy(false);
    }
  };

  const dismiss = () => {
    setSudoPrompt(session.id, false);
    if (session.ptyId) void dismissSudoPrompt(session.ptyId).catch(() => undefined);
  };

  return (
    <div className="sudo-toast">
      <span className="sudo-toast__icon">#</span>
      <div className="sudo-toast__body">
        <div className="sudo-toast__title">{t("sudo.title")}</div>
        <div className="sudo-toast__sub">{t("sudo.warning")}</div>
        {error && <div className="sudo-toast__sub sudo-toast__sub--error">{error}</div>}
        <div className="sudo-toast__context">{session.sudoContext}</div>
      </div>
      <button
        type="button"
        className="sudo-toast__authorize"
        disabled={busy}
        onClick={() => void authorize()}
      >
        {busy
          ? t("sudo.authorizing")
          : confirming
            ? t("sudo.confirm")
            : t("sudo.authorize")}
      </button>
      <button
        type="button"
        className="sudo-toast__close"
        aria-label={t("sudo.dismiss")}
        onClick={dismiss}
      >
        ×
      </button>
    </div>
  );
}

// Uma pane por sessão aberta, todas montadas (o xterm mantém buffer e estado);
// as inativas ficam com visibility:hidden para preservar as dimensões.
export function TerminalView() {
  const t = useT();
  const sessions = useSessionsStore((s) => s.sessions);
  const activeId = useSessionsStore((s) => s.activeId);
  const hosts = useHostsStore((s) => s.hosts);

  if (sessions.length === 0) {
    return (
      <div className="term term--empty">
        <span className="term__hint">{t("term.pick")}</span>
      </div>
    );
  }

  return (
    <div className="term-stack">
      {sessions.map((session) => {
        const host = hosts.find((h) => h.id === session.hostId);
        return (
          <div
            key={session.id}
            className={`term term-pane${session.id === activeId ? "" : " term-pane--hidden"}`}
          >
            <TermHost
              key={`${session.id}:${session.generation}`}
              uiId={session.id}
              hostId={session.hostId}
              active={session.id === activeId}
              fontSize={13}
            />
            {(session.status === "connecting" ||
              session.status === "reconnecting" ||
              session.status === "vpn") && (
              <ConnectingOverlay
                host={host}
                reconnect={session.status === "reconnecting"}
                vpn={session.status === "vpn"}
                attempt={session.attempt}
              />
            )}
            {session.status === "error" && <ErrorOverlay session={session} host={host} />}
            {session.status === "detached" && <DetachedOverlay session={session} host={host} />}
            {session.id === activeId && session.sudoPrompt && <SudoToast session={session} />}
          </div>
        );
      })}
      <AttentionToast />
    </div>
  );
}

/** Toast de atenção no canto inferior direito, para a 1ª sessão em atenção
 * que não seja a ativa. "Jump →" foca a sessão. */
function AttentionToast() {
  const t = useT();
  const sessions = useSessionsStore((s) => s.sessions);
  const activeId = useSessionsStore((s) => s.activeId);
  const focus = useSessionsStore((s) => s.focus);
  const hosts = useHostsStore((s) => s.hosts);

  if (sessions.some((s) => s.id === activeId && s.sudoPrompt)) return null;
  const target = sessions.find((s) => s.attention && s.id !== activeId);
  if (!target) return null;
  const host = hosts.find((h) => h.id === target.hostId);

  return (
    <div className="attn-toast">
      <span className="attn-toast__dot" />
      <div>
        <div className="attn-toast__title">{t("attn.waiting", { host: host?.name ?? target.hostId })}</div>
        <div className="attn-toast__sub">{t("attn.sub")}</div>
      </div>
      <div className="attn-toast__jump" onClick={() => focus(target.id)}>
        {t("attn.jump")}
      </div>
    </div>
  );
}
