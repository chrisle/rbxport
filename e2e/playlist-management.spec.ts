import { expect, test, type Locator, type Page } from "@playwright/test";

/**
 * Managing playlists from the interface: made from the tree's menu, filled
 * by a drop, emptied from the track's menu, and deleted again. The mock
 * backend owns the tree and the memberships the way Rust does, so what the
 * list shows after each step is the backend's answer, not the view's guess.
 */

/** The mock stands in for a library rekordbox holds unless told otherwise. */
async function open(page: Page) {
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
}

function item(page: Page, name: string): Locator {
  return page.getByRole("treeitem").filter({ hasText: name }).first();
}

/** The indent the tree draws for a node, which is how deep it is. */
async function depthOf(node: Locator): Promise<number> {
  const padding = await node.evaluate((el) => parseFloat(getComputedStyle(el).paddingLeft));
  return (padding - 14) / 20;
}

async function chooseFromTreeMenu(page: Page, node: Locator, entry: string) {
  await node.click({ button: "right" });
  const menu = page.getByRole("menu", { name: /^(Playlist|Folder)$/ });
  await expect(menu).toBeVisible();
  await menu.getByRole("menuitem", { name: entry, exact: true }).click();
  await expect(menu).toBeHidden();
}

const rows = (page: Page) => page.getByRole("row").filter({ has: page.getByRole("gridcell") });

function collection(page: Page): Locator {
  return page.locator('[role="treeitem"][data-kind="collection"]');
}

async function openCollectionMenu(page: Page) {
  await collection(page).click({ button: "right" });
  const menu = page.getByRole("menu", { name: "Playlists" });
  await expect(menu).toBeVisible();
  return menu;
}

test("RBX-23: an empty library exposes creation from the Playlists collection", async ({ page }) => {
  await page.goto("/?writable=1&playlistFixture=empty");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await expect(page.locator('[role="treeitem"][data-kind="playlist"], [role="treeitem"][data-kind="folder"]')).toHaveCount(0);

  const menu = await openCollectionMenu(page);
  await expect(menu.getByRole("menuitem", { name: "Create New Playlist" })).toBeEnabled();
  await expect(menu.getByRole("menuitem", { name: "Create New Folder" })).toBeEnabled();
  await menu.getByRole("menuitem", { name: "Create New Playlist" }).click();

  await expect(page.getByRole("contentinfo")).toContainText("Created New playlist.");
  await expect(item(page, "New playlist")).toBeVisible();
});

test("RBX-22: deleting the last CUE playlist leaves collection creation available", async ({ page }) => {
  await page.goto("/?writable=1&playlistFixture=cueOnly");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await chooseFromTreeMenu(page, item(page, "CUE"), "Delete Playlist");
  await expect(item(page, "CUE")).toHaveCount(0);

  const menu = await openCollectionMenu(page);
  await menu.getByRole("menuitem", { name: "Create New Folder" }).click();
  await expect(page.getByRole("contentinfo")).toContainText("Created New folder.");
  await expect(item(page, "New folder")).toBeVisible();
});

test("a playlist made from another's menu lands in the same folder, empty", async ({ page }) => {
  await open(page);
  const sibling = item(page, "Melodic Vox");
  const siblingDepth = await depthOf(sibling);

  await chooseFromTreeMenu(page, sibling, "Create New Playlist");
  await expect(page.getByRole("contentinfo")).toContainText("Created New playlist.");

  // rekordbox's default name, beside the playlist the menu was opened on
  // rather than at the top of the tree.
  const made = item(page, "New playlist");
  await expect(made).toBeVisible();
  expect(await depthOf(made)).toBe(siblingDepth);

  await made.click();
  await expect(page.getByTestId("browser-title")).toHaveText("New playlist (0 Tracks)");
  await expect(rows(page)).toHaveCount(0);
});

test("a folder made from a folder's menu goes inside it", async ({ page }) => {
  await open(page);
  const folder = item(page, "CURRENT");
  const folderDepth = await depthOf(folder);

  await chooseFromTreeMenu(page, folder, "Create New Folder");
  await expect(page.getByRole("contentinfo")).toContainText("Created New folder.");
  const made = item(page, "New folder");
  await expect(made).toBeVisible();
  expect(await depthOf(made)).toBe(folderDepth + 1);
});

