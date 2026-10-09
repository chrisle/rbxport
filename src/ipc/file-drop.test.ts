// @vitest-environment jsdom
import { beforeEach, expect, it, vi } from "vitest";

const { invoke, onDragDropEvent, scaleFactor, unlisten } = vi.hoisted(() => ({
  invoke: vi.fn(),
  onDragDropEvent: vi.fn(),
  scaleFactor: vi.fn(),
  unlisten: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent }),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ scaleFactor }),
}));

beforeEach(() => {
  vi.resetModules();
  invoke.mockReset();
  onDragDropEvent.mockReset();
  scaleFactor.mockReset();
  unlisten.mockReset();
  Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
  delete (window as { chrome?: unknown }).chrome;
});

/** A stand-in for WebView2's `window.chrome.webview` and its host. */
function fakeWebView2(answer: (request: Record<string, unknown>, files: File[]) => Record<string, unknown> | undefined) {
  const listeners = new Set<(event: { data: unknown }) => void>();
  const webview = {
    postMessageWithAdditionalObjects: vi.fn((message: Record<string, unknown>, files: File[]) => {
      const reply = answer(message, files);
      // The host answers asynchronously, after other pages' traffic.
      queueMicrotask(() => {
        for (const listener of [...listeners]) listener({ data: { unrelated: true } });
        if (reply) for (const listener of [...listeners]) listener({ data: reply });
      });
    }),
    addEventListener: vi.fn((_: "message", listener: (event: { data: unknown }) => void) => listeners.add(listener)),
    removeEventListener: vi.fn((_: "message", listener: (event: { data: unknown }) => void) => listeners.delete(listener)),
  };
  Object.defineProperty(window, "chrome", { value: { webview }, configurable: true });
  return { webview, listeners };
}

it("resolves Explorer drops through WebView2's additional objects on Windows", async () => {
  const { webview, listeners } = fakeWebView2((request, files) => ({
    ...request,
    paths: files.map((file) => `C:\\Users\\dj\\Music\\${file.name}`),
  }));
  const { droppedFilePaths } = await import("./client");
  const files = [new File([], "é #.mp3"), new File([], "b.mp3")];
  expect(await droppedFilePaths(files)).toEqual(["C:\\Users\\dj\\Music\\é #.mp3", "C:\\Users\\dj\\Music\\b.mp3"]);
  const [request, posted] = webview.postMessageWithAdditionalObjects.mock.calls[0] ?? [];
  // An object, not a string, so wry hands it to neither Tauri nor its IPC.
  expect(request).toEqual({ rbxportDroppedFiles: expect.any(String) });
  expect(posted).toBe(files);
  expect(invoke).not.toHaveBeenCalled();
  expect(listeners.size).toBe(0);
});

it("reports the host's refusal of a WebView2 drop", async () => {
  fakeWebView2((request) => ({ ...request, error: "This platform did not provide the dropped files' locations." }));
  const { droppedFilePaths } = await import("./client");
  await expect(droppedFilePaths([new File([], "a.mp3")])).rejects.toThrow("did not provide");
});

it("refuses a WebView2 answer that leaves a file without a path", async () => {
  fakeWebView2((request) => ({ ...request, paths: ["C:\\Music\\a.mp3"] }));
  const { droppedFilePaths } = await import("./client");
  await expect(droppedFilePaths([new File([], "a.mp3"), new File([], "b.mp3")])).rejects.toThrow("did not provide");
});

it("gives up on a WebView2 host that never answers", async () => {
  vi.useFakeTimers();
  try {
    const { listeners } = fakeWebView2(() => undefined);
    const { droppedFilePaths } = await import("./client");
    const result = droppedFilePaths([new File([], "a.mp3")]);
    const settled = expect(result).rejects.toThrow("did not provide");
    await vi.advanceTimersByTimeAsync(10_000);
    await settled;
    expect(listeners.size).toBe(0);
  } finally {
    vi.useRealTimers();
  }
});

it("converts native Linux drop coordinates to CSS pixels", async () => {
  let handler: ((event: { payload: unknown }) => void) | undefined;
  scaleFactor.mockResolvedValue(2);
  onDragDropEvent.mockImplementation((next) => {
    handler = next;
    return Promise.resolve(unlisten);
  });
  const { subscribeNativeFileDrops } = await import("./client");
  const received = vi.fn();
  const stop = subscribeNativeFileDrops(received);
  await vi.waitFor(() => expect(handler).toBeTypeOf("function"));
  handler?.({ payload: { type: "drop", paths: ["/Music/a.mp3"], position: { x: 240, y: 100 } } });
  expect(received).toHaveBeenCalledWith({ paths: ["/Music/a.mp3"], x: 120, y: 50 });
  stop();
  expect(unlisten).toHaveBeenCalledOnce();
});

it("resolves ordinary WKWebView Files through the native bridge", async () => {
  const { droppedFilePaths } = await import("./client");
  invoke.mockResolvedValue(["/Music/é #.mp3", "/Music/b.mp3"]);
  const files = [new File([], "é #.mp3"), new File([], "b.mp3")];
  expect(await droppedFilePaths(files)).toEqual(["/Music/é #.mp3", "/Music/b.mp3"]);
  expect(invoke).toHaveBeenCalledWith("dropped_file_paths", { names: ["é #.mp3", "b.mp3"] });
});

it("never silently drops files whose paths are missing", async () => {
  const { droppedFilePaths } = await import("./client");
  invoke.mockRejectedValue(new Error("Could not resolve the drop"));
  const withPath = Object.assign(new File([], "a.mp3"), { path: "/a.mp3" });
  await expect(droppedFilePaths([withPath, new File([], "b.mp3")])).rejects.toThrow("Could not resolve");
  expect(invoke).toHaveBeenCalledWith("dropped_file_paths", { names: ["a.mp3", "b.mp3"] });
});

it("preserves paths supplied by other desktop hosts", async () => {
  const { droppedFilePaths } = await import("./client");
  const file = Object.assign(new File([], "a.mp3"), { path: "C:\\Music\\a.mp3" });
  expect(await droppedFilePaths([file])).toEqual(["C:\\Music\\a.mp3"]);
  expect(invoke).not.toHaveBeenCalled();
});

it("does not read the pasteboard for an empty drop", async () => {
  const { droppedFilePaths } = await import("./client");
  await expect(droppedFilePaths([])).rejects.toThrow("No files");
  expect(invoke).not.toHaveBeenCalled();
});

it("uses native track drags only in the macOS desktop host", async () => {
  Object.defineProperty(navigator, "platform", { value: "MacIntel", configurable: true });
  const { nativeTrackDragging } = await import("./client");
  expect(nativeTrackDragging()).toBe(true);
  Object.defineProperty(navigator, "platform", { value: "Win32", configurable: true });
  expect(nativeTrackDragging()).toBe(false);
});

it("passes the complete track selection to the native copy drag", async () => {
  const { dragTracksToDesktop } = await import("./client");
  invoke.mockResolvedValue(undefined);
  await dragTracksToDesktop(["12", "34"]);
  expect(invoke).toHaveBeenCalledWith("drag_tracks", { ids: ["12", "34"] });
});

it("reports native drag errors as readable errors", async () => {
  const { dragTracksToDesktop } = await import("./client");
  invoke.mockRejectedValue("The audio file is missing");
  await expect(dragTracksToDesktop(["12"])).rejects.toThrow("The audio file is missing");
});
