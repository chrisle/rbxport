/**
 * Shift + cursor up/down extend and shrink the track list's selection from
 * the cursor row, as rekordbox 7.2.19's track list does
 * (`browse::CustomListBox::keyPressed`; see `extendSelection` in
 * `src/lib/selection.ts`), and keep the row they reach on screen (#514).
 */
import { expect, test, type Page } from "@playwright/test";

const scroller = (page: Page) => page.getByTestId("track-scroll");
const selectedTitles = (page: Page) =>
  page.locator('[role="row"][aria-selected="true"] [data-col="title"]').allTextContents();

test("shift + cursor keys grow, shrink and follow the selection", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="treeitem"]', { hasText: /^All Tracks/ }).first().click();
  const titles = page.locator('[role="row"] [data-col="title"]');
  await expect.poll(() => titles.count()).toBeGreaterThan(10);

  await titles.nth(2).click();
  await page.keyboard.press("Shift+ArrowDown");
  await page.keyboard.press("Shift+ArrowDown");
  await expect.poll(async () => (await selectedTitles(page)).length).toBe(3);
  const three = await selectedTitles(page);
  expect(three[0]).toBe(await titles.nth(2).textContent());

  // Reversing gives the rows back, down to the clicked one, then grows upward.
  await page.keyboard.press("Shift+ArrowUp");
  await expect.poll(() => selectedTitles(page)).toEqual(three.slice(0, 2));
  await page.keyboard.press("Shift+ArrowUp");
  await page.keyboard.press("Shift+ArrowUp");
  await expect.poll(async () => (await selectedTitles(page)).length).toBe(2);
  expect((await selectedTitles(page))[1]).toBe(three[0]);

  // Taken past the bottom of the window, the list scrolls to keep the row
  // reached on screen.
  await page.keyboard.press("ArrowDown");
  for (let i = 0; i < 80; i++) await page.keyboard.press("Shift+ArrowDown");
  await expect.poll(() => scroller(page).evaluate((el) => el.scrollTop)).toBeGreaterThan(0);
  await expect
    .poll(async () => {
      const view = await scroller(page).boundingBox();
      const last = await page.locator('[role="row"][aria-selected="true"]').last().boundingBox();
      return view !== null && last !== null && last.y >= view.y && last.y + last.height <= view.y + view.height + 1;
    })
    .toBe(true);
});
