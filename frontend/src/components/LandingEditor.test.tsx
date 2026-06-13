import { describe, test, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { LandingEditor } from "./LandingEditor";
import type { ApiLanding } from "../hooks/useSites";

const sampleLanding: ApiLanding = {
  location: { latitude: 47, longitude: 10, name: "LZ Alpha", country: "DE" },
  elevation: 800,
};

describe("LandingEditor", () => {
  test("renders the landing name in the header", () => {
    render(
      <LandingEditor
        landing={sampleLanding}
        index={0}
        onChange={() => {}}
        onRemove={() => {}}
      />,
    );
    expect(screen.getByText("LZ Alpha")).toBeTruthy();
  });

  test("falls back to 'Landing N+1' when name is empty", () => {
    const landing = { ...sampleLanding, location: { ...sampleLanding.location, name: "" } };
    render(
      <LandingEditor
        landing={landing}
        index={2}
        onChange={() => {}}
        onRemove={() => {}}
      />,
    );
    expect(screen.getByText("Landing 3")).toBeTruthy();
  });

  test("is collapsed initially - name input not visible", () => {
    render(
      <LandingEditor
        landing={sampleLanding}
        index={0}
        onChange={() => {}}
        onRemove={() => {}}
      />,
    );
    expect(screen.queryByPlaceholderText("Landing name")).toBeNull();
  });

  test("expands on header click", () => {
    render(
      <LandingEditor
        landing={sampleLanding}
        index={0}
        onChange={() => {}}
        onRemove={() => {}}
      />,
    );
    fireEvent.click(screen.getByText("LZ Alpha"));
    expect(screen.getByPlaceholderText("Landing name")).toBeTruthy();
  });

  test("onChange fires with updated name on input change", () => {
    const onChange = vi.fn();
    render(
      <LandingEditor
        landing={sampleLanding}
        index={1}
        onChange={onChange}
        onRemove={() => {}}
      />,
    );
    fireEvent.click(screen.getByText("LZ Alpha"));
    const input = screen.getByPlaceholderText("Landing name") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "LZ Bravo" } });
    expect(onChange).toHaveBeenCalledWith(1, {
      ...sampleLanding,
      location: { ...sampleLanding.location, name: "LZ Bravo" },
    });
  });

  test("Remove button stops propagation and calls onRemove with index", () => {
    const onRemove = vi.fn();
    render(
      <LandingEditor
        landing={sampleLanding}
        index={3}
        onChange={() => {}}
        onRemove={onRemove}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Remove" }));
    expect(onRemove).toHaveBeenCalledWith(3);
    // Header should remain collapsed
    expect(screen.queryByPlaceholderText("Landing name")).toBeNull();
  });
});
