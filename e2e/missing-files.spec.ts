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

test("Auto Relocate shows Searching... and analysis asked for meanwhile waits for it (#286)", async ({ page }) => {
  await open(page);
  await page.evaluate(() => { (window as unknown as { __relocateHeld: boolean }).__relocateHeld = true; });
  const status = page.getByRole("contentinfo");
  await rowOf(page, 1).locator('[data-col="title"]').click({ button: "right" });
  await page.getByRole("menu", { name: "Track" }).getByRole("menuitem", { name: "Auto Relocate" }).click();
  // [OBS rekordbox 7.2.19 static] rekordbox's own word for the search.
  const search = status.locator("[data-state='running']");
  await expect(search).toHaveText("Searching...");
  // One backend call: there is nothing for Stop to cut short.
  await expect(search.getByRole("button")).toHaveCount(0);

  await rowOf(page, 0).locator('[data-col="title"]').click();
  await page.keyboard.press("Shift+Meta+A");
  const dialog = page.getByRole("dialog", { name: "Analysis Setting" });
  await dialog.getByRole("button", { name: "OK", exact: true }).click();
  await expect(status).toContainText("Analyzing 1 trackQueued");
  await expect(status.getByRole("progressbar", { name: "Analysis progress" })).toHaveCount(0);

  await page.evaluate(() => { (window as unknown as { __releaseRelocate: () => void }).__releaseRelocate(); });
  await expect(search).toHaveCount(0);
  await expect(status).toContainText(/relocated/);
  await expect(status).not.toContainText("Queued");
  await expect(status.getByRole("button", { name: "Stop" })).toHaveCount(0, { timeout: 15_000 });
});

/** Opens Preferences › Advanced › Database's Auto Relocate Search Folders. */
async function searchFolders(page: Page) {
  await page.getByRole("banner").getByRole("button", { name: "Settings" }).click();
  const preferences = page.getByRole("dialog", { name: "Preferences" });
  await preferences.getByRole("tab", { name: "Advanced" }).click();
  return preferences.getByRole("region", { name: "Auto Relocate Search Folders" });
}

/** File › Display All Missing Files. */
async function openManager(page: Page) {
  await page.waitForFunction(() => "__menu" in window);
  await page.evaluate(() => (window as unknown as { __menu: (id: string) => void }).__menu("missing"));
  const manager = page.getByRole("dialog", { name: "Missing File Manager" });
  await expect(manager).toBeVisible();
  return manager;
}

test("Auto Relocate Search Folders has rekordbox's boxes, Music, Movies and Desktop ticked", async ({ page }) => {
  await open(page);
  const folders = await searchFolders(page);
  // [OBS rekordbox 7.2.19 static, DetailAutoRelocate] Music, Movies,
  // Desktop, then Specified user folders; only the last starts unticked and
  // greys the list, Add and Del.
  const boxes = folders.getByRole("checkbox");
  await expect(boxes).toHaveCount(4);
  await expect(folders.getByRole("checkbox", { name: "Music" })).toBeChecked();
  await expect(folders.getByRole("checkbox", { name: "Movies" })).toBeChecked();
  await expect(folders.getByRole("checkbox", { name: "Desktop" })).toBeChecked();
  const own = folders.getByRole("checkbox", { name: "Specified user folders" });
  await expect(own).not.toBeChecked();
  await expect(folders.getByRole("button", { name: "Add" })).toBeDisabled();
  await expect(folders.getByRole("combobox", { name: "Search folders" })).toBeDisabled();
  await own.check();
  await folders.getByRole("button", { name: "Add" }).click();
  await expect(folders.getByRole("combobox", { name: "Search folders" })).toHaveValue("/Users/mock/Music/Moved");
  await folders.getByRole("checkbox", { name: "Desktop" }).uncheck();
  await page.keyboard.press("Escape");

  // Auto Relocate searches what is ticked, the user's folders first.
  const manager = await openManager(page);
  await manager.getByRole("button", { name: "Auto Relocate" }).click();
  await expect(manager).toContainText("143 Track");
  const searched = await page.evaluate(() => (window as unknown as { __relocateSearch: unknown[] }).__relocateSearch);
  expect(searched).toEqual([{ folders: ["/Users/mock/Music/Moved"], music: true, video: true, desktop: false }]);
});

