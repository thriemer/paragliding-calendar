import { describe, test, expect, beforeEach, vi, afterEach } from "vitest";
import { fetchJson } from "./fetchJson";

describe("fetchJson", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  test("returns parsed JSON on 2xx", async () => {
    const data = { hello: "world" };
    (fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      json: async () => data,
    });

    const result = await fetchJson<typeof data>("/foo");
    expect(result).toEqual(data);
    expect(fetch).toHaveBeenCalledWith("/foo", undefined);
  });

  test("forwards RequestInit to fetch", async () => {
    (fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      json: async () => ({}),
    });
    const init = { method: "POST", body: "x" } as RequestInit;
    await fetchJson("/foo", init);
    expect(fetch).toHaveBeenCalledWith("/foo", init);
  });

  test("throws with detail from JSON error body when present", async () => {
    (fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: false,
      status: 500,
      statusText: "Internal Server Error",
      json: async () => ({ message: "boom" }),
      text: async () => "ignored",
    });

    await expect(fetchJson("/foo")).rejects.toThrow(
      "500 Internal Server Error: boom",
    );
  });

  test("falls back to text body when JSON parsing fails", async () => {
    (fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: false,
      status: 400,
      statusText: "Bad Request",
      json: async () => {
        throw new Error("not json");
      },
      text: async () => "plain text error",
    });

    await expect(fetchJson("/foo")).rejects.toThrow(
      "400 Bad Request: plain text error",
    );
  });

  test("uses stringified JSON when error has no message field", async () => {
    (fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: false,
      status: 422,
      statusText: "Unprocessable",
      json: async () => ({ code: 42 }),
      text: async () => "",
    });

    await expect(fetchJson("/foo")).rejects.toThrow(
      '422 Unprocessable: {"code":42}',
    );
  });

  test("omits trailing colon when no detail available", async () => {
    (fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: false,
      status: 404,
      statusText: "Not Found",
      json: async () => {
        throw new Error("not json");
      },
      text: async () => {
        throw new Error("no body");
      },
    });

    await expect(fetchJson("/foo")).rejects.toThrow("404 Not Found");
  });
});
