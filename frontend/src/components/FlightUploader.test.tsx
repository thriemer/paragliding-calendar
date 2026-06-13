import { describe, test, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { makeWrapper } from "../test/queryWrapper";
import { FlightUploader } from "./FlightUploader";

function renderInProvider() {
  const { wrapper: Wrapper } = makeWrapper();
  return render(
    <Wrapper>
      <FlightUploader />
    </Wrapper>,
  );
}

const sampleAnalysis = {
  path: [],
  duration: "1h 0m 0.0s",
  distance: "10 km",
  max_altitude: "1000 m",
  track_length: "12 km",
  max_climb: "3 m/s",
  max_sink: "-3 m/s",
  min_speed: "10 km/h",
  max_speed: "40 km/h",
  min_glide: 8,
  avg_glide: 10,
  total_elevation_gain: "500 m",
};

describe("FlightUploader", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  test("renders default prompt", () => {
    renderInProvider();
    expect(screen.getByText(/Drop KML flight file here or click to browse/)).toBeTruthy();
  });

  test("file input accepts .kml", () => {
    const { container } = renderInProvider();
    const input = container.querySelector('input[type="file"]') as HTMLInputElement;
    expect(input.getAttribute("accept")).toBe(".kml");
  });

  test("on success renders metric cards and Clear button", async () => {
    (fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      json: async () => sampleAnalysis,
    });
    const { container } = renderInProvider();
    const input = container.querySelector('input[type="file"]') as HTMLInputElement;
    const file = new File(["kml"], "flight.kml");
    Object.defineProperty(input, "files", { value: [file] });
    fireEvent.change(input);
    await waitFor(() =>
      expect(screen.getByText("Flight Results")).toBeTruthy(),
    );
    expect(screen.getByRole("button", { name: "Clear" })).toBeTruthy();
    expect(screen.getByText("Flight Analysis")).toBeTruthy();
    expect(screen.getByText("Duration")).toBeTruthy();
  });

  test("Clear button hides results", async () => {
    (fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      json: async () => sampleAnalysis,
    });
    const { container } = renderInProvider();
    const input = container.querySelector('input[type="file"]') as HTMLInputElement;
    const file = new File(["kml"], "f.kml");
    Object.defineProperty(input, "files", { value: [file] });
    fireEvent.change(input);
    await waitFor(() => expect(screen.getByText("Flight Results")).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "Clear" }));
    await waitFor(() =>
      expect(screen.queryByText("Flight Results")).toBeNull(),
    );
  });

  test("on failure shows error", async () => {
    (fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: false,
      status: 500,
      statusText: "Server Error",
      json: async () => ({ message: "broken kml" }),
      text: async () => "",
    });
    const { container } = renderInProvider();
    const input = container.querySelector('input[type="file"]') as HTMLInputElement;
    const file = new File(["kml"], "broken.kml");
    Object.defineProperty(input, "files", { value: [file] });
    fireEvent.change(input);
    await waitFor(() =>
      expect(screen.getByText(/broken kml/)).toBeTruthy(),
    );
  });

  test("error from a previous failed upload clears on a successful retry", async () => {
    const fetchMock = vi.fn();
    fetchMock.mockResolvedValueOnce({
      ok: false,
      status: 500,
      statusText: "Server Error",
      json: async () => ({ message: "first fail" }),
      text: async () => "",
    });
    fetchMock.mockResolvedValueOnce({
      ok: true,
      json: async () => sampleAnalysis,
    });
    vi.stubGlobal("fetch", fetchMock);

    const { container } = renderInProvider();
    const input = container.querySelector('input[type="file"]') as HTMLInputElement;

    // First attempt: fails, error appears.
    Object.defineProperty(input, "files", {
      value: [new File(["bad"], "bad.kml")],
      configurable: true,
    });
    fireEvent.change(input);
    await waitFor(() => expect(screen.getByText(/first fail/)).toBeTruthy());

    // Second attempt: succeeds. The old error should not still be on screen.
    Object.defineProperty(input, "files", {
      value: [new File(["good"], "good.kml")],
      configurable: true,
    });
    fireEvent.change(input);
    await waitFor(() => expect(screen.getByText("Flight Results")).toBeTruthy());
    expect(screen.queryByText(/first fail/)).toBeNull();
  });
});
