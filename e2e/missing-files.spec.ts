import { expect, test, type Page } from "@playwright/test";

/**
 * Missing files, as rekordbox 7.2.14 shows them [OBS Winrig chris-win11
 * 2026-10-08, issue #201]: an orange [!] in the Attribute column, a short
 * "File is Missing" menu, and File › Display All Missing Files opening the
 * Missing File Manager. `?missing=7` takes the files of rows 2, 9, 16, …
 */

async function open(page: Page, query = "?missing=7&writable=1") {
  await page.goto(`/${query}`);
  await page.getByRole("treeitem", { name: /All Tracks/ }).click();
  await expect(page.getByTestId("browser-title")).toContainText("All Tracks");
}

/** Shows the Attribute column, where the [!] is drawn. */
async function showAttribute(page: Page) {
  await page.getByRole("columnheader", { name: "Track Title" }).click({ button: "right" });
  await page.getByRole("menu", { name: "Columns" }).getByRole("menuitemcheckbox", { name: "Attribute" }).click();
  await page.keyboard.press("Escape");
}

/** The mock's track at `index`, in the Collection's default order. */
const rowOf = (page: Page, index: number) => page.getByRole("row").filter({ has: page.locator('[data-col="title"]') }).nth(index);

test("a missing track's menu is rekordbox's short one under its heading", async ({ page }) => {
  await open(page);
  // The second row is missing; the first is not.
  await rowOf(page, 1).locator('[data-col="title"]').click({ button: "right" });
  const menu = page.getByRole("menu", { name: "Track" });
  await expect(menu).toBeVisible();
  await expect(menu).toContainText("File is Missing");
  await expect(menu.getByRole("menuitem")).toHaveText(["Auto Relocate", "Relocate", "Remove from Collection"]);
  await page.keyboard.press("Escape");

  await rowOf(page, 0).locator('[data-col="title"]').click({ button: "right" });
  await expect(menu).not.toContainText("File is Missing");
  await expect(menu.getByRole("menuitem", { name: "Analyze Track" })).toBeVisible();
});

test("a missing track's menu cannot write while rekordbox holds the library", async ({ page }) => {
  await open(page, "?missing=7");
  await rowOf(page, 1).locator('[data-col="title"]').click({ button: "right" });
  const menu = page.getByRole("menu", { name: "Track" });
  for (const name of ["Auto Relocate", "Relocate", "Remove from Collection"]) {
    await expect(menu.getByRole("menuitem", { name, exact: true })).toBeDisabled();
  }
});

test("Relocate from the menu clears the track's [!]", async ({ page }) => {
  await open(page);
  await showAttribute(page);
  const row = rowOf(page, 1);
  await expect(rowOf(page, 0).getByRole("img", { name: "File is Missing" })).toHaveCount(0);
  await expect(row.getByRole("img", { name: "File is Missing" })).toHaveCount(1);
  await row.locator('[data-col="title"]').click({ button: "right" });
  await page.getByRole("menu", { name: "Track" }).getByRole("menuitem", { name: "Relocate", exact: true }).click();
  await expect(row.getByRole("img", { name: "File is Missing" })).toHaveCount(0);
});

test("the Missing File Manager lists every missing track and relocates them", async ({ page }) => {
  // Preferences, then the manager, then three rescans: longer than one view.
  test.setTimeout(60_000);
  await open(page);
  // A search folder first, from Preferences, where rekordbox keeps them.
  await page.getByRole("banner").getByRole("button", { name: "Settings" }).click();
  const preferences = page.getByRole("dialog", { name: "Preferences" });
  await preferences.getByRole("tab", { name: "Advanced" }).click();
  const folders = preferences.getByRole("region", { name: "Auto Relocate Search Folders" });
  await folders.getByRole("button", { name: "Add" }).click();
  await expect(folders.getByRole("combobox", { name: "Search folders" })).toHaveValue("/Users/mock/Music/Moved");
  await page.keyboard.press("Escape");
  await expect(preferences).toHaveCount(0);

  await page.waitForFunction(() => "__menu" in window);
  await page.evaluate(() => (window as unknown as { __menu: (id: string) => void }).__menu("missing"));

  const manager = page.getByRole("dialog", { name: "Missing File Manager" });
  await expect(manager).toBeVisible();
  // 2000 mock tracks, every seventh from the second: 286.
  await expect(manager).toContainText("286 Track");
  const grid = manager.getByRole("grid", { name: "Missing files" });
  await expect(grid.getByRole("columnheader")).toHaveText(["Track Title", "artist", "album", "location"]);
  // Every row is selected when it opens, as rekordbox's are.
  const first = grid.getByRole("row", { selected: true }).first();
  await expect(first).toBeVisible();
  await expect(grid.getByRole("row", { selected: false })).toHaveCount(1); // the header

  // The mock's search folder holds every other missing file.
  await manager.getByRole("button", { name: "Auto Relocate" }).click();
  await expect(manager).toContainText("143 Track");
  await expect(manager).toContainText("143 relocated, 143 not found in the search folders.");

  // One row, then Delete: only that one goes.
  await grid.getByRole("row").nth(1).click();
  await manager.getByRole("button", { name: "Delete" }).click();
  await expect(manager).toContainText("142 Track");

  await manager.getByRole("button", { name: "OK" }).click();
  await expect(manager).toHaveCount(0);
});
