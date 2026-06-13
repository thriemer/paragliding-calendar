import { describe, test, expect, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { CompassRose } from "./CompassRose";

beforeEach(() => {
  // Mock getBoundingClientRect for drag functionality
  Element.prototype.getBoundingClientRect = () => ({
    width: 140,
    height: 140,
    top: 0,
    left: 0,
    right: 140,
    bottom: 140,
    x: 0,
    y: 0,
    toJSON: () => "",
  });
});

describe("CompassRose rendering", () => {
  test("renders without crashing", () => {
    const mockOnChange = () => {};
    render(<CompassRose startDegrees={0} stopDegrees={90} onChange={mockOnChange} />);
    
    expect(screen.getByText("N")).toBeTruthy();
    expect(screen.getByText("E")).toBeTruthy();
    expect(screen.getByText("S")).toBeTruthy();
    expect(screen.getByText("W")).toBeTruthy();
  });

  test("displays start and stop labels with correct values", () => {
    const mockOnChange = () => {};
    render(<CompassRose startDegrees={45} stopDegrees={180} onChange={mockOnChange} />);
    
    expect(screen.getByText(/Start: 45/)).toBeTruthy();
    expect(screen.getByText(/Stop: 180/)).toBeTruthy();
  });

  test("displays correct labels for wrap-around case", () => {
    const mockOnChange = () => {};
    render(<CompassRose startDegrees={270} stopDegrees={90} onChange={mockOnChange} />);
    
    expect(screen.getByText(/Start: 270/)).toBeTruthy();
    expect(screen.getByText(/Stop: 90/)).toBeTruthy();
  });

  test("renders SVG with correct dimensions", () => {
    const mockOnChange = () => {};
    render(<CompassRose startDegrees={0} stopDegrees={90} onChange={mockOnChange} />);
    
    const svg = document.querySelector("svg");
    expect(svg).toBeTruthy();
    expect(svg?.getAttribute("width")).toBe("140");
    expect(svg?.getAttribute("height")).toBe("140");
  });

  test("has start and stop degree indicators", () => {
    const mockOnChange = () => {};
    render(<CompassRose startDegrees={0} stopDegrees={180} onChange={mockOnChange} />);
    
    // Check that both start and stop labels are present
    const labels = screen.getAllByText(/°/);
    expect(labels.length).toBe(2);
  });

  test("renders path element for arc", () => {
    const mockOnChange = () => {};
    render(<CompassRose startDegrees={0} stopDegrees={90} onChange={mockOnChange} />);
    
    const path = document.querySelector("path");
    expect(path).toBeTruthy();
  });

  test("renders circles for start and stop points", () => {
    const mockOnChange = () => {};
    render(<CompassRose startDegrees={0} stopDegrees={90} onChange={mockOnChange} />);
    
    // There are 3 circles: start line endpoint, start handle, and stop handle
    const circles = document.querySelectorAll("circle");
    expect(circles.length).toBe(3);
  });
});

describe("CompassRose interaction", () => {
  // SVG center is (70,70). Start handle at 0° = (70, 10) (N), stop handle at 90° = (130, 70) (E).
  // Three circles render in order: [0] background, [1] start handle, [2] stop handle.
  // Handlers live on the <svg>, so mousemove must be fired on the svg.
  function getHandles(container: HTMLElement) {
    const svg = container.querySelector("svg")!;
    const all = container.querySelectorAll("circle");
    return { svg, startCircle: all[1]!, stopCircle: all[2]! };
  }

  test("dragging the start handle to the east updates the start angle to ~90°", () => {
    const onChange = vi.fn();
    const { container } = render(
      <CompassRose startDegrees={0} stopDegrees={90} onChange={onChange} />,
    );
    const { svg, startCircle } = getHandles(container);

    fireEvent.mouseDown(startCircle, { clientX: 70, clientY: 10 });
    fireEvent.mouseMove(svg, { clientX: 140, clientY: 70 });

    expect(onChange).toHaveBeenCalled();
    const last = onChange.mock.calls.at(-1)!;
    expect(last[0]).toBeCloseTo(90, 0);
    expect(last[1]).toBe(90);
  });

  test("dragging the stop handle to the north updates the stop angle to ~0°", () => {
    const onChange = vi.fn();
    const { container } = render(
      <CompassRose startDegrees={0} stopDegrees={90} onChange={onChange} />,
    );
    const { svg, stopCircle } = getHandles(container);

    fireEvent.mouseDown(stopCircle, { clientX: 130, clientY: 70 });
    fireEvent.mouseMove(svg, { clientX: 70, clientY: 10 });

    expect(onChange).toHaveBeenCalled();
    const last = onChange.mock.calls.at(-1)!;
    expect(last[0]).toBe(0);
    // North is 0° (radiansToDegrees normalizes negative angles).
    expect(last[1] === 0 || Math.abs(last[1] - 360) < 1).toBe(true);
  });

  test("mouse move without prior mousedown does not fire onChange", () => {
    const onChange = vi.fn();
    const { container } = render(
      <CompassRose startDegrees={0} stopDegrees={90} onChange={onChange} />,
    );
    const { svg } = getHandles(container);
    fireEvent.mouseMove(svg, { clientX: 140, clientY: 70 });
    expect(onChange).not.toHaveBeenCalled();
  });

  test("mouseUp on SVG stops further drag updates", () => {
    const onChange = vi.fn();
    const { container } = render(
      <CompassRose startDegrees={0} stopDegrees={90} onChange={onChange} />,
    );
    const { svg, startCircle } = getHandles(container);
    fireEvent.mouseDown(startCircle, { clientX: 70, clientY: 10 });
    fireEvent.mouseMove(svg, { clientX: 140, clientY: 70 });
    const callsAfterFirstMove = onChange.mock.calls.length;
    expect(callsAfterFirstMove).toBeGreaterThan(0);
    fireEvent.mouseUp(svg);
    fireEvent.mouseMove(svg, { clientX: 70, clientY: 130 });
    expect(onChange.mock.calls.length).toBe(callsAfterFirstMove);
  });

  // Forgiving click handling: a user who clicks near (but not on) a handle
  // should still get the closer handle to snap to where they clicked.
  test("clicking inside the rose far from any handle snaps the closer handle (start)", () => {
    const onChange = vi.fn();
    const { container } = render(
      <CompassRose startDegrees={0} stopDegrees={90} onChange={onChange} />,
    );
    const { svg } = getHandles(container);
    // (100, 18) is at ~30°, closer to start (0°) than stop (90°).
    fireEvent.mouseDown(svg, { clientX: 100, clientY: 18 });
    expect(onChange).toHaveBeenCalled();
    const last = onChange.mock.calls.at(-1)!;
    expect(last[0]).toBeCloseTo(30, 0);
    expect(last[1]).toBe(90);
  });

  test("clicking inside the rose closer to stop snaps the stop handle", () => {
    const onChange = vi.fn();
    const { container } = render(
      <CompassRose startDegrees={0} stopDegrees={90} onChange={onChange} />,
    );
    const { svg } = getHandles(container);
    // (122, 40) is at ~60°, closer to stop (90°) than start (0°).
    fireEvent.mouseDown(svg, { clientX: 122, clientY: 40 });
    expect(onChange).toHaveBeenCalled();
    const last = onChange.mock.calls.at(-1)!;
    expect(last[0]).toBe(0);
    expect(last[1]).toBeCloseTo(60, 0);
  });

  test("nearest-handle selection respects the angular wrap-around", () => {
    // Start at 350° (just W of N) and stop at 10° (just E of N).
    // Click at 355° (5° west of N) — closer to start (5° away) than stop (15° away).
    const onChange = vi.fn();
    const { container } = render(
      <CompassRose startDegrees={350} stopDegrees={10} onChange={onChange} />,
    );
    const { svg } = getHandles(container);
    // 355° position: x = 70 + 60*cos(deg2rad(355)) ≈ 70 + 60*(-0.087) ≈ 64.8
    //                y = 70 + 60*sin(deg2rad(355)) ≈ 70 + 60*(-0.996) ≈ 10.2
    fireEvent.mouseDown(svg, { clientX: 65, clientY: 10 });
    expect(onChange).toHaveBeenCalled();
    const last = onChange.mock.calls.at(-1)!;
    // Start moved, stop unchanged.
    expect(last[0]).not.toBe(350);
    expect(last[1]).toBe(10);
  });

  test("after a near-miss click, dragging continues to move the same handle", () => {
    const onChange = vi.fn();
    const { container } = render(
      <CompassRose startDegrees={0} stopDegrees={90} onChange={onChange} />,
    );
    const { svg } = getHandles(container);
    fireEvent.mouseDown(svg, { clientX: 100, clientY: 18 }); // ~30°, picks start
    fireEvent.mouseMove(svg, { clientX: 122, clientY: 40 }); // ~60°
    // Even though 60° is closer to stop in absolute terms, the active handle is start —
    // and drag should keep moving start, not switch handles mid-drag.
    const last = onChange.mock.calls.at(-1)!;
    expect(last[0]).toBeCloseTo(60, 0);
    expect(last[1]).toBe(90);
  });
});
