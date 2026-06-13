import { describe, test, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { useSites } from "./useSites";
import { makeWrapper } from "../test/queryWrapper";

describe("useSites", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  test("returns loading initially and sites after fetch", async () => {
    const sites = [
      { name: "S1", country: "DE", launches: [], landings: [], data_source: "DHV" },
    ];
    (fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      json: async () => sites,
    });

    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useSites(), { wrapper });
    expect(result.current.loading).toBe(true);
    expect(result.current.sites).toEqual([]);

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.sites).toEqual(sites);
    expect(result.current.error).toBeNull();
  });

  test("returns error message when fetch fails", async () => {
    (fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: false,
      status: 500,
      statusText: "Server Error",
      json: async () => ({ message: "down" }),
      text: async () => "",
    });

    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useSites(), { wrapper });
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.sites).toEqual([]);
    expect(result.current.error).toContain("500");
  });
});
