/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { FolderClashDialog } from "./FolderClashDialog";

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

const button = (label: string) => [...host.querySelectorAll("button")].find((b) => b.textContent === label);

it("names the folder, offers the three answers with Import as a copy focused, and is not for the rest by default", () => {
  const onAnswer = vi.fn();
  act(() => root.render(<FolderClashDialog name="Friday Set" onAnswer={onAnswer} />));
  expect(host.textContent).toContain("A list named Friday Set already exists here.");
  expect([...host.querySelectorAll("button")].map((b) => b.textContent))
    .toEqual(["Keep existing", "Replace existing", "Import as a copy"]);
  expect(document.activeElement).toBe(button("Import as a copy"));
  act(() => button("Replace existing")?.click());
  expect(onAnswer).toHaveBeenCalledWith({ choice: "replace", all: false });
});

it("applies the answer to the rest of the drop when the box is ticked", () => {
  const onAnswer = vi.fn();
  act(() => root.render(<FolderClashDialog name="Friday Set" onAnswer={onAnswer} />));
  const box = host.querySelector<HTMLInputElement>('input[type="checkbox"]');
  expect(box?.checked).toBe(false);
  act(() => box?.click());
  act(() => button("Import as a copy")?.click());
  expect(onAnswer).toHaveBeenCalledWith({ choice: "copy", all: true });
});

it("keeps the old list on Escape, as Cancel does on rekordbox's question", () => {
  const onAnswer = vi.fn();
  act(() => root.render(<FolderClashDialog name="Friday Set" onAnswer={onAnswer} />));
  act(() => { window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })); });
  expect(onAnswer).toHaveBeenCalledWith({ choice: "keep", all: false });
});
