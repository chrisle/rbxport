/**
 * @vitest-environment jsdom
 *
 * What the Update Manager window shows in each of the updater's phases:
 * the words, the buttons at the foot, the progress bar, and what Escape
 * does. The state machine itself is `useUpdater`'s business and tested
 * there; these hand the window a state and read the DOM.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { UpdateCheck } from "@/ipc/types";
import type { UpdaterState } from "@/store/useUpdater";
import { UpdateManager } from "./UpdateManager";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

let host: HTMLDivElement;
let root: Root;
let onCheck: ReturnType<typeof vi.fn>;
let onRetry: ReturnType<typeof vi.fn>;
let onRestart: ReturnType<typeof vi.fn>;
let onClose: ReturnType<typeof vi.fn>;

const CHECK: UpdateCheck = {
  currentVersion: "0.4.0",
  version: "0.6.0",
  date: "2026-09-12T00:00:00Z",
  changes: [
    {
      version: "0.6.0",
      date: "2026-09-12",
      body: "## [0.6.0] — 2026-09-12\n\n### Added\n- A `limiter`.\n- Updates.",
    },
    { version: "0.5.0", date: "2026-09-11", body: "## [0.5.0] — 2026-09-11\n\n### Fixed\n- A crash." },
  ],
  ready: null,
  storeInstall: false,
};

function mount(state: UpdaterState) {
  act(() => {
    root.render(
      <UpdateManager state={state} onCheck={onCheck} onRetry={onRetry} onRestart={onRestart} onClose={onClose} />,
    );
  });
}

const text = () => host.textContent ?? "";

function buttons(): string[] {
  return Array.from(host.querySelectorAll("footer button")).map((b) => b.textContent ?? "");
}

function click(label: string) {
  const button = Array.from(host.querySelectorAll("button")).find((b) => b.textContent === label);
  if (!button) throw new Error(`no button "${label}"`);
  act(() => button.click());
}

function escape() {
  act(() => {
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
  });
}

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  onCheck = vi.fn();
  onRetry = vi.fn();
  onRestart = vi.fn();
  onClose = vi.fn();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

describe("UpdateManager", () => {
  it("says it is checking, with only a way out", () => {
    mount({ phase: "checking" });
    expect(text()).toContain("Checking for updates…");
    expect(buttons()).toEqual(["OK"]);
    click("OK");
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("says the running version is the latest", () => {
    mount({ phase: "upToDate", currentVersion: "0.4.0" });
    expect(text()).toContain("rbxport 0.4.0 is the latest version.");
    expect(buttons()).toEqual(["OK"]);
    expect(host.querySelector('[aria-label="Close"]')).not.toBeNull();
  });

  it("a Microsoft Store install points to the Store, with only a way out", () => {
    mount({ phase: "store", currentVersion: "1.2.0" });
    expect(text()).toContain("This copy of rbxport is from the Microsoft Store. Get updates from the Microsoft Store.");
    expect(text()).not.toContain("latest version");
    expect(buttons()).toEqual(["OK"]);
    click("OK");
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("shows a download with both versions and what changed in between, and can be closed over it", () => {
    mount({ phase: "downloading", check: CHECK, progress: { downloaded: 4 * 1024 * 1024, total: 16 * 1024 * 1024 } });
    const versions = Array.from(host.querySelectorAll("dl dd")).map((d) => d.textContent);
    expect(versions).toEqual(["0.4.0", "0.6.0"]);
    expect(text()).toContain("Downloading…");
    const bar = host.querySelector('[role="progressbar"]');
    expect(bar?.getAttribute("aria-valuenow")).toBe("25");
    expect(bar?.hasAttribute("data-indeterminate")).toBe(false);
    expect(text()).toContain("4.0 MB of 16.0 MB");

    // Both sections, newest first, as headings and lists rather than raw markdown.
    const releases = Array.from(host.querySelectorAll("h3")).map((h) => h.textContent);
    expect(releases).toEqual(["Version 0.6.02026-09-12", "Version 0.5.02026-09-11"]);
    expect(Array.from(host.querySelectorAll("h4")).map((h) => h.textContent)).toEqual(["Added", "Fixed"]);
    expect(Array.from(host.querySelectorAll("li")).map((li) => li.textContent)).toEqual([
      "A limiter.",
      "Updates.",
      "A crash.",
    ]);
    expect(host.querySelector("li code")?.textContent).toBe("limiter");
    expect(text()).not.toContain("## [");

    // The download is the app's, not the window's: it can be closed over it.
    expect(buttons()).toEqual(["Close"]);
    expect(host.querySelector('[aria-label="Close"]')).not.toBeNull();
    click("Close");
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("says when there are no notes for the version on offer", () => {
    mount({ phase: "downloading", check: { ...CHECK, changes: [] }, progress: null });
    expect(text()).toContain("No release notes for this version.");
  });

  it("shows a download whose size is unknown as a moving bar with the bytes so far", () => {
    mount({ phase: "downloading", check: CHECK, progress: { downloaded: 300 * 1024, total: null } });
    const bar = host.querySelector('[role="progressbar"]');
    expect(bar?.hasAttribute("aria-valuenow")).toBe(false);
    expect(bar?.hasAttribute("data-indeterminate")).toBe(true);
    expect(text()).toContain("300 KB");
    expect(text()).not.toContain(" of ");
  });

  it("shows an install as a bar with nothing to measure", () => {
    mount({ phase: "installing", check: CHECK });
    expect(text()).toContain("The latest version has been downloaded. Installing…");
    const bar = host.querySelector('[role="progressbar"]');
    expect(bar?.hasAttribute("data-indeterminate")).toBe(true);
    expect(bar?.hasAttribute("aria-valuenow")).toBe(false);
    expect(buttons()).toEqual(["Close"]);
  });

  it("an update in place says the next launch runs it, and offers to restart now", () => {
    mount({ phase: "ready", check: CHECK, ready: { version: "0.6.0", installed: true } });
    expect(text()).toContain("The latest version has been downloaded. It will be used the next time rbxport opens.");
    expect(host.querySelector('[role="progressbar"]')).toBeNull();
    expect(host.querySelectorAll("h3")).toHaveLength(2);
    expect(buttons()).toEqual(["Later", "Restart Now"]);
    click("Restart Now");
    expect(onRestart).toHaveBeenCalledTimes(1);
    click("Later");
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("a failed download says so, keeps the notes, and offers the download again", () => {
    mount({ phase: "failed", message: "The update could not be installed.", check: CHECK });
    const alert = host.querySelector('[role="alert"]');
    expect(alert?.textContent).toContain("An error occurred. Please try later.");
    expect(alert?.textContent).toContain("The update could not be installed.");
    expect(host.querySelectorAll("h3")).toHaveLength(2);
    expect(buttons()).toEqual(["Close", "Try Again"]);
    click("Try Again");
    expect(onRetry).toHaveBeenCalledTimes(1);
    expect(onCheck).not.toHaveBeenCalled();
  });

  it("a failed check says so and offers another check", () => {
    mount({ phase: "failed", message: "The update check could not reach the download server.", check: null });
    expect(host.querySelector('[role="alert"]')?.textContent).toContain(
      "The update check could not reach the download server.",
    );
    expect(host.querySelector("dl")).toBeNull();
    expect(buttons()).toEqual(["Close", "Check Again"]);
    click("Check Again");
    expect(onCheck).toHaveBeenCalledTimes(1);
    expect(onRetry).not.toHaveBeenCalled();
    click("Close");
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("Escape closes the window in every phase, the download going on behind it", () => {
    mount({ phase: "downloading", check: CHECK, progress: null });
    escape();
    expect(onClose).toHaveBeenCalledTimes(1);

    mount({ phase: "installing", check: CHECK });
    escape();
    expect(onClose).toHaveBeenCalledTimes(2);

    mount({ phase: "ready", check: CHECK, ready: { version: "0.6.0", installed: false } });
    escape();
    expect(onClose).toHaveBeenCalledTimes(3);

    mount({ phase: "failed", message: "", check: CHECK });
    escape();
    expect(onClose).toHaveBeenCalledTimes(4);
  });
});
