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

test("CUE can preview again after leaving and releasing outside the button", async ({ page }) => {
  await page.goto("/");
  await page.locator('[role="gridcell"][data-col="title"]').first().dblclick();
  const play = page.getByRole("button", { name: "Play", exact: true });
  await expect(play).toBeEnabled();
  await play.click();
  await page.waitForTimeout(500);
  await page.getByRole("button", { name: "Pause", exact: true }).click();
  await page.keyboard.press("c");
  const clock = page.getByTestId("player-time");
  const atCue = await clock.textContent();
  const cue = page.getByRole("button", { name: "Cue", exact: true });
  const box = await cue.boundingBox();
  if (!box) throw new Error("no CUE button");
  const x = box.x + box.width / 2;
  const y = box.y + box.height / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await expect(clock).not.toHaveText(atCue ?? "");
  await page.mouse.move(box.x + box.width + 20, y);
  await expect(play).toBeVisible();
  await expect(clock).toHaveText(atCue ?? "");
  await page.mouse.up();

  // The next press must still preview; the previous press ended off-button.
  await page.mouse.move(x, y);
  await page.mouse.down();
  await expect(clock).not.toHaveText(atCue ?? "");
  await page.mouse.up();
  await expect(play).toBeVisible();
  await expect(clock).toHaveText(atCue ?? "");
});
