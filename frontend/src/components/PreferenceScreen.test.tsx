import { describe, test, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { PreferenceScreen } from "./PreferenceScreen";
import { makeWrapper } from "../test/queryWrapper";

const pair = {
  pair_id: "p1",
  a: { id: "t1", kind: "hiking", title: "Tour A", description: "d", stats: {}, image_hashes: [], current_score: 1 },
  b: { id: "t2", kind: "event", title: "Event B", description: "d", stats: {}, image_hashes: [], current_score: 2 },
};

const summary = {
  comparisons_done: 2,
  ratings_done: 0,
  kinds: {},
  validation: { pairwise_accuracy: null, pairwise_count: 0, rating_mse: null, rating_count: 0, k: 0 },
};

function mockFetch() {
  vi.stubGlobal(
    "fetch",
    vi.fn((url: string) => {
      if (url.includes("/compare"))
        return Promise.resolve({ ok: true, json: async () => pair });
      if (url.includes("/api/preferences") && !url.includes("/matrix"))
        return Promise.resolve({ ok: true, json: async () => summary });
      if (url.includes("/matrix"))
        return Promise.resolve({ ok: true, json: async () => ({ hiking: { event: 1 } }) });
      return Promise.resolve({ ok: true, json: async () => ({}) });
    }) as never,
  );
}

function renderScreen(onBack = vi.fn()) {
  const { wrapper: Wrapper } = makeWrapper();
  return render(
    <Wrapper>
      <PreferenceScreen onBack={onBack} />
    </Wrapper>,
  );
}

describe("PreferenceScreen", () => {
  afterEach(() => vi.unstubAllGlobals());

  test("shows the pair and metrics side by side", async () => {
    mockFetch();
    renderScreen();
    await waitFor(() => expect(screen.getByText("Tour A")).toBeTruthy());
    // Metrics section is visible alongside the pair.
    expect(screen.getByText(/^\d+ comparisons$/)).toBeTruthy();
    expect(screen.getByText("Re-embed activities")).toBeTruthy();
  });

  test("Back to Main calls onBack", async () => {
    mockFetch();
    const onBack = vi.fn();
    renderScreen(onBack);
    await waitFor(() => expect(screen.getByText("Tour A")).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "Back to Main" }));
    expect(onBack).toHaveBeenCalledTimes(1);
  });
});
