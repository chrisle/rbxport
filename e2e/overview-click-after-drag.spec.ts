/**
 * The first click on the overview after a track was dragged onto the deck
 * seeks. WebKit sends no `pointerdown` for the first press after an HTML5
 * drag-and-drop (WebKit bug 202287), and the overview took only pointer
 * events, so on Linux that click did nothing: see `usePress`.
 */
import { expect, test } from "@playwright/test";

test("one click on the overview seeks after a drag-and-drop load", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  const deck = page.getByRole("region", { name: "Preview player", exact: true });
  const title = page.locator('[role="gridcell"][data-col="title"]').first();
  const loaded = await title.innerText();
  await title.dragTo(deck);
  await expect(deck.getByTestId("player-title")).toHaveText(loaded);
  await expect(deck.getByRole("button", { name: "Play", exact: true })).toBeEnabled();

  const overview = deck.getByTestId("player-overview");
  const box = await overview.boundingBox();
  if (!box) throw new Error("no overview");
  const total = Number(await overview.getAttribute("aria-valuemax"));
  expect(total).toBeGreaterThan(0);
  await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);

  // Once, not twice: the second click is what used to land.
  await expect.poll(async () => Number(await overview.getAttribute("aria-valuenow")))
    .toBeGreaterThan(total * 0.4);
});
