import { describe, test, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { SitesMap } from "./SitesMap";
import type { ApiSite } from "./../hooks/useSites";
import type { UserSettings } from "./../hooks/useSettings";

const mkSite = (
  name: string,
  launches: Array<{ lat: number; lng: number; site_type: string }> = [],
  landings: Array<{ lat: number; lng: number }> = [],
): ApiSite => ({
  name,
  country: "DE",
  launches: launches.map((l) => ({
    location: { latitude: l.lat, longitude: l.lng, name, country: "DE" },
    direction_degrees_start: 0,
    direction_degrees_stop: 360,
    elevation: 1000,
    site_type: l.site_type,
  })),
  landings: landings.map((l) => ({
    location: { latitude: l.lat, longitude: l.lng, name, country: "DE" },
    elevation: 800,
  })),
  data_source: "API",
});

describe("SitesMap", () => {
  test("renders map container with default zoom when no view provided", () => {
    render(
      <SitesMap
        sites={[]}
        mapView={null}
        onMapViewChange={() => {}}
      />,
    );
    const map = screen.getByTestId("map-container");
    expect(map.getAttribute("data-zoom")).toBe("6");
  });

  test("centers map on default coords when no sites", () => {
    render(
      <SitesMap sites={[]} mapView={null} onMapViewChange={() => {}} />,
    );
    const map = screen.getByTestId("map-container");
    expect(map.getAttribute("data-center")).toBe("[47,10]");
  });

  test("renders overview markers (one per launch) when zoom < 11", () => {
    const sites = [
      mkSite("S1", [{ lat: 47, lng: 10, site_type: "Hang" }]),
      mkSite("S2", [
        { lat: 48, lng: 11, site_type: "Winch" },
        { lat: 48.1, lng: 11.1, site_type: "Hang" },
      ]),
    ];
    render(
      <SitesMap sites={sites} mapView={{ center: [47, 10], zoom: 6 }} onMapViewChange={() => {}} />,
    );
    const markers = screen.getAllByTestId("marker");
    expect(markers.length).toBe(3);
  });

  test("renders launch+landing markers when zoom >= 11", () => {
    const sites = [
      mkSite(
        "S1",
        [{ lat: 47, lng: 10, site_type: "Hang" }],
        [{ lat: 47.01, lng: 10.01 }],
      ),
    ];
    render(
      <SitesMap sites={sites} mapView={{ center: [47, 10], zoom: 12 }} onMapViewChange={() => {}} />,
    );
    const markers = screen.getAllByTestId("marker");
    expect(markers.length).toBe(2);
  });

  test("renders user location marker and search radius circle when settings present", () => {
    const settings: UserSettings = {
      location_name: "Home",
      location_latitude: 47.5,
      location_longitude: 10.5,
      search_radius_km: 100,
      calendar_name: "Cal",
      minimum_flyable_hours: 3,
      excluded_calendar_names: new Set(),
      all_calendar_names: [],
    };
    render(
      <SitesMap
        sites={[]}
        mapView={{ center: [47, 10], zoom: 6 }}
        onMapViewChange={() => {}}
        settings={settings}
      />,
    );
    const circle = screen.getByTestId("circle");
    expect(circle.getAttribute("data-center")).toBe("[47.5,10.5]");
    expect(circle.getAttribute("data-radius")).toBe("100000");
  });

  test("does not show user location when latitude is 0", () => {
    const settings: UserSettings = {
      location_name: "",
      location_latitude: 0,
      location_longitude: 0,
      search_radius_km: 100,
      calendar_name: "",
      minimum_flyable_hours: 3,
      excluded_calendar_names: new Set(),
      all_calendar_names: [],
    };
    render(
      <SitesMap
        sites={[]}
        mapView={{ center: [47, 10], zoom: 6 }}
        onMapViewChange={() => {}}
        settings={settings}
      />,
    );
    expect(screen.queryByTestId("circle")).toBeNull();
  });

  test("clicking edit in popup invokes onSiteClick with the matching site", () => {
    const sites = [mkSite("S1", [{ lat: 47, lng: 10, site_type: "Hang" }])];
    const onSiteClick = vi.fn();
    render(
      <SitesMap
        sites={sites}
        mapView={{ center: [47, 10], zoom: 6 }}
        onMapViewChange={() => {}}
        onSiteClick={onSiteClick}
      />,
    );
    const editBtn = screen.getByRole("button", { name: "Edit" });
    editBtn.click();
    expect(onSiteClick).toHaveBeenCalledTimes(1);
    expect(onSiteClick.mock.calls[0]?.[0]?.name).toBe("S1");
  });
});
