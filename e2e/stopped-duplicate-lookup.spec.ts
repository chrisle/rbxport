import { expect, test, type Page } from "@playwright/test";

type Observation = { lookups: number; settled: number; writes: string[]; destination: string[]; next: string[]; destinationId: string; nextId: string };
type Fixture = { release: () => void; read: () => Promise<Observation> };
const read = (page: Page) => page.evaluate(() => (window as unknown as { stoppedLookup: Fixture }).stoppedLookup.read());

async function setup(page: Page) {
  await page.addInitScript(() => localStorage.setItem("rbl.preferences", JSON.stringify({
    advanced: { protectLibrary: false, duplicateTracks: "ask" },
  })));
  await page.route("**/src/main.tsx*", async route => {
    const response = await route.fetch();
    await route.fulfill({ response, body: `
      import { getBackend as lookupBackend } from "/src/ipc/client.ts";
      const backend = await lookupBackend();
      for (const name of ["Lookup source", "Lookup destination", "Next destination"]) await backend.edits.createPlaylist(name, "root");
      const nodes = await backend.playlistTree();
      const source = nodes.find(node => node.name === "Lookup source");
      const destination = nodes.find(node => node.name === "Lookup destination");
      const next = nodes.find(node => node.name === "Next destination");
      await backend.edits.addTracksToPlaylist(source.id, ["100000"], false);
      await backend.edits.addTracksToPlaylist(destination.id, ["100000"], false);
      let release;
      const held = new Promise(resolve => { release = resolve; });
      const calls = { lookups: 0, settled: 0, writes: [] };
      const duplicates = backend.edits.playlistDuplicates.bind(backend.edits);
      backend.edits.playlistDuplicates = async (...args) => {
        calls.lookups++;
        if (args[0] === destination.id) await held;
        const found = await duplicates(...args);
        calls.settled++;
        return found;
      };
      const add = backend.edits.addTracksToPlaylist.bind(backend.edits);
      backend.edits.addTracksToPlaylist = (...args) => { calls.writes.push(args[0]); return add(...args); };
      const members = async id => {
        const view = await backend.openView({ source: { kind: "playlist", id }, sort: "trackNo", descending: false, query: "" });
        return backend.viewIdsInRange(view.viewId, 0, view.len - 1);
      };
      window.stoppedLookup = {
        release,
        read: async () => ({ ...calls, destination: await members(destination.id), next: await members(next.id), destinationId: destination.id, nextId: next.id }),
      };
    ` + await response.text() });
  });
  await page.goto("/?writable=1");
  await page.getByRole("treeitem").filter({ hasText: /^\s*Lookup source/ }).click();
  const row = page.locator('[role="row"][draggable="true"]').first();
  await expect(row).toBeVisible();
  await row.locator('[data-col="title"]').dragTo(page.getByRole("treeitem").filter({ hasText: /^\s*Lookup destination/ }));
  await expect.poll(async () => (await read(page)).lookups).toBe(1);
  return row;
}

test("Stop during duplicate lookup retires the late question and drains the next playlist add", async ({ page }) => {
  const row = await setup(page);
  await page.getByRole("button", { name: /^Stop: Adding 1 to Lookup destination/ }).click();
  await expect(page.getByRole("button", { name: /^Stop: Adding 1 to Lookup destination/ })).toHaveText("Stopping…");
  await row.locator('[data-col="title"]').dragTo(page.getByRole("treeitem").filter({ hasText: /^\s*Next destination/ }));
  await expect(page.getByRole("contentinfo")).toContainText("Queued");
  await page.evaluate(() => (window as unknown as { stoppedLookup: Fixture }).stoppedLookup.release());
  await expect.poll(async () => (await read(page)).settled).toBeGreaterThan(0);
  await expect(page.getByRole("alertdialog", { name: "Duplicate tracks" })).toHaveCount(0);
  await expect(page.getByRole("contentinfo")).toContainText("Added 1 track to Next destination.");
  const found = await read(page);
  expect(found.destination).toEqual(["100000"]);
  expect(found.next).toEqual(["100000"]);
  expect(found.writes).toEqual([found.nextId]);
  await expect(page.getByRole("button", { name: /^Stop: Adding/ })).toHaveCount(0);
});

for (const choice of ["Add duplicates", "Cancel"] as const) {
  test(`a live duplicate lookup still offers ${choice}`, async ({ page }) => {
    await setup(page);
    await page.evaluate(() => (window as unknown as { stoppedLookup: Fixture }).stoppedLookup.release());
    const dialog = page.getByRole("alertdialog", { name: "Duplicate tracks" });
    await expect(dialog).toContainText("1 of the selected tracks are already in Lookup destination.");
    await dialog.getByRole("button", { name: choice, exact: true }).click();
    await expect(page.getByRole("contentinfo")).toContainText(choice === "Cancel" ? "Nothing added to Lookup destination." : "Added 1 track to Lookup destination.");
    const found = await read(page);
    expect(found.destination).toEqual(choice === "Cancel" ? ["100000"] : ["100000", "100000"]);
    expect(found.writes).toEqual(choice === "Cancel" ? [] : [found.destinationId]);
  });
}
