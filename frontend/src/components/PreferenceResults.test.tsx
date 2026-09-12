import { describe, test, expect } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { PreferenceResults } from "./PreferenceResults";
import type { PreferenceSummary } from "../hooks/usePreferences";

const summary: PreferenceSummary = {
  comparisons_done: 12,
  ratings_done: 3,
  validation: { pairwise_accuracy: null, pairwise_count: 0, rating_mse: null, rating_count: 0, k: 0 },
  kinds: {
    hiking: { base_pref: 0.5, activity_count: 45, features: [] },
    paragliding: {
      base_pref: 1.2,
      activity_count: 12,
      features: [
        { feature: "paragliding_height_difference_log", weight: 0.8, direction: "higher is better" },
      ],
    },
  },
};

const baseProps = {
  summary: null as PreferenceSummary | null,
  loading: false,
  error: null as string | null,
  matrix: null,
  matrixLoading: false,
  matrixError: null as string | null,
  onReEmbed: () => {},
  reEmbedding: false,
};

describe("PreferenceResults", () => {
  test("shows an error state", () => {
    render(<PreferenceResults {...baseProps} error="boom" />);
    expect(screen.getByText(/boom/)).toBeTruthy();
  });

  test("renders tallies, kinds and collapsible features", () => {
    render(<PreferenceResults {...baseProps} summary={summary} />);
    expect(screen.getByText("Hiking")).toBeTruthy();
    expect(screen.getByText("Paragliding")).toBeTruthy();

    // Features are collapsed by default — not visible.
    expect(screen.queryByText("paragliding_height_difference_log")).toBeNull();
    expect(screen.queryByText("higher is better")).toBeNull();
  });

  test("clicking a kind row reveals its features", () => {
    render(<PreferenceResults {...baseProps} summary={summary} />);

    // Initially collapsed
    expect(screen.queryByText("paragliding_height_difference_log")).toBeNull();

    // Click the Paragliding button to expand
    const paraglidingBtn = screen.getByText("Paragliding").closest("button")!;
    fireEvent.click(paraglidingBtn);

    // Now features are visible
    expect(screen.getByText("paragliding_height_difference_log")).toBeTruthy();
    expect(screen.getByText("higher is better")).toBeTruthy();
  });

  test("clicking the same kind again hides its features", () => {
    render(<PreferenceResults {...baseProps} summary={summary} />);

    const paraglidingBtn = screen.getByText("Paragliding").closest("button")!;
    fireEvent.click(paraglidingBtn);
    expect(screen.getByText("paragliding_height_difference_log")).toBeTruthy();

    fireEvent.click(paraglidingBtn);
    expect(screen.queryByText("paragliding_height_difference_log")).toBeNull();
  });

  test("orders kinds by descending base preference", () => {
    render(<PreferenceResults {...baseProps} summary={summary} />);
    const badges = screen.getAllByText(/Hiking|Paragliding/);
    expect(badges.map((b) => b.textContent)).toEqual(["Paragliding", "Hiking"]);
  });

  test("shows re-embed button", () => {
    render(<PreferenceResults {...baseProps} />);
    expect(screen.getByText("Re-embed activities")).toBeTruthy();
  });
});