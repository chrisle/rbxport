import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { __setBackend } from "@/ipc/client";
import type { Backend } from "@/ipc/types";
import { __previewSnapshot, __resetPreview, onPreviewError, startPreview } from "@/store/usePreview";

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
});
