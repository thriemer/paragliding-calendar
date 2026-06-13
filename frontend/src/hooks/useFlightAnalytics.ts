import { useState } from "react";
import { API } from "../config/api";
import { fetchJson } from "../utils/fetchJson";

export interface TrackPoint {
  latitude: number;
  longitude: number;
  height: number;
  time: string;
  climb_rate: number;
}

export interface FlightAnalysis {
  path: TrackPoint[];
  duration: string;
  distance: string;
  max_altitude: string;
  track_length: string;
  max_climb: string;
  max_sink: string;
  min_speed: string;
  max_speed: string;
  min_glide: number;
  avg_glide: number;
  total_elevation_gain: string;
}

export function useFlightAnalytics() {
  const [analyzing, setAnalyzing] = useState(false);
  const [analysis, setAnalysis] = useState<FlightAnalysis | null>(null);
  const [error, setError] = useState<string | null>(null);

  const analyzeFlight = async (file: File): Promise<FlightAnalysis | null> => {
    setAnalyzing(true);
    setError(null);
    setAnalysis(null);

    try {
      const data = await fetchJson<FlightAnalysis>(API.flightAnalyze, {
        method: "POST",
        headers: { "Content-Type": "application/octet-stream" },
        body: file,
        signal: AbortSignal.timeout(300000),
      });
      setAnalysis(data);
      return data;
    } catch (err) {
      setError(err instanceof Error ? err.message : "Analysis failed");
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
