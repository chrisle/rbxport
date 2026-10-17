/**
 * The sub-browser, as the 2026-09-09 capture draws it: a second complete
 * browser beside the main one — its own rail, tree and list — opened from
 * the icon column at the right edge, at the measured widths, holding a
 * selection of its own.
 */
import { expect, test, type Page } from "@playwright/test";

/** Reads a token from the running page, so the test and the app agree. */
async function token(page: Page, name: string): Promise<number> {
  const value = await page.evaluate(
    (n) => getComputedStyle(document.documentElement).getPropertyValue(n),
    name,
  );
  return Number.parseFloat(value);
}

const subToggle = (page: Page) => page.getByRole("button", { name: "Sub-Browser Window" });
const sub = (page: Page) => page.getByRole("region", { name: "Sub-Browser" });
const edgeOf = (page: Page) =>
  sub(page).getByRole("separator", { name: "Resize the sub-browser", exact: true });

// The capture's window: the device presets in playwright.config.ts narrow the
// default viewport to 1280, where the panel is rightly clamped below the
// measured width, so the measurements are checked at the size they were made.
test.use({ viewport: { width: 1800, height: 1130 } });

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
});

test("the icon column is the measured width and the sub-browser's box lights when it is open", async ({ page }) => {
  const rail = page.getByRole("toolbar", { name: "Browser panels" });
  const railBox = await rail.boundingBox();
  expect(railBox?.width).toBeCloseTo(await token(page, "--s-right-rail-w"), 0);
  // Three boxes, My Tag, Information and Sub-Browser: the capture's other
  // two (Related Tracks, Track Suggestion) are left out by request.
  const buttons = rail.getByRole("button");
  await expect(buttons).toHaveCount(3);
  await expect(buttons.nth(0)).toHaveAccessibleName("My Tag");
  await expect(buttons.nth(1)).toHaveAccessibleName("Information");
  await expect(buttons.nth(2)).toHaveAccessibleName("Sub-Browser Window");
  const box = await buttons.nth(2).boundingBox();
  expect(box?.width).toBeCloseTo(await token(page, "--s-right-rail-button"), 0);

  await expect(subToggle(page)).toHaveAttribute("aria-pressed", "false");
  await subToggle(page).click();
  await expect(sub(page)).toBeVisible();
  await expect(subToggle(page)).toHaveAttribute("aria-pressed", "true");
});

test("opening it shows two trees and two lists at the measured widths, and closing gives the main list its width back", async ({ page }) => {
  const mainList = page.getByRole("grid").first();
  const before = await mainList.boundingBox();

  await subToggle(page).click();
  const panel = sub(page);
  await expect(panel).toBeVisible();

  // Two of everything: the sub-browser is a browser, not a second view of
  // the main one.
  await expect(page.getByRole("navigation", { name: "Library" })).toHaveCount(2);
  await expect(page.getByRole("tablist", { name: "Library sources" })).toHaveCount(2);
  await expect(page.getByRole("grid")).toHaveCount(2);
  await expect(panel.getByRole("searchbox", { name: "Search within this track list" })).toBeVisible();

  const panelBox = await panel.boundingBox();
  expect(panelBox?.width).toBeCloseTo(await token(page, "--s-sub-browse-w"), 0);
  const subTree = await panel.getByRole("navigation", { name: "Library" }).boundingBox();
  expect(subTree?.width).toBeCloseTo(await token(page, "--s-sub-tree-w"), 0);
  // Full height: the panel runs the browser row from top to bottom.
  const body = await page.getByTestId("body").boundingBox();
  expect(panelBox?.height).toBeCloseTo(body?.height ?? 0, 0);

  // The main list gave up the width rather than being covered.
  const during = await mainList.boundingBox();
  expect((during?.width ?? 0) + (panelBox?.width ?? 0)).toBeLessThanOrEqual(before?.width ?? 0);

  await subToggle(page).click();
  await expect(panel).toBeHidden();
  const after = await mainList.boundingBox();
  expect(after?.width).toBeCloseTo(before?.width ?? 0, 0);
});

test("selecting in one browser does not move the other", async ({ page }) => {
  await subToggle(page).click();
  const panel = sub(page);
  await expect(panel).toBeVisible();

  const mainTree = page.getByRole("tree").first();
  const subTree = panel.getByRole("tree");
  const mainTitle = page.getByTestId("browser-title").first();
  const subTitle = panel.getByTestId("browser-title");

  // A playlist in the main tree, then a different one in the sub-browser's.
  await mainTree.getByRole("treeitem").filter({ hasText: "Melodic Vox" }).first().click();
  await expect(mainTitle).toContainText("Melodic Vox");
  await expect(subTitle).not.toContainText("Melodic Vox");

  await subTree.getByRole("treeitem").filter({ hasText: "All Tracks" }).first().click();
  await expect(subTitle).toContainText("All Tracks");
  await expect(mainTitle).toContainText("Melodic Vox");

  // And back the other way: the main tree's selection is its own too.
  await mainTree.getByRole("treeitem").filter({ hasText: "All Tracks" }).first().click();
  await expect(mainTitle).toContainText("All Tracks");
  await expect(subTitle).toContainText("All Tracks");
  await subTree.getByRole("treeitem").filter({ hasText: "Melodic Vox" }).first().click();
  await expect(subTitle).toContainText("Melodic Vox");
  await expect(mainTitle).toContainText("All Tracks");

  // Its search is its own as well.
  await panel.getByRole("searchbox", { name: "Search within this track list" }).fill("zzz-nothing-matches");
  await expect(subTitle).toContainText("(0 Tracks)");
  await expect(mainTitle).not.toContainText("(0 Tracks)");
});

