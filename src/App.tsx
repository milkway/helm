import { Titlebar } from "./components/Titlebar";
import { Sidebar } from "./components/Sidebar";
import { TabsToolbar } from "./components/TabsToolbar";
import { TerminalView } from "./components/TerminalView";
import { StatusBar } from "./components/StatusBar";
import { Inspector } from "./components/Inspector";

export default function App() {
  return (
    <div className="app">
      <Titlebar />
      <div className="body">
        <Sidebar />
        <div className="main">
          <TabsToolbar />
          <TerminalView />
          <StatusBar />
        </div>
        <Inspector />
      </div>
    </div>
  );
}
