import { describe, test, expect } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { FlightMap } from "./FlightMap";

describe("FlightMap (smoke)", () => {
  test("renders 'Loading terrain...' placeholder until terrain resolves, then the viewer", async () => {
    render(<FlightMap path={[]} />);
    expect(screen.getByText(/Loading terrain/)).toBeTruthy();
    await waitFor(() =>
      expect(screen.getByTestId("cesium-viewer")).toBeTruthy(),
    );
  });

  test("does not render Primitive when path is empty", async () => {
    render(<FlightMap path={[]} />);
    await waitFor(() => expect(screen.getByTestId("cesium-viewer")).toBeTruthy());
    expect(screen.queryByTestId("cesium-primitive")).toBeNull();
  });

  test("renders Primitive when path has more than one point", async () => {
    const path = [
      { latitude: 47, longitude: 10, height: 1000, time: "t0", climb_rate: 0 },
      { latitude: 47.1, longitude: 10.1, height: 1100, time: "t1", climb_rate: 1 },
    ];
    render(<FlightMap path={path} />);
    await waitFor(() => expect(screen.getByTestId("cesium-viewer")).toBeTruthy());
    expect(screen.getByTestId("cesium-primitive")).toBeTruthy();
  });
});
