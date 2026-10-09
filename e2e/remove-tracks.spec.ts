import { expect, test, type Page } from "@playwright/test";

/**
 * Removing several selected tracks at once (#136). rekordbox removes the whole
 * selection with the Delete key (⌫ on a Mac keyboard) or the track menu, and
 * asks OK/Cancel first: from the Collection, a playlist, a history or the Tag
 * List [OBS static, rekordbox 7.2.19 `browse::ListViewer::deleteKeyPressed`
 * and `showPopupMenu`; rekordbox 7.2.18 manual p.20 and p.39]. The mock
 * answers every confirmation with OK unless a test sets
 * `window.__confirmAnswer = false`, and keeps what it was asked in
 * `window.__confirmed`.
 */

type Page$ = { __confirmAnswer?: boolean; __confirmed?: string[] };

const PLAYLIST_PROMPT =
  "Are you sure you want to remove the selected track(s) from the playlist?\nTrack(s) will be removed from the playlists of all synced devices.";
const HISTORY_PROMPT = "Are you sure you want to remove the selected tracks?";
const TAG_LIST_PROMPT =
  "Are you sure you want to remove the selected track(s) from the Tag List?\nTrack(s) will be removed from the Tag Lists of all synced devices.";

async function open(page: Page, url = "/?writable=1") {
  await page.goto(url);
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
}

const collection = (page: Page) => page.locator('[role="treeitem"][data-kind="collection"]');
const rows = (page: Page) => page.getByRole("row").filter({ has: page.getByRole("gridcell") });
const selected = (page: Page) => rows(page).and(page.locator("[data-selected]"));
const status = (page: Page) => page.getByRole("contentinfo");
const asked = (page: Page) => page.evaluate(() => (window as unknown as Page$).__confirmed ?? []);
const answerCancel = (page: Page) => page.evaluate(() => {
  (window as unknown as Page$).__confirmAnswer = false;
});

async function selectThree(page: Page) {
  await rows(page).nth(2).click();
  await rows(page).nth(4).click({ modifiers: ["Shift"] });
  await expect(selected(page)).toHaveCount(3);
}

async function openPlaylist(page: Page) {
  await page.getByRole("treeitem").filter({ hasText: "Melodic Vox" }).first().click();
  await expect(page.getByTestId("browser-title")).toContainText("Melodic Vox");
  return rows(page).count();
}

async function selectFirstTwo(page: Page) {
  await rows(page).nth(0).click();
  await rows(page).nth(1).click({ modifiers: ["Shift"] });
  await expect(selected(page)).toHaveCount(2);
}

/** Nothing happened: no question, no removal reported. */
async function expectNothingRemoved(page: Page) {
  // Give a removal the time it would take to report, then check none did.
  await page.waitForTimeout(500);
  expect(await asked(page)).toEqual([]);
  await expect(status(page).filter({ hasText: "Removed" })).toHaveCount(0);
}

test("Delete removes every selected track from the collection, after asking", async ({ page }) => {
  await open(page);
  await collection(page).click();
  await selectThree(page);
  await page.keyboard.press("Delete");
  await expect(status(page)).toContainText("Removed 3 tracks from the collection.");
  expect(await asked(page)).toHaveLength(1);
});

test("Cancel on the collection question removes nothing", async ({ page }) => {
  await open(page);
  await collection(page).click();
  await selectThree(page);
  await answerCancel(page);
  await page.keyboard.press("Delete");
  await expect.poll(() => asked(page)).toHaveLength(1);
  await page.waitForTimeout(500);
  await expect(status(page).filter({ hasText: "Removed" })).toHaveCount(0);
  // The selection stays, as rekordbox leaves it after Cancel.
  await expect(selected(page)).toHaveCount(3);
});

