import { describe, test, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { useWeatherModels } from "./useWeatherModels";
import { makeWrapper } from "../test/queryWrapper";

describe("useWeatherModels", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  test("returns models array from response", async () => {
    const models = [
      { id: "icon", name: "ICON" },
      { id: "gfs", name: "GFS" },
    ];
    (fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      json: async () => ({ models }),
    });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useWeatherModels(), { wrapper });
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.models).toEqual(models);
  });

  test("returns empty array when models missing from response", async () => {
    (fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      json: async () => ({}),
    });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useWeatherModels(), { wrapper });
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.models).toEqual([]);
  });

  test("exposes error on fetch failure", async () => {
    (fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: false,
      status: 503,
      statusText: "Unavailable",
      json: async () => ({ message: "down" }),
      text: async () => "",
    });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useWeatherModels(), { wrapper });
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.models).toEqual([]);
    expect(result.current.error).toContain("503");
  });
});
