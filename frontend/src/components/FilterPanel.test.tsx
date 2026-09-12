import { describe, test, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { FilterPanel } from "./FilterPanel";
import type { ApiActivity, SiteType } from "../hooks/useSites";

const activity = (id: string, kind: string, types: SiteType[]): ApiActivity => ({
  id,
  kind,
  title: id,
  latitude: 47,
  longitude: 10,
  description: "",
  image_urls: [],
  launches: types.map((site_type) => ({
    location: { latitude: 47, longitude: 10, name: "", country: "DE" },
    direction_degrees_start: 0,
    direction_degrees_stop: 360,
    elevation: 0,
    site_type,
  })),
  landings: [],
  country: "DE",
  data_source: "API",
});

describe("FilterPanel", () => {
  test("renders all activity types as checked checkboxes when no filter active", () => {
    const activities = [activity("a", "paragliding", ["Hang"])];
    render(
      <FilterPanel filters={{ activityTypes: [] }} onFilterChange={() => {}} activities={activities} />,
    );
    expect(screen.getByText("Paragliding")).toBeTruthy();
  });

  test("derives unique activity kinds from activities", () => {
    const activities = [
      activity("a", "paragliding", ["Hang"]),
      activity("b", "hiking", []),
      activity("c", "paragliding", ["Winch"]),
      activity("d", "biking", []),
    ];
    render(
      <FilterPanel filters={{ activityTypes: [] }} onFilterChange={() => {}} activities={activities} />,
    );
    const checkboxes = screen.getAllByRole("checkbox");
    expect(checkboxes.length).toBe(3);
  });

  test("excludes event and commitment kinds", () => {
    const activities = [
      activity("a", "paragliding", ["Hang"]),
      activity("b", "event", []),
      activity("c", "commitment", []),
    ];
    render(
      <FilterPanel filters={{ activityTypes: [] }} onFilterChange={() => {}} activities={activities} />,
    );
    const checkboxes = screen.getAllByRole("checkbox");
    expect(checkboxes.length).toBe(1);
  });

  test("calls onFilterChange when checkbox is toggled", () => {
    const onFilterChange = vi.fn();
    const activities = [activity("a", "paragliding", ["Hang"])];
    render(
      <FilterPanel
        filters={{ activityTypes: [] }}
        onFilterChange={onFilterChange}
        activities={activities}
      />,
    );
    const checkbox = screen.getByRole("checkbox");
    fireEvent.click(checkbox);
    expect(onFilterChange).toHaveBeenCalledWith({ activityTypes: ["paragliding"] });
  });

  test("unchecking removes kind from activityTypes", () => {
    const onFilterChange = vi.fn();
    const activities = [activity("a", "paragliding", ["Hang"])];
    render(
      <FilterPanel
        filters={{ activityTypes: ["paragliding"] }}
        onFilterChange={onFilterChange}
        activities={activities}
      />,
    );
    const checkbox = screen.getByRole("checkbox");
    fireEvent.click(checkbox);
    expect(onFilterChange).toHaveBeenCalledWith({ activityTypes: [] });
  });
});