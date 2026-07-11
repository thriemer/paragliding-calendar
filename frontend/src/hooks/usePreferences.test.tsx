import { describe, test, expect, vi, afterEach } from "vitest";
import { renderHook, act, waitFor } from "@testing-library/react";
import { usePreferences } from "./usePreferences";
import { makeWrapper } from "../test/queryWrapper";

const activity = (id: string, kind: string) => ({
  id,
  kind,
  title: `Title ${id}`,
  description: "desc",
  stats: {},
  image_hashes: [],
  current_score: 1,
});

const pairA = { pair_id: "p1", a: activity("t1", "hiking"), b: activity("t2", "event") };
const pairB = { pair_id: "p2", a: activity("t3", "biking"), b: activity("t4", "event") };

type Handler = (url: string, init?: RequestInit) => Promise<unknown>;

function mockFetch(handler: Handler) {
  vi.stubGlobal("fetch", vi.fn(handler as never));
}

describe("usePreferences", () => {
  afterEach(() => vi.unstubAllGlobals());

  test("loads the initial pair from /compare", async () => {
    mockFetch((url) => {
      if (url.includes("/compare"))
        return Promise.resolve({ ok: true, json: async () => pairA });
      return Promise.resolve({ ok: true, json: async () => ({}) });
    });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => usePreferences(), { wrapper });
    await waitFor(() => expect(result.current.pair?.pair_id).toBe("p1"));
  });

  test("vote advances to the next pair and updates progress", async () => {
    mockFetch((url) => {
      if (url.includes("/vote"))
        return Promise.resolve({
          ok: true,
          json: async () => ({ next: pairB, progress: { comparisons_done: 1 } }),
        });
      if (url.includes("/compare"))
        return Promise.resolve({ ok: true, json: async () => pairA });
      return Promise.resolve({ ok: true, json: async () => ({}) });
    });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => usePreferences(), { wrapper });
    await waitFor(() => expect(result.current.pair?.pair_id).toBe("p1"));

    act(() => result.current.vote("t1"));
    await waitFor(() => expect(result.current.pair?.pair_id).toBe("p2"));
    expect(result.current.comparisonsDone).toBe(1);
  });

  test("skip re-samples a fresh pair", async () => {
    let compareCalls = 0;
    mockFetch((url) => {
      if (url.includes("/compare")) {
        compareCalls += 1;
        return Promise.resolve({ ok: true, json: async () => (compareCalls === 1 ? pairA : pairB) });
      }
      return Promise.resolve({ ok: true, json: async () => ({}) });
    });
    const { wrapper } = makeWrapper();
    const { result } = renderHook(() => usePreferences(), { wrapper });
    await waitFor(() => expect(result.current.pair?.pair_id).toBe("p1"));

    act(() => result.current.skip());
    await waitFor(() => expect(result.current.pair?.pair_id).toBe("p2"));
  });
});
