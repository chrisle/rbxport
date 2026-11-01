/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { ImportSkipsDialog } from "./ImportSkipsDialog";

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

const skipped = [
  "Set A.aiff: its tags could not be read: Aiff: File does not contain a COMM chunk",
  "Set B.aiff: its tags could not be read: failed to fill whole buffer",
];

it("lists every skipped file with its own reason, not just the first", () => {
  act(() => root.render(<ImportSkipsDialog skipped={skipped} onClose={vi.fn()} />));
  expect(host.textContent).toContain("2 files were not imported:");
  expect([...host.querySelectorAll("li")].map((li) => li.textContent)).toEqual(skipped);
});

it("closes from the focused Close button and from Escape", () => {
  const onClose = vi.fn();
  act(() => root.render(<ImportSkipsDialog skipped={skipped} onClose={onClose} />));
  const close = [...host.querySelectorAll("button")].find((b) => b.textContent === "Close");
  expect(document.activeElement).toBe(close);
  act(() => close?.click());
  expect(onClose).toHaveBeenCalledTimes(1);
  act(() => { window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })); });
  expect(onClose).toHaveBeenCalledTimes(2);
});
