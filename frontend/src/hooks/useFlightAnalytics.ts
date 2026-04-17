import { useState } from "react";
import { API } from "../config/api";

export function useFlightAnalytics() {
  const [analyzing, setAnalyzing] = useState(false);
  const [analysis, setAnalysis] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const analyzeFlight = async (file: File): Promise<string | null> => {
    setAnalyzing(true);
    setError(null);
    setAnalysis(null);

    try {
      const response = await fetch(API.flightAnalyze, {
        method: "POST",
        headers: {
          "Content-Type": "application/octet-stream",
        },
        body: file,
        signal: AbortSignal.timeout(300000),
      });

      if (!response.ok) {
        let detail = "";
        try {
          const errorData = await response.json();
          detail = errorData.message || JSON.stringify(errorData);
        } catch {
          detail = await response.text();
        }
        throw new Error(`Analysis failed: ${response.status} ${response.statusText} ${detail}`);
      }

      const data: string = await response.text();
      setAnalysis(data);
      return data;
    } catch (err) {
      const message = err instanceof Error ? err.message : "Analysis failed";
      setError(message);
      return null;
    } finally {
      setAnalyzing(false);
    }
  };

  const clearAnalysis = () => {
    setAnalysis(null);
    setError(null);
  };

  return { analyzeFlight, analyzing, analysis, error, clearAnalysis };
}