test("a track dropped on a new playlist is in it, and its menu takes it out again", async ({
  page,
}) => {
  await open(page);
  await chooseFromTreeMenu(page, item(page, "Hardstyle"), "Create New Playlist");
  const made = item(page, "New playlist");
  await expect(made).toBeVisible();

  // From All Tracks, the second row, so the title is known before the drop.
  const source = rows(page).nth(1);
  const title = (await source.locator('[data-col="title"]').innerText()).trim();
  await source.dragTo(made);
  await expect(page.getByRole("contentinfo")).toContainText("Added 1 track to New playlist.");
  // Dropping it again asks what to do with the duplicate; skipping it
  // changes nothing.
  await source.dragTo(made);
  await page.getByRole("alertdialog", { name: "Duplicate tracks" })
    .getByRole("button", { name: "Skip duplicates" }).click();
  await expect(page.getByRole("alertdialog")).toHaveCount(0);

  await made.click();
  await expect(page.getByTestId("browser-title")).toHaveText("New playlist (1 Tracks)");
  const only = rows(page).first();
  await expect(only.locator('[data-col="title"]')).toHaveText(title);
  // Numbered by its place in the playlist, not the collection.
  await expect(only.locator('[data-col="trackNo"]')).toHaveText("1");

  // Out again from the track's own menu, which is live only inside a playlist.
  await rows(page).first().locator('[data-col="title"]').click({ button: "right" });
  const menu = page.getByRole("menu", { name: "Track" });
  const remove = menu.getByRole("menuitem", { name: "Remove from Playlist" });
  await expect(remove).toBeEnabled();
  await remove.click();
  await expect(page.getByRole("contentinfo")).toContainText("Removed 1 track.");
  await expect(rows(page)).toHaveCount(0);
  await expect(page.getByTestId("browser-title")).toHaveText("New playlist (0 Tracks)");
});

test("Tag List stays available after a library write", async ({ page }) => {
  await open(page);
  await chooseFromTreeMenu(page, item(page, "Hardstyle"), "Create New Playlist");
  await page.getByRole("tab", { name: "Tag List" }).click();
  await expect(page.getByRole("tab", { name: "Tag List" })).toHaveAttribute("aria-selected", "true");
  await expect(page.getByTestId("browser-title")).toHaveText("Tag List (0 Tracks)");
});

test("a playlist folder shows its child tracks instead of the collection", async ({ page }) => {
  await open(page);
  const source = rows(page).first();
  const title = (await source.locator('[data-col="title"]').innerText()).trim();
  const current = item(page, "CURRENT");
  await chooseFromTreeMenu(page, current, "Create New Playlist");
  await source.dragTo(item(page, "New playlist"));
  await current.click();
  await expect(page.getByTestId("browser-title")).toContainText("CURRENT (");
  await expect(rows(page).filter({ hasText: title }).first()).toBeVisible();
  await expect(page.getByTestId("browser-title")).not.toContainText("2000 Tracks");
});

test("dragging a selection drops every row in it, not just the one under the hand", async ({ page }) => {
  await open(page);
  await chooseFromTreeMenu(page, item(page, "Hardstyle"), "Create New Playlist");
  const made = item(page, "New playlist");
  await expect(made).toBeVisible();

  // Three rows, shift-click; the press that starts the drag lands on a row
  // already selected, and must not collapse the selection to it.
  await rows(page).nth(1).locator('[data-col="title"]').click();
  await rows(page).nth(3).locator('[data-col="title"]').click({ modifiers: ["Shift"] });
  await expect(page.getByText("Selected: 3 Tracks")).toBeVisible();
  await rows(page).nth(2).dragTo(made);
  await expect(page.getByRole("contentinfo")).toContainText("Added 3 tracks to New playlist.");
  await expect(page.getByText("Selected: 3 Tracks")).toBeVisible();

  // A plain click on a selected row, released without a drag, does collapse
  // to it — the ordinary click still works.
  await rows(page).nth(2).locator('[data-col="title"]').click();
  await expect(page.getByText("Selected: 1 Track")).toBeVisible();

  await made.click();
  await expect(page.getByTestId("browser-title")).toHaveText("New playlist (3 Tracks)");
});

