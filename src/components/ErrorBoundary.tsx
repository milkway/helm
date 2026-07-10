import { Component, type ErrorInfo, type ReactNode } from "react";

interface State {
  error: Error | null;
  info: string;
}

/** Captura erros de render e mostra a mensagem em vez de tela preta. */
export class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { error: null, info: "" };

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    this.setState({ info: info.componentStack ?? "" });
    console.error("[ErrorBoundary]", error, info);
  }

  render() {
    if (this.state.error) {
      return (
        <div
          style={{
            position: "fixed",
            inset: 0,
            zIndex: 99999,
            background: "#15110f",
            color: "#f5a88f",
            font: "13px/1.6 ui-monospace, monospace",
            padding: 24,
            overflow: "auto",
            whiteSpace: "pre-wrap",
          }}
        >
          <div style={{ fontSize: 15, fontWeight: 700, marginBottom: 10 }}>
            💥 Erro de render capturado
          </div>
          <div style={{ color: "#f0c99a", marginBottom: 12 }}>
            {this.state.error.message}
          </div>
          <div style={{ color: "#a97a68", fontSize: 11 }}>{this.state.error.stack}</div>
          <div style={{ color: "#7d848d", fontSize: 11, marginTop: 12 }}>
            {this.state.info}
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}
