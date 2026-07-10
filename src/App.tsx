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
import { useUiStore } from "./stores/ui";

export default function App() {
  const view = useUiStore((s) => s.view);
  const modal = useUiStore((s) => s.modal);
  const loadPrefs = useUiStore((s) => s.loadPrefs);

  useEffect(() => {
    void loadPrefs();
  }, [loadPrefs]);

  return (
    <div className="app">
      <Titlebar />
      <div className="body">
        <Sidebar />
        <div className="main">
          <TabsToolbar />
          {view === "term" ? <TerminalView /> : <GridView />}
          <StatusBar />
        </div>
        <Inspector />
      </div>
      <VaultModal />
      {modal?.kind === "addHost" && <HostModal />}
      {modal?.kind === "editHost" && <HostModal editHostId={modal.hostId} />}
      {modal?.kind === "newSession" && <NewSessionModal presetHostId={modal.hostId} />}
      {modal?.kind === "installTmux" && <InstallTmuxModal hostId={modal.hostId} />}
    </div>
  );
}