test("removing from an existing playlist shortens it by exactly the selection", async ({ page }) => {
  await open(page);
  const playlist = item(page, "Eurodance");
  await playlist.click();
  const title = page.getByTestId("browser-title");
  await expect(title).toContainText("Eurodance (");
  const before = Number(/\((\d+) Tracks\)/.exec(await title.innerText())?.[1]);
  expect(before).toBeGreaterThan(2);

  const first = rows(page).first();
  const second = rows(page).nth(1);
  const third = (await rows(page).nth(2).locator('[data-col="title"]').innerText()).trim();

  // Two rows, shift-click, and a right-click on the selection keeps it so
  // the menu acts on both — not on the one under the pointer.
  await first.locator('[data-col="title"]').click();
  await second.locator('[data-col="title"]').click({ modifiers: ["Shift"] });
  await expect(page.getByText("Selected: 2 Tracks")).toBeVisible();
  await second.locator('[data-col="title"]').click({ button: "right" });
  await expect(page.getByText("Selected: 2 Tracks")).toBeVisible();
  await page.getByRole("menu", { name: "Track" }).getByRole("menuitem", { name: "Remove from Playlist" }).click();

  await expect(page.getByRole("contentinfo")).toContainText("Removed 2 tracks.");
  await expect(title).toHaveText(`Eurodance (${before - 2} Tracks)`);
  // What was third is first: the two above it went, and nothing else moved.
  await expect(rows(page).first().locator('[data-col="title"]')).toHaveText(third);
  await expect(rows(page).first().locator('[data-col="trackNo"]')).toHaveText("1");
});

test("deleting a playlist takes it out of the tree and the view moves off it", async ({ page }) => {
  await open(page);
  await chooseFromTreeMenu(page, item(page, "Melodic Vox"), "Create New Playlist");
  const made = item(page, "New playlist");
  await made.click();
  await expect(page.getByTestId("browser-title")).toHaveText("New playlist (0 Tracks)");

  await chooseFromTreeMenu(page, made, "Delete Playlist");
  await expect(page.getByRole("contentinfo")).toContainText("Deleted New playlist.");
  await expect(page.getByRole("treeitem").filter({ hasText: "New playlist" })).toHaveCount(0);
  // The selection does not stay on a list that no longer exists.
  await expect(page.getByTestId("browser-title")).not.toContainText("New playlist");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");

  // Fire the native Edit menu callback exposed by the mock. Undo restores
  // the identical tree node; redo removes it again.
  await page.evaluate(() => (window as unknown as { __menu?: (id: string) => void }).__menu?.("undo"));
  await expect(page.getByRole("contentinfo")).toContainText("Edit undone.");
  await expect(item(page, "New playlist")).toBeVisible();
  await page.evaluate(() => (window as unknown as { __menu?: (id: string) => void }).__menu?.("redo"));
  await expect(page.getByRole("contentinfo")).toContainText("Edit redone.");
  await expect(page.getByRole("treeitem").filter({ hasText: "New playlist" })).toHaveCount(0);

  // A playlist that was there from the start goes the same way.
  await chooseFromTreeMenu(page, item(page, "Drum and Bass"), "Delete Playlist");
  await expect(page.getByRole("treeitem").filter({ hasText: "Drum and Bass" })).toHaveCount(0);
  await expect(item(page, "Eurodance")).toBeVisible();
});

test("a folder's menu deletes the folder", async ({ page }) => {
  await open(page);
  await chooseFromTreeMenu(page, item(page, "CURRENT"), "Create New Folder");
  const made = item(page, "New folder");
  await expect(made).toBeVisible();
  await made.click({ button: "right" });
  const menu = page.getByRole("menu", { name: "Folder" });
  await expect(menu.getByRole("menuitem", { name: "Delete Folder" })).toBeVisible();
  await menu.getByRole("menuitem", { name: "Delete Folder" }).click();
  await expect(page.getByRole("treeitem").filter({ hasText: "New folder" })).toHaveCount(0);
});

test("with the library protected, the tree menu will not create or delete", async ({ page }) => {
  await open(page);
  await page.getByRole("banner").getByRole("button", { name: "Settings" }).click();
  const dialog = page.getByRole("dialog", { name: "Preferences" });
  await dialog.getByRole("tab", { name: "Advanced" }).click();
  await dialog.getByRole("tab", { name: "Browse", exact: true }).click();
  await dialog.getByRole("switch", { name: "Protect library edit." }).click();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("contentinfo")).toContainText(/read-only/i);

  await item(page, "Melodic Vox").click({ button: "right" });
  const menu = page.getByRole("menu", { name: "Playlist" });
  for (const label of ["Create New Playlist", "Create New Folder", "Delete Playlist"]) {
    await expect(menu.getByRole("menuitem", { name: label, exact: true })).toBeDisabled();
  }
  // Reading is still fine.
  await expect(menu.getByRole("menuitem", { name: "Export Playlist" })).toBeEnabled();
  await page.keyboard.press("Escape");

  await item(page, "Melodic Vox").click();
  await rows(page).first().locator('[data-col="title"]').click({ button: "right" });
  await expect(
    page.getByRole("menu", { name: "Track" }).getByRole("menuitem", { name: "Remove from Playlist" }),
  ).toBeDisabled();
});

