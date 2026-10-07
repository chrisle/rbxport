/** @vitest-environment jsdom */
import { act, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import type { DriveLibrary } from "@/ipc/types";
import { NewLibraryDialog, type LibraryQuestion } from "./NewLibraryDialog";

declare global { var IS_REACT_ACT_ENVIRONMENT: boolean; }

let host: HTMLDivElement;
let root: Root;

const MISSING: LibraryQuestion = { kind: "missing", masterDb: "/default/master.db" };
const UNAVAILABLE: LibraryQuestion = {
  kind: "unavailable",
  masterDb: "/Volumes/DJ SSD/PIONEER/Master/master.db",
  configuredBy: "rekordbox",
  defaultMasterDb: "/default/master.db",
  defaultExists: false,
};
const ON_DRIVE: DriveLibrary = { name: "T7", volume: "/media/ryan/T7", masterDb: "/media/ryan/T7/PIONEER/Master/master.db" };

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
    onDiscover: vi.fn().mockResolvedValue([]),
    onDrivesChanged: vi.fn().mockReturnValue(() => undefined),
    onOpen: vi.fn().mockResolvedValue(undefined),
    onChoose: vi.fn().mockResolvedValue(false),
    onCreate: vi.fn().mockResolvedValue(undefined),
    onRetry: vi.fn().mockResolvedValue(undefined),
    onQuit: vi.fn(),
    ...overrides,
  };
}

async function render(p: Props) {
  act(() => root.render(<NewLibraryDialog {...p} />));
  await settle();
}

async function settle() {
  await act(async () => { await Promise.resolve(); });
}

const button = (name: string) => [...host.querySelectorAll("button")].find((item) =>
  item.textContent === name || item.getAttribute("aria-label") === name);

it("lists libraries on connected drives and opens the one chosen", async () => {
  const p = props({ onDiscover: vi.fn().mockResolvedValue([ON_DRIVE]) });
  await render(p);

  expect(host.textContent).toContain("Libraries on connected drives");
  expect(host.textContent).toContain(ON_DRIVE.masterDb);
  act(() => { button("Open the library on T7")!.click(); });
  await settle();
  expect(p.onOpen).toHaveBeenCalledWith(ON_DRIVE.masterDb);
});

it("looks again when a drive is connected", async () => {
  let changed: () => void = () => undefined;
  const onDiscover = vi.fn().mockResolvedValueOnce([]).mockResolvedValueOnce([ON_DRIVE]);
  await render(props({ onDiscover, onDrivesChanged: (listener) => { changed = listener; return () => undefined; } }));
  expect(host.textContent).not.toContain(ON_DRIVE.masterDb);

  act(() => changed());
  await settle();
  expect(onDiscover).toHaveBeenCalledTimes(2);
  expect(host.textContent).toContain(ON_DRIVE.masterDb);
});

it("passes localized picker text when choosing a master.db by hand", async () => {
  const p = props();
  await render(p);
  act(() => { button("Choose master.db…")!.click(); });
  await settle();

  expect(p.onChoose).toHaveBeenCalledWith("Choose an existing rekordbox library", "rekordbox database");
  expect(button("Choose master.db…")!.disabled).toBe(false);
});

it("shows a selection error and keeps all options available", async () => {
  const p = props({ onChoose: vi.fn().mockRejectedValue(new Error("That is not a rekordbox database.")) });
  await render(p);
  act(() => { button("Choose master.db…")!.click(); });
  await settle();

  expect(host.querySelector('[role="alert"]')?.textContent).toBe("That is not a rekordbox database.");
  expect([...host.querySelectorAll("button")].every((item) => !item.disabled)).toBe(true);
});

it("for a library on a missing drive offers to try again or use the default, never to create there", async () => {
  const p = props({ problem: UNAVAILABLE });
  await render(p);

  expect(host.querySelector("h2")?.textContent).toBe("Cannot Find Library");
  expect(host.textContent).toContain("rekordbox is set to use a library that cannot be found.");
  expect(host.textContent).toContain(UNAVAILABLE.masterDb);
  expect(host.textContent).toContain("Preferences > Advanced > Database management");
  expect(button("Create New")).toBeUndefined();
  expect(button("Create in Default Location")!.title).toBe("/default/master.db");

  act(() => { button("Try Again")!.click(); });
  await settle();
  expect(p.onRetry).toHaveBeenCalledTimes(1);

  // Still not there: the backend reports the problem again.
  act(() => root.render(<NewLibraryDialog {...p} problem={{ ...UNAVAILABLE }} />));
  await settle();
  expect(host.querySelector('[role="alert"]')?.textContent).toContain("still not there");
  expect(button("Try Again")!.disabled).toBe(false);

  act(() => { button("Create in Default Location")!.click(); });
  await settle();
  expect(p.onCreate).toHaveBeenCalledTimes(1);
});

it("offers the default library by name when one is there", async () => {
  await render(props({ problem: { ...UNAVAILABLE, configuredBy: "rbxport", defaultExists: true } }));
  expect(host.textContent).toContain("The library chosen in RBXport cannot be found.");
  expect(button("Use Default Library")).toBeDefined();
});
