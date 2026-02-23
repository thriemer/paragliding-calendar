import { useState } from "react";
import "@gorules/jdm-editor/dist/style.css";
import "./styles/App.css";
import { JdmConfigProvider, DecisionGraph } from "@gorules/jdm-editor";
import { useDecisionGraph } from "./hooks/useDecisionGraph";
import { Header } from "./components/Header";

type Screen = "main" | "edit";

function App() {
  const [screen, setScreen] = useState<Screen>("main");
  const { graph, setGraph, loading, saving, load, save } = useDecisionGraph();

  if (loading) {
    return <div>Loading...</div>;
  }

  if (screen === "edit") {
    return (
      <JdmConfigProvider>
        <div className="app">
          <Header onLoad={load} onSave={save} saving={saving} onBack={() => setScreen("main")} />
          <div className="editor">
            <DecisionGraph value={graph} onChange={setGraph} />
          </div>
        </div>
      </JdmConfigProvider>
    );
  }

  return (
    <div className="app">
      <div className="main-screen">
        <aside className="side-panel">
          <h2>Menu</h2>
          <button className="btn" onClick={() => setScreen("edit")}>
            Edit Flyable Decision Rule
          </button>
        </aside>
        <main className="main-content">
          <h1>Main Screen</h1>
          <p>Welcome to the Travel AI application.</p>
        </main>
      </div>
    </div>
  );
}

export { App };
export default App;
