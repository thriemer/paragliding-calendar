import { useState, useEffect, useCallback } from "react";
import { API } from "../config/api";

const DEFAULT_GRAPH = {
  nodes: [],
  edges: [],
};

export function useDecisionGraph() {
  const [graph, setGraph] = useState(DEFAULT_GRAPH);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const response = await fetch(API.decisionGraph);
      if (!response.ok) {
        throw new Error(`HTTP ${response.status}`);
      }
      const data = await response.json();
      setGraph(data);
    } catch (e) {
      console.error("Error loading:", e);
    } finally {
      setLoading(false);
    }
  }, []);

  const save = useCallback(async () => {
    setSaving(true);
    try {
      const response = await fetch(API.decisionGraph, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(graph),
      });
      if (!response.ok) {
        throw new Error(`HTTP ${response.status}`);
      }
      return true;
    } catch (e) {
      console.error("Error saving:", e);
      return false;
    } finally {
      setSaving(false);
    }
  }, [graph]);

  useEffect(() => {
    load();
  }, [load]);

  return { graph, setGraph, loading, saving, load, save };
}
