import { expect, test, type Locator, type Page } from "@playwright/test";

/**
 * Import Playlist over a stick's playlist (such as a TAG LIST a player saved
 * as one), against the mock's TEST stick: the entries whose track the
 * collection has go into a new playlist at the end of the collection's top
 * level, in the stick's order, as rekordbox's Devices tree imports one. A
 * name the top level already has is numbered " (1)", " (2)"…, with
 * rekordbox's note. The mock's stick tracks 1-4 and 6 were exported from
 * the collection; track 5 was not.
 */

async function openStick(page: Page) {
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.getByRole("tablist", { name: "Library sources" }).getByRole("tab", { name: "Devices" }).click();
  await page.getByRole("button", { name: "Expand TEST" }).dispatchEvent("mousedown");
  await expect(page.locator('[role="treeitem"][data-kind="deviceLibrary"]')).toHaveCount(2);
}

/** The OneLibrary's playlist by name. */
function onStick(page: Page, name: string): Locator {
  return page.locator('[role="treeitem"][data-kind="devicePlaylist"]').filter({ hasText: new RegExp(`^\\s*${name}`) }).nth(1);
}

/** A collection playlist by its whole name. */
function inCollection(page: Page, name: string): Locator {
  return page.locator('[role="treeitem"][data-kind="playlist"]').filter({ hasText: new RegExp(`^\\s*${name.replace(/[()]/g, "\\$&")}\\s*(\\(\\d+\\))?\\s*$`) });
}

const source = (page: Page, name: "Playlists" | "Devices") =>
  page.getByRole("tablist", { name: "Library sources" }).getByRole("tab", { name });

async function importPlaylist(page: Page, name: string) {
  await source(page, "Devices").click();
  await onStick(page, name).click({ button: "right" });
  await page.getByRole("menu", { name: "Playlist" }).getByRole("menuitem", { name: "Import Playlist" }).click();
}

const rows = (page: Page) => page.getByRole("row").filter({ has: page.getByRole("gridcell") });

const collectionNodes = (page: Page) =>
  page.locator('[role="treeitem"][data-kind="playlist"], [role="treeitem"][data-kind="folder"], [role="treeitem"][data-kind="smartPlaylist"]');

test("Import Playlist puts a stick's playlist at the end of the collection's top level", async ({ page }) => {
  await openStick(page);
  await source(page, "Playlists").click();
  const before = await collectionNodes(page).count();

  await importPlaylist(page, "Melodic Vox");
  await expect(page.getByRole("contentinfo")).toContainText("Imported Melodic Vox.");
  await source(page, "Playlists").click();
  // The mock's CURRENT folder already holds a "Melodic Vox": a name inside
  // a folder does not clash with the top level, so the import keeps its
  // name, as rekordbox compares only the destination's own children.
  await expect(inCollection(page, "Melodic Vox")).toHaveCount(2);
  const made = inCollection(page, "Melodic Vox").last();
  await expect(collectionNodes(page)).toHaveCount(before + 1);
  // Last of the collection's own nodes: nothing of the collection follows it.
  await expect(made.locator("xpath=following::*[@role='treeitem'][@data-kind='playlist' or @data-kind='folder' or @data-kind='smartPlaylist']")).toHaveCount(0);
  await made.click();
  await expect(page.getByTestId("browser-title")).toContainText("Melodic Vox");
  await expect(rows(page)).toHaveCount(3);

  // The same playlist again: the name is taken, so it is numbered, and
  // rekordbox's own note says why.
  await importPlaylist(page, "Melodic Vox");
  await expect(page.getByRole("contentinfo")).toContainText(
    'Playlist name was changed because there was a playlist with the same name in your collection. "Melodic Vox (1)"',
  );
  await source(page, "Playlists").click();
  await expect(inCollection(page, "Melodic Vox (1)")).toHaveCount(1);
});

test("a track the collection does not have is left out, and the note says so", async ({ page }) => {
  await openStick(page);
  await importPlaylist(page, "NP3-TEST-MP3");
  await expect(page.getByRole("contentinfo")).toContainText(
    "Imported NP3-TEST-MP3. 1 track is not in the collection and was left out.",
  );
  await source(page, "Playlists").click();
  await inCollection(page, "NP3-TEST-MP3").click();
  await expect(rows(page)).toHaveCount(1);
});
