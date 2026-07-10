import { TERM_LINES } from "../mock";

// Fase 0: conteúdo estático transcrito do protótipo. Na Fase 1 este
// componente passa a hospedar o xterm conectado ao PTY.
export function TerminalView() {
  return (
    <div className="term">
      {TERM_LINES.map((line, i) => (
        <div className="term__line" key={i} style={{ color: line.color }}>
          {line.text}
        </div>
      ))}
      <div className="term__prompt">
        <span className="term__prompt-arrow">➜</span>
        <span className="term__prompt-path">~/apps/atlas-api</span>
        <span className="term__prompt-cmd">claude </span>
        <span className="term__cursor" />
      </div>

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
