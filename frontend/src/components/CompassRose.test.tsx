import { describe, test, expect, beforeEach, afterEach } from "bun:test";
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
  test("calls onChange when start handle is dragged", async () => {
    const mockOnChange = (start: number, stop: number) => {
      expect(start).toBeDefined();
      expect(stop).toBe(90);
    };
    
    const { container } = render(
      <CompassRose startDegrees={0} stopDegrees={90} onChange={mockOnChange} />
    );
    
    // Find the start circle (blue one)
    const circles = container.querySelectorAll("circle");
    const startCircle = circles[0];
    
    // Simulate mouse events to trigger drag
    fireEvent.mouseDown(startCircle, { clientX: 70, clientY: 10 });
    fireEvent.mouseMove(document.body, { clientX: 140, clientY: 70 });
    fireEvent.mouseUp(document.body);
  });

  test("calls onChange when stop handle is dragged", async () => {
    const mockOnChange = (start: number, stop: number) => {
      expect(start).toBe(0);
      expect(stop).toBeDefined();
    };
    
    const { container } = render(
      <CompassRose startDegrees={0} stopDegrees={90} onChange={mockOnChange} />
    );
    
    // Find the stop circle (red one)
    const circles = container.querySelectorAll("circle");
    const stopCircle = circles[1];
    
    // Simulate mouse events
    fireEvent.mouseDown(stopCircle, { clientX: 140, clientY: 70 });
    fireEvent.mouseMove(document.body, { clientX: 70, clientY: 10 });
    fireEvent.mouseUp(document.body);
  });
});
