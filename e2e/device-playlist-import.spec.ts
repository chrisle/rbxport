import { expect, test, type Locator, type Page } from "@playwright/test";

/**
 * Import Playlist over a stick's playlist (such as a TAG LIST a player saved
 * as one), against the mock's TEST stick: the stick's entries go into a new
 * playlist at the end of the collection's top level, in the stick's order,
 * as rekordbox's Devices tree imports one. A name the top level already has
 * is numbered " (1)", " (2)"…, with rekordbox's note. The mock's stick
 * tracks 1-4 and 6 were exported from the collection; track 5 was not, and
 * what happens to it is Preferences › Advanced's choice: copied in (the
 * default), asked about with rekordbox's question, left out, or the import
 * cancelled.
 */

async function openStick(page: Page, deviceImportMissing?: string) {
  if (deviceImportMissing) {
    await page.addInitScript((choice) => localStorage.setItem("rbl.preferences", JSON.stringify({ advanced: { protectLibrary: false, deviceImportMissing: choice } })), deviceImportMissing);
  }
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

test("a track the collection does not have is copied in by default, and the note says so", async ({ page }) => {
  await openStick(page);
  await importPlaylist(page, "NP3-TEST-MP3");
  await expect(page.getByRole("contentinfo")).toContainText(
    "Imported NP3-TEST-MP3. Copied 1 music file to PioneerDJ/Imported from Device.",
  );
  await source(page, "Playlists").click();
  await inCollection(page, "NP3-TEST-MP3").click();
  await expect(rows(page)).toHaveCount(2);
  // The stick's track 5, now a collection track of its own.
  await expect(rows(page).filter({ hasText: "Mirage" })).toHaveCount(1);
});

test("Import only already-matched tracks leaves the missing one out", async ({ page }) => {
  await openStick(page, "matched");
  await importPlaylist(page, "NP3-TEST-MP3");
  await expect(page.getByRole("contentinfo")).toContainText(
    "Imported NP3-TEST-MP3. 1 track is not in the collection and was left out.",
  );
  await source(page, "Playlists").click();
  await inCollection(page, "NP3-TEST-MP3").click();
  await expect(rows(page)).toHaveCount(1);
});

test("Cancel when any track is missing imports nothing", async ({ page }) => {
  await openStick(page, "cancel");
  await source(page, "Playlists").click();
  const before = await collectionNodes(page).count();
  await importPlaylist(page, "NP3-TEST-MP3");
  await expect(page.getByRole("contentinfo")).toContainText(
    "Cannot import the playlist: NP3-TEST-MP3 1 track is not in the collection.",
  );
  await source(page, "Playlists").click();
  await expect(collectionNodes(page)).toHaveCount(before);
});

test("Ask each import asks rekordbox's question: Cancel imports nothing, OK copies", async ({ page }) => {
  await openStick(page, "ask");
  await source(page, "Playlists").click();
  const before = await collectionNodes(page).count();
  await importPlaylist(page, "NP3-TEST-MP3");
  const question = page.getByRole("alertdialog", { name: "Import Playlist from Device" });
  await expect(question).toContainText("These music files will be copied to this computer.");
  await expect(question).toContainText("1 file 9.0 MB to the folder : /Users/dj/Music/PioneerDJ/Imported from Device");
  await question.getByRole("button", { name: "Cancel" }).click();
  await expect(question).toHaveCount(0);
  await source(page, "Playlists").click();
  await expect(collectionNodes(page)).toHaveCount(before);

  await importPlaylist(page, "NP3-TEST-MP3");
  await question.getByRole("button", { name: "OK" }).click();
  await expect(page.getByRole("contentinfo")).toContainText("Copied 1 music file to PioneerDJ/Imported from Device.");
  await source(page, "Playlists").click();
  await inCollection(page, "NP3-TEST-MP3").click();
  await expect(rows(page)).toHaveCount(2);
});

test("Ask each import does not ask when every track is in the collection", async ({ page }) => {
  await openStick(page, "ask");
  await importPlaylist(page, "Melodic Vox");
  await expect(page.getByRole("contentinfo")).toContainText("Imported Melodic Vox.");
  await expect(page.getByRole("alertdialog")).toHaveCount(0);
});
