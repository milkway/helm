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
import { AutoInstallTmuxProgress, InstallTmuxModal } from "./components/InstallTmuxModal";
import { CommandPalette } from "./components/CommandPalette";
import { AboutModal } from "./components/AboutModal";
import { EmptyState } from "./components/EmptyState";
import { VpnPanel } from "./components/VpnPanel";
import { detachTab } from "./lib/termRegistry";
import { useHostsStore } from "./stores/hosts";
import { useSessionsStore } from "./stores/sessions";
import { useUiStore } from "./stores/ui";
import { useLangStore } from "./i18n";
import { sessionUsesTmux } from "./types";

export default function App() {
  const view = useUiStore((s) => s.view);
  const modal = useUiStore((s) => s.modal);
  const loadPrefs = useUiStore((s) => s.loadPrefs);
  const togglePalette = useUiStore((s) => s.togglePalette);
  const loadLang = useLangStore((s) => s.load);
  const hosts = useHostsStore((s) => s.hosts);
  const hostsLoaded = useHostsStore((s) => s.loaded);
  const hostsError = useHostsStore((s) => s.error);

  useEffect(() => {
    void loadPrefs();
    void loadLang();
  }, [loadPrefs, loadLang]);

  // atalhos globais: ⌘K abre a palette, ⌘D destacha a sessão ativa
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        togglePalette();
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "d") {
        const { sessions, activeId } = useSessionsStore.getState();
        const active = sessions.find((s) => s.id === activeId);
        const host = active && useHostsStore.getState().hosts.find((h) => h.id === active.hostId);
        if (active && sessionUsesTmux(active, host || undefined)) {
          e.preventDefault();
          detachTab(active.id);
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [togglePalette]);

  const showEmpty = hostsLoaded && !hostsError && hosts.length === 0;

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
      <VpnPanel />
      <AutoInstallTmuxProgress />
      {modal?.kind === "addHost" && <HostModal />}
      {modal?.kind === "editHost" && <HostModal editHostId={modal.hostId} />}
      {modal?.kind === "newSession" && <NewSessionModal presetHostId={modal.hostId} />}
      {modal?.kind === "installTmux" && (
        <InstallTmuxModal
          key={`${modal.hostId}:${modal.resumeSessionId ?? "manual"}`}
          hostId={modal.hostId}
          resumeSessionId={modal.resumeSessionId}
          initialInfo={modal.initialInfo}
          initialError={modal.initialError}
        />
      )}
      {modal?.kind === "about" && <AboutModal />}
    </div>
  );
}
