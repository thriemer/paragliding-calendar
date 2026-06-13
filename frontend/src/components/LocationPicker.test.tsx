import { describe, test, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor, act } from "@testing-library/react";
import { LocationPicker } from "./LocationPicker";

const sampleLocation = {
  latitude: 47,
  longitude: 10,
  name: "X",
  country: "DE",
};

describe("LocationPicker", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  test("renders elevation rounded to int", () => {
    render(
      <LocationPicker
        location={sampleLocation}
        elevation={1234.7}
        onChange={() => {}}
      />,
    );
    expect(screen.getByText("Elevation: 1235m")).toBeTruthy();
  });

  test("renders a map container with given coords", () => {
    render(
      <LocationPicker
        location={sampleLocation}
        elevation={0}
        onChange={() => {}}
      />,
    );
    const map = screen.getByTestId("map-container");
    expect(map.getAttribute("data-center")).toBe("[47,10]");
  });

  test("renders a marker (draggable)", () => {
    render(
      <LocationPicker
        location={sampleLocation}
        elevation={0}
        onChange={() => {}}
      />,
    );
    const marker = screen.getByTestId("marker");
    expect(marker.getAttribute("data-draggable")).toBe("true");
  });

  test("fetches elevation when map is clicked and calls onChange", async () => {
    (fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      json: async () => ({ elevation: 1700 }),
    });
    const onChange = vi.fn();
    render(
      <LocationPicker
        location={sampleLocation}
        elevation={0}
        onChange={onChange}
      />,
    );
    // Call the captured map click handler from the mock.
    const reactLeaflet = (await import("react-leaflet")) as unknown as {
      __clickHandlerRefs: Array<(e: { latlng: { lat: number; lng: number } }) => void>;
    };
    const handler = reactLeaflet.__clickHandlerRefs.at(-1)!;
    act(() => {
      handler({ latlng: { lat: 48, lng: 11 } });
    });

    await waitFor(() => {
      expect(onChange).toHaveBeenCalledWith(
        { ...sampleLocation, latitude: 48, longitude: 11 },
        1700,
      );
    });

    const callUrl = (fetch as unknown as ReturnType<typeof vi.fn>).mock.calls[0]?.[0];
    expect(callUrl).toContain("/api/elevation?latitude=48&longitude=11");
  });

  test("still notifies parent of new location even if elevation fetch fails", async () => {
    // Expected behavior: the marker moves visually on click, so the parent
    // must hear about the new coordinates — otherwise the saved site will
    // diverge from what the user sees on screen.
    vi.spyOn(console, "error").mockImplementation(() => {});
    (fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: false,
      status: 500,
      json: async () => ({ message: "down" }),
      text: async () => "",
    });
    const onChange = vi.fn();
    render(
      <LocationPicker
        location={sampleLocation}
        elevation={500}
        onChange={onChange}
      />,
    );
    const reactLeaflet = (await import("react-leaflet")) as unknown as {
      __clickHandlerRefs: Array<(e: { latlng: { lat: number; lng: number } }) => void>;
    };
    const handler = reactLeaflet.__clickHandlerRefs.at(-1)!;
    act(() => {
      handler({ latlng: { lat: 48, lng: 11 } });
    });

    await waitFor(() => expect(fetch).toHaveBeenCalled());
    await waitFor(() => {
      expect(onChange).toHaveBeenCalled();
      const [loc] = onChange.mock.calls[0] ?? [];
      expect(loc).toMatchObject({ latitude: 48, longitude: 11 });
    });
  });
});
