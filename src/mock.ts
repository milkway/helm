// Dados de placeholder da Fase 0, transcritos do protótipo
// design_handoff_helm/Helm - Terminal Manager.dc.html. Serão substituídos
// pelo modelo real (SQLite + SessionManager) nas fases 2+.

export type Status = "connected" | "reconnect" | "attention" | "idle";

export const STATUS_COLORS: Record<Status, string> = {
  connected: "#63d29b",
  reconnect: "#e0b15e",
  attention: "#f0785a",
  idle: "#565c64",
};

export interface MockHost {
  id: string;
  name: string;
  addr: string;
  status: Status;
  tmux: boolean;
  badge: string;
}

export interface MockGroup {
  name: string;
  count: number;
  hosts: MockHost[];
}

export const GROUPS: MockGroup[] = [
  {
    name: "Production",
    count: 3,
    hosts: [
      { id: "atlas", name: "atlas-api", addr: "deploy@10.4.2.18", status: "connected", tmux: true, badge: "" },
      { id: "edge-1", name: "edge-cache", addr: "ops@10.4.2.31", status: "connected", tmux: true, badge: "" },
      { id: "gpu", name: "gpu-train v3", addr: "ml@10.4.9.4", status: "attention", tmux: true, badge: "!" },
    ],
  },
  {
    name: "Staging",
    count: 2,
    hosts: [
      { id: "stg-api", name: "atlas-api · stg", addr: "deploy@10.6.0.12", status: "reconnect", tmux: false, badge: "⟳" },
      { id: "stg-db", name: "db-maint", addr: "root@10.6.0.20", status: "connected", tmux: true, badge: "" },
    ],
  },
  {
    name: "Personal",
    count: 2,
    hosts: [
      { id: "homelab", name: "homelab", addr: "kai@192.168.1.40", status: "connected", tmux: true, badge: "" },
      { id: "vps-eu", name: "blog", addr: "root@88.99.12.4", status: "idle", tmux: false, badge: "" },
    ],
  },
];

export const TABS: { id: string; label: string; status: Status }[] = [
  { id: "atlas", label: "atlas-api", status: "connected" },
  { id: "gpu", label: "gpu-train v3", status: "attention" },
  { id: "homelab", label: "homelab", status: "connected" },
];

export interface TermLine {
  text: string;
  color: string;
}

export const TERM_LINES: TermLine[] = [
  { text: "$ ssh -tt deploy@10.4.2.18", color: "#7d848d" },
  { text: 'deploy@atlas: reconnected · auto-attached tmux session "atlas-api"', color: "#63d29b" },
  { text: "", color: "#c9cdd2" },
  { text: '$ tmux new -As main \\; send-keys "cd ~/apps/atlas-api && clmux" Enter', color: "#7d848d" },
  { text: "[atlas] tmux 3.4  ·  window 0: claude*  1: logs  2: shell", color: "#5aa9e0" },
  { text: "", color: "#c9cdd2" },
  { text: "╭─ claude ─────────────────────────────────────────────────╮", color: "#565c64" },
  { text: "│  Analyzing atlas-api deploy pipeline…                     │", color: "#c9cdd2" },
  { text: "│  ✓ read 14 files   ✓ ran tests (218 passed)               │", color: "#63d29b" },
  { text: "│  Ready to apply migration 0043_add_index.sql?             │", color: "#e0a15e" },
  { text: "╰──────────────────────────────────────────────────────────╯", color: "#565c64" },
  { text: "", color: "#c9cdd2" },
  { text: "deploy@atlas resumed after 2.1s dropout — 0 output lost", color: "#63d29b" },
];

export const META: { k: string; v: string }[] = [
  { k: "Host", v: "10.4.2.18" },
  { k: "User", v: "deploy" },
  { k: "Port", v: "22" },
  { k: "Latency", v: "24 ms" },
  { k: "Uptime", v: "6d 04h" },
  { k: "Auto-reconnect", v: "on" },
  { k: "Auto-attach", v: "tmux: atlas-api" },
];

export const ACTIONS: { icon: string; title: string; sub: string }[] = [
  { icon: "⏏", title: "Detach session", sub: "keeps running on server" },
  { icon: "⟳", title: "Install / attach tmux", sub: "ssh -tt · auto" },
  { icon: "⌘", title: "Open project shell", sub: "cd ~/apps/atlas-api" },
  { icon: "⇄", title: "Port forward", sub: "8080 → localhost" },
];
