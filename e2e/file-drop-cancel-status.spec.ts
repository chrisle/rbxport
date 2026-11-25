import { expect, test } from "@playwright/test";

// Fixture-provided file paths exercise App import jobs, not a native OS drag bridge.
for (const choice of ["Cancel", "Skip duplicates", "Add duplicates", "Stop"] as const) {
  test(`an OS file drop reports ${choice} without losing the completed collection import`, async ({ page }) => {
    await page.addInitScript(() => localStorage.setItem("rbl.preferences", JSON.stringify({
      advanced: { protectLibrary: false, duplicateTracks: "ask" }, analysis: { auto: true },
    })));
    await page.route("**/src/main.tsx*", async route => {
      const response = await route.fetch();
      const setup = `
        import { getBackend as dropBackend } from "/src/ipc/client.ts";
        const backend = await dropBackend();
        const held = await backend.importPathsSlice(["/tmp/held.mp3"]);
        await backend.edits.createPlaylist("OS files", "root");
        const playlist = (await backend.playlistTree()).find(node => node.name === "OS files");
        await backend.edits.addTracksToPlaylist(playlist.id, [held.tracks[0].id], false);
        const calls = { writes: [], reloads: 0, analysed: [] };
        const write = backend.edits.addTracksToPlaylist.bind(backend.edits);
        backend.edits.addTracksToPlaylist = (...args) => { calls.writes.push(args); return write(...args); };
        const reload = backend.reloadLibrary.bind(backend);
        backend.reloadLibrary = (...args) => { calls.reloads++; return reload(...args); };
        backend.analyseTrack = async id => {
          calls.analysed.push(id);
          return { trackId: id, bpmX100: 12000, key: "Am", beats: 120, peak: 0.5, durationSec: 60, elapsedMs: 1, analysisPath: "fixture" };
        };
        backend.unanalysedTracks = () => Promise.resolve({ tracks: [], next: null });
        const slice = backend.importPathsSlice.bind(backend);
        backend.importPathsSlice = async paths => {
          const result = await slice(paths.filter(path => !path.endsWith("broken.mp3")));
          return { ...result, skipped: ["broken.mp3: fixture skip"] };
        };
        window.fileDropResult = async () => {
          const view = await backend.openView({ source: { kind: "playlist", id: playlist.id }, sort: "trackNo", descending: false, query: "" });
          const collection = await backend.openView({ source: { kind: "collection" }, sort: "title", descending: false, query: "fresh" });
          return { ...calls, members: await backend.viewIdsInRange(view.viewId, 0, view.len - 1), fresh: await backend.fetchRows(collection.viewId, 0, 10, []) };
        };
      `;
      await route.fulfill({ response, body: setup + await response.text() });
    });
    await page.goto("/?writable=1&dropFixture=1");
    const playlist = page.getByRole("treeitem", { name: /OS files/ }).first();
    await expect(playlist).toBeVisible();
    const read = () => page.evaluate(async () => (window as unknown as { fileDropResult: () => Promise<{
      writes: unknown[][]; reloads: number; analysed: string[]; members: string[]; fresh: { id: string }[];
    }> }).fileDropResult());
    await playlist.evaluate(element => {
      const transfer = new DataTransfer();
      for (const name of ["held", "fresh", "broken"]) {
        transfer.items.add(new File([], `${name}.mp3`, { type: "audio/mpeg" }));
      }
      for (const file of transfer.files) Object.defineProperty(file, "path", { value: `/tmp/${file.name}` });
      element.dispatchEvent(new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer: transfer }));
    });
    const dialog = page.getByRole("alertdialog", { name: "Duplicate tracks" });
    await expect(dialog).toContainText("1 of the 2 selected tracks are already in OS files.");
    expect(await read()).toMatchObject({ reloads: 1, writes: [], analysed: [] });
    expect((await read()).fresh).toHaveLength(1);
    if (choice === "Stop") {
      await page.getByRole("button", { name: /^Stop: Adding/ }).dispatchEvent("click");
      await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
    } else await dialog.getByRole("button", { name: choice, exact: true }).click();
    const status = page.getByRole("contentinfo");
    const prefix = choice === "Cancel" ? "Nothing added to OS files"
      : choice === "Stop" ? "Stopped: 0 tracks added to OS files"
        : choice === "Skip duplicates" ? "Added 1 track to OS files" : "Added 2 tracks to OS files";
    await expect(status).toContainText(prefix);
    await expect(status).toContainText("1 skipped (broken.mp3: fixture skip)");
    await expect(status).toContainText("1 already in the library");
    await expect.poll(async () => (await read()).analysed.length).toBe(1);
    await expect.poll(async () => (await read()).reloads).toBe(2);
    const result = await read();
    expect(result.reloads).toBe(2);
    expect(result.fresh).toHaveLength(1);
    expect(result.analysed).toEqual([result.fresh[0]!.id]);
    expect(result.writes).toHaveLength(choice === "Cancel" || choice === "Stop" ? 0 : 1);
    expect(result.members).toHaveLength(choice === "Cancel" || choice === "Stop" ? 1 : choice === "Skip duplicates" ? 2 : 3);
  });
}
