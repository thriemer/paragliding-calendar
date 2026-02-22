import { useState, useEffect } from "react";
import '@gorules/jdm-editor/dist/style.css';
import { JdmConfigProvider, DecisionGraph } from "@gorules/jdm-editor";

const DEFAULT_GRAPH = {
  nodes: [],
  edges: [],
};

function App() {
  const [graph, setGraph] = useState(DEFAULT_GRAPH);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    handleLoad();
  }, []);

  const handleSave = async () => {
    try {
      const response = await fetch("http://localhost:8080/api/decision-graph", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(graph),
      });
      if (response.ok) {
        alert("Saved successfully!");
      } else {
        alert("Failed to save");
      }
    } catch (e) {
      alert("Error: " + e);
    }
  };

  const handleLoad = async () => {
    setLoading(true);
    try {
      const response = await fetch("http://localhost:8080/api/decision-graph");
      const data = await response.json();
      setGraph(data);
    } catch (e) {
      console.error("Error loading:", e);
    } finally {
      setLoading(false);
    }
  };

  if (loading) {
    return <div>Loading...</div>;
  }

  return (
    <JdmConfigProvider>
      <div style={{ display: "flex", flexDirection: "column", height: "100vh" }}>
        <div style={{ padding: "10px", background: "#1a1a2e", color: "white", display: "flex", gap: "10px", alignItems: "center" }}>
          <h2 style={{ margin: 0 }}>Flyable Decision Rule Editor</h2>
          <button onClick={handleLoad} style={btnStyle}>Load</button>
          <button onClick={handleSave} style={btnStyle}>Save</button>
        </div>
        <div style={{ flex: 1 }}>
          <DecisionGraph
            value={graph}
            onChange={setGraph}
          />
        </div>
      </div>
    </JdmConfigProvider>
  );
}

const btnStyle = {
  padding: "8px 16px",
  background: "#4CAF50",
  color: "white" as const,
  border: "none",
  borderRadius: "4px",
  cursor: "pointer" as const,
  fontWeight: 600 as const,
};

export { App };
export default App;
