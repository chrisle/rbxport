/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { confirmHandler } from "@/ipc/confirmHost";
import { ConfirmHost } from "./ConfirmDialog";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

let host: HTMLDivElement;
let root: Root;
let showModal: ReturnType<typeof vi.fn>;
let close: ReturnType<typeof vi.fn>;

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  // jsdom has no modal dialogs or a real top layer.
  showModal = vi.fn();
  close = vi.fn();
  HTMLDialogElement.prototype.showModal = showModal;
  HTMLDialogElement.prototype.close = close;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  act(() => root.render(<ConfirmHost />));
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const buttons = () => [...host.querySelectorAll("button")];

it("asks in its own window, with No focused, and answers with the button pressed", async () => {
  let answer: Promise<boolean> | undefined;
  act(() => { answer = confirmHandler()?.("Remove 495 tracks?"); });
  expect(host.textContent).toContain("Remove 495 tracks?");
  expect(document.activeElement?.textContent).toBe("No");
  act(() => buttons().find((b) => b.textContent === "Yes")?.click());
  await expect(answer).resolves.toBe(true);
  expect(host.textContent).toBe("");
});

it("answers no on Escape and asks a second question after the first", async () => {
  let first: Promise<boolean> | undefined;
  let second: Promise<boolean> | undefined;
  act(() => {
    first = confirmHandler()?.("One?", { yes: "OK", no: "Cancel" });
    second = confirmHandler()?.("Two?");
  });
  expect(host.textContent).toContain("One?");
  expect(buttons().map((b) => b.textContent)).toEqual(["Cancel", "OK"]);
  act(() => { window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })); });
  await expect(first).resolves.toBe(false);
  expect(host.textContent).toContain("Two?");
  act(() => buttons().find((b) => b.textContent === "Yes")?.click());
  await expect(second).resolves.toBe(true);
});

// #839: a div-based backdrop painted behind any open native <dialog>
// (Missing File Manager and the rest use showModal()) regardless of
// z-index, so its OK/Cancel buttons could not be reached until that dialog
// was closed. The fix is to open the prompt itself as a native <dialog> via
// showModal(), which always joins the browser's top layer above regular DOM
// and above a dialog that entered the top layer earlier.
it("asks inside a native dialog opened with showModal, so it joins the top layer above an already-open app dialog", async () => {
  // A Missing-File-Manager-style native dialog, already open, as in the
  // report. Its own showModal/close are instance overrides so calling them
  // does not count against the shared prototype mock asserted on below.
  const otherShowModal = vi.fn();
  const otherClose = vi.fn();
  const other = document.createElement("dialog");
  document.body.append(other);
  other.showModal = otherShowModal;
  other.close = otherClose;
  other.showModal();
  expect(otherShowModal).toHaveBeenCalledTimes(1);

  let answer: Promise<boolean> | undefined;
  act(() => { answer = confirmHandler()?.("Remove 495 tracks?"); });

  const dialog = host.querySelector("dialog");
  expect(dialog).not.toBeNull();
  // Calling showModal() after the other dialog's own call is what makes the
  // UA paint this one on top; a plain backdrop <div> never had this call.
  expect(showModal).toHaveBeenCalledTimes(1);
  expect(dialog?.getAttribute("role")).toBe("alertdialog");

  // Still reachable and clickable while the other dialog is open.
  act(() => buttons().find((b) => b.textContent === "Yes")?.click());
  await expect(answer).resolves.toBe(true);
  expect(close).toHaveBeenCalledTimes(1);

  other.close();
  other.remove();
});
