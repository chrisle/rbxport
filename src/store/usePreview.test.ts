import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { __setBackend } from "@/ipc/client";
import type { Backend } from "@/ipc/types";
import { PREVIEW_POLL_MS, __previewSnapshot, __resetPreview, onPreviewError, startPreview } from "@/store/usePreview";

let errors: string[];
let refuse: Set<string>;
let backendTrack: string | null;

describe("startPreview refusal", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    __resetPreview();
    errors = [];
    refuse = new Set();
    backendTrack = null;
    onPreviewError((m) => errors.push(m));
    __setBackend({
      previewPlay: (id: string) => {
        if (refuse.has(id)) return Promise.reject(new Error("file missing"));
        backendTrack = id;
        return Promise.resolve();
      },
      previewStop: () => Promise.resolve(),
      previewState: () =>
        Promise.resolve({ track: backendTrack, playing: backendTrack !== null, positionMs: 1234, durationMs: 5000 }),
    } as unknown as Backend);
  });
  afterEach(() => {
    __resetPreview();
    vi.useRealTimers();
  });

  it("keeps the playing track when another track's file is missing", async () => {
    await startPreview("A", 0, 5000);
    refuse.add("B");
    await startPreview("B", 0, 5000);
    expect(errors).toEqual(["file missing"]);
    expect(__previewSnapshot()).toMatchObject({ trackId: "A", playing: true, positionMs: 1234 });
  });

  it("returns to idle when nothing was playing", async () => {
    refuse.add("B");
    await startPreview("B", 0, 5000);
    expect(errors).toHaveLength(1);
    expect(__previewSnapshot()).toMatchObject({ trackId: null, playing: false });
  });

  it("starts the new track when its file opens", async () => {
    await startPreview("A", 0, 5000);
    await startPreview("B", 0, 5000);
    expect(__previewSnapshot()).toMatchObject({ trackId: "B", playing: true });
  });

  it("goes idle when the backend is no longer previewing the previous track", async () => {
    await startPreview("A", 0, 5000);
    backendTrack = null;
    refuse.add("B");
    await startPreview("B", 0, 5000);
    expect(__previewSnapshot()).toMatchObject({ trackId: null });
  });

  it("keeps the playing track when a second refusal lands while the first is pending", async () => {
    await startPreview("A", 0, 5000);
    const pending: Array<() => void> = [];
    const backend = {
      previewPlay: (id: string) =>
        new Promise<void>((_, reject) => {
          pending.push(() => reject(new Error(`${id} missing`)));
        }),
      previewStop: () => Promise.resolve(),
      previewState: () => Promise.resolve({ track: "A", playing: true, positionMs: 1234, durationMs: 5000 }),
    } as unknown as Backend;
    __setBackend(backend);
    const b = startPreview("B", 0, 5000);
    const c = startPreview("C", 0, 5000);
    await vi.advanceTimersByTimeAsync(0);
    pending[0]!(); // B's refusal: stale, discarded
    await b;
    expect(__previewSnapshot()).toMatchObject({ trackId: "C" });
    pending[1]!(); // C's refusal: previous was B's optimistic snapshot
    await c;
    expect(errors).toEqual(["C missing"]);
    expect(__previewSnapshot()).toMatchObject({ trackId: "A", playing: true, positionMs: 1234, durationMs: 5000 });
  });

  it("polls again after restoring the playing track", async () => {
    await startPreview("A", 0, 5000);
    refuse.add("B");
    await startPreview("B", 0, 5000);
    backendTrack = "A";
    const state = vi.fn(() =>
      Promise.resolve({ track: "A", playing: true, positionMs: 4000, durationMs: 5000 }),
    );
    __setBackend({ previewState: state } as unknown as Backend);
    await vi.advanceTimersByTimeAsync(PREVIEW_POLL_MS + 1);
    expect(state).toHaveBeenCalled();
    expect(__previewSnapshot()).toMatchObject({ trackId: "A", positionMs: 4000 });
  });
});
