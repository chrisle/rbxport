// @vitest-environment jsdom
import { beforeEach, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const report = { imported: 0, existing: 2, skipped: [], playlists: 0, cues: 0, tracks: [] };

beforeEach(() => {
  vi.resetModules();
  invoke.mockReset();
});

it("imports straight away when no list of the same name stands in the library", async () => {
  const { importReplacing } = await import("./client");
  invoke.mockResolvedValueOnce({ ...report, imported: 3, sameNamed: [] });
  const confirm = vi.fn(() => Promise.resolve(true));
  await expect(importReplacing("import_xml", { path: "/a.xml" }, confirm)).resolves.toMatchObject({ imported: 3 });
  expect(confirm).not.toHaveBeenCalled();
  expect(invoke).toHaveBeenCalledTimes(1);
  expect(invoke).toHaveBeenCalledWith("import_xml", { path: "/a.xml", replace: false });
});

it("asks before replacing same-named lists and imports nothing when declined", async () => {
  const { importReplacing } = await import("./client");
  invoke.mockResolvedValueOnce({ ...report, existing: 0, sameNamed: ["Sets", "Warm up"] });
  const confirm = vi.fn(() => Promise.resolve(false));
  await expect(importReplacing("import_itunes", { path: "/L.xml" }, confirm)).resolves.toBeNull();
  expect(confirm).toHaveBeenCalledWith(["Sets", "Warm up"]);
  // Only the first call, which writes nothing while lists stand in the way.
  expect(invoke).toHaveBeenCalledTimes(1);
  expect(invoke).toHaveBeenCalledWith("import_itunes", { path: "/L.xml", replace: false });
});

it("replaces the same-named lists on OK", async () => {
  const { importReplacing } = await import("./client");
  invoke
    .mockResolvedValueOnce({ ...report, existing: 0, sameNamed: ["Sets"] })
    .mockResolvedValueOnce({ ...report, sameNamed: [] });
  const confirm = vi.fn(() => Promise.resolve(true));
  await expect(
    importReplacing("import_itunes_selected", { path: "/L.xml", ids: ["itunes:1"] }, confirm),
  ).resolves.toMatchObject({ existing: 2 });
  expect(invoke).toHaveBeenLastCalledWith("import_itunes_selected", { path: "/L.xml", ids: ["itunes:1"], replace: true });
});