test("a rating set after a playlist edit stays set once the reload lands", async ({ page }) => {
  // A playlist edit and a rating both end in a reload the backend announces
  // with its generation. The view must key on that number alone: a count the
  // interface bumped itself on the playlist edit could land on the same value
  // the backend then announced for the rating, and a generation that does not
  // change is a page that is not refetched — the star lit for a moment and
  // went out when the overlay was dropped.
  await open(page);
  await chooseFromTreeMenu(page, item(page, "Melodic Vox"), "Create New Playlist");
  await expect(page.getByRole("contentinfo")).toContainText("Created New playlist.");
  await chooseFromTreeMenu(page, item(page, "CURRENT"), "Create New Folder");
  await expect(page.getByRole("contentinfo")).toContainText("Created New folder.");

  // A star the row does not already have: clicking the set one clears it.
  const stars = rows(page).nth(3).locator('[data-col="rating"] [role="radiogroup"]');
  const had = (await stars.getByRole("radio", { checked: true }).count()) > 0
    ? await stars.getByRole("radio", { checked: true }).getAttribute("aria-label")
    : null;
  const pick = had === "3 of 5" ? 2 : 3;
  await stars.getByRole("radio", { name: `${pick} of 5` }).click();
  await expect(page.getByRole("contentinfo")).toContainText(`Rated ${pick} of 5`);
  // Past the moment the optimistic overlay is dropped for the re-read rows.
  await page.waitForTimeout(500);
  await expect(stars.getByRole("radio", { name: `${pick} of 5` })).toHaveAttribute("aria-checked", "true");
  await expect(stars.locator("svg[data-lit]")).toHaveCount(pick);
  await expect(stars.locator("svg")).toHaveCount(5);
});

test("a playlist is renamed in the row, and Escape puts the old name back", async ({ page }) => {
  await open(page);
  await chooseFromTreeMenu(page, item(page, "Melodic Vox"), "Rename Playlist");

  // The name becomes a field with the whole of it selected, so typing
  // replaces it rather than appending to it.
  const field = page.getByRole("textbox", { name: "Rename Melodic Vox" });
  await expect(field).toBeVisible();
  await expect(field).toHaveValue("Melodic Vox");

  await field.fill("Melodic Vox 2026");
  await field.press("Enter");
  await expect(page.getByRole("contentinfo")).toContainText("Renamed to Melodic Vox 2026.");
  await expect(item(page, "Melodic Vox 2026")).toBeVisible();
  await expect(field).toHaveCount(0);

  // Escape is a cancel: the typing goes, the name stays.
  await chooseFromTreeMenu(page, item(page, "Melodic Vox 2026"), "Rename Playlist");
  const again = page.getByRole("textbox", { name: "Rename Melodic Vox 2026" });
  await again.fill("Something else entirely");
  await again.press("Escape");
  await expect(again).toHaveCount(0);
  await expect(item(page, "Melodic Vox 2026")).toBeVisible();
  await expect(page.getByRole("treeitem").filter({ hasText: "Something else entirely" })).toHaveCount(0);
});

test("clicking a selected playlist name edits it and clicking away saves", async ({ page }) => {
  await open(page);
  await item(page, "Eurodance").click();
  const name = item(page, "Melodic Vox").getByText("Melodic Vox", { exact: true });
  await name.click();
  await expect(page.getByRole("textbox", { name: "Rename Melodic Vox" })).toHaveCount(0);
  await name.click();
  const field = page.getByRole("textbox", { name: "Rename Melodic Vox" });
  await field.fill("Evening set");
  await page.getByRole("searchbox", { name: "Search library tree", exact: true }).click();
  await expect(item(page, "Evening set")).toBeVisible();
  await expect(page.getByRole("contentinfo")).toContainText("Renamed to Evening set.");
});

