/** @vitest-environment jsdom */
import { act, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { NewLibraryDialog, type LibraryQuestion } from "./NewLibraryDialog";

declare global { var IS_REACT_ACT_ENVIRONMENT: boolean; }

let host: HTMLDivElement;
let root: Root;

const MISSING: LibraryQuestion = { kind: "missing", masterDb: "/default/master.db" };
const UNAVAILABLE: LibraryQuestion = {
  kind: "unavailable",
  masterDb: "/Volumes/DJ SSD/PIONEER/Master/master.db",
  defaultMasterDb: "/default/master.db",
};

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

type Props = ComponentProps<typeof NewLibraryDialog>;

function props(overrides: Partial<Props> = {}): Props {
  return {
    problem: MISSING,
    onCreate: vi.fn().mockResolvedValue(undefined),
    onUseDefault: vi.fn().mockResolvedValue(undefined),
    onConfirm: vi.fn().mockResolvedValue(true),
    onQuit: vi.fn(),
    ...overrides,
  };
}

async function render(p: Props) {
  act(() => root.render(<NewLibraryDialog {...p} />));
  await settle();
}

async function settle() {
  await act(async () => { await Promise.resolve(); await Promise.resolve(); });
}

const button = (name: string) => [...host.querySelectorAll("button")].find((item) => item.textContent === name);
const buttons = () => [...host.querySelectorAll("button")].map((item) => item.textContent);

it("with no library anywhere asks to create one or quit, and nothing else", async () => {
  const p = props();
  await render(p);
  expect(host.textContent).toContain("Would you like to create a new database?");
  expect(buttons()).toEqual(["Create", "Quit"]);
  act(() => { button("Create")?.click(); });
  expect(p.onCreate).toHaveBeenCalledOnce();
});

it("a missing drive asks rekordbox's question, Yes or No, with no other way out", async () => {
  await render(props({ problem: UNAVAILABLE }));
  expect(host.querySelector("p")?.textContent).toBe(
    "Cannot find Master Database."
    + "Launch RBXport after connecting a drive where Master Database is stored."
    + "Do you want to open Master Database in the default drive?",
  );
  expect(buttons()).toEqual(["Yes", "No"]);
  expect(host.textContent).not.toContain("Create");
});

it("Yes confirms the move to the default drive before using it", async () => {
  const p = props({ problem: UNAVAILABLE });
  await render(p);
  act(() => { button("Yes")?.click(); });
  await settle();
  expect(p.onConfirm).toHaveBeenCalledWith(
    "Location of Master Database will be changed to the default drive.\n"
    + "The location can be changed at [Advanced] tab of [Preferences] window.",
    { yes: "OK", no: "Cancel" },
  );
  expect(p.onUseDefault).toHaveBeenCalledOnce();
  expect(p.onCreate).not.toHaveBeenCalled();
});

it("Cancel at the confirmation keeps the question open and changes nothing", async () => {
  const p = props({ problem: UNAVAILABLE, onConfirm: vi.fn().mockResolvedValue(false) });
  await render(p);
  act(() => { button("Yes")?.click(); });
  await settle();
  expect(p.onUseDefault).not.toHaveBeenCalled();
  expect(button("Yes")?.disabled).toBe(false);
});

it("No quits, as rekordbox ends the launch", async () => {
  const p = props({ problem: UNAVAILABLE });
  await render(p);
  act(() => button("No")?.click());
  expect(p.onQuit).toHaveBeenCalledOnce();
  expect(p.onUseDefault).not.toHaveBeenCalled();
});

it("a failed switch says so and lets the question be answered again", async () => {
  const p = props({ problem: UNAVAILABLE, onUseDefault: vi.fn().mockRejectedValue({ kind: "internal", message: "Failed to switch Master Database." }) });
  await render(p);
  act(() => { button("Yes")?.click(); });
  await settle();
  expect(host.querySelector('[role="alert"]')?.textContent).toBe("Failed to switch Master Database.");
  expect(button("Yes")?.disabled).toBe(false);
});
