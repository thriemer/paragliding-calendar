import { describe, test, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { makeWrapper } from "./test/queryWrapper";
import { App } from "./App";

const sites = [
  {
    name: "Alpha",
    country: "DE",
    launches: [
      {
        location: { latitude: 47, longitude: 10, name: "Top", country: "DE" },
        direction_degrees_start: 0,
        direction_degrees_stop: 180,
        elevation: 1500,
        site_type: "Hang",
      },
    ],
    landings: [],
    data_source: "API",
  },
];

const settings = {
  location_name: "Home",
  location_latitude: 47.5,
  location_longitude: 10.5,
  search_radius_km: 100,
  calendar_name: "Cal",
  minimum_flyable_hours: 3,
  excluded_calendar_names: [],
  all_calendar_names: ["Cal"],
};

function setupFetch(overrides: { sitesPut?: "ok" | "fail"; siteDelete?: "ok" | "fail"; settingsPut?: "ok" | "fail" } = {}) {
  const fetchMock = vi.fn((url: string, init?: RequestInit) => {
    if (url.includes("/api/sites/import")) {
      return Promise.resolve({ ok: true, json: async () => ({ imported: 0 }) });
    }
    if (url.includes("/api/sites/") && init?.method === "DELETE") {
      const ok = overrides.siteDelete !== "fail";
      return Promise.resolve({ ok, status: ok ? 200 : 500, text: async () => (ok ? "" : "delete failed") });
    }
    if (url.includes("/api/sites") && init?.method === "PUT") {
      const ok = overrides.sitesPut !== "fail";
      return Promise.resolve({ ok, status: ok ? 200 : 500, text: async () => (ok ? "" : "save failed") });
    }
    if (url.includes("/api/sites")) {
      return Promise.resolve({ ok: true, json: async () => sites });
    }
    if (url.includes("/api/settings") && init?.method === "PUT") {
      const ok = overrides.settingsPut !== "fail";
      return Promise.resolve({ ok, status: ok ? 200 : 500, text: async () => (ok ? "" : "settings save failed") });
    }
    if (url.includes("/api/settings")) {
      return Promise.resolve({ ok: true, json: async () => settings });
    }
    if (url.includes("/api/weather-models")) {
      return Promise.resolve({ ok: true, json: async () => ({ models: [] }) });
    }
    return Promise.resolve({ ok: true, json: async () => ({}) });
  });
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

function renderApp() {
  const { wrapper: Wrapper } = makeWrapper();
  return render(
    <Wrapper>
      <App />
    </Wrapper>,
  );
}

describe("App", () => {
  beforeEach(() => {
    setupFetch();
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  test("shows loading state until sites resolve", async () => {
    renderApp();
    expect(screen.getByText(/Loading sites/)).toBeTruthy();
    await waitFor(() =>
      expect(screen.queryByText(/Loading sites/)).toBeNull(),
    );
  });

  test("renders Menu sidebar with primary actions", async () => {
    renderApp();
    await waitFor(() =>
      expect(screen.queryByText(/Loading sites/)).toBeNull(),
    );
    expect(screen.getByRole("button", { name: "Create New Site" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Flight Analytics" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Settings" })).toBeTruthy();
  });

  test("Create New Site opens the SiteEditor in create mode", async () => {
    renderApp();
    await waitFor(() => expect(screen.queryByText(/Loading sites/)).toBeNull());
    fireEvent.click(screen.getByRole("button", { name: "Create New Site" }));
    await waitFor(() =>
      expect(screen.getByRole("heading", { name: "Create Site" })).toBeTruthy(),
    );
  });

  test("Cancel closes the SiteEditor", async () => {
    renderApp();
    await waitFor(() => expect(screen.queryByText(/Loading sites/)).toBeNull());
    fireEvent.click(screen.getByRole("button", { name: "Create New Site" }));
    await waitFor(() =>
      expect(screen.getByRole("heading", { name: "Create Site" })).toBeTruthy(),
    );
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("heading", { name: "Create Site" })).toBeNull();
  });

  test("Flight Analytics navigates to flights screen and Back returns to main", async () => {
    renderApp();
    await waitFor(() => expect(screen.queryByText(/Loading sites/)).toBeNull());
    fireEvent.click(screen.getByRole("button", { name: "Flight Analytics" }));
    expect(screen.getByText(/Drop KML flight file/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Back to Main" }));
    expect(screen.getByRole("button", { name: "Create New Site" })).toBeTruthy();
  });

  test("Settings opens the SettingsModal once settings have loaded", async () => {
    renderApp();
    await waitFor(() => expect(screen.queryByText(/Loading sites/)).toBeNull());
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    await waitFor(() =>
      expect(screen.getByRole("heading", { name: "Settings" })).toBeTruthy(),
    );
  });

  test("saving a site closes the editor on success", async () => {
    setupFetch({ sitesPut: "ok" });
    renderApp();
    await waitFor(() => expect(screen.queryByText(/Loading sites/)).toBeNull());
    fireEvent.click(screen.getByRole("button", { name: "Create New Site" }));
    await waitFor(() =>
      expect(screen.getByRole("heading", { name: "Create Site" })).toBeTruthy(),
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(screen.queryByRole("heading", { name: "Create Site" })).toBeNull(),
    );
  });

  test("saving a site keeps the editor open on failure", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    setupFetch({ sitesPut: "fail" });
    renderApp();
    await waitFor(() => expect(screen.queryByText(/Loading sites/)).toBeNull());
    fireEvent.click(screen.getByRole("button", { name: "Create New Site" }));
    await waitFor(() =>
      expect(screen.getByRole("heading", { name: "Create Site" })).toBeTruthy(),
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    // Give the failed PUT time to resolve, then assert the modal is still up.
    await new Promise((r) => setTimeout(r, 50));
    expect(screen.getByRole("heading", { name: "Create Site" })).toBeTruthy();
  });

  test("saving settings closes the SettingsModal on success", async () => {
    setupFetch({ settingsPut: "ok" });
    renderApp();
    await waitFor(() => expect(screen.queryByText(/Loading sites/)).toBeNull());
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    await waitFor(() =>
      expect(screen.getByRole("heading", { name: "Settings" })).toBeTruthy(),
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(screen.queryByRole("heading", { name: "Settings" })).toBeNull(),
    );
  });

  test("saving settings keeps the modal open on failure", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    setupFetch({ settingsPut: "fail" });
    renderApp();
    await waitFor(() => expect(screen.queryByText(/Loading sites/)).toBeNull());
    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    await waitFor(() =>
      expect(screen.getByRole("heading", { name: "Settings" })).toBeTruthy(),
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await new Promise((r) => setTimeout(r, 50));
    expect(screen.getByRole("heading", { name: "Settings" })).toBeTruthy();
  });

  test("deleting an existing site closes the editor on confirm + success", async () => {
    setupFetch({ siteDelete: "ok" });
    vi.stubGlobal("confirm", vi.fn().mockReturnValue(true));
    renderApp();
    await waitFor(() => expect(screen.queryByText(/Loading sites/)).toBeNull());

    // Sample data contains one site "Alpha"; its popup Edit button opens the editor.
    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    await waitFor(() =>
      expect(screen.getByRole("heading", { name: "Edit Site" })).toBeTruthy(),
    );

    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() =>
      expect(screen.queryByRole("heading", { name: "Edit Site" })).toBeNull(),
    );
  });

  test("deleting a site keeps the editor open on server failure", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    setupFetch({ siteDelete: "fail" });
    vi.stubGlobal("confirm", vi.fn().mockReturnValue(true));
    renderApp();
    await waitFor(() => expect(screen.queryByText(/Loading sites/)).toBeNull());

    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    await waitFor(() =>
      expect(screen.getByRole("heading", { name: "Edit Site" })).toBeTruthy(),
    );

    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await new Promise((r) => setTimeout(r, 50));
    expect(screen.getByRole("heading", { name: "Edit Site" })).toBeTruthy();
  });

  test("filter selection narrows the map to only matching sites", async () => {
    // Override sites with two types so the FilterPanel exposes both options.
    const fetchMock = vi.fn((url: string) => {
      if (url.includes("/api/sites")) {
        return Promise.resolve({
          ok: true,
          json: async () => [
            {
              name: "Alpha",
              country: "DE",
              launches: [
                {
                  location: { latitude: 47, longitude: 10, name: "T", country: "DE" },
                  direction_degrees_start: 0,
                  direction_degrees_stop: 360,
                  elevation: 1500,
                  site_type: "Hang",
                },
              ],
              landings: [],
              data_source: "API",
            },
            {
              name: "Beta",
              country: "DE",
              launches: [
                {
                  location: { latitude: 48, longitude: 11, name: "T", country: "DE" },
                  direction_degrees_start: 0,
                  direction_degrees_stop: 360,
                  elevation: 1500,
                  site_type: "Winch",
                },
              ],
              landings: [],
              data_source: "API",
            },
          ],
        });
      }
      if (url.includes("/api/settings")) {
        return Promise.resolve({ ok: true, json: async () => settings });
      }
      if (url.includes("/api/weather-models")) {
        return Promise.resolve({ ok: true, json: async () => ({ models: [] }) });
      }
      return Promise.resolve({ ok: true, json: async () => ({}) });
    });
    vi.stubGlobal("fetch", fetchMock);

    renderApp();
    await waitFor(() => expect(screen.queryByText(/Loading sites/)).toBeNull());

    // Both site markers (one per launch) should be present initially.
    const alphaMarker = () =>
      screen
        .queryAllByTestId("marker")
        .find((m) => m.getAttribute("data-position") === "[47,10]");
    const betaMarker = () =>
      screen
        .queryAllByTestId("marker")
        .find((m) => m.getAttribute("data-position") === "[48,11]");

    expect(alphaMarker()).toBeTruthy();
    expect(betaMarker()).toBeTruthy();

    // Filter to Hang — Beta (Winch) should disappear.
    const filterSelect = screen.getAllByRole("combobox")[0] as HTMLSelectElement;
    fireEvent.change(filterSelect, { target: { value: "Hang" } });
    await waitFor(() => {
      expect(alphaMarker()).toBeTruthy();
      expect(betaMarker()).toBeUndefined();
    });
  });
});
