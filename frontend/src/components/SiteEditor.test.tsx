import { describe, test, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { SiteEditor } from "./SiteEditor";
import { makeWrapper } from "../test/queryWrapper";
import type { ApiSite } from "../hooks/useSites";

const sampleSite: ApiSite = {
  name: "Greifenberg",
  country: "DE",
  launches: [
    {
      location: { latitude: 47.5, longitude: 10.5, name: "Top", country: "DE" },
      direction_degrees_start: 0,
      direction_degrees_stop: 180,
      elevation: 1500,
      site_type: "Hang",
    },
  ],
  landings: [
    {
      location: { latitude: 47.4, longitude: 10.4, name: "LZ", country: "DE" },
      elevation: 800,
    },
  ],
  data_source: "API",
  rating: 3,
  mute_alerts: false,
};

const emptySite: ApiSite = {
  name: "",
  country: null,
  launches: [],
  landings: [],
  data_source: "API",
};

function renderEditor(site: ApiSite, opts: {
  onSave?: (s: ApiSite) => void;
  onDelete?: (n: string) => void;
  onCancel?: () => void;
} = {}) {
  const { wrapper: Wrapper } = makeWrapper();
  return render(
    <Wrapper>
      <SiteEditor
        site={site}
        defaultCenter={[47.0, 10.0]}
        onSave={opts.onSave ?? (() => {})}
        onDelete={opts.onDelete}
        onCancel={opts.onCancel ?? (() => {})}
      />
    </Wrapper>,
  );
}

describe("SiteEditor", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ models: [] }),
    }));
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  test("renders 'Edit Site' title for existing site", () => {
    renderEditor(sampleSite);
    expect(screen.getByRole("heading", { name: "Edit Site" })).toBeTruthy();
  });

  test("renders 'Create Site' title when site name is empty", () => {
    renderEditor(emptySite);
    expect(screen.getByRole("heading", { name: "Create Site" })).toBeTruthy();
  });

  test("renders existing launches and landings", () => {
    renderEditor(sampleSite);
    expect(screen.getByText(/Top \(Hang\)/)).toBeTruthy();
    expect(screen.getByText("LZ")).toBeTruthy();
  });

  test("Save passes current form state to onSave", () => {
    const onSave = vi.fn();
    renderEditor(sampleSite, { onSave });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(onSave).toHaveBeenCalledTimes(1);
    const saved = onSave.mock.calls[0]?.[0] as ApiSite;
    expect(saved.name).toBe("Greifenberg");
    expect(saved.country).toBe("DE");
    expect(saved.launches.length).toBe(1);
    expect(saved.landings.length).toBe(1);
    expect(saved.rating).toBe(3);
    expect(saved.data_source).toBe("API");
  });

  test("editing name flows through to Save payload", () => {
    const onSave = vi.fn();
    renderEditor(sampleSite, { onSave });
    const inputs = screen.getAllByRole("textbox") as HTMLInputElement[];
    // first input is Site Name
    fireEvent.change(inputs[0]!, { target: { value: "Other" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    const saved = onSave.mock.calls[0]?.[0] as ApiSite;
    expect(saved.name).toBe("Other");
  });

  test("empty country saves as null", () => {
    const onSave = vi.fn();
    renderEditor(sampleSite, { onSave });
    const inputs = screen.getAllByRole("textbox") as HTMLInputElement[];
    // second input is Country
    fireEvent.change(inputs[1]!, { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    const saved = onSave.mock.calls[0]?.[0] as ApiSite;
    expect(saved.country).toBeNull();
  });

  test("Cancel triggers onCancel", () => {
    const onCancel = vi.fn();
    renderEditor(sampleSite, { onCancel });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalled();
  });

  test("Delete button appears only when onDelete provided", () => {
    const { rerender } = renderEditor(sampleSite);
    expect(screen.queryByRole("button", { name: "Delete" })).toBeNull();

    const { wrapper: Wrapper } = makeWrapper();
    rerender(
      <Wrapper>
        <SiteEditor
          site={sampleSite}
          defaultCenter={[47.0, 10.0]}
          onSave={() => {}}
          onDelete={() => {}}
          onCancel={() => {}}
        />
      </Wrapper>,
    );
    expect(screen.getByRole("button", { name: "Delete" })).toBeTruthy();
  });

  test("Delete confirms then calls onDelete with site name", () => {
    const onDelete = vi.fn();
    vi.stubGlobal("confirm", vi.fn().mockReturnValue(true));
    renderEditor(sampleSite, { onDelete });
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(onDelete).toHaveBeenCalledWith("Greifenberg");
  });

  test("Delete does not call onDelete when confirm is cancelled", () => {
    const onDelete = vi.fn();
    vi.stubGlobal("confirm", vi.fn().mockReturnValue(false));
    renderEditor(sampleSite, { onDelete });
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(onDelete).not.toHaveBeenCalled();
  });

  test("Add launch button appends a new launch with defaults", () => {
    const onSave = vi.fn();
    renderEditor(sampleSite, { onSave });
    const addButtons = screen.getAllByRole("button", { name: "+ Add" });
    fireEvent.click(addButtons[0]!); // first +Add is for Launches
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    const saved = onSave.mock.calls[0]?.[0] as ApiSite;
    expect(saved.launches.length).toBe(2);
    expect(saved.launches[1]?.site_type).toBe("Hang");
    expect(saved.launches[1]?.direction_degrees_start).toBe(0);
    expect(saved.launches[1]?.direction_degrees_stop).toBe(360);
  });

  test("Add landing button appends a new landing with defaults", () => {
    const onSave = vi.fn();
    renderEditor(sampleSite, { onSave });
    const addButtons = screen.getAllByRole("button", { name: "+ Add" });
    fireEvent.click(addButtons[1]!); // second +Add is for Landings
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    const saved = onSave.mock.calls[0]?.[0] as ApiSite;
    expect(saved.landings.length).toBe(2);
    expect(saved.landings[1]?.elevation).toBe(0);
  });

  test("Mute alerts checkbox toggles muteAlerts in save payload", () => {
    const onSave = vi.fn();
    renderEditor(sampleSite, { onSave });
    fireEvent.click(screen.getByLabelText(/Mute Site Alerts/));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    const saved = onSave.mock.calls[0]?.[0] as ApiSite;
    expect(saved.mute_alerts).toBe(true);
  });

  test("rating > 0 is preserved; rating 0 saves as undefined", () => {
    const onSave = vi.fn();
    renderEditor({ ...sampleSite, rating: 0 }, { onSave });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    const saved = onSave.mock.calls[0]?.[0] as ApiSite;
    expect(saved.rating).toBeUndefined();
  });

  test("Add Parking Location button reveals parking picker", () => {
    renderEditor({ ...sampleSite, parking_location: undefined });
    expect(screen.queryByRole("button", { name: "Remove Parking" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Add Parking Location" }));
    expect(screen.getByRole("button", { name: "Remove Parking" })).toBeTruthy();
  });
});
