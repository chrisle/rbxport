import { expect, test, type Page } from "@playwright/test";

const heading = (page: Page) => page.locator('[data-browser-pane="tree"][data-browser-side="main"] [data-kind="collection"]');
const menu = (page: Page, id: string) => page.evaluate(item => (window as unknown as { __menu: (id: string) => void }).__menu(item), id);
type Snapshot = { ids: string[]; tracks: number; writes: number; message: string | null };
const snapshot = (page: Page) => page.evaluate(() => (window as unknown as { headingRead: () => Promise<Snapshot> }).headingRead());
async function open(page: Page) {
  await page.route("**/src/main.tsx*", async route => {
    const response = await route.fetch();
    const setup = `
      import { getBackend as headingBackend } from "/src/ipc/client.ts";
      const backend = await headingBackend();
      let resolveConfirmation;
      let message = null;
      let writes = 0;
      backend.confirm = text => { message = text; return new Promise(resolve => { resolveConfirmation = resolve; }); };
      const remove = backend.edits.deletePlaylistChildren.bind(backend.edits);
      backend.edits.deletePlaylistChildren = (...args) => { writes++; return remove(...args); };
      const prepare = backend.edits.playlistDeletionContext.bind(backend.edits);
      window.headingHoldPreparation = false;
      window.headingPreparationPending = false;
      backend.edits.playlistDeletionContext = () => {
        if (!window.headingHoldPreparation) return prepare();
        window.headingPreparationPending = true;
        return new Promise(resolve => { window.headingReleasePreparation = async () => resolve(await prepare()); });
      };
      const ready = backend.onLibraryReady.bind(backend);
      backend.onLibraryReady = listener => { window.headingReady = listener; return ready(listener); };
      window.headingAnswer = answer => resolveConfirmation(answer);
      window.headingSwap = async () => {
        const next = (await backend.databaseDrives()).find(drive => !drive.current);
        await backend.switchLibrary(next.masterDb);
        window.headingReady();
      };
      window.headingAdd = async nested => {
        const tree = await backend.playlistTree();
        const parent = nested ? tree.find(node => node.kind === "folder").id : "root";
        return backend.edits.createPlaylist("Arrived during confirmation", parent);
      };
      window.headingRead = async () => {
        const tree = await backend.playlistTree();
        const view = await backend.openView({ source: { kind: "collection" }, sort: "title", descending: false, query: "" });
        return { ids: tree.filter(node => ["playlist", "smartPlaylist", "folder"].includes(node.kind)).map(node => node.id), tracks: view.len, writes, message };
      };
    `;
    await route.fulfill({ response, body: setup + await response.text() });
  });
  await page.goto("/?writable=1&drivelibrary=1");
  await expect(heading(page)).toBeVisible();
  await heading(page).click();
}
const answer = (page: Page, value: boolean) => page.evaluate(value => (window as unknown as { headingAnswer: (value: boolean) => void }).headingAnswer(value), value);

for (const choice of ["cancel", "delete", "new sibling", "new descendant", "library swap"] as const) {
  test(`Playlists heading confirmation: ${choice}`, async ({ page }) => {
    await open(page);
    const before = await snapshot(page);
    expect(before.ids.length).toBeGreaterThan(1);
    await menu(page, "delete-playlist");
    await expect.poll(async () => (await snapshot(page)).message).toContain("Tracks and their files will remain in the Collection");
    expect((await snapshot(page)).writes).toBe(0);
    if (choice === "new sibling" || choice === "new descendant") {
      await page.evaluate(nested => (window as unknown as { headingAdd: (nested: boolean) => Promise<unknown> }).headingAdd(nested), choice === "new descendant");
    }
    if (choice === "library swap") await page.evaluate(() => (window as unknown as { headingSwap: () => Promise<void> }).headingSwap());
    const confirmed = await snapshot(page);
    await answer(page, choice !== "cancel");
    if (choice === "cancel") {
      expect(await snapshot(page)).toMatchObject({ ids: before.ids, tracks: before.tracks, writes: 0 });
      return;
    }
    if (choice === "new descendant" || choice === "library swap") {
      await expect(page.getByRole("contentinfo")).toContainText(choice === "library swap" ? "The library changed. Try this edit again." : "confirmed playlists have changed");
      expect(await snapshot(page)).toMatchObject({ ids: confirmed.ids, tracks: before.tracks, writes: choice === "library swap" ? 0 : 1 });
      return;
    }
    await expect(page.getByRole("contentinfo")).toContainText("Deleted local playlists and folders.");
    const after = await snapshot(page);
    expect(after.tracks).toBe(before.tracks);
    expect(after.ids).toEqual(choice === "new sibling" ? confirmed.ids.filter(id => !before.ids.includes(id)) : []);
    expect(after.writes).toBe(1);
    await menu(page, "undo");
    await expect(page.getByRole("contentinfo")).toContainText("Edit undone.");
    await expect.poll(async () => (await snapshot(page)).ids).toEqual(confirmed.ids);
    await menu(page, "redo");
    await expect(page.getByRole("contentinfo")).toContainText("Edit redone.");
    await expect.poll(async () => (await snapshot(page)).ids).toEqual(after.ids);
    expect((await snapshot(page)).tracks).toBe(before.tracks);
  });
}


test("a same-ID library ready event during preparation cannot show an old deletion question", async ({ page }) => {
  await open(page);
  const before = await snapshot(page);
  await page.evaluate(() => { (window as unknown as { headingHoldPreparation: boolean }).headingHoldPreparation = true; });
  await menu(page, "delete-playlist");
  await expect.poll(() => page.evaluate(() => (window as unknown as { headingPreparationPending: boolean }).headingPreparationPending)).toBe(true);
  await page.evaluate(async () => {
    const fixture = window as unknown as { headingSwap: () => Promise<void>; headingReleasePreparation: () => Promise<void> };
    await fixture.headingSwap();
    await fixture.headingReleasePreparation();
  });
  await expect(page.getByRole("contentinfo")).toContainText("The library changed. Try this edit again.");
  expect(await snapshot(page)).toMatchObject({ ids: before.ids, tracks: before.tracks, writes: 0, message: null });
});
