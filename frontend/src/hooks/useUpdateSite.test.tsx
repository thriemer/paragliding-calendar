import { describe, test, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useUpdateSite } from "./useUpdateSite";
import { makeWrapper } from "../test/queryWrapper";
import type { ApiSite } from "./useSites";

const sampleSite: ApiSite = {
  name: "S1",
  country: "DE",
  launches: [],
  landings: [],
  data_source: "API",
};

describe("useUpdateSite", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
    vi.spyOn(console, "error").mockImplementation(() => {});
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  test("updateSite PUTs the site JSON and returns true on 2xx", async () => {
    (fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      text: async () => "",
    });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useUpdateSite(), { wrapper });
    let ok: boolean = false;
    await act(async () => {
      ok = await result.current.updateSite(sampleSite);
    });
    expect(ok).toBe(true);
    expect(fetch).toHaveBeenCalledWith(
      "/api/sites",
      expect.objectContaining({
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(sampleSite),
      }),
    );
  });

  test("updateSite returns false on non-2xx", async () => {
    (fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: false,
      status: 400,
      text: async () => "bad",
    });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useUpdateSite(), { wrapper });
    let ok: boolean = true;
    await act(async () => {
      ok = await result.current.updateSite(sampleSite);
    });
    expect(ok).toBe(false);
  });

  test("deleteSite DELETEs the encoded URL and returns true on 2xx", async () => {
    (fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      text: async () => "",
    });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useUpdateSite(), { wrapper });
    let ok: boolean = false;
    await act(async () => {
      ok = await result.current.deleteSite("My Site");
    });
    expect(ok).toBe(true);
    expect(fetch).toHaveBeenCalledWith(
      "/api/sites/My%20Site",
      expect.objectContaining({ method: "DELETE" }),
    );
  });

  test("deleteSite returns false on non-2xx", async () => {
    (fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: false,
      status: 500,
      text: async () => "err",
    });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useUpdateSite(), { wrapper });
    let ok: boolean = true;
    await act(async () => {
      ok = await result.current.deleteSite("foo");
    });
    expect(ok).toBe(false);
  });
});
