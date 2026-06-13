import { describe, test, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { FileUploader } from "./FileUploader";
import { makeWrapper } from "../test/queryWrapper";

function renderInProvider() {
  const { wrapper: Wrapper } = makeWrapper();
  return render(
    <Wrapper>
      <FileUploader />
    </Wrapper>,
  );
}

describe("FileUploader", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  test("renders default prompt text", () => {
    renderInProvider();
    expect(screen.getByText(/Drop DHV XML file here or click to browse/)).toBeTruthy();
  });

  test("hidden file input accepts .xml", () => {
    const { container } = renderInProvider();
    const input = container.querySelector('input[type="file"]') as HTMLInputElement;
    expect(input).toBeTruthy();
    expect(input.getAttribute("accept")).toBe(".xml");
  });

  test("on success shows imported count", async () => {
    (fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      json: async () => ({ imported: 7 }),
    });
    const { container } = renderInProvider();
    const input = container.querySelector('input[type="file"]') as HTMLInputElement;
    const file = new File(["<xml/>"], "sites.xml");
    Object.defineProperty(input, "files", { value: [file] });
    fireEvent.change(input);
    await waitFor(() =>
      expect(screen.getByText("Imported 7 sites")).toBeTruthy(),
    );
  });

  test("on failure shows error message", async () => {
    (fetch as unknown as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: false,
      status: 400,
      statusText: "Bad Request",
      json: async () => ({ message: "invalid" }),
      text: async () => "",
    });
    const { container } = renderInProvider();
    const input = container.querySelector('input[type="file"]') as HTMLInputElement;
    const file = new File(["x"], "broken.xml");
    Object.defineProperty(input, "files", { value: [file] });
    fireEvent.change(input);
    await waitFor(() => expect(screen.getByText(/invalid/)).toBeTruthy());
  });

  test("a successful retry clears the previous error message", async () => {
    const fetchMock = vi.fn();
    fetchMock.mockResolvedValueOnce({
      ok: false,
      status: 400,
      statusText: "Bad Request",
      json: async () => ({ message: "bad xml" }),
      text: async () => "",
    });
    fetchMock.mockResolvedValueOnce({
      ok: true,
      json: async () => ({ imported: 5 }),
    });
    vi.stubGlobal("fetch", fetchMock);

    const { container } = renderInProvider();
    const input = container.querySelector('input[type="file"]') as HTMLInputElement;

    Object.defineProperty(input, "files", {
      value: [new File(["x"], "bad.xml")],
      configurable: true,
    });
    fireEvent.change(input);
    await waitFor(() => expect(screen.getByText(/bad xml/)).toBeTruthy());

    Object.defineProperty(input, "files", {
      value: [new File(["x"], "good.xml")],
      configurable: true,
    });
    fireEvent.change(input);
    await waitFor(() =>
      expect(screen.getByText("Imported 5 sites")).toBeTruthy(),
    );
    expect(screen.queryByText(/bad xml/)).toBeNull();
  });
});
