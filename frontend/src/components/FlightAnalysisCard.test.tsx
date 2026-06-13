import { describe, test, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { FlightAnalysisCard } from "./FlightAnalysisCard";
import type { FlightAnalysis } from "../hooks/useFlightAnalytics";

const baseAnalysis: FlightAnalysis = {
  path: [],
  duration: "1h 30m 15.0s",
  distance: "20 km",
  max_altitude: "2000 m",
  track_length: "25 km",
  max_climb: "5 m/s",
  max_sink: "-4 m/s",
  min_speed: "10 km/h",
  max_speed: "50 km/h",
  min_glide: 8.5,
  avg_glide: 10.25,
  total_elevation_gain: "1500 m",
};

describe("FlightAnalysisCard", () => {
  test("renders all metric labels and values", () => {
    render(<FlightAnalysisCard analysis={baseAnalysis} />);
    expect(screen.getByText("Flight Analysis")).toBeTruthy();
    expect(screen.getByText("Duration")).toBeTruthy();
    expect(screen.getByText("Distance")).toBeTruthy();
    expect(screen.getByText("20 km")).toBeTruthy();
    expect(screen.getByText("2000 m")).toBeTruthy();
    expect(screen.getByText("Elevation Gain")).toBeTruthy();
    expect(screen.getByText("1500 m")).toBeTruthy();
  });

  test("formats hh:mm:ss duration", () => {
    render(<FlightAnalysisCard analysis={baseAnalysis} />);
    expect(screen.getByText("1h 30m 15.0s")).toBeTruthy();
  });

  test("strips zero-hour part from duration", () => {
    const a = { ...baseAnalysis, duration: "0h 30m 15.0s" };
    render(<FlightAnalysisCard analysis={a} />);
    expect(screen.getByText("30m 15.0s")).toBeTruthy();
  });

  test("strips zero-hour and zero-minute parts from duration", () => {
    const a = { ...baseAnalysis, duration: "0h 0m 12.5s" };
    render(<FlightAnalysisCard analysis={a} />);
    expect(screen.getByText("12.5s")).toBeTruthy();
  });

  test("keeps minutes part visible when hours > 0 to avoid ambiguity", () => {
    // "1h 15.5s" reads like 1 hour 15 seconds — silently dropping 0m is misleading.
    const a = { ...baseAnalysis, duration: "1h 0m 15.5s" };
    render(<FlightAnalysisCard analysis={a} />);
    expect(screen.queryByText("1h 15.5s")).toBeNull();
  });

  test("passes through unparseable duration strings unchanged", () => {
    const a = { ...baseAnalysis, duration: "12.5 minutes" };
    render(<FlightAnalysisCard analysis={a} />);
    expect(screen.getByText("12.5 minutes")).toBeTruthy();
  });

  test("formats finite glide ratios to one decimal place", () => {
    render(<FlightAnalysisCard analysis={baseAnalysis} />);
    expect(screen.getByText("8.5:1")).toBeTruthy();
    expect(screen.getByText("10.3:1")).toBeTruthy();
  });

  test("displays infinity symbol for non-finite glide ratios", () => {
    const a = { ...baseAnalysis, min_glide: Infinity, avg_glide: NaN };
    render(<FlightAnalysisCard analysis={a} />);
    expect(screen.getAllByText("∞:1").length).toBe(2);
  });
});