test("Backspace removes every selected track from the playlist, after rekordbox's question", async ({ page }) => {
  await open(page);
  const before = await openPlaylist(page);
  await selectFirstTwo(page);
  await page.keyboard.press("Backspace");
  await expect(status(page)).toContainText("Removed 2 tracks.");
  await expect(rows(page)).toHaveCount(before - 2);
  expect(await asked(page)).toEqual([PLAYLIST_PROMPT]);
});

test("Cancel on the playlist question removes nothing", async ({ page }) => {
  await open(page);
  const before = await openPlaylist(page);
  await selectFirstTwo(page);
  await answerCancel(page);
  await page.keyboard.press("Delete");
  await expect.poll(() => asked(page)).toEqual([PLAYLIST_PROMPT]);
  await page.waitForTimeout(500);
  await expect(rows(page)).toHaveCount(before);
  await expect(status(page).filter({ hasText: "Removed" })).toHaveCount(0);
});

/** Key presses sent in one go, before any question could have been answered. */
const pressAtOnce = (page: Page, presses: readonly { repeat: boolean }[]) =>
  page.evaluate((list) => {
    for (const { repeat } of list) {
      document.body.dispatchEvent(new KeyboardEvent("keydown", { key: "Delete", code: "Delete", repeat, bubbles: true, cancelable: true }));
    }
  }, presses);

test("holding Delete removes once and asks once", async ({ page }) => {
  await open(page);
  const before = await openPlaylist(page);
  await selectFirstTwo(page);
  // A repeat alone is a key held down from before; it removes nothing.
  await pressAtOnce(page, [{ repeat: true }]);
  await page.waitForTimeout(300);
  expect(await asked(page)).toEqual([]);
  // A held key: one press, then the keyboard's repeats.
  await pressAtOnce(page, [{ repeat: false }, { repeat: true }, { repeat: true }]);
  await expect(status(page)).toContainText("Removed 2 tracks.");
  await expect(rows(page)).toHaveCount(before - 2);
  await page.waitForTimeout(500);
  expect(await asked(page)).toHaveLength(1);
});

test("a quick second press while the first is still asking does nothing", async ({ page }) => {
  await open(page);
  await collection(page).click();
  await selectThree(page);
  await pressAtOnce(page, [{ repeat: false }, { repeat: false }]);
  await expect(status(page)).toContainText("Removed 3 tracks from the collection.");
  await page.waitForTimeout(500);
  expect(await asked(page)).toHaveLength(1);
  // The removed tracks leave the selection, so a later press names none of them.
  await expect(selected(page)).toHaveCount(0);
  await page.keyboard.press("Delete");
  await page.waitForTimeout(500);
  expect(await asked(page)).toHaveLength(1);
});

test("Delete in a history asks first, and Cancel keeps every play", async ({ page }) => {
  await open(page);
  const tree = page.getByRole("navigation", { name: "Library" });
  await page.getByRole("tablist", { name: "Library sources" }).getByRole("tab", { name: "Histories" }).click();
  await tree.getByRole("treeitem").filter({ hasText: /^September$/ }).getByRole("button").click();
  await tree.getByRole("treeitem").filter({ hasText: "LINK HISTORY 2026-09-04" }).click();
  await expect(page.getByTestId("browser-title")).toContainText("LINK HISTORY 2026-09-04");
  const before = await rows(page).count();
  expect(before).toBeGreaterThan(2);
  await selectFirstTwo(page);

  await answerCancel(page);
  await page.keyboard.press("Delete");
  await expect.poll(() => asked(page)).toEqual([HISTORY_PROMPT]);
  await page.waitForTimeout(500);
  await expect(rows(page)).toHaveCount(before);
  await expect(status(page).filter({ hasText: "Removed" })).toHaveCount(0);

  await page.evaluate(() => {
    (window as unknown as Page$).__confirmAnswer = true;
  });
  await page.keyboard.press("Delete");
  await expect(status(page)).toContainText("Removed 2 plays from the history.");
  await expect(rows(page)).toHaveCount(before - 2);
});

