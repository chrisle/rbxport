/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { __setBackend } from "@/ipc/client";
import type { Backend } from "@/ipc/types";
import { STOCK_COLOR_NAMES, useColorNames } from "./useColorNames";

let host: HTMLDivElement;
let root: Root;
let names: string[];
let changed: (() => void) | null;
let colorNames: ReturnType<typeof vi.fn>;

function Names() {
  return <span>{useColorNames().join(",")}</span>;
}

const settle = () => act(async () => {
  await new Promise((resolve) => setTimeout(resolve, 0));
});

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  names = [...STOCK_COLOR_NAMES];
  changed = null;
  colorNames = vi.fn(() => Promise.resolve([...names]));
  __setBackend({
    colorNames,
    onLibraryChanged: (listener: () => void) => {
      changed = listener;
      return () => { changed = null; };
    },
    onLibraryReady: () => () => {},
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

it("shows the library's names and reads them again when the library changes", async () => {
  names[5] = "Peak";
  act(() => root.render(<><Names /><Names /></>));
  await settle();
  expect(host.textContent).toContain("Pink,Red,Orange,Yellow,Green,Peak,Blue,Purple");
  // One store for every view: one read, however many views name a colour.
  expect(colorNames).toHaveBeenCalledTimes(1);

  names[5] = "Aqua";
  names[0] = "Opener";
  act(() => { changed?.(); });
  await settle();
  expect(host.textContent).toContain("Opener,Red,Orange,Yellow,Green,Aqua,Blue,Purple");
});

it("keeps the stock names while there is no library to ask", async () => {
  colorNames.mockRejectedValue(new Error("not ready"));
  act(() => root.render(<Names />));
  await settle();
  expect(host.textContent).toBe(STOCK_COLOR_NAMES.join(","));
});

it("separates device formats and master owners, and rejects obsolete answers after switching", async () => {
  const pending: { owner: Parameters<Backend["colorNames"]>[0]; done: (names: string[]) => void }[] = [];
  __setBackend({
    colorNames: (owner: Parameters<Backend["colorNames"]>[0]) => new Promise<string[]>(done => pending.push({ owner, done })),
    onLibraryChanged: () => () => {}, onLibraryReady: () => () => {}, onDevicesChanged: () => () => {},
  } as unknown as Backend);
  function Owner({ format }: { format: "oneLibrary" | "deviceLibrary" }) {
    return <span>{useColorNames({ path: "/stick", format }).join(",")}</span>;
  }
  act(() => root.render(<Owner format="oneLibrary" />)); await settle();
  act(() => root.render(<Owner format="deviceLibrary" />)); await settle();
  expect(pending.map(({ owner }) => owner && ({ path: owner.path, format: owner.format }))).toEqual([
    { path: "/stick", format: "oneLibrary" }, { path: "/stick", format: "deviceLibrary" },
  ]);
  await act(async () => { pending[1]!.done(["PDB label", ...STOCK_COLOR_NAMES.slice(1)]); await Promise.resolve(); });
  await act(async () => { pending[0]!.done(["obsolete OneLibrary", ...STOCK_COLOR_NAMES.slice(1)]); await Promise.resolve(); });
  expect(host.textContent).toContain("PDB label");
  expect(host.textContent).not.toContain("obsolete");
  act(() => root.render(<Names />)); await settle();
  expect(pending[2]!.owner).toBeUndefined();
  expect(host.textContent).not.toContain("PDB label");
  await act(async () => { pending[2]!.done(["Master label", ...STOCK_COLOR_NAMES.slice(1)]); await Promise.resolve(); });
  expect(host.textContent).toContain("Master label");
});

it("ignores an earlier refresh and clears owner state when its last subscriber leaves", async () => {
  const pending: ((value: string[]) => void)[] = [];
  colorNames.mockImplementation(() => new Promise<string[]>(done => pending.push(done)));
  act(() => root.render(<Names />)); await settle();
  act(() => { changed?.(); }); await settle();
  await act(async () => { pending[1]!(["Newest", ...STOCK_COLOR_NAMES.slice(1)]); await Promise.resolve(); });
  await act(async () => { pending[0]!(["Old", ...STOCK_COLOR_NAMES.slice(1)]); await Promise.resolve(); });
  expect(host.textContent).toContain("Newest");
  act(() => root.render(null));
  act(() => root.render(<Names />)); await settle();
  expect(host.textContent).not.toContain("Newest");
});

it("uses an explicit unbound fallback rather than reading the master namespace", async () => {
  function Unbound() { return <span>{useColorNames(null).join(",")}</span>; }
  names[0] = "Master name";
  act(() => root.render(<Unbound />)); await settle();
  expect(host.textContent).toBe(STOCK_COLOR_NAMES.join(","));
  expect(colorNames).not.toHaveBeenCalled();
  act(() => root.render(<Names />)); await settle();
  expect(host.textContent).toContain("Master name");
});
