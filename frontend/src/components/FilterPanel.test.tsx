import { describe, test, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { FilterPanel } from "./FilterPanel";
import type { ApiSite } from "../hooks/useSites";

const site = (name: string, types: string[]): ApiSite => ({
  name,
  country: "DE",
  launches: types.map((site_type) => ({
    location: { latitude: 47, longitude: 10, name: "", country: "DE" },
    direction_degrees_start: 0,
    direction_degrees_stop: 360,
    elevation: 0,
    site_type,
  })),
  landings: [],
  data_source: "API",
});

describe("FilterPanel", () => {
  test("renders 'All' option by default", () => {
    render(
      <FilterPanel filters={{ siteType: "" }} onFilterChange={() => {}} sites={[]} />,
    );
    expect(screen.getByRole("option", { name: "All" })).toBeTruthy();
  });

  test("derives unique sorted site types from sites' launches", () => {
    const sites = [
      site("A", ["Hang", "Winch"]),
      site("B", ["Hang"]),
      site("C", ["Winch", "Winch"]),
    ];
    render(
      <FilterPanel filters={{ siteType: "" }} onFilterChange={() => {}} sites={sites} />,
    );
    const options = screen
      .getAllByRole("option")
      .map((o) => (o as HTMLOptionElement).value);
    expect(options).toEqual(["", "Hang", "Winch"]);
  });

  test("filters out empty site types", () => {
    const sites = [site("A", ["Hang", ""])];
    render(
      <FilterPanel filters={{ siteType: "" }} onFilterChange={() => {}} sites={sites} />,
    );
    const options = screen
      .getAllByRole("option")
      .map((o) => (o as HTMLOptionElement).value);
    expect(options).toEqual(["", "Hang"]);
  });

  test("calls onFilterChange when selection changes", () => {
    const onFilterChange = vi.fn();
    render(
      <FilterPanel
        filters={{ siteType: "" }}
        onFilterChange={onFilterChange}
        sites={[site("A", ["Hang", "Winch"])]}
      />,
    );
    const select = screen.getByRole("combobox") as HTMLSelectElement;
    fireEvent.change(select, { target: { value: "Winch" } });
    expect(onFilterChange).toHaveBeenCalledWith({ siteType: "Winch" });
  });

  test("reflects current filter value", () => {
    render(
      <FilterPanel
        filters={{ siteType: "Hang" }}
        onFilterChange={() => {}}
        sites={[site("A", ["Hang", "Winch"])]}
      />,
    );
    const select = screen.getByRole("combobox") as HTMLSelectElement;
    expect(select.value).toBe("Hang");
  });
});
