import { expect, test, type Locator, type Page } from "@playwright/test";

/**
 * Sort Items (#443): a folder's, or the Playlists heading's, children put in
 * name order from the tree's menu, as rekordbox 7.2.19 does
 * (`TreeViewer::treeMessageSortItem` @0x10156621c); the row is left out of
 * an empty folder's menu (`showTreeViewPopupMenu` @0x10011b3cc).
 */

async function open(page: Page) {
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
}

function item(page: Page, name: string): Locator {
  return page.getByRole("treeitem").filter({ hasText: name }).first();
}

/** The names of the rows between two top-level folders, in tree order. */
async function between(page: Page, first: string, next: string): Promise<string[]> {
  const labels = await page.getByRole("treeitem").evaluateAll((rows) =>
    rows.map((row) => row.querySelector("span[class*='label']")?.textContent ?? ""));
  return labels.slice(labels.indexOf(first) + 1, labels.indexOf(next));
}

async function menuOf(page: Page, node: Locator, name: string): Promise<Locator> {
  await node.click({ button: "right" });
  const menu = page.getByRole("menu", { name });
  await expect(menu).toBeVisible();
  return menu;
}

test("Sort Items puts a folder's playlists in name order", async ({ page }) => {
  await open(page);
  expect(await between(page, "CURRENT", "USB")).toEqual(["Melodic Vox", "Hardstyle", "Drum and Bass", "Fresh 128s"]);

  const menu = await menuOf(page, item(page, "CURRENT"), "Folder");
  await menu.getByRole("menuitem", { name: "Sort Items", exact: true }).click();
  await expect(page.getByRole("contentinfo")).toContainText("Sorted CURRENT.");
  await expect.poll(() => between(page, "CURRENT", "USB")).toEqual(["Drum and Bass", "Fresh 128s", "Hardstyle", "Melodic Vox"]);
});

test("the Playlists heading offers Sort Items, and an empty folder does not", async ({ page }) => {
  await open(page);
  const heading = page.locator('[role="treeitem"][data-kind="collection"]');
  let menu = await menuOf(page, heading, "Playlists");
  await expect(menu.getByRole("menuitem", { name: "Sort Items", exact: true })).toBeEnabled();
  await page.keyboard.press("Escape");
  await expect(menu).toBeHidden();

  menu = await menuOf(page, item(page, "CURRENT"), "Folder");
  await menu.getByRole("menuitem", { name: "Create New Folder", exact: true }).click();
  await expect(page.getByRole("contentinfo")).toContainText("Created New folder.");
  menu = await menuOf(page, item(page, "New folder"), "Folder");
  await expect(menu.getByRole("menuitem", { name: "Delete Folder", exact: true })).toBeVisible();
  await expect(menu.getByRole("menuitem", { name: "Sort Items", exact: true })).toHaveCount(0);
});
