import { Term } from "./Term";

// Fase 1: xterm vivo conectado a um PTY local (shell do sistema).
// O toast de atenção segue como placeholder visual até a Fase 8.
export function TerminalView() {
  return (
    <div className="term">
      <Term sessionId="local" />

      <div className="attn-toast">
        <span className="attn-toast__dot" />
        <div>
          <div className="attn-toast__title">gpu-train v3 waiting</div>
          <div className="attn-toast__sub">Claude asked a question 40s ago</div>
        </div>
        <div className="attn-toast__jump">Jump →</div>
      </div>
    </div>
  );
}
