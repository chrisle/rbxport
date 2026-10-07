/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { NewLibraryDialog } from "./NewLibraryDialog";

declare global { var IS_REACT_ACT_ENVIRONMENT: boolean; }

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  HTMLDialogElement.prototype.showModal = vi.fn();
  HTMLDialogElement.prototype.close = vi.fn();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

it("offers an existing database and passes localized picker text", async () => {
  const choose = vi.fn().mockResolvedValue(false);
  act(() => root.render(
    <NewLibraryDialog masterDb="/default/master.db" onCreate={vi.fn()} onChoose={choose} onQuit={vi.fn()} />,
  ));

  const button = [...host.querySelectorAll("button")].find((item) => item.textContent === "Choose Existing…")!;
  act(() => { button.click(); });
  await act(async () => { await Promise.resolve(); });

  expect(choose).toHaveBeenCalledWith("Choose an existing rekordbox library", "rekordbox database");
  expect(button.disabled).toBe(false);
});

it("shows a selection error and keeps all options available", async () => {
  const choose = vi.fn().mockRejectedValue(new Error("That is not a rekordbox database."));
  act(() => root.render(
    <NewLibraryDialog masterDb="/default/master.db" onCreate={vi.fn()} onChoose={choose} onQuit={vi.fn()} />,
  ));

  act(() => { host.querySelector("button")!.click(); });
  await act(async () => { await Promise.resolve(); });

  expect(host.querySelector('[role="alert"]')?.textContent).toBe("That is not a rekordbox database.");
  expect([...host.querySelectorAll("button")].every((button) => !button.disabled)).toBe(true);
});