test("tracks drag between the two playlist lists", async ({ page }) => {
  await page.goto("/?writable=1");
  await subToggle(page).click();
  const panel = sub(page);
  const main = page.getByRole("grid").first();
  const right = panel.getByRole("grid");
  const mainTree = page.getByRole("tree").first();
  const rightTree = panel.getByRole("tree");
  const mainRows = main.locator('[role="row"]:has([role="gridcell"])');
  const rightRows = right.locator('[role="row"]:has([role="gridcell"])');

  await rightTree.getByRole("treeitem").filter({ hasText: "Hardstyle" }).first().click({ button: "right" });
  await page.getByRole("menu", { name: "Playlist" }).getByRole("menuitem", { name: "Create New Playlist" }).click();
  await rightTree.getByRole("treeitem").filter({ hasText: "New playlist" }).first().click();
  await mainTree.getByRole("treeitem").filter({ hasText: "All Tracks" }).first().click();
  const firstTitle = await mainRows.first().locator('[data-col="title"]').innerText();
  await mainRows.first().dragTo(right);
  await expect(panel.getByTestId("browser-title")).toContainText("New playlist (1 Tracks)");
  await expect(rightRows.first().locator('[data-col="title"]')).toHaveText(firstTitle.trim());

  await mainTree.getByRole("treeitem").filter({ hasText: "Melodic Vox" }).first().click();
  await rightRows.first().dragTo(main);
  await expect(page.getByRole("contentinfo")).toContainText("Added 1 track to Melodic Vox.");
});

test("the right playlist owns its track actions and drives Information", async ({ page }) => {
  await page.goto("/?writable=1");
  await subToggle(page).click();
  const panel = sub(page);
  const main = page.getByRole("grid").first();
  const right = panel.getByRole("grid");
  const mainTree = page.getByRole("tree").first();
  const rightTree = panel.getByRole("tree");
  const mainRows = main.locator('[role="row"]:has([role="gridcell"])');
  const rightRows = right.locator('[role="row"]:has([role="gridcell"])');

  // Make a playlist in the right browser, then populate it from the left.
  // This keeps the regression tied to the same cross-panel drag that exposed
  // the missing actions rather than relying on a pre-existing playlist.
  await rightTree.getByRole("treeitem").filter({ hasText: "Hardstyle" }).first().click({ button: "right" });
  await page.getByRole("menu", { name: "Playlist" }).getByRole("menuitem", { name: "Create New Playlist" }).click();
  await rightTree.getByRole("treeitem").filter({ hasText: "New playlist" }).first().click();
  await mainTree.getByRole("treeitem").filter({ hasText: "All Tracks" }).first().click();
  const title = (await mainRows.first().locator('[data-col="title"]').innerText()).trim();
  await mainRows.first().dragTo(right);
  await expect(panel.getByTestId("browser-title")).toHaveText("New playlist (1 Tracks)");

  // A right-side selection is the active selection for Information.
  await page.getByRole("toolbar", { name: "Browser panels" })
    .getByRole("button", { name: "Information" }).click();
  // The panel loads on first open and narrows the right grid when it lands;
  // clicking before then leaves the title cell part-scrolled, and the next
  // right-click's focus scroll would close the Track menu.
  const info = page.getByRole("complementary", { name: "Information" });
  await expect(info).toBeVisible();
  await rightRows.first().locator('[data-col="title"]').click();
  await expect(info.getByText(title, { exact: true }).first()).toBeVisible();

  // The menu must mutate the right-side source, not silently do nothing or
  // use whichever playlist happens to be open in the left browser.
  // The menu closes on any scroll outside it, and the right grid can still
  // clamp its scroll as the Information panel settles; on a slow runner that
  // closed the menu under the click. Open it again until the choice lands.
  await expect(async () => {
    await rightRows.first().locator('[data-col="title"]').click({ button: "right" });
    await page.getByRole("menu", { name: "Track" })
      .getByRole("menuitem", { name: "Remove from Playlist" }).click({ timeout: 2_000 });
  }).toPass({ timeout: 15_000 });
  await expect(page.getByRole("contentinfo")).toContainText("Removed 1 track.");
  await expect(panel.getByTestId("browser-title")).toHaveText("New playlist (0 Tracks)");
  await expect(rightRows).toHaveCount(0);
  // Dropping into a playlist copied the source; removing from the destination
  // must not remove the collection row on the left.
  await expect(mainRows.first().locator('[data-col="title"]')).toHaveText(title);
});

