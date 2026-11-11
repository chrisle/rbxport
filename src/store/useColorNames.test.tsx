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
