/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { __setBackend } from "@/ipc/client";
import type { Backend } from "@/ipc/types";
import { DEFAULT_PREFERENCES } from "@/lib/preferences";
import { PreferencesProvider } from "@/store/usePreferences";
import { MyTagPanel } from "./MyTagPanel";

let host: HTMLDivElement;
let root: Root;
let edits: { putMyTag: ReturnType<typeof vi.fn>; renameMyTag: ReturnType<typeof vi.fn>; deleteMyTag: ReturnType<typeof vi.fn> };
let backend: Backend;

const CATEGORIES = [{ id: "1", name: "Situation", tags: [{ id: "10", name: "Peak" }, { id: "11", name: "Warm-up" }] }];

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  edits = {
    putMyTag: vi.fn().mockResolvedValue({}),
    renameMyTag: vi.fn().mockResolvedValue(1),
    deleteMyTag: vi.fn().mockResolvedValue(1),
  };
  backend = {
    trackLookups: vi.fn().mockResolvedValue({ keys: [], genres: [], myTagCategories: CATEGORIES }),
    selectionMyTags: vi.fn().mockResolvedValue([]),
    edits,
  } as unknown as Backend;
  __setBackend(backend);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  __setBackend(null);
});

async function mount(myTagComments: boolean) {
  const preferences = { ...DEFAULT_PREFERENCES, advanced: { ...DEFAULT_PREFERENCES.advanced, myTagComments } };
  const onEdit = async (_what: string, edit: (b: Backend) => Promise<unknown>) => { await edit(backend); };
  await act(async () => {
    root.render(
      <PreferencesProvider value={{ preferences, update: vi.fn(), reset: vi.fn() }}>
        <MyTagPanel selection={["t1", "t2"]} readOnly={false} libraryGeneration={0} onEdit={onEdit} />
      </PreferencesProvider>,
    );
    for (let i = 0; i < 6; i++) await Promise.resolve();
  });
}

async function tick(name: string) {
  const box = host.querySelector<HTMLButtonElement>(`button[role="checkbox"][aria-label="${name}"]`);
  if (!box) throw new Error(`no box for ${name}`);
  await act(async () => { box.click(); await Promise.resolve(); });
}

async function rename(from: string, to: string) {
  const row = [...host.querySelectorAll<HTMLElement>('[role="listitem"]')].find((r) => r.textContent === from);
  if (!row) throw new Error(`no row ${from}`);
  await act(async () => { row.click(); await Promise.resolve(); });
  const input = host.querySelector<HTMLInputElement>('input[aria-label="Name"]');
  if (!input) throw new Error("no name box");
  input.value = to;
  await act(async () => { input.blur(); await Promise.resolve(); });
}

it("rewrites the selection's comments with a tick, a rename and a delete when the setting is on (#605)", async () => {
  await mount(true);
  await tick("Peak");
  expect(edits.putMyTag).toHaveBeenCalledWith(["t1", "t2"], "10", true, true);

  await rename("Warm-up", "Opener");
  expect(edits.renameMyTag).toHaveBeenCalledWith("11", "Opener", ["t1", "t2"]);

  const row = [...host.querySelectorAll<HTMLElement>('[role="listitem"]')].find((r) => r.textContent === "Peak");
  await act(async () => {
    row?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, clientX: 5, clientY: 5 }));
    await Promise.resolve();
  });
  const del = [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].find((item) => item.textContent?.includes("Delete"));
  if (!del) throw new Error("no Delete entry");
  await act(async () => { del.click(); await Promise.resolve(); });
  expect(edits.deleteMyTag).toHaveBeenCalledWith("10", ["t1", "t2"]);
});

it("leaves comments alone when the setting is off", async () => {
  await mount(false);
  await tick("Peak");
  expect(edits.putMyTag).toHaveBeenCalledWith(["t1", "t2"], "10", true, false);
  await rename("Warm-up", "Opener");
  expect(edits.renameMyTag).toHaveBeenCalledWith("11", "Opener", undefined);
});
