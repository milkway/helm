import { useEffect, useMemo, useRef, useState } from "react";
import { importSshConfig } from "../lib/ipc";
import { detachTab } from "../lib/termRegistry";
import { useHostsStore } from "../stores/hosts";
import { useVpnStore } from "../stores/vpn";
import { useSessionsStore } from "../stores/sessions";
import { useUiStore } from "../stores/ui";
import { sessionUsesTmux, statusColor, tmuxSessionName, type Host, type SessionInfo } from "../types";
import { useT } from "../i18n";

interface Item {
  key: string;
  section: string;
  label: string;
  sub?: string;
  hint?: string;
  dotColor?: string;
  icon?: string;
  run: () => void;
}

function sessionStatusText(s: SessionInfo, t: (k: string, v?: Record<string, string | number>) => string): string {
  switch (s.status) {
    case "connected":
      return t("sess.connected");
    case "connecting":
      return t("sess.connecting");
    case "reconnecting":
      return t("sess.reconnecting", { n: s.attempt ?? 1 });
    case "vpn":
      return t("sess.vpn");
    case "error":
      return t("sess.error");
    case "detached":
      return t("sess.detached");
    default:
      return t("sess.exited");
  }
}

export function CommandPalette() {
  const t = useT();
  const open = useUiStore((s) => s.paletteOpen);
  const togglePalette = useUiStore((s) => s.togglePalette);
  const openModal = useUiStore((s) => s.openModal);
  const sessions = useSessionsStore((s) => s.sessions);
  const activeId = useSessionsStore((s) => s.activeId);
  const focus = useSessionsStore((s) => s.focus);
  const openSession = useSessionsStore((s) => s.open);
  const hosts = useHostsStore((s) => s.hosts);
  const loadHosts = useHostsStore((s) => s.load);
  const toggleVpnPanel = useVpnStore((s) => s.togglePanel);
  const [query, setQuery] = useState("");
  const [sel, setSel] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setQuery("");
      setSel(0);
      setTimeout(() => inputRef.current?.focus(), 0);
    }
  }, [open]);

  const hostOf = (id: string): Host | undefined => hosts.find((h) => h.id === id);
  const activeSession = sessions.find((s) => s.id === activeId);
  const activeHost = activeSession ? hostOf(activeSession.hostId) : undefined;

  const items = useMemo<Item[]>(() => {
    const list: Item[] = [];
    for (const s of sessions) {
      const h = hostOf(s.hostId);
      list.push({
        key: `sess-${s.id}`,
        section: t("pal.sessions"),
        label: h?.name ?? s.hostId,
        sub: `${h ? (h.user ? `${h.user}@${h.host}` : h.host) : ""} · ${sessionStatusText(s, t)}`,
        hint: t("pal.open"),
        dotColor: s.attention ? "#f0785a" : statusColor(s.status),
        run: () => {
          focus(s.id);
          togglePalette(false);
        },
      });
    }
    // comandos contextuais à sessão ativa
    if (activeHost) {
      list.push({
        key: "cmd-clmux",
        section: t("pal.commands"),
        label: t("pal.clmux", { host: activeHost.name }),
        hint: "⌘⏎",
        icon: "cl",
        run: () => {
          openSession(activeHost.id, {
            mode: "clmux",
            sessionName: tmuxSessionName(activeHost.name),
            projectDir: activeHost.projectDir ?? undefined,
          });
          togglePalette(false);
        },
      });
      if (activeSession && sessionUsesTmux(activeSession, activeHost)) {
        list.push({
          key: "cmd-detach",
          section: t("pal.commands"),
          label: t("pal.detach", { host: activeHost.name }),
          hint: "⌘D",
          icon: "⏏",
          run: () => {
            detachTab(activeSession.id);
            togglePalette(false);
          },
        });
      }
      list.push({
        key: "cmd-tmux",
        section: t("pal.commands"),
        label: t("pal.installTmux", { host: activeHost.name }),
        icon: "⟳",
        run: () => openModal({ kind: "installTmux", hostId: activeHost.id }),
      });
    }
    list.push({
      key: "cmd-newhost",
      section: t("pal.commands"),
      label: t("pal.addHost"),
      icon: "+",
      run: () => openModal({ kind: "addHost" }),
    });
    list.push({
      key: "cmd-newsession",
      section: t("pal.commands"),
      label: t("pal.newSession"),
      icon: "$",
      run: () => openModal({ kind: "newSession", hostId: activeHost?.id }),
    });
    list.push({
      key: "cmd-import",
      section: t("pal.commands"),
      label: t("pal.import"),
      icon: "⇩",
      run: () => {
        togglePalette(false);
        void importSshConfig().then(() => loadHosts());
      },
    });
    list.push({
      key: "cmd-vpn",
      section: t("pal.commands"),
      label: t("pal.vpn"),
      icon: "⛨",
      run: () => {
        togglePalette(false);
        toggleVpnPanel(true);
      },
    });
    list.push({
      key: "cmd-about",
      section: t("pal.commands"),
      label: t("pal.about"),
      icon: "?",
      run: () => openModal({ kind: "about" }),
    });
    return list;
  }, [t, sessions, hosts, activeHost, activeSession, focus, openSession, openModal, togglePalette, loadHosts, toggleVpnPanel]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return items;
    return items.filter(
      (it) => it.label.toLowerCase().includes(q) || (it.sub ?? "").toLowerCase().includes(q),
    );
  }, [items, query]);

  useEffect(() => {
    setSel((s) => Math.min(s, Math.max(0, filtered.length - 1)));
  }, [filtered.length]);

  if (!open) return null;

  const runSel = () => filtered[sel]?.run();
  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSel((s) => Math.min(s + 1, filtered.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSel((s) => Math.max(s - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (e.metaKey) {
        // ação padrão: clmux se houver host ativo
        const clmux = filtered.find((it) => it.key === "cmd-clmux");
        (clmux ?? filtered[sel])?.run();
      } else {
        runSel();
      }
    } else if (e.key === "Escape") {
      togglePalette(false);
    }
  };

  // agrupa preservando ordem
  let lastSection = "";
  let idx = -1;

  return (
    <div className="modal-backdrop palette-backdrop" onClick={() => togglePalette(false)}>
      <div className="palette" onClick={(e) => e.stopPropagation()}>
        <div className="palette__header">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="#7d848d" strokeWidth="2">
            <circle cx="11" cy="11" r="7" />
            <path d="M21 21l-4-4" />
          </svg>
          <input
            ref={inputRef}
            className="palette__input"
            placeholder={t("pal.search")}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onKey}
          />
          <span className="palette__esc">{t("pal.esc")}</span>
        </div>
        <div className="palette__list">
          {filtered.length === 0 && <div className="palette__empty">{t("pal.none")}</div>}
          {filtered.map((it) => {
            idx += 1;
            const showSection = it.section !== lastSection;
            lastSection = it.section;
            const selected = idx === sel;
            const myIdx = idx;
            return (
              <div key={it.key}>
                {showSection && <div className="palette__section">{it.section}</div>}
                <div
                  className={`palette__item${selected ? " palette__item--sel" : ""}`}
                  onMouseEnter={() => setSel(myIdx)}
                  onClick={it.run}
                >
                  {it.dotColor ? (
                    <span className="palette__dot" style={{ background: it.dotColor }} />
                  ) : (
                    <span className="palette__icon">{it.icon}</span>
                  )}
                  <span className="palette__label">{it.label}</span>
                  {it.sub && <span className="palette__sub">{it.sub}</span>}
                  <div className="palette__spacer" />
                  {it.hint && <span className="palette__hint">{it.hint}</span>}
                </div>
              </div>
            );
          })}
        </div>
        <div className="palette__footer">
          <span>{t("pal.navigate")}</span>
          <span>{t("pal.open")}</span>
          <span>{t("pal.default")}</span>
        </div>
      </div>
    </div>
  );
}