test("Delete in the Tag List asks first, and Cancel keeps every track", async ({ page }) => {
  await open(page);
  await collection(page).click();
  await selectThree(page);
  await rows(page).nth(3).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Add To Tag List" }).click();
  await page.getByRole("tab", { name: "Tag List" }).click();
  await expect(page.getByTestId("browser-title")).toHaveText("Tag List (3 Tracks)");
  await selectFirstTwo(page);

  await answerCancel(page);
  await page.keyboard.press("Backspace");
  await expect.poll(() => asked(page)).toEqual([TAG_LIST_PROMPT]);
  await page.waitForTimeout(500);
  await expect(page.getByTestId("browser-title")).toHaveText("Tag List (3 Tracks)");
  await expect(status(page).filter({ hasText: "Removed" })).toHaveCount(0);

  await page.evaluate(() => {
    (window as unknown as Page$).__confirmAnswer = true;
  });
  await page.keyboard.press("Backspace");
  await expect(status(page)).toContainText("Removed 2 tracks from the Tag List.");
  await expect(page.getByTestId("browser-title")).toHaveText("Tag List (1 Tracks)");
});

test("a read-only library explains the lock and writes nothing", async ({ page }) => {
  await open(page, "/");
  const before = await openPlaylist(page);
  await selectFirstTwo(page);
  await page.keyboard.press("Delete");
  await expect(status(page)).toContainText("Editing is locked");
  await page.waitForTimeout(500);
  expect(await asked(page)).toEqual([]);
  await expect(rows(page)).toHaveCount(before);
});

test("Delete in the tree does not remove the tracks selected in the list", async ({ page }) => {
  await open(page);
  await collection(page).click();
  await selectThree(page);
  // The tree takes the focus; the list keeps its selection.
  await collection(page).click();
  await expect(collection(page)).toBeFocused();
  await expect(selected(page)).toHaveCount(3);
  await page.keyboard.press("Delete");
  await expectNothingRemoved(page);
});

test("after a click on a deck control, Backspace does not remove the selection", async ({ page }) => {
  await open(page);
  await openPlaylist(page);
  await selectFirstTwo(page);
  await page.getByRole("region", { name: "Preview player" }).getByRole("button", { name: "Quantize" }).click();
  await page.keyboard.press("Backspace");
  await expectNothingRemoved(page);
});

test("after a click on the deck's waveform, Delete does not remove the selection", async ({ page }) => {
  await open(page);
  await openPlaylist(page);
  await selectFirstTwo(page);
  // The waveform takes no focus, but the list no longer has it.
  await page.getByRole("region", { name: "Preview player" }).getByRole("progressbar", { name: "Position" }).click();
  await page.keyboard.press("Delete");
  await expectNothingRemoved(page);
  // Back in the list (a row not selected yet, so the click does not start
  // editing its cell), the key works again.
  await rows(page).nth(3).click();
  await page.keyboard.press("Delete");
  await expect(status(page)).toContainText("Removed 1 track.");
});

test("Backspace while typing in the search field edits the text, not the list", async ({ page }) => {
  await open(page);
  await openPlaylist(page);
  await selectFirstTwo(page);
  const search = page.getByRole("searchbox", { name: /search within/i });
  await search.click();
  await search.pressSequentially("ab");
  await page.keyboard.press("Backspace");
  await expect(search).toHaveValue("a");
  await expectNothingRemoved(page);
});

test("Backspace while editing a cell edits the text, not the list", async ({ page }) => {
  await open(page);
  await collection(page).click();
  await selectThree(page);
  await page.locator('[role="gridcell"][data-col="comment"]').nth(3).dblclick();
  const field = page.getByRole("textbox", { name: "Comment" });
  await field.fill("ab");
  await field.press("Backspace");
  await expect(field).toHaveValue("a");
  await field.press("Escape");
  await expectNothingRemoved(page);
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
