/**
 * Going back to a list returns to where it was left, and Track › Locate
 * track loaded on deck (command + L) finds the deck's track, opening All
 * Tracks when the open list lacks it, as rekordbox 7.2.19 does (#338; the
 * functions read are cited in `src/lib/locateLoaded.ts`).
 */
import { expect, test, type Page } from "@playwright/test";

const scroller = (page: Page) => page.getByTestId("track-scroll");
const treeItem = (page: Page, name: string | RegExp) => page.locator('[role="treeitem"]', { hasText: name }).first();
const scrollTop = (page: Page) => scroller(page).evaluate((el) => el.scrollTop);

async function open(page: Page) {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
}

test("a list opens again where it was left", async ({ page }) => {
  await open(page);
  await treeItem(page, /^All Tracks/).click();
  await expect.poll(() => scroller(page).evaluate((el) => el.scrollHeight - el.clientHeight)).toBeGreaterThan(10_000);
  await scroller(page).evaluate((el) => {
    el.scrollTop = 5000;
  });
  await expect.poll(() => scrollTop(page)).toBe(5000);
  await treeItem(page, "Hardstyle").click();
  await expect.poll(() => scrollTop(page)).toBe(0);
  await treeItem(page, /^All Tracks/).click();
  await expect.poll(() => scrollTop(page)).toBe(5000);
});

test("command + L opens All Tracks on the deck's track when the playlist lacks it", async ({ page }) => {
  await open(page);
  // Deck 1 takes the first track of the playlist that opens first.
  const first = page.locator('[role="gridcell"][data-col="title"]').first();
  const title = (await first.textContent())?.trim() ?? "";
  await first.dblclick();
  // Another playlist, without that track.
  const playlists = page.locator('[role="treeitem"]');
  const count = await playlists.count();
  let opened = false;
  for (let i = count - 1; i >= 0 && !opened; i--) {
    const item = playlists.nth(i);
    const name = (await item.textContent()) ?? "";
    if (/^(All Tracks|Playlists)/.test(name) || (await item.getAttribute("aria-selected")) === "true") continue;
    await item.click();
    await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
    const titles = await page.locator('[role="gridcell"][data-col="title"]').allTextContents();
    opened = titles.length > 0 && !titles.some((t) => t.trim() === title);
  }
  expect(opened).toBe(true);
  await page.keyboard.press("ControlOrMeta+l");
  await expect(treeItem(page, /^All Tracks/)).toHaveAttribute("aria-selected", "true");
  const row = page.locator('[role="row"][aria-selected="true"]');
  await expect(row).toHaveCount(1);
  await expect(row.locator('[data-col="title"]')).toHaveText(title);
});
