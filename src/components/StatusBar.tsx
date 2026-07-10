export function StatusBar() {
  return (
    <div className="statusbar">
      <span className="statusbar__ssh">● SSH</span>
      <span className="statusbar__dim statusbar__gap">atlas · deploy@10.4.2.18:22</span>
      <span className="statusbar__dim">↔ 24ms</span>
      <span className="statusbar__sep">·</span>
      <span className="statusbar__mid">reconnect: auto · attach: auto</span>
      <div className="statusbar__spacer" />
      <span className="statusbar__dim">tmux 3.4</span>
      <span className="statusbar__sep">·</span>
      <span className="statusbar__accent">clmux ready</span>
      <span className="statusbar__sep">·</span>
      <span className="statusbar__dim">utf-8</span>
    </div>
  );
}
