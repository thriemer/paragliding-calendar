import { describe, test, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act, waitFor } from "@testing-library/react";
import { useSettings, type UserSettings } from "./useSettings";
import { makeWrapper } from "../test/queryWrapper";

const responseBody = {
  location_name: "Home",
  location_latitude: 47.5,
  location_longitude: 10.0,
  search_radius_km: 100,
  calendar_name: "Cal",
  minimum_flyable_hours: 3,
  excluded_calendar_names: ["Work", "Errands"],
  all_calendar_names: ["Cal", "Work", "Errands"],
};

describe("useSettings", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  test("loads settings and converts excluded list to Set", async () => {
    (fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      json: async () => responseBody,
    });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useSettings(), { wrapper });
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.settings).not.toBeNull();
    expect(result.current.settings!.excluded_calendar_names).toBeInstanceOf(Set);
    expect(result.current.settings!.excluded_calendar_names.has("Work")).toBe(true);
    expect(result.current.settings!.excluded_calendar_names.has("Cal")).toBe(false);
  });

  test("updateSettings PUTs with array form and returns true on success", async () => {
    const fetchMock = fetch as unknown as ReturnType<typeof vi.fn>;
    fetchMock.mockResolvedValueOnce({ ok: true, json: async () => responseBody });
    fetchMock.mockResolvedValueOnce({ ok: true, text: async () => "" });

    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useSettings(), { wrapper });
    await waitFor(() => expect(result.current.loading).toBe(false));

    const next: UserSettings = {
      ...result.current.settings!,
      location_name: "Updated",
      excluded_calendar_names: new Set(["Work"]),
    };

    let ok: boolean = false;
    await act(async () => {
      ok = await result.current.updateSettings(next);
    });
    expect(ok).toBe(true);

    const putCall = fetchMock.mock.calls.find((c) => c[1]?.method === "PUT");
    expect(putCall).toBeTruthy();
    const body = JSON.parse(putCall![1].body as string);
    expect(Array.isArray(body.excluded_calendar_names)).toBe(true);
    expect(body.excluded_calendar_names).toEqual(["Work"]);
    expect(body.location_name).toBe("Updated");
  });

  test("updateSettings returns false on server error", async () => {
    const fetchMock = fetch as unknown as ReturnType<typeof vi.fn>;
    fetchMock.mockResolvedValueOnce({ ok: true, json: async () => responseBody });
    fetchMock.mockResolvedValueOnce({ ok: false, status: 500, text: async () => "boom" });

    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useSettings(), { wrapper });
    await waitFor(() => expect(result.current.loading).toBe(false));

    let ok: boolean = true;
    await act(async () => {
      ok = await result.current.updateSettings(result.current.settings!);
    });
    expect(ok).toBe(false);
    await waitFor(() => expect(result.current.error).toBeTruthy());
  });
});
