import { describe, test, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { PreferencePair } from "./PreferencePair";
import type { PreferencePairData } from "../hooks/usePreferences";

const pair: PreferencePairData = {
  pair_id: "p1",
  a: {
    id: "t1",
    kind: "hiking",
    title: "Rundwanderung Schrecksee",
    description: "Eine anspruchsvolle Tour durch die Berge.",
    stats: { duration: "4h", difficulty: "mittel" },
    image_hashes: ["abc123def456", "def789ghi012", "jkl345mno678"],
    current_score: 4.2,
  },
  b: {
    id: "s1",
    kind: "paragliding",
    title: "Tegelberg",
    description: "x".repeat(400),
    stats: {},
    image_hashes: [],
    current_score: 3.1,
  },
};

describe("PreferencePair", () => {
  test("renders both activities with kind labels and stats", () => {
    render(<PreferencePair pair={pair} onVote={vi.fn()} onSkip={vi.fn()} />);
    expect(screen.getByText("Rundwanderung Schrecksee")).toBeTruthy();
    expect(screen.getByText("Tegelberg")).toBeTruthy();
    expect(screen.getByText("Hiking")).toBeTruthy();
    expect(screen.getByText("Paragliding")).toBeTruthy();
    expect(screen.getByText("4h")).toBeTruthy();
    expect(screen.getByText("mittel")).toBeTruthy();
  });

  test("renders the primary image only for activities that have one", () => {
    render(<PreferencePair pair={pair} onVote={vi.fn()} onSkip={vi.fn()} />);
    // A has images (primary hash → served URL); B has none.
    const img = screen.getByAltText("Rundwanderung Schrecksee") as HTMLImageElement;
    expect(img.tagName).toBe("IMG");
    expect(img.src).toContain("abc123def456");
    expect(screen.queryByAltText("Tegelberg")).toBeNull();
    // No carousel controls for B (no images).
    expect(screen.queryByRole("button", { name: "Next image" })).toBeTruthy();
  });

  test("the carousel cycles through the whole gallery and wraps around", () => {
    render(<PreferencePair pair={pair} onVote={vi.fn()} onSkip={vi.fn()} />);
    const shown = () =>
      (screen.getByAltText("Rundwanderung Schrecksee") as HTMLImageElement).src;

    expect(shown()).toContain("abc123def456");
    fireEvent.click(screen.getByRole("button", { name: "Next image" }));
    expect(shown()).toContain("def789ghi012");
    fireEvent.click(screen.getByRole("button", { name: "Next image" }));
    expect(shown()).toContain("jkl345mno678");
    // Wraps back to the primary image past the end.
    fireEvent.click(screen.getByRole("button", { name: "Next image" }));
    expect(shown()).toContain("abc123def456");
    // …and backwards from the start to the last image.
    fireEvent.click(screen.getByRole("button", { name: "Previous image" }));
    expect(shown()).toContain("jkl345mno678");
  });

  test("shows the full description, untruncated", () => {
    render(<PreferencePair pair={pair} onVote={vi.fn()} onSkip={vi.fn()} />);
    // The 400-char description must render in full (no ellipsis) so the user
    // can read it before deciding — critical for events, which carry no stats.
    const full = screen.getByText("x".repeat(400));
    expect(full.textContent).toBe("x".repeat(400));
  });

  test("Prefer A / Prefer B / Skip fire the right callbacks", () => {
    const onVote = vi.fn();
    const onSkip = vi.fn();
    render(<PreferencePair pair={pair} onVote={onVote} onSkip={onSkip} />);

    fireEvent.click(screen.getByRole("button", { name: "Prefer A" }));
    expect(onVote).toHaveBeenCalledWith("t1");
    fireEvent.click(screen.getByRole("button", { name: "Prefer B" }));
    expect(onVote).toHaveBeenCalledWith("s1");
    fireEvent.click(screen.getByRole("button", { name: "Skip" }));
    expect(onSkip).toHaveBeenCalledTimes(1);
  });

  test("disables all actions while a vote is in flight", () => {
    render(<PreferencePair pair={pair} onVote={vi.fn()} onSkip={vi.fn()} disabled />);
    expect(screen.getByRole("button", { name: "Prefer A" })).toHaveProperty("disabled", true);
    expect(screen.getByRole("button", { name: "Skip" })).toHaveProperty("disabled", true);
    expect(screen.getByRole("button", { name: "Prefer B" })).toHaveProperty("disabled", true);
  });
});
