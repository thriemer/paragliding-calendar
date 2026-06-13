import { useMutation } from "@tanstack/react-query";
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
  const mutation = useMutation({
    mutationFn: (file: File) =>
      fetchJson<FlightAnalysis>(API.flightAnalyze, {
        method: "POST",
        headers: { "Content-Type": "application/octet-stream" },
        body: file,
        signal: AbortSignal.timeout(300000),
      }),
  });

  const analyzeFlight = async (file: File): Promise<FlightAnalysis | null> => {
    try {
      return await mutation.mutateAsync(file);
    } catch {
      return null;
    }
  };

  return {
    analyzeFlight,
    analyzing: mutation.isPending,
    analysis: mutation.data ?? null,
    error: mutation.error
      ? mutation.error instanceof Error
        ? mutation.error.message
        : "Analysis failed"
      : null,
    clearAnalysis: mutation.reset,
  };
}
