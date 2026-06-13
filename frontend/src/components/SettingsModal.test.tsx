import { describe, test, expect, vi } from "vitest";
import { render, screen, fireEvent, act } from "@testing-library/react";
import { SettingsModal } from "./SettingsModal";
import type { UserSettings } from "../hooks/useSettings";

const baseSettings: UserSettings = {
  location_name: "Home",
  location_latitude: 47.5,
  location_longitude: 10.0,
  search_radius_km: 100,
  calendar_name: "Cal",
  minimum_flyable_hours: 3,
  excluded_calendar_names: new Set(["Work"]),
  all_calendar_names: ["Cal", "Work", "Errands"],
};

describe("SettingsModal", () => {
  test("populates form fields from settings", () => {
    render(
      <SettingsModal settings={baseSettings} onSave={() => {}} onCancel={() => {}} />,
    );
    const locName = screen.getByPlaceholderText("e.g., Gornau/Erz") as HTMLInputElement;
    expect(locName.value).toBe("Home");
    const calName = screen.getByPlaceholderText("e.g., Paragliding") as HTMLInputElement;
    expect(calName.value).toBe("Cal");
    expect(screen.getByText(/Lat: 47\.5000/)).toBeTruthy();
    expect(screen.getByText(/Lng: 10\.0000/)).toBeTruthy();
  });

  test("renders a checkbox per available calendar with correct checked state", () => {
    render(
      <SettingsModal settings={baseSettings} onSave={() => {}} onCancel={() => {}} />,
    );
    const work = screen.getByLabelText("Work") as HTMLInputElement;
    const cal = screen.getByLabelText("Cal") as HTMLInputElement;
    expect(work.checked).toBe(true);
    expect(cal.checked).toBe(false);
  });

  test("toggling a calendar updates excluded set on save", () => {
    const onSave = vi.fn();
    render(
      <SettingsModal settings={baseSettings} onSave={onSave} onCancel={() => {}} />,
    );
    fireEvent.click(screen.getByLabelText("Errands"));
    fireEvent.click(screen.getByLabelText("Work"));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(onSave).toHaveBeenCalledTimes(1);
    const saved = onSave.mock.calls[0]?.[0] as UserSettings;
    expect(Array.from(saved.excluded_calendar_names).sort()).toEqual(["Errands"]);
  });

  test("Cancel button triggers onCancel without firing onSave", () => {
    const onCancel = vi.fn();
    const onSave = vi.fn();
    render(
      <SettingsModal settings={baseSettings} onSave={onSave} onCancel={onCancel} />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalled();
    expect(onSave).not.toHaveBeenCalled();
  });

  test("Save passes the edited settings", () => {
    const onSave = vi.fn();
    render(
      <SettingsModal settings={baseSettings} onSave={onSave} onCancel={() => {}} />,
    );
    fireEvent.change(screen.getByPlaceholderText("e.g., Gornau/Erz"), {
      target: { value: "Office" },
    });
    fireEvent.change(screen.getByPlaceholderText("e.g., Paragliding"), {
      target: { value: "FlyDays" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    const saved = onSave.mock.calls[0]?.[0] as UserSettings;
    expect(saved.location_name).toBe("Office");
    expect(saved.calendar_name).toBe("FlyDays");
    expect(saved.all_calendar_names).toEqual(baseSettings.all_calendar_names);
  });

  test("clicking the map updates lat/lng display and is included in save", async () => {
    const onSave = vi.fn();
    render(
      <SettingsModal settings={baseSettings} onSave={onSave} onCancel={() => {}} />,
    );
    const reactLeaflet = (await import("react-leaflet")) as unknown as {
      __clickHandlerRefs: Array<(e: { latlng: { lat: number; lng: number } }) => void>;
    };
    const handler = reactLeaflet.__clickHandlerRefs.at(-1)!;
    act(() => {
      handler({ latlng: { lat: 48.5, lng: 11.25 } });
    });
    expect(screen.getByText(/Lat: 48\.5000/)).toBeTruthy();
    expect(screen.getByText(/Lng: 11\.2500/)).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    const saved = onSave.mock.calls[0]?.[0] as UserSettings;
    expect(saved.location_latitude).toBe(48.5);
    expect(saved.location_longitude).toBe(11.25);
  });
});
