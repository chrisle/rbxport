import { expect, test, type Page } from "@playwright/test";

/**
 * The native Track and Playlist menus (#513): each item runs the command its
 * context-menu entry runs, and is greyed by rekordbox's rules — nothing until
 * a track list or the tree has been clicked, the Track menu for the focused
 * list only [OBS: the Windows menu bar; OBS static: rekordbox 7.2.19
 * `ListViewer::isActiveCommand`, `TreeViewer::isActiveCommand`]. The mock
 * keeps the shell's enabled states in `window.__menuEnabled` and lets a test
 * click an item with `window.__menu(id)`.
 */

type Page$ = {
  __menu?: (id: string) => void;
  __menuEnabled?: Record<string, boolean>;
  __confirmed?: string[];
};

const rows = (page: Page) => page.getByRole("row").filter({ has: page.getByRole("gridcell") });
const status = (page: Page) => page.getByRole("contentinfo");
const enabled = (page: Page, id: string) =>
  page.evaluate((item) => (window as unknown as Page$).__menuEnabled?.[item] === true, id);
const click = (page: Page, id: string) =>
  page.evaluate((item) => (window as unknown as Page$).__menu?.(item), id);

async function open(page: Page) {
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
}

test("the Track menu waits for the list and runs the context menu's command", async ({ page }) => {
  await open(page);
  // A fresh start: nothing has the focus, so everything is greyed.
  expect(await enabled(page, "remove-from-collection")).toBe(false);
  expect(await enabled(page, "create-playlist")).toBe(false);

  await rows(page).nth(1).click();
  await expect.poll(() => enabled(page, "remove-from-collection")).toBe(true);
  expect(await enabled(page, "analyze-track")).toBe(true);
  expect(await enabled(page, "show-in-finder")).toBe(true);
  expect(await enabled(page, "relocate")).toBe(false);

  await click(page, "remove-from-collection");
  await expect(status(page)).toContainText("Removed 1 track from the collection.");
  expect(await page.evaluate(() => (window as unknown as Page$).__confirmed ?? [])).toHaveLength(1);
});

test("the Playlist menu follows the tree's selection", async ({ page }) => {
  await open(page);
  await page.getByRole("treeitem").filter({ hasText: "Melodic Vox" }).first().click();
  await expect(page.getByTestId("browser-title")).toContainText("Melodic Vox");
  await expect.poll(() => enabled(page, "delete-playlist")).toBe(true);
  expect(await enabled(page, "renumber-track-order")).toBe(true);
  expect(await enabled(page, "export-playlist-file")).toBe(true);
  // The tree has the focus now, so the Track menu is greyed.
  expect(await enabled(page, "remove-from-collection")).toBe(false);

  await click(page, "create-playlist");
  await expect(status(page)).toContainText("Created New playlist.");
});
