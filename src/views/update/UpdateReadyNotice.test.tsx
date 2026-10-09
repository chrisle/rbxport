/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { UpdateReadyNotice } from "./UpdateReadyNotice";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

describe("UpdateReadyNotice", () => {
  it("announces the downloaded update in the status bar and offers its actions", () => {
    const onRestart = vi.fn();
    const onWhatsNew = vi.fn();
    const onDismiss = vi.fn();
    act(() => root.render(
      <UpdateReadyNotice
        state={{
          phase: "ready",
          check: { currentVersion: "1.2.2", version: "1.2.3", date: null, changes: [], ready: null, storeInstall: false },
          ready: { version: "1.2.3", installed: true },
        }}
        onRestart={onRestart}
        onWhatsNew={onWhatsNew}
        onDismiss={onDismiss}
      />,
    ));

    expect(host.textContent).toContain("Update ready. Restart to apply.");
    const buttons = Array.from(host.querySelectorAll("button"));
    expect(buttons.map((button) => button.textContent)).toEqual(["What's new", "Restart now"]);
    act(() => buttons[0]?.click());
    expect(onWhatsNew).toHaveBeenCalledOnce();
    act(() => buttons[1]?.click());
    expect(onRestart).toHaveBeenCalledOnce();
  });

  it("dismisses itself after 15 seconds", () => {
    vi.useFakeTimers();
    const onDismiss = vi.fn();
    act(() => root.render(
      <UpdateReadyNotice
        state={{ phase: "ready", check: { currentVersion: "1.2.2", version: "1.2.3", date: null, changes: [], ready: null, storeInstall: false }, ready: { version: "1.2.3", installed: true } }}
        onRestart={vi.fn()}
        onWhatsNew={vi.fn()}
        onDismiss={onDismiss}
      />,
    ));
    act(() => {
      vi.advanceTimersByTime(15_000);
    });
    expect(onDismiss).toHaveBeenCalledOnce();
    vi.useRealTimers();
  });
});