test("playlist names respect double-click to edit and F2 supports folders", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("rbl.preferences", JSON.stringify({ advanced: { doubleClickToEdit: true } })));
  await open(page);
  const name = item(page, "Melodic Vox").getByText("Melodic Vox", { exact: true });
  await name.click();
  await name.click();
  const field = page.getByRole("textbox", { name: "Rename Melodic Vox" });
  await expect(field).toHaveCount(0);
  await name.dblclick();
  await field.fill("Cancelled name");
  await field.press("Escape");
  await expect(name).toBeVisible();
  const folder = item(page, "CURRENT");
  await folder.click();
  await folder.press("F2");
  const folderField = page.getByRole("textbox", { name: "Rename CURRENT", exact: true });
  await folderField.fill("Current sets");
  await folderField.press("Enter");
  await expect(item(page, "Current sets")).toBeVisible();
});

test("renaming is refused while rekordbox holds the library", async ({ page }) => {
  // Every write is greyed rather than raced, which is the rule the rest of
  // the menu follows.
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await item(page, "Melodic Vox").getByText("Melodic Vox", { exact: true }).dblclick();
  await item(page, "Melodic Vox").press("F2");
  await expect(page.getByRole("textbox", { name: "Rename Melodic Vox" })).toHaveCount(0);
  await item(page, "Melodic Vox").click({ button: "right" });
  const menu = page.getByRole("menu", { name: "Playlist" });
  await expect(menu.getByRole("menuitem", { name: "Rename Playlist" })).toBeDisabled();
});

const titles = (page: Page) =>
  page.locator('[role="gridcell"][data-col="title"]').allTextContents();

/**
 * Drags row `from` onto the lower half of row `to`, which lands it after it.
 *
 * `dragTo` rather than the mouse by hand: it drives the drag through the
 * browser's own protocol, which is the only form WebKit takes.
 */
async function dragRow(page: Page, from: number, to: number) {
  const list = rows(page);
  const box = await list.nth(to).boundingBox();
  if (!box) throw new Error("the target row is not on screen");
  await list.nth(from).dragTo(list.nth(to), {
    targetPosition: { x: 120, y: box.height - 3 },
  });
}

/** Opens a playlist, which is the only thing with an order of its own. */
async function openPlaylist(page: Page) {
  await item(page, "Melodic Vox").click();
  await expect(page.getByTestId("browser-title")).toContainText("Melodic Vox");
}

test("a track dragged down a playlist lands where it was dropped", async ({ page }) => {
  await open(page);
  await openPlaylist(page);
  const before = await titles(page);

  await dragRow(page, 0, 3);
  await expect(page.getByRole("contentinfo")).toContainText("Playlist reordered.");

  // The first row took the fourth place and the three above it moved up.
  await expect.poll(async () => (await titles(page))[3]).toBe(before[0]);
  const after = await titles(page);
  expect(after.slice(0, 4)).toEqual([before[1], before[2], before[3], before[0]]);
  // Nothing was lost or duplicated: the whole order is written, not a patch.
  expect([...after].sort()).toEqual([...before].sort());
});

test("the line shows where the rows would land before they are let go", async ({
  page,
  browserName,
}) => {
  // Driven by hand, so the drag can be inspected halfway. Chromium alone
  // synthesises HTML5 drag events from raw mouse moves.
  // TODO: enable WebKit when raw mouse moves can start native HTML5 drags.
  test.skip(browserName !== "chromium", "only Chromium drags from mouse events");
  await open(page);
  await openPlaylist(page);

  const list = rows(page);
  const a = await list.nth(0).boundingBox();
  const b = await list.nth(3).boundingBox();
  if (!a || !b) throw new Error("the rows are not on screen");
  await page.mouse.move(a.x + 120, a.y + a.height / 2);
  await page.mouse.down();
  await page.mouse.move(b.x + 120, b.y + b.height - 3, { steps: 12 });

  // One line, on the edge the rows would go in at.
  await expect(page.locator('[role="row"][data-drop]')).toHaveCount(1);
  await expect(page.locator('[role="row"][data-drop="below"]')).toHaveCount(1);

  // And it goes when the drag does.
  await page.mouse.up();
  await expect(page.locator('[role="row"][data-drop]')).toHaveCount(0);
});

