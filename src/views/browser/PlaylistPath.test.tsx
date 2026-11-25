/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { __setBackend } from "@/ipc/client";
import type { Backend, RelatedPlaylist } from "@/ipc/types";
import { PlaylistPath } from "./PlaylistPath";
let host: HTMLDivElement;
let root: Root;
let requests: { id: string; done: (paths: RelatedPlaylist[]) => void; fail: (error: unknown) => void }[];
const choose = vi.fn();
const log = vi.fn();
const settle = () => act(async () => { await Promise.resolve(); });
function render(track: string | null, owner = "collection") {
  act(() => root.render(<PlaylistPath caption="Collection (20 Tracks)" track={track} owner={owner} onChoose={choose} />));
}
beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  requests = []; choose.mockClear(); log.mockClear();
  __setBackend({ trackPlaylists: (id: string) => new Promise<RelatedPlaylist[]>((done, fail) => requests.push({ id, done, fail })),
    logStatusMessage: log.mockResolvedValue(undefined),
  } as unknown as Backend);
  host = document.createElement("div"); root = createRoot(host);
});
afterEach(() => { act(() => root.unmount()); __setBackend(null); });
it("offers literal full paths by ID and opens the selected track in the chosen list", async () => {
  render("101"); await settle();
  expect(host.querySelector("select")!.disabled).toBe(true);
  await act(async () => { requests[0]!.done([{ id: "1", name: "Play", path: "Shows" }, { id: "2", name: "Play", path: "Archive" }]); await Promise.resolve(); });
  const select = host.querySelector("select")!;
  expect(select.disabled).toBe(false);
  expect([...select.options].slice(2).map(option => [option.value, option.textContent?.trim()])).toEqual([["1", "Playlists / Shows / Play"], ["2", "Playlists / Archive / Play"]]);
  expect(select.options[2]!.hasAttribute("data-i18n-ignore")).toBe(true);
  act(() => { select.value = "2"; select.dispatchEvent(new Event("change", { bubbles: true })); });
  expect(choose).toHaveBeenCalledWith("2", "101");
});
it("hides stale track and library answers without reading an unbound selection", async () => {
  render("101"); await settle();
  render("102"); await settle();
  await act(async () => { requests[0]!.done([{ id: "old", name: "Old", path: "" }]); await Promise.resolve(); });
  expect(host.querySelector("select")!.disabled).toBe(true);
  await act(async () => { requests[1]!.done([{ id: "new", name: "New", path: "" }]); await Promise.resolve(); });
  render("102", "other library"); await settle();
  expect(host.textContent).not.toContain("New");
  render(null); await settle();
  await act(async () => { requests[2]!.done([{ id: "late", name: "Late", path: "" }]); await Promise.resolve(); });
  expect(host.querySelector("select")!.disabled).toBe(true);
  expect(requests).toHaveLength(3);
});
it("keeps failed lookup empty and reports its reason", async () => {
  render("101"); await settle();
  await act(async () => { requests[0]!.fail({ message: "unavailable", detail: "The playlist database is busy." }); await Promise.resolve(); });
  expect(host.querySelector("select")!.disabled).toBe(true);
  expect(log).toHaveBeenCalledWith(true, "unavailable The playlist database is busy.");
});

it.each([null, "102"])("waits for a fresh answer after A→%s→A", async middle => {
  render("101"); await settle();
  await act(async () => { requests[0]!.done([{ id: "old", name: "Old membership", path: "" }]); await Promise.resolve(); });
  render(middle); await settle();
  render("101"); await settle();
  expect(host.querySelector("select")!.disabled).toBe(true);
  expect(host.textContent).not.toContain("Old membership");
  await act(async () => { requests.at(-1)!.done([{ id: "new", name: "Current membership", path: "" }]); await Promise.resolve(); });
  expect(host.textContent).toContain("Current membership");
});