test("tracks reorder inside the right playlist without changing the left view", async ({ page }) => {
  await page.goto("/?writable=1");
  await subToggle(page).click();
  const panel = sub(page);
  const rightTree = panel.getByRole("tree");
  const rightRows = panel.getByRole("grid").locator('[role="row"]:has([role="gridcell"])');
  const titles = () => rightRows.locator('[data-col="title"]').allTextContents();
  const leftTitle = page.getByTestId("browser-title").first();
  const leftBefore = await leftTitle.innerText();

  await rightTree.getByRole("treeitem").filter({ hasText: "Melodic Vox" }).first().click();
  const before = await titles();
  expect(before.length).toBeGreaterThan(3);
  const target = await rightRows.nth(3).boundingBox();
  if (!target) throw new Error("the right playlist's target row is not visible");
  await rightRows.first().dragTo(rightRows.nth(3), {
    targetPosition: { x: 120, y: target.height - 3 },
  });

  await expect(page.getByRole("contentinfo")).toContainText("Playlist reordered.");
  await expect.poll(async () => (await titles()).slice(0, 4))
    .toEqual([before[1], before[2], before[3], before[0]]);
  // The independently selected main browser stays where it was.
  await expect(leftTitle).toHaveText(leftBefore);
});

test("the sub-browser's splitters move it and its tree within the clamp", async ({ page }) => {
  await subToggle(page).click();
  const panel = sub(page);
  await expect(panel).toBeVisible();
  const edge = edgeOf(page);
  const start = await panel.boundingBox();

  // Drag the left edge 50pt to the left: the panel grows by that much.
  const handle = await edge.boundingBox();
  const x = (handle?.x ?? 0) + (handle?.width ?? 0) / 2;
  const y = (handle?.y ?? 0) + 200;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x - 50, y, { steps: 5 });
  await page.mouse.up();
  const grown = await panel.boundingBox();
  expect(grown?.width).toBeCloseTo((start?.width ?? 0) + 50, 0);

  // Dragging far past the window is clamped, and the main list survives.
  await page.mouse.move(x - 50, y);
  await page.mouse.down();
  await page.mouse.move(x - 3000, y, { steps: 5 });
  await page.mouse.up();
  const clamped = await panel.boundingBox();
  const viewport = page.viewportSize();
  expect(clamped?.width).toBeLessThanOrEqual((viewport?.width ?? 0) * 0.65 + 1);
  await expect(page.getByRole("grid").first()).toBeVisible();

  // The inner splitter resizes the sub-browser's own tree.
  const inner = panel.getByRole("separator", { name: "Resize the sub-browser's tree" });
  const subTree = panel.getByRole("navigation", { name: "Library" });
  const treeBefore = await subTree.boundingBox();
  const innerBox = await inner.boundingBox();
  const ix = (innerBox?.x ?? 0) + (innerBox?.width ?? 0) / 2;
  await page.mouse.move(ix, y);
  await page.mouse.down();
  await page.mouse.move(ix + 60, y, { steps: 5 });
  await page.mouse.up();
  const treeAfter = await subTree.boundingBox();
  expect(treeAfter?.width).toBeCloseTo((treeBefore?.width ?? 0) + 60, 0);
});

test("the information panel opening beside it shrinks it rather than the main list", async ({ page }) => {
  await subToggle(page).click();
  const panel = sub(page);
  await expect(panel).toBeVisible();
  const mainList = page.getByRole("grid").first();
  const before = await mainList.boundingBox();

  const rail = page.getByRole("toolbar", { name: "Browser panels" });
  await rail.getByRole("button", { name: "Information" }).click();
  const info = page.getByRole("complementary", { name: "Information" });
  await expect(info).toBeVisible();

  // The clamp leaves the main list what the tree splitter's rule leaves it,
  // and the panel gives up the rest.
  await expect
    .poll(async () => (await mainList.boundingBox())?.width ?? 0)
    .toBeGreaterThanOrEqual(360);
  const during = await panel.boundingBox();
  expect(during?.width).toBeLessThan(await token(page, "--s-sub-browse-w"));

  await rail.getByRole("button", { name: "Information" }).click();
  await expect(info).toBeHidden();
  await expect
    .poll(async () => (await mainList.boundingBox())?.width ?? 0)
    .toBeCloseTo(before?.width ?? 0, 0);
});

test("the sub-browser's widths come back next run", async ({ page }) => {
  await subToggle(page).click();
  const panel = sub(page);
  await expect(panel).toBeVisible();
  const edge = edgeOf(page);
  const handle = await edge.boundingBox();
  const x = (handle?.x ?? 0) + (handle?.width ?? 0) / 2;
  const y = (handle?.y ?? 0) + 200;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x - 80, y, { steps: 5 });
  await page.mouse.up();
  const dragged = await panel.boundingBox();

  await page.reload();
  await expect(page.getByTestId("browser-title").first()).toContainText("Tracks)");
  await expect(sub(page)).toBeVisible();
  const restored = await sub(page).boundingBox();
  expect(restored?.width).toBeCloseTo(dragged?.width ?? 0, 0);
});
