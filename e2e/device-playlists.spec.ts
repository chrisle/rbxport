import { expect, test, type Locator, type Page } from "@playwright/test";

/**
 * A stick's own playlists, browsed and edited from the Devices tree, against
 * the mock's TEST stick: both of its libraries hold the real stick's three
 * playlists. Each edit changes the library it is made in and not the other,
 * as rekordbox's Devices tree does.
 */

async function openStick(page: Page) {
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.getByRole("tablist", { name: "Library sources" }).getByRole("tab", { name: "Devices" }).click();
  await page.getByRole("button", { name: "Expand TEST" }).dispatchEvent("mousedown");
  await expect(kind(page, "deviceLibrary")).toHaveCount(2);
}

function kind(page: Page, which: string): Locator {
  return page.locator(`[role="treeitem"][data-kind="${which}"]`);
}

/** The nth library's node by name: 0 is Device Library, 1 OneLibrary. */
function inLibrary(page: Page, library: 0 | 1, name: string): Locator {
  return page.locator('[role="treeitem"]').filter({ hasText: new RegExp(`^\\s*${name}`) }).nth(library);
}

const rows = (page: Page) => page.getByRole("row").filter({ has: page.getByRole("gridcell") });

test("a stick lists each of its libraries with its own playlists, and a playlist opens its tracks", async ({ page }) => {
  await openStick(page);
  await expect(kind(page, "deviceLibrary")).toHaveText(["Device Library", "OneLibrary"]);
  // The real stick's order: the newest playlist sits first.
  const names = await page.locator('[role="treeitem"][data-kind="devicePlaylist"]').allTextContents();
  expect(names.map((n) => n.trim().replace(/\(\d+\)$/, ""))).toEqual([
    "NP3-TEST-MP3", "Melodic Vox", "Now Playing Test", "NP3-TEST-MP3", "Melodic Vox", "Now Playing Test",
  ]);

  await inLibrary(page, 0, "Melodic Vox").click();
  await expect(page.getByTestId("browser-title")).toContainText("Melodic Vox");
  await expect(rows(page)).toHaveCount(3);
  await kind(page, "deviceAllTracks").first().click();
  await expect(rows(page)).toHaveCount(6);
});

test("a playlist made, filled and deleted on the stick changes only its own library", async ({ page }) => {
  await openStick(page);

  // Create New Playlist from the Device Library's Playlists heading.
  await kind(page, "devicePlaylists").first().click({ button: "right" });
  const menu = page.getByRole("menu", { name: "Playlists" });
  await expect(menu.getByRole("menuitem", { name: "Delete All" })).toBeDisabled();
  await menu.getByRole("menuitem", { name: "Create New Playlist" }).click();
  await expect(page.getByRole("contentinfo")).toContainText("Created Untitled Playlist.");
  // At the top of the heading, as rekordbox puts it, and in that library alone.
  const lists = page.locator('[role="treeitem"][data-kind="devicePlaylist"]');
  await expect(lists.first()).toHaveText(/^\s*Untitled Playlist/);
  await expect(lists.filter({ hasText: "Untitled Playlist" })).toHaveCount(1);

  // Add To Playlist from the library's All Tracks.
  await kind(page, "deviceAllTracks").first().click();
  await rows(page).first().click();
  await rows(page).first().click({ button: "right" });
  const trackMenu = page.getByRole("menu", { name: "Track" });
  await trackMenu.getByRole("menuitem", { name: "Add To Playlist" }).hover();
  await page.getByRole("menuitem", { name: "Untitled Playlist" }).click();
  await expect(page.getByRole("contentinfo")).toContainText("Added to Untitled Playlist.");
  await inLibrary(page, 0, "Untitled Playlist").click();
  await expect(rows(page)).toHaveCount(1);

  // Remove from Playlist inside it.
  await rows(page).first().click();
  await rows(page).first().click({ button: "right" });
  await page.getByRole("menu", { name: "Track" }).getByRole("menuitem", { name: "Remove from Playlist" }).click();
  await expect(rows(page)).toHaveCount(0);

  // And Delete Playlist from its own menu.
  await inLibrary(page, 0, "Untitled Playlist").click({ button: "right" });
  await page.getByRole("menu", { name: "Playlist" }).getByRole("menuitem", { name: "Delete Playlist" }).click();
  await expect(page.getByRole("contentinfo")).toContainText("Deleted Untitled Playlist.");
  await expect(lists.filter({ hasText: "Untitled Playlist" })).toHaveCount(0);
  await expect(lists).toHaveCount(6);
});

test("a stick's playlists cannot be changed while rekordbox holds the library", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("tablist", { name: "Library sources" }).getByRole("tab", { name: "Devices" }).click();
  await page.getByRole("button", { name: "Expand TEST" }).dispatchEvent("mousedown");
  await kind(page, "devicePlaylists").first().click({ button: "right" });
  const menu = page.getByRole("menu", { name: "Playlists" });
  await expect(menu.getByRole("menuitem", { name: "Create New Playlist" })).toBeDisabled();
});
