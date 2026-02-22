import "@gorules/jdm-editor/dist/style.css";
import "./styles/App.css";
import { JdmConfigProvider, DecisionGraph } from "@gorules/jdm-editor";
import { useDecisionGraph } from "./hooks/useDecisionGraph";
import { Header } from "./components/Header";

function App() {
  const { graph, setGraph, loading, saving, load, save } = useDecisionGraph();

  if (loading) {
    return <div>Loading...</div>;
  }

  return (
    <JdmConfigProvider>
      <div className="app">
        <Header onLoad={load} onSave={save} saving={saving} />
        <div className="editor">
          <DecisionGraph value={graph} onChange={setGraph} />
        </div>
      </div>
    </JdmConfigProvider>
  );
}

export { App };
export default App;