test("a reorder drag held at the top of the rows scrolls the list up", async ({
  page,
  browserName,
}) => {
  // Held by hand, so the pointer can rest in the edge zone. Chromium alone
  // synthesises HTML5 drag events from raw mouse moves.
  // TODO: enable WebKit when raw mouse moves can start native HTML5 drags.
  test.skip(browserName !== "chromium", "only Chromium drags from mouse events");
  await page.setViewportSize({ width: 1400, height: 620 });
  await open(page);
  await openPlaylist(page);

  const scroller = page.getByTestId("track-scroll");
  await expect.poll(() => scroller.evaluate((el) => el.scrollHeight > el.clientHeight + 100)).toBe(true);
  await scroller.evaluate((el) => { el.scrollTop = el.scrollHeight; });
  const start = await scroller.evaluate((el) => el.scrollTop);
  expect(start).toBeGreaterThan(0);

  const list = rows(page);
  const last = list.last();
  await last.scrollIntoViewIfNeeded();
  const a = await last.boundingBox();
  const head = await page.locator('[role="row"]').first().boundingBox();
  if (!a || !head) throw new Error("the rows are not on screen");
  await page.mouse.move(a.x + 120, a.y + a.height / 2);
  await page.mouse.down();
  // Into rekordbox's 30px zone just below the column heading, then rest.
  await page.mouse.move(a.x + 120, head.y + head.height + 5, { steps: 12 });
  // Chromium sends dragover for only some synthesised moves; nudge so one
  // lands in the zone. After that the pointer rests and the repeat does it.
  await page.mouse.move(a.x + 121, head.y + head.height + 5);
  await page.mouse.move(a.x + 122, head.y + head.height + 5);

  // Resting there keeps scrolling (100ms repeats) until the top is reached.
  await expect.poll(() => scroller.evaluate((el) => el.scrollTop), { timeout: 10_000 }).toBe(0);
  await page.mouse.up();
});

/**
 * Starts a mouse drag on the last row of the open playlist, scrolled to its
 * end, and rests it in the top edge zone so the list scrolls up under it.
 */
async function holdLastRowAtTop(page: Page, depth: number) {
  await page.setViewportSize({ width: 1400, height: 620 });
  await open(page);
  await openPlaylist(page);
  const scroller = page.getByTestId("track-scroll");
  await expect.poll(() => scroller.evaluate((el) => el.scrollHeight > el.clientHeight + 100)).toBe(true);
  await scroller.evaluate((el) => { el.scrollTop = el.scrollHeight; });
  const last = rows(page).last();
  await last.scrollIntoViewIfNeeded();
  const a = await last.boundingBox();
  const head = await page.locator('[role="row"]').first().boundingBox();
  if (!a || !head) throw new Error("the rows are not on screen");
  // Far enough that the virtualizer (16 rows of overscan past what is in
  // view) would drop the source row once the list is at its top.
  const reach = await scroller.evaluate((el) => el.scrollTop);
  expect(reach).toBeGreaterThan(a.height * 17);
  const dragged = await last.locator('[data-col="title"]').textContent();
  // `depth` px into rekordbox's 30px zone, which scrolls 30 - depth px a step.
  const zone = { x: a.x + 120, y: head.y + head.height + depth };
  await page.mouse.move(a.x + 120, a.y + a.height / 2);
  await page.mouse.down();
  await page.mouse.move(zone.x, zone.y, { steps: 12 });
  await page.mouse.move(zone.x + 1, zone.y);
  await page.mouse.move(zone.x + 2, zone.y);
  return { scroller, reach, zone: { x: zone.x + 2, y: zone.y }, head, dragged };
}

/** A Finder file dropped on a tree playlist: refused in the browser, not added. */
async function dropFileOnHardstyle(page: Page) {
  const target = item(page, "Hardstyle");
  const files = await page.evaluateHandle(() => {
    const transfer = new DataTransfer();
    transfer.items.add(new File(["audio"], "a.mp3", { type: "audio/mpeg" }));
    return transfer;
  });
  await target.dispatchEvent("dragover", { dataTransfer: files });
  await target.dispatchEvent("drop", { dataTransfer: files });
}