test("Relocate over several tracks asks for the first file, then finds the rest from its location", async ({ page }) => {
  await open(page, "?missing=2&writable=1");
  await showAttribute(page);
  // Rows 1 and 3 are missing; select rows 0 to 3.
  await rowOf(page, 0).locator('[data-col="title"]').click();
  await rowOf(page, 3).locator('[data-col="title"]').click({ modifiers: ["Shift"] });
  await rowOf(page, 1).locator('[data-col="title"]').click({ button: "right" });
  await page.getByRole("menu", { name: "Track" }).getByRole("menuitem", { name: "Relocate", exact: true }).click();
  // One chooser, for the first missing track, titled with its file name.
  await expect.poll(() => page.evaluate(() => (window as unknown as { __told?: string[] }).__told ?? [])).toHaveLength(1);
  const asked = await page.evaluate(() => {
    const w = window as unknown as { __relocateChooser: string[]; __confirmed: string[]; __confirmTitles: (string | null)[]; __confirmLabels: (string | null)[]; __told: string[]; __relocatedBy: string[] };
    return { chooser: w.__relocateChooser, confirmed: w.__confirmed, titles: w.__confirmTitles, labels: w.__confirmLabels, told: w.__told, by: w.__relocatedBy };
  });
  expect(asked.chooser).toHaveLength(1);
  expect(asked.chooser[0]).toMatch(/^Choose a new fullpath for : \d+\.mp3 \| $/);
  expect(asked.confirmed).toHaveLength(1);
  expect(asked.confirmed[0]).toMatch(/^Would you like RBXport to find other missing file using the location of this track \?\n./);
  expect(asked.titles).toEqual(["Missing File Manager"]);
  expect(asked.labels).toEqual(["Yes/No"]);
  expect(asked.told).toEqual(["Missing File Manager: RBXport found 1 files."]);
  expect(asked.by).toHaveLength(1);
  // Both [!] are gone.
  for (const at of [1, 3]) await expect(rowOf(page, at).getByRole("img", { name: "File is Missing" })).toHaveCount(0);
});

test("the manager's Relocate over every row reaches every missing track past the first page", async ({ page }) => {
  // 286 missing, three pages of the list. Each relocate saves, and the list
  // is scanned again without the track, so the run must not page through it
  // as it goes.
  await open(page);
  const manager = await openManager(page);
  await expect(manager).toContainText("286 Track");
  await manager.getByRole("button", { name: "Relocate", exact: true }).click();
  await expect(manager.locator("[aria-live=polite]")).toHaveText("0 Track");
  const told = await page.evaluate(() => (window as unknown as { __told: string[] }).__told);
  expect(told).toEqual(["Missing File Manager: RBXport found 285 files."]);
});

test("the manager's Relocate by hand, track by track, reaches every missing track past the first page", async ({ page }) => {
  test.setTimeout(60_000);
  await open(page);
  const manager = await openManager(page);
  await expect(manager).toContainText("286 Track");
  // No to every "find the others?": a chooser for each of the 286.
  await page.evaluate(() => { (window as unknown as { __confirmAnswer: boolean }).__confirmAnswer = false; });
  await manager.getByRole("button", { name: "Relocate", exact: true }).click();
  await expect(manager.locator("[aria-live=polite]")).toHaveText("0 Track");
  const chosen = await page.evaluate(() => (window as unknown as { __relocateChooser: string[] }).__relocateChooser.length);
  expect(chosen).toBe(286);
});

test("a missing track does not load:the deck keeps its track and the status bar says why", async ({ page }) => {
  // Read-only, so a double-click loads rather than edits.
  await open(page, "?missing=7");
  await rowOf(page, 0).locator('[data-col="title"]').dblclick();
  const title = page.getByTestId("player-title").first();
  const loaded = await rowOf(page, 0).locator('[data-col="title"]').innerText();
  await expect(title).toHaveText(loaded);
  await rowOf(page, 1).locator('[data-col="title"]').dblclick();
  await expect(page.getByRole("alert")).toHaveText("Load error. The file could not be found.");
  await expect(title).toHaveText(loaded);
});

test("the Missing File Manager lists every missing track and relocates them", async ({ page }) => {
  // Preferences, then the manager, then three rescans: longer than one view.
  test.setTimeout(60_000);
  await open(page);
  // A search folder first, from Preferences, where rekordbox keeps them.
  const folders = await searchFolders(page);
  await folders.getByRole("checkbox", { name: "Specified user folders" }).check();
  await folders.getByRole("button", { name: "Add" }).click();
  await expect(folders.getByRole("combobox", { name: "Search folders" })).toHaveValue("/Users/mock/Music/Moved");
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog", { name: "Preferences" })).toHaveCount(0);

  const manager = await openManager(page);

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
  expect(await page.evaluate(() => (window as unknown as { __relocateSearch: unknown[] }).__relocateSearch))
    .toEqual([{ folders: ["/Users/mock/Music/Moved"], music: true, video: true, desktop: true }]);

  // One row, then Delete: rekordbox's question, then only that one goes.
  await grid.getByRole("row").nth(1).click();
  await manager.getByRole("button", { name: "Delete" }).click();
  await expect(manager).toContainText("142 Track");
  const asked = await page.evaluate(() => {
    const w = window as unknown as { __confirmed: string[]; __confirmTitles: (string | null)[]; __confirmLabels: (string | null)[] };
    return { message: w.__confirmed.at(-1), title: w.__confirmTitles.at(-1), labels: w.__confirmLabels.at(-1) };
  });
  expect(asked).toEqual({ message: "Are you sure you want to remove the selected tracks?", title: "Remove", labels: "OK/Cancel" });

  // The Delete key asks the same; Cancel keeps the row.
  await page.evaluate(() => { (window as unknown as { __confirmAnswer: boolean }).__confirmAnswer = false; });
  await grid.getByRole("row").nth(1).click();
  await page.keyboard.press("Delete");
  await expect.poll(() => page.evaluate(() => (window as unknown as { __confirmed: string[] }).__confirmed.length)).toBe(2);
  await expect(manager).toContainText("142 Track");

  await manager.getByRole("button", { name: "OK" }).click();
  await expect(manager).toHaveCount(0);
});
