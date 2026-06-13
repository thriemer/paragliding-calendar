import { describe, test, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act, waitFor } from "@testing-library/react";
import { useSiteImport } from "./useSiteImport";
import { makeWrapper } from "../test/queryWrapper";

describe("useSiteImport", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  test("importSites POSTs the file and returns import response", async () => {
    (fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      json: async () => ({ imported: 12 }),
    });
    const file = new File(["<xml/>"], "sites.xml");
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useSiteImport(), { wrapper });

    let r: { imported: number } | null = null;
    await act(async () => {
      r = await result.current.importSites(file);
    });

    expect(r).toEqual({ imported: 12 });
    expect(fetch).toHaveBeenCalledWith(
      "/api/sites/import",
      expect.objectContaining({
        method: "POST",
        body: file,
        headers: { "Content-Type": "application/octet-stream" },
      }),
    );
    await waitFor(() => expect(result.current.result).toEqual({ imported: 12 }));
  });

  test("importSites returns null on failure and exposes error", async () => {
    (fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: false,
      status: 400,
      json: async () => ({ message: "bad xml" }),
      text: async () => "",
    });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => useSiteImport(), { wrapper });

    let r: unknown = "x";
    await act(async () => {
      r = await result.current.importSites(new File(["x"], "f.xml"));
    });
    expect(r).toBeNull();
    await waitFor(() => expect(result.current.error).toContain("bad xml"));
  });
});
