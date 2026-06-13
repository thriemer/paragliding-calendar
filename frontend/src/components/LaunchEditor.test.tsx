import { describe, test, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { LaunchEditor } from "./LaunchEditor";
import type { ApiLaunch } from "../hooks/useSites";

const sampleLaunch: ApiLaunch = {
  location: { latitude: 47, longitude: 10, name: "Ridge", country: "DE" },
  direction_degrees_start: 0,
  direction_degrees_stop: 90,
  elevation: 1500,
  site_type: "Hang",
};

describe("LaunchEditor", () => {
  test("renders launch name and site type in header", () => {
    render(
      <LaunchEditor
        launch={sampleLaunch}
        index={0}
        onChange={() => {}}
        onRemove={() => {}}
      />,
    );
    expect(screen.getByText(/Ridge \(Hang\)/)).toBeTruthy();
  });

  test("falls back to 'Launch N+1' when name is empty", () => {
    const launch = { ...sampleLaunch, location: { ...sampleLaunch.location, name: "" } };
    render(
      <LaunchEditor
        launch={launch}
        index={4}
        onChange={() => {}}
        onRemove={() => {}}
      />,
    );
    expect(screen.getByText(/Launch 5 \(Hang\)/)).toBeTruthy();
  });

  test("starts collapsed", () => {
    render(
      <LaunchEditor
        launch={sampleLaunch}
        index={0}
        onChange={() => {}}
        onRemove={() => {}}
      />,
    );
    expect(screen.queryByPlaceholderText("Launch name")).toBeNull();
  });

  test("expands on header click and shows inputs", () => {
    render(
      <LaunchEditor
        launch={sampleLaunch}
        index={0}
        onChange={() => {}}
        onRemove={() => {}}
      />,
    );
    fireEvent.click(screen.getByText(/Ridge \(Hang\)/));
    expect(screen.getByPlaceholderText("Launch name")).toBeTruthy();
    expect(screen.getByRole("combobox")).toBeTruthy();
  });

  test("changing name calls onChange with patched location", () => {
    const onChange = vi.fn();
    render(
      <LaunchEditor
        launch={sampleLaunch}
        index={2}
        onChange={onChange}
        onRemove={() => {}}
      />,
    );
    fireEvent.click(screen.getByText(/Ridge \(Hang\)/));
    fireEvent.change(screen.getByPlaceholderText("Launch name"), {
      target: { value: "Summit" },
    });
    expect(onChange).toHaveBeenCalledWith(2, {
      ...sampleLaunch,
      location: { ...sampleLaunch.location, name: "Summit" },
    });
  });

  test("changing site_type calls onChange with new type", () => {
    const onChange = vi.fn();
    render(
      <LaunchEditor
        launch={sampleLaunch}
        index={0}
        onChange={onChange}
        onRemove={() => {}}
      />,
    );
    fireEvent.click(screen.getByText(/Ridge \(Hang\)/));
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "Winch" } });
    expect(onChange).toHaveBeenCalledWith(0, { ...sampleLaunch, site_type: "Winch" });
  });

  test("Remove button stops propagation and calls onRemove", () => {
    const onChange = vi.fn();
    const onRemove = vi.fn();
    render(
      <LaunchEditor
        launch={sampleLaunch}
        index={3}
        onChange={onChange}
        onRemove={onRemove}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Remove" }));
    expect(onRemove).toHaveBeenCalledWith(3);
    expect(screen.queryByPlaceholderText("Launch name")).toBeNull();
  });
});
