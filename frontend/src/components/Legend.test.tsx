import { describe, test, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { Legend } from "./Legend";

describe("Legend", () => {
  test("renders overview labels when not zoomed in", () => {
    render(<Legend isZoomedIn={false} />);
    expect(screen.getByText("Winch")).toBeTruthy();
    expect(screen.getByText("Hang")).toBeTruthy();
    expect(screen.getByText("Winch + Hang")).toBeTruthy();
  });

  test("renders detailed labels when zoomed in", () => {
    render(<Legend isZoomedIn={true} />);
    expect(screen.getByText("Winch Launch")).toBeTruthy();
    expect(screen.getByText("Hang Launch")).toBeTruthy();
    expect(screen.getByText("Landing")).toBeTruthy();
  });

  test("omits 'Your Location' and 'Search Radius' when no settings", () => {
    render(<Legend isZoomedIn={false} />);
    expect(screen.queryByText("Your Location")).toBeNull();
    expect(screen.queryByText("Search Radius")).toBeNull();
  });

  test("shows 'Your Location' and 'Search Radius' when location settings present", () => {
    render(<Legend isZoomedIn={false} hasLocationSettings={true} />);
    expect(screen.getByText("Your Location")).toBeTruthy();
    expect(screen.getByText("Search Radius")).toBeTruthy();
  });

  test("zoomed-in legend does not show overview-only 'Winch + Hang' label", () => {
    render(<Legend isZoomedIn={true} />);
    expect(screen.queryByText("Winch + Hang")).toBeNull();
  });
});
