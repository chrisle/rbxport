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
