import { describe, test, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { PreferenceProgress } from "./PreferenceProgress";

describe("PreferenceProgress", () => {
  test("renders the done/total label and progressbar aria values", () => {
    render(<PreferenceProgress done={12} total={30} />);
    expect(screen.getByText("12 / 30")).toBeTruthy();
    const bar = screen.getByRole("progressbar");
    expect(bar.getAttribute("aria-valuenow")).toBe("12");
    expect(bar.getAttribute("aria-valuemax")).toBe("30");
  });

  test("guards against a zero total", () => {
    render(<PreferenceProgress done={0} total={0} />);
    expect(screen.getByText("0 / 0")).toBeTruthy();
  });
});
