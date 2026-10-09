import { expect, test, type Page } from "@playwright/test";

/**
 * Removing several selected tracks at once (#136). rekordbox removes the whole
 * selection with the Delete key (⌫ on a Mac keyboard) or the track menu: from
 * the Collection after asking, from a playlist, a history or the Tag List
 * [OBS static, rekordbox 7.2.19 `browse::ListViewer::deleteKeyPressed`;
 * rekordbox 7.2.18 manual p.20 and p.39]. The mock answers the confirmation
 * with OK, so the status line reports how many tracks the removal was given.
 */

async function open(page: Page) {
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
}

const collection = (page: Page) => page.locator('[role="treeitem"][data-kind="collection"]');
const rows = (page: Page) => page.getByRole("row").filter({ has: page.getByRole("gridcell") });
const status = (page: Page) => page.getByRole("contentinfo");

async function selectThree(page: Page) {
  await rows(page).nth(2).click();
  await rows(page).nth(4).click({ modifiers: ["Shift"] });
  await expect(rows(page).and(page.locator("[data-selected]"))).toHaveCount(3);
}

test("Delete removes every selected track from the collection", async ({ page }) => {
  await open(page);
  await collection(page).click();
  await selectThree(page);
  await page.keyboard.press("Delete");
  await expect(status(page)).toContainText("Removed 3 tracks from the collection.");
});

test("Backspace removes every selected track from the playlist", async ({ page }) => {
  await open(page);
  await page.getByRole("treeitem").filter({ hasText: "Melodic Vox" }).first().click();
  const before = await rows(page).count();
  await rows(page).nth(0).click();
  await rows(page).nth(1).click({ modifiers: ["Shift"] });
  await page.keyboard.press("Backspace");
  await expect(status(page)).toContainText("Removed 2 tracks.");
  await expect(rows(page)).toHaveCount(before - 2);
});

test("Delete in the tree does not remove the tracks selected in the list", async ({ page }) => {
  await open(page);
  await collection(page).click();
  await selectThree(page);
  // The tree takes the focus; the list keeps its selection.
  await collection(page).click();
  await expect(collection(page)).toBeFocused();
  await expect(rows(page).and(page.locator("[data-selected]"))).toHaveCount(3);
  await page.keyboard.press("Delete");
  // Give a removal the time it would take to report, then check none did.
  await page.waitForTimeout(500);
  await expect(status(page)).not.toContainText("Removed");
});

test("a macOS Control-click menu removes the whole selection (#136, #135)", async ({ page }) => {
  await open(page);
  test.skip(!(await page.evaluate(() => /Mac/.test(navigator.platform))), "macOS only");
  await collection(page).click();
  await selectThree(page);
  // A left press with Control, then contextmenu: what macOS sends.
  await rows(page).nth(3).click({ modifiers: ["Control"] });
  await page.getByRole("menuitem", { name: "Remove from Collection" }).click();
  await expect(status(page)).toContainText("Removed 3 tracks from the collection.");
});
