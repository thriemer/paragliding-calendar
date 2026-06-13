import { describe, test, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act, waitFor } from "@testing-library/react";
import { useFlightAnalytics } from "./useFlightAnalytics";
import { makeWrapper } from "../test/queryWrapper";

const sampleAnalysis = {
  path: [],
  duration: "1h 30m 0.0s",
  distance: "20 km",
  max_altitude: "2000 m",
  track_length: "25 km",
  max_climb: "5 m/s",
  max_sink: "-4 m/s",
  min_speed: "10 km/h",
  max_speed: "50 km/h",
  min_glide: 8,
  avg_glide: 10,
  total_elevation_gain: "1500 m",
};

describe("useFlightAnalytics", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  test("analyzeFlight POSTs file as octet-stream and returns the analysis", async () => {
    (fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      json: async () => sampleAnalysis,
    });
    const file = new File(["kml content"], "flight.kml");
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useFlightAnalytics(), { wrapper });

    let analysis: typeof sampleAnalysis | null = null;
    await act(async () => {
      analysis = await result.current.analyzeFlight(file);
    });

    expect(analysis).toEqual(sampleAnalysis);
    expect(fetch).toHaveBeenCalledWith(
      "/api/flights/analyze",
      expect.objectContaining({
        method: "POST",
        body: file,
        headers: { "Content-Type": "application/octet-stream" },
      }),
    );
    await waitFor(() => expect(result.current.analysis).toEqual(sampleAnalysis));
  });

  test("analyzeFlight returns null on failure", async () => {
    (fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: false,
      status: 500,
      json: async () => ({ message: "broken" }),
      text: async () => "",
    });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useFlightAnalytics(), { wrapper });

    let analysis: unknown = "x";
    await act(async () => {
      analysis = await result.current.analyzeFlight(new File(["x"], "f.kml"));
    });
    expect(analysis).toBeNull();
    await waitFor(() => expect(result.current.error).toContain("500"));
  });

  test("clearAnalysis resets analysis to null", async () => {
    (fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      json: async () => sampleAnalysis,
    });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useFlightAnalytics(), { wrapper });

    await act(async () => {
      await result.current.analyzeFlight(new File(["x"], "f.kml"));
    });
    await waitFor(() => expect(result.current.analysis).toEqual(sampleAnalysis));

    act(() => {
      result.current.clearAnalysis();
    });
    await waitFor(() => expect(result.current.analysis).toBeNull());
  });
});
