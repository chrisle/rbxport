/**
 * @vitest-environment jsdom
 *
 * The Updates section in About: the switch writes the preference, and the
 * button checks in place — this pane may be a window of its own, so it
 * talks to the backend directly rather than asking the main window to.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { __setBackend } from "@/ipc/client";
import type { Backend, UpdateCheck, UpdateProgress } from "@/ipc/types";
import { DEFAULT_PREFERENCES, type Preferences } from "@/lib/preferences";
import { PreferencesProvider, type PreferencesStore } from "@/store/usePreferences";
import { AboutPane } from "./AboutPane";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

let host: HTMLDivElement;
let root: Root;
let update: ReturnType<typeof vi.fn>;
let checkForUpdate: ReturnType<typeof vi.fn>;
let downloadUpdate: ReturnType<typeof vi.fn>;
let readyUpdate: ReturnType<typeof vi.fn>;
let openUrl: ReturnType<typeof vi.fn>;
let progressListeners: Set<(progress: UpdateProgress) => void>;

function mount(preferences: Preferences = DEFAULT_PREFERENCES) {
  const store = { preferences, update, reset: vi.fn() } as unknown as PreferencesStore;
  act(() => {
    root.render(
      <PreferencesProvider value={store}>
        <AboutPane />
      </PreferencesProvider>,
    );
  });
}

const settle = () =>
  act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });

function section(title = "About"): Element {
  const found = host.querySelector(`section[aria-label="${title}"]`);
  if (!found) throw new Error(`no ${title} section`);
  return found;
}

function checkButton(): HTMLButtonElement {
  const button = Array.from(section().querySelectorAll("button")).find(
    (b) => b.textContent === "Check for updates" || b.textContent === "Checking…",
  );
  if (!button) throw new Error("no Check for Updates button");
  return button;
}

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  update = vi.fn();
  progressListeners = new Set();
  checkForUpdate = vi.fn<() => Promise<UpdateCheck>>();
  downloadUpdate = vi.fn().mockResolvedValue({ version: "0.5.0", installed: true });
  readyUpdate = vi.fn().mockResolvedValue(null);
  openUrl = vi.fn().mockResolvedValue(undefined);
  __setBackend({
    appVersion: () => Promise.resolve("0.4.0"),
    checkForUpdate,
    downloadUpdate,
    readyUpdate,
    onUpdateProgress: (listener: (progress: UpdateProgress) => void) => {
      progressListeners.add(listener);
      return () => progressListeners.delete(listener);
    },
    openUrl,
  } as unknown as Backend);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  __setBackend(null);
});

describe("AboutPane › Updates", () => {
  it("offers a support link below automatic updates", async () => {
    mount();
    await settle();
    const button = Array.from(section().querySelectorAll("button")).find((candidate) => candidate.textContent?.includes("Support rbxport"));
    expect(section().textContent).toContain("rbxport is free to use and independently developed by TRIODE.");
    expect(section().textContent).toContain("If it makes your DJ workflow easier, consider supporting continued development and future features.");
    act(() => button?.click());
    await settle();
    expect(openUrl).toHaveBeenCalledWith("https://www.paypal.com/donate/?hosted_button_id=H6GGU8PHP8CJE");
  });

  it("the switch shows the preference and writes its opposite", async () => {
    mount();
    await settle();
    const toggle = section().querySelector<HTMLInputElement>('input[role="switch"]');
    expect(toggle?.checked).toBe(true);
    act(() => toggle?.click());
    expect(update).toHaveBeenCalledWith("advanced", { checkUpdates: false });
    expect(update).toHaveBeenCalledTimes(1);
  });

  it("the switch reads a stored no as off and writes yes back", async () => {
    mount({ ...DEFAULT_PREFERENCES, advanced: { ...DEFAULT_PREFERENCES.advanced, checkUpdates: false } });
    await settle();
    const toggle = section().querySelector<HTMLInputElement>('input[role="switch"]');
    expect(toggle?.checked).toBe(false);
    act(() => toggle?.click());
    expect(update).toHaveBeenCalledWith("advanced", { checkUpdates: true });
  });

  it("the button checks here and reports an update found", async () => {
    downloadUpdate.mockReturnValue(new Promise(() => {}));
    checkForUpdate.mockResolvedValue({
      currentVersion: "0.4.0", version: "0.5.0", date: null, changes: [], ready: null, storeInstall: false,
    });
    mount();
    act(() => checkButton().click());
    await settle();
    expect(checkForUpdate).toHaveBeenCalledTimes(1);
    expect(downloadUpdate).toHaveBeenCalledTimes(1);
    expect(update).not.toHaveBeenCalled();
    expect(section().textContent).toContain("Update available v0.5.0.");
  });

  it("shows an update already downloaded before Check for updates is clicked", async () => {
    readyUpdate.mockResolvedValue({ version: "0.5.0", installed: true });
    mount();
    await settle();
    expect(section().textContent).toContain("Update v0.5.0 downloaded — restart rbxport to use it.");
    expect(checkForUpdate).not.toHaveBeenCalled();
  });

  it("the button reports being up to date", async () => {
    checkForUpdate.mockResolvedValue({
      currentVersion: "0.4.0", version: null, date: null, changes: [], ready: null, storeInstall: false,
    });
    mount();
    act(() => checkButton().click());
    await settle();
    expect(downloadUpdate).not.toHaveBeenCalled();
    expect(section().textContent).toContain("rbxport v0.4.0 is up to date.");
  });

  it("the button in a Microsoft Store install points to the Store and downloads nothing", async () => {
    checkForUpdate.mockResolvedValue({
      currentVersion: "1.2.0", version: null, date: null, changes: [], ready: null, storeInstall: true,
    });
    mount();
    act(() => checkButton().click());
    await settle();
    expect(downloadUpdate).not.toHaveBeenCalled();
    expect(section().textContent).toContain("This copy of rbxport is from the Microsoft Store. Get updates from the Microsoft Store.");
    expect(section().textContent).not.toContain("is up to date");
  });

  it("a check that cannot reach the server shows an error, not a crash", async () => {
    checkForUpdate.mockRejectedValue(new Error("offline"));
    mount();
    act(() => checkButton().click());
    await settle();
    expect(section().querySelector('[role="alert"]')?.textContent).toBe("Couldn’t check for updates. Please try again.");
  });

  it("a download in progress shows a bar under Automatic updates, whoever started it", async () => {
    mount();
    await settle();
    expect(section().querySelector('[role="progressbar"]')).toBeNull();
    act(() => {
      for (const listener of progressListeners) listener({ downloaded: 500_000, total: 2_000_000 });
    });
    const bar = section().querySelector('[role="progressbar"]');
    expect(bar).not.toBeNull();
    expect(bar?.getAttribute("aria-valuenow")).toBe("25");
    act(() => {
      for (const listener of progressListeners) listener({ downloaded: 2_000_000, total: 2_000_000 });
    });
    expect(section().querySelector('[role="progressbar"]')).toBeNull();
  });
});
