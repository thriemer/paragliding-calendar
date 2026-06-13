import { describe, test, expect, vi } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useFileDragDrop } from "./useFileDragDrop";

function makeDragEvent(file?: File): unknown {
  return {
    preventDefault: vi.fn(),
    dataTransfer: { files: file ? [file] : [] },
  };
}

describe("useFileDragDrop", () => {
  test("starts with isDragging false", () => {
    const { result } = renderHook(() => useFileDragDrop(() => {}));
    expect(result.current.isDragging).toBe(false);
  });

  test("onDragOver sets isDragging true and calls preventDefault", () => {
    const { result } = renderHook(() => useFileDragDrop(() => {}));
    const e = makeDragEvent();
    act(() => {
      result.current.dropZoneProps.onDragOver(e as React.DragEvent);
    });
    expect(result.current.isDragging).toBe(true);
    expect((e as { preventDefault: ReturnType<typeof vi.fn> }).preventDefault).toHaveBeenCalled();
  });

  test("onDragLeave clears isDragging", () => {
    const { result } = renderHook(() => useFileDragDrop(() => {}));
    act(() => {
      result.current.dropZoneProps.onDragOver(makeDragEvent() as React.DragEvent);
    });
    act(() => {
      result.current.dropZoneProps.onDragLeave();
    });
    expect(result.current.isDragging).toBe(false);
  });

  test("onDrop calls onFile with first dropped file and clears isDragging", async () => {
    const onFile = vi.fn();
    const { result } = renderHook(() => useFileDragDrop(onFile));
    act(() => {
      result.current.dropZoneProps.onDragOver(makeDragEvent() as React.DragEvent);
    });

    const file = new File(["x"], "test.kml");
    await act(async () => {
      await result.current.dropZoneProps.onDrop(makeDragEvent(file) as React.DragEvent);
    });

    expect(onFile).toHaveBeenCalledWith(file);
    expect(result.current.isDragging).toBe(false);
  });

  test("onDrop with no file does not call onFile", async () => {
    const onFile = vi.fn();
    const { result } = renderHook(() => useFileDragDrop(onFile));
    await act(async () => {
      await result.current.dropZoneProps.onDrop(makeDragEvent() as React.DragEvent);
    });
    expect(onFile).not.toHaveBeenCalled();
  });

  test("fileInput onChange calls onFile and resets value", async () => {
    const onFile = vi.fn();
    const { result } = renderHook(() => useFileDragDrop(onFile));
    const file = new File(["x"], "a.xml");
    const input = document.createElement("input");
    input.type = "file";
    // Simulate the ref attaching the DOM node
    (result.current.fileInputProps.ref as React.RefObject<HTMLInputElement>).current = input;

    const event = {
      target: { files: [file], value: "fake-path" },
    } as unknown as React.ChangeEvent<HTMLInputElement>;

    await act(async () => {
      await result.current.fileInputProps.onChange(event);
    });

    expect(onFile).toHaveBeenCalledWith(file);
    expect(input.value).toBe("");
  });

  test("fileInputProps has type=file and hidden style", () => {
    const { result } = renderHook(() => useFileDragDrop(() => {}));
    expect(result.current.fileInputProps.type).toBe("file");
    expect(result.current.fileInputProps.style.display).toBe("none");
  });
});
