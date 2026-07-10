import { useState } from "react";
import { retrySession } from "../lib/ipc";
import { reattachTab } from "../lib/termRegistry";
import { useHostsStore } from "../stores/hosts";
import { useSessionsStore } from "../stores/sessions";
import { useUiStore } from "../stores/ui";
import { hostAddr, type Host, type SessionInfo } from "../types";
import { TermHost } from "./TermHost";

function ConnectingOverlay({ host, reconnect, attempt }: { host?: Host; reconnect: boolean; attempt: number | null }) {
  return (
    <div className="connect-overlay">
      <span className="connect-overlay__spinner" />
      <div>
        <div className="connect-overlay__title">
          {reconnect
            ? `Reconectando — tentativa ${attempt ?? 1}/5`
            : `Conectando a ${host?.name ?? "host"}…`}
        </div>
        <div className="connect-overlay__sub">
          ssh {host ? (host.user ? `${host.user}@${host.host}` : host.host) : ""}
        </div>
      </div>
    </div>
  );
}

/** Tela de erro pós-falha — design 3d. */
function ErrorOverlay({ session, host }: { session: SessionInfo; host?: Host }) {
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
              Reconexão falhou — {host?.name ?? session.hostId}
            </div>
            <div className="error-card__sub">
              5 tentativas · {host ? hostAddr(host) : ""}
            </div>
          </div>
        </div>
        <div className="error-card__log">
          {session.log.slice(-6).map((line, i, arr) => (
            <div key={i} className={i === arr.length - 1 ? "error-card__log-last" : undefined}>
              {line}
            </div>
          ))}
        </div>
        <div className="error-card__note">
          Sua sessão tmux continua rodando no servidor. Ao reconectar, o Helm re-atacha
          automaticamente.
        </div>
        <div className="error-card__actions">
          <div className="error-card__btn error-card__btn--primary" onClick={retryNow}>
            Tentar agora
          </div>
          <div
            className="error-card__btn error-card__btn--secondary"
            onClick={() => host && openModal({ kind: "editHost", hostId: host.id })}
          >
            Editar host
          </div>
          <div className="error-card__btn error-card__btn--ghost">Ver log completo</div>
          <div className="error-card__spacer" />
          <div className="error-card__auto">retry auto: 60s</div>
        </div>
      </div>
    </div>
  );
}

function DetachedOverlay({ session, host }: { session: SessionInfo; host?: Host }) {
  const [clicked, setClicked] = useState(false);
  return (
    <div className="error-overlay">
      <div className="error-card error-card--detached">
        <div className="error-card__head">
          <div className="error-card__badge error-card__badge--amber">⏏</div>
          <div className="error-card__titles">
            <div className="error-card__title error-card__title--amber">
              Sessão detachada — {host?.name ?? session.hostId}
            </div>
            <div className="error-card__sub">tmux segue rodando no servidor</div>
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
            {clicked ? "Re-atachando…" : "Re-atachar"}
          </button>
        </div>
      </div>
    </div>
  );
}

// Uma pane por sessão aberta, todas montadas (o xterm mantém buffer e estado);
// as inativas ficam com visibility:hidden para preservar as dimensões.
export function TerminalView() {
  const sessions = useSessionsStore((s) => s.sessions);
  const activeId = useSessionsStore((s) => s.activeId);
  const hosts = useHostsStore((s) => s.hosts);

  if (sessions.length === 0) {
    return (
      <div className="term term--empty">
        <span className="term__hint">Selecione um host na sidebar para abrir uma sessão</span>
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
            {(session.status === "connecting" || session.status === "reconnecting") && (
              <ConnectingOverlay
                host={host}
                reconnect={session.status === "reconnecting"}
                attempt={session.attempt}
              />
            )}
            {session.status === "error" && <ErrorOverlay session={session} host={host} />}
            {session.status === "detached" && <DetachedOverlay session={session} host={host} />}
          </div>
        );
      })}
    </div>
  );
}
