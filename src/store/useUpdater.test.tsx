/**
 * @vitest-environment jsdom
 *
 * The Update Manager's state machine against a stub backend: what a check
 * finds, that an update found is downloaded without asking, when the window
 * opens on its own (never, now that nothing waits on an answer), how a
 * download reports, and what a restart that comes back means.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { __setBackend } from "@/ipc/client";
import type { Backend, UpdateCheck, UpdateProgress, UpdateReady } from "@/ipc/types";
import { useUpdater, type Updater } from "./useUpdater";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

let host: HTMLDivElement;
let root: Root;
let updater: Updater;
let progressed: (progress: UpdateProgress) => void;
let check: () => Promise<UpdateCheck>;
let download: () => Promise<UpdateReady>;
let restart: () => Promise<void>;

const AVAILABLE: UpdateCheck = {
  currentVersion: "0.4.0",
  version: "0.5.0",
  date: "2026-09-11T00:00:00Z",
  changes: [{ version: "0.5.0", date: "2026-09-11", body: "## [0.5.0]\n\n- A thing." }],
  ready: null,
};

const UP_TO_DATE: UpdateCheck = { currentVersion: "0.4.0", version: null, date: null, changes: [], ready: null };

const INSTALLED: UpdateReady = { version: "0.5.0", installed: true };

function stubBackend(): Backend {
  return {
    checkForUpdate: () => check(),
    downloadUpdate: () => download(),
    restartToUpdate: () => restart(),
    onUpdateProgress: (listener: (progress: UpdateProgress) => void) => {
      progressed = listener;
      return () => {};
    },
  } as unknown as Backend;
}

function Probe({ auto }: { auto: boolean }) {
  updater = useUpdater(auto);
  return null;
}

async function mount(auto: boolean) {
  act(() => {
    root.render(<Probe auto={auto} />);
  });
  // Let the backend promise settle so the progress listener is registered.
  await act(async () => {
    await Promise.resolve();
  });
}

/** Lets the hook's awaited backend calls resolve. */
async function settle() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  });
}

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  vi.useFakeTimers();
  check = () => Promise.resolve(AVAILABLE);
  download = () => new Promise(() => {});
  restart = () => new Promise(() => {});
  __setBackend(stubBackend());
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => {
    root.unmount();
  });
  host.remove();
  vi.useRealTimers();
});

describe("useUpdater", () => {
  it("a check the app runs on its own never opens the window", async () => {
    check = () => Promise.resolve(UP_TO_DATE);
    await mount(true);
    expect(updater.open).toBe(false);
    act(() => {
      vi.advanceTimersByTime(15_000);
    });
    await settle();
    expect(updater.state.phase).toBe("upToDate");
    expect(updater.open).toBe(false);
  });

  it("downloads a newer version on its own, with nothing shown, and holds it ready", async () => {
    const fetched = vi.fn(() => Promise.resolve(INSTALLED));
    download = fetched;
    await mount(true);
    act(() => {
      vi.advanceTimersByTime(15_000);
    });
    await settle();
    expect(fetched).toHaveBeenCalledTimes(1);
    expect(updater.state).toEqual({ phase: "ready", check: AVAILABLE, ready: INSTALLED });
    expect(updater.automatic).toBe(true);
    expect(updater.open).toBe(false);
  });

  it("does not check on its own when Preferences says not to", async () => {
    const spy = vi.fn(() => Promise.resolve(AVAILABLE));
    check = spy;
    await mount(false);
    act(() => {
      vi.advanceTimersByTime(60_000);
    });
    expect(spy).not.toHaveBeenCalled();
    expect(updater.open).toBe(false);
  });

  it("a check somebody asked for opens the window whatever the answer", async () => {
    check = () => Promise.resolve(UP_TO_DATE);
    await mount(false);
    act(() => updater.check(true));
    expect(updater.open).toBe(true);
    expect(updater.automatic).toBe(false);
    expect(updater.state.phase).toBe("checking");
    await settle();
    expect(updater.state).toEqual({ phase: "upToDate", currentVersion: "0.4.0" });
  });

  it("a download reports its progress and turns into an install at the end", async () => {
    await mount(false);
    act(() => updater.check(true));
    await settle();
    expect(updater.state.phase).toBe("downloading");
    act(() => progressed({ downloaded: 4_000, total: 10_000 }));
    expect(updater.state).toEqual({
      phase: "downloading",
      check: AVAILABLE,
      progress: { downloaded: 4_000, total: 10_000 },
    });
    act(() => progressed({ downloaded: 10_000, total: 10_000 }));
    expect(updater.state).toEqual({ phase: "installing", check: AVAILABLE });
  });

  it("a version already downloaded this run is ready at once, with nothing fetched again", async () => {
    check = () => Promise.resolve({ ...AVAILABLE, ready: INSTALLED });
    const fetched = vi.fn(() => Promise.resolve(INSTALLED));
    download = fetched;
    await mount(false);
    act(() => updater.check(true));
    await settle();
    expect(fetched).not.toHaveBeenCalled();
    expect(updater.state).toEqual({ phase: "ready", check: { ...AVAILABLE, ready: INSTALLED }, ready: INSTALLED });
  });

  it("a check asked for while the download runs shows it rather than starting over", async () => {
    const checked = vi.fn(() => Promise.resolve(AVAILABLE));
    check = checked;
    await mount(true);
    act(() => {
      vi.advanceTimersByTime(15_000);
    });
    await settle();
    expect(updater.state.phase).toBe("downloading");
    expect(updater.open).toBe(false);
    act(() => updater.check(true));
    expect(updater.open).toBe(true);
    expect(updater.state.phase).toBe("downloading");
    expect(checked).toHaveBeenCalledTimes(1);
  });

  it("a download that fails is a failure with the update kept, and can be tried again", async () => {
    download = () => Promise.reject(new Error("The update could not be downloaded."));
    await mount(false);
    act(() => updater.check(true));
    await settle();
    expect(updater.state).toEqual({
      phase: "failed",
      message: "The update could not be downloaded.",
      check: AVAILABLE,
    });
    download = () => Promise.resolve(INSTALLED);
    act(() => updater.retry());
    expect(updater.state.phase).toBe("downloading");
    await settle();
    expect(updater.state.phase).toBe("ready");
  });

  it("a restart that comes back opens the failure, with its reason and the update kept", async () => {
    download = () => Promise.resolve(INSTALLED);
    restart = () => Promise.reject(new Error("The update could not be installed."));
    await mount(false);
    act(() => updater.check(false));
    await settle();
    expect(updater.state.phase).toBe("ready");
    expect(updater.open).toBe(false);
    act(() => updater.restart());
    await settle();
    expect(updater.state).toEqual({
      phase: "failed",
      message: "The update could not be installed.",
      check: AVAILABLE,
    });
    expect(updater.open).toBe(true);
  });

  it("a check that cannot reach the server is a failure with nothing to download", async () => {
    check = () => Promise.reject(new Error("The update check could not reach the download server."));
    await mount(false);
    act(() => updater.check(true));
    await settle();
    expect(updater.state).toEqual({
      phase: "failed",
      message: "The update check could not reach the download server.",
      check: null,
    });
    // Nothing to download again: the button offers another check instead.
    act(() => updater.retry());
    expect(updater.state.phase).toBe("failed");
  });
});
