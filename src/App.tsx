import { useEffect } from "react";
import { Titlebar } from "./components/Titlebar";
import { Sidebar } from "./components/Sidebar";
import { TabsToolbar } from "./components/TabsToolbar";
import { TerminalView } from "./components/TerminalView";
import { GridView } from "./components/GridView";
import { StatusBar } from "./components/StatusBar";
import { Inspector } from "./components/Inspector";
import { VaultModal } from "./components/VaultModal";
import { HostModal } from "./components/HostModal";
import { NewSessionModal } from "./components/NewSessionModal";
import { InstallTmuxModal } from "./components/InstallTmuxModal";
import { CommandPalette } from "./components/CommandPalette";
import { AboutModal } from "./components/AboutModal";
import { EmptyState } from "./components/EmptyState";
import { detachTab } from "./lib/termRegistry";
import { useHostsStore } from "./stores/hosts";
import { useSessionsStore } from "./stores/sessions";
import { useUiStore } from "./stores/ui";

export default function App() {
  const view = useUiStore((s) => s.view);
  const modal = useUiStore((s) => s.modal);
  const loadPrefs = useUiStore((s) => s.loadPrefs);
  const togglePalette = useUiStore((s) => s.togglePalette);
  const hosts = useHostsStore((s) => s.hosts);
  const hostsLoaded = useHostsStore((s) => s.loaded);

  useEffect(() => {
    void loadPrefs();
  }, [loadPrefs]);

  // atalhos globais: ⌘K abre a palette, ⌘D destacha a sessão ativa
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        togglePalette();
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "d") {
        const { sessions, activeId } = useSessionsStore.getState();
        const active = sessions.find((s) => s.id === activeId);
        if (active) {
          e.preventDefault();
          detachTab(active.id);
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [togglePalette]);

  const showEmpty = hostsLoaded && hosts.length === 0;

  return (
    <div className="app">
      <Titlebar />
      <div className="body">
        <Sidebar />
        <div className="main">
          <TabsToolbar />
          {showEmpty ? <EmptyState /> : view === "term" ? <TerminalView /> : <GridView />}
          <StatusBar />
        </div>
        <Inspector />
      </div>
      <VaultModal />
      <CommandPalette />
      {modal?.kind === "addHost" && <HostModal />}
      {modal?.kind === "editHost" && <HostModal editHostId={modal.hostId} />}
      {modal?.kind === "newSession" && <NewSessionModal presetHostId={modal.hostId} />}
      {modal?.kind === "installTmux" && <InstallTmuxModal hostId={modal.hostId} />}
      {modal?.kind === "about" && <AboutModal />}
    </div>
  );
}