test("a reorder drop made while the list scrolls ends the drag and stops the scroll", async ({
  page,
  browserName,
}) => {
  test.skip(browserName !== "chromium", "only Chromium drags from mouse events");
  // Slowly (10px a step), so the drop can be timed while the list moves.
  const { scroller, zone } = await holdLastRowAtTop(page, 20);
  // Let go on a row while the repeat is still moving the list, once the
  // dragged row has left the rows drawn (what is in view plus 16 each way).
  await expect.poll(() => scroller.evaluate((el) => el.scrollTop), { timeout: 15_000, intervals: [25] })
    .toBeLessThan(90);
  // A dragover over the row now under the pointer, which the drop lands on.
  for (let i = 1; i <= 4; i++) {
    await page.mouse.move(zone.x + i, zone.y);
    await page.waitForTimeout(20);
  }
  await page.mouse.up();
  await expect(page.getByRole("contentinfo")).toContainText("Playlist reordered.");
  const after = await scroller.evaluate((el) => el.scrollTop);
  expect(after).toBeGreaterThan(0);
  await page.waitForTimeout(500);
  expect(await scroller.evaluate((el) => el.scrollTop)).toBe(after);

  // The drag is over: a file let go on a playlist is a file, not the row.
  await dropFileOnHardstyle(page);
  await expect(page.getByRole("contentinfo")).toContainText("Drop files in the desktop app to import them.");
});

test("the line follows the rows scrolling under a resting pointer, and the drop lands on it", async ({
  page,
  browserName,
}) => {
  test.skip(browserName !== "chromium", "only Chromium drags from mouse events");
  // rekordbox re-places its insertion marker at the pointer on every 100ms
  // repeat (ListViewer::itemDragMove), so the line never drifts off with the
  // rows. The pointer does not move again after `holdLastRowAtTop`.
  const { scroller, zone, dragged } = await holdLastRowAtTop(page, 20);

  /** The line's row, its edge, and whether the pointer is over that row. */
  const line = () => page.evaluate((y) => {
    const marked = [...document.querySelectorAll<HTMLElement>('[role="row"][data-drop]')];
    const row = marked[0];
    if (marked.length !== 1 || !row) return { count: marked.length, under: false, edge: null, title: null };
    const box = row.getBoundingClientRect();
    return {
      count: 1,
      under: y >= box.top && y <= box.bottom,
      edge: row.dataset.drop ?? null,
      title: row.querySelector('[data-col="title"]')?.textContent ?? null,
    };
  }, zone.y);

  // While the list moves under it, sampled at several scroll positions.
  const seen = new Set<number>();
  const off: string[] = [];
  await expect.poll(async () => {
    const top = await scroller.evaluate((el) => el.scrollTop);
    const at = await line();
    if (at.count !== 1 || !at.under) off.push(`scrollTop ${top}: ${JSON.stringify(at)}`);
    seen.add(top);
    return top;
  }, { timeout: 15_000, intervals: [60] }).toBe(0);
  expect(off).toEqual([]);
  expect(seen.size).toBeGreaterThan(3);

  // At the top, at rest: the line is still under the pointer, and a release
  // with no move since puts the row exactly there.
  const at = await line();
  expect(at.under).toBe(true);
  const before = await titles(page);
  const index = before.indexOf(at.title ?? "");
  expect(index).toBeGreaterThanOrEqual(0);
  // A drag update at the very same point, which a real browser sends on its
  // own while the pointer rests; Chromium's synthesised drag sends none, and
  // will not drop without a fresh one. It does not move the pointer.
  await page.mouse.move(zone.x, zone.y);
  await page.mouse.up();
  await expect(page.getByRole("contentinfo")).toContainText("Playlist reordered.");
  await expect.poll(async () => (await titles(page))[index + (at.edge === "below" ? 1 : 0)]).toBe(dragged);
  await expect(page.locator('[role="row"][data-drop]')).toHaveCount(0);
});

test("a drag scrolled away from its row and let go over nothing still ends", async ({
  page,
  browserName,
}) => {
  test.skip(browserName !== "chromium", "only Chromium drags from mouse events");
  const { scroller, zone, head } = await holdLastRowAtTop(page, 5);
  await expect.poll(() => scroller.evaluate((el) => el.scrollTop), { timeout: 10_000 }).toBe(0);
  // Over the column heading, which takes no drop: only the source row's
  // dragend can end this drag, and the row is far out of view.
  await page.mouse.move(zone.x, head.y + head.height / 2);
  await page.mouse.move(zone.x + 1, head.y + head.height / 2);
  await page.mouse.up();
  await expect(page.locator('[role="row"][data-drop]')).toHaveCount(0);

  await dropFileOnHardstyle(page);
  await expect(page.getByRole("contentinfo")).toContainText("Drop files in the desktop app to import them.");
});

