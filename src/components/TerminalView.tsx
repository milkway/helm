import { useHostsStore } from "../stores/hosts";
import { useSessionsStore } from "../stores/sessions";
import { Term } from "./Term";

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
            <Term uiId={session.id} hostId={session.hostId} active={session.id === activeId} />
            {session.status === "connecting" && (
              <div className="connect-overlay">
                <span className="connect-overlay__spinner" />
                <div>
                  <div className="connect-overlay__title">
                    Conectando a {host?.name ?? session.hostId}…
                  </div>
                  <div className="connect-overlay__sub">
                    ssh {host ? (host.user ? `${host.user}@${host.host}` : host.host) : ""}
                  </div>
                </div>
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