test("a drag resting mid-list does not scroll it", async ({ page, browserName }) => {
  test.skip(browserName !== "chromium", "only Chromium drags from mouse events");
  await page.setViewportSize({ width: 1400, height: 620 });
  await open(page);
  await openPlaylist(page);

  const scroller = page.getByTestId("track-scroll");
  await expect.poll(() => scroller.evaluate((el) => el.scrollHeight > el.clientHeight + 100)).toBe(true);
  const box = await scroller.boundingBox();
  const first = await rows(page).first().boundingBox();
  if (!box || !first) throw new Error("the rows are not on screen");
  await page.mouse.move(first.x + 120, first.y + first.height / 2);
  await page.mouse.down();
  await page.mouse.move(first.x + 120, box.y + box.height / 2, { steps: 12 });
  await page.mouse.move(first.x + 121, box.y + box.height / 2);
  await page.mouse.move(first.x + 122, box.y + box.height / 2);
  await page.waitForTimeout(500);
  expect(await scroller.evaluate((el) => el.scrollTop)).toBe(0);
  await page.mouse.up();
});

test("the order cannot be dragged where the rows are not the playlist's own", async ({ page }) => {
  await open(page);
  await openPlaylist(page);
  // Sorted by a column, the rows are a rearrangement of the playlist, so
  // writing what is on screen as the whole order would scramble the rest.
  await page.getByRole("columnheader", { name: /BPM/ }).click();
  const before = await titles(page);

  await dragRow(page, 0, 3);
  await expect(page.getByRole("contentinfo")).not.toContainText("Playlist reordered.");
  expect(await titles(page)).toEqual(before);
});

test("the collection has no order of its own to drag", async ({ page }) => {
  await open(page);
  await page.getByRole("treeitem", { name: /All Tracks/ }).click();
  await expect(page.getByTestId("browser-title")).not.toContainText("Melodic Vox");
  const before = await titles(page);

  await dragRow(page, 0, 3);
  await expect(page.getByRole("contentinfo")).not.toContainText("Playlist reordered.");
  expect(await titles(page)).toEqual(before);
});

/** The tree's node names, without the track counts. */
const treeNames = async (page: Page) =>
  (await page.getByRole("treeitem").allTextContents()).map((s) =>
    s.replace(/\(\d+\)$/, "").trim(),
  );

test("a playlist dragged down the tree takes the place it was dropped at", async ({ page }) => {
  await open(page);
  const before = await treeNames(page);
  const from = before.indexOf("Melodic Vox");
  expect(from).toBeGreaterThan(0);
  // Two rows down, which is a sibling at the same level.
  const target = before[from + 2]!;

  const dst = item(page, target);
  const box = await dst.boundingBox();
  if (!box) throw new Error("the target row is not on screen");
  await item(page, "Melodic Vox").dragTo(dst, {
    targetPosition: { x: 40, y: box.height - 2 },
  });

  await expect(page.getByRole("contentinfo")).toContainText("Moved Melodic Vox.");
  await expect
    .poll(async () => (await treeNames(page)).indexOf("Melodic Vox"))
    .toBe(before.indexOf(target));
  // Nothing was lost: the same nodes, in a different order.
  expect([...(await treeNames(page))].sort()).toEqual([...before].sort());
});

test("a playlist dropped into a folder goes inside it", async ({ page }) => {
  await open(page);
  await chooseFromTreeMenu(page, item(page, "Melodic Vox"), "Create New Folder");
  await expect(page.getByRole("contentinfo")).toContainText("Created New folder.");

  const folder = item(page, "New folder");
  const depthBefore = await depthOf(item(page, "Melodic Vox"));
  const box = await folder.boundingBox();
  if (!box) throw new Error("the folder is not on screen");
  // The middle of a folder means inside it; its edges mean beside it.
  await item(page, "Melodic Vox").dragTo(folder, {
    targetPosition: { x: 40, y: box.height / 2 },
  });

  await expect(page.getByRole("contentinfo")).toContainText("Moved Melodic Vox.");
  await expect.poll(async () => depthOf(item(page, "Melodic Vox"))).toBe(depthBefore + 1);
});

test("the tree will not be rearranged while rekordbox holds the library", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  // Undraggable rather than refused on the drop: a write that cannot happen
  // should not be offered in the first place.
  await expect(item(page, "Melodic Vox")).not.toHaveAttribute("draggable", "true");
});
