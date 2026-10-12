/**
 * The 1 PLAYER track skip buttons (issue #341). rekordbox 7.2.18 manual p.68:
 * the next one moves to the next track; the previous one to the beginning of
 * the track playing, and clicked twice to the previous track. Rows are picked
 * as `ListViewer::getNextTrackIndex` does: see `src/lib/continuousPlay.ts`.
 */
import { expect, test, type Page } from "@playwright/test";

const player = (page: Page) => page.getByRole("region", { name: "Preview player", exact: true });
const titles = (page: Page) => page.locator('[role="gridcell"][data-col="title"]');
const deckTitle = (page: Page) => player(page).getByTestId("player-title");
const skip = (page: Page, name: "Previous track" | "Next track") =>
  player(page).getByRole("button", { name, exact: true });
const elapsed = (page: Page) => player(page).locator('[class*="elapsed"]');
async function elapsedSeconds(page: Page): Promise<number> {
  const text = (await elapsed(page).textContent()) ?? "";
  const [, minutes, seconds] = /(\d+):(\d+(?:\.\d)?)/.exec(text) ?? [];
  return Number(minutes) * 60 + Number(seconds);
}

async function loadRow(page: Page, row: number) {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.getByRole("treeitem", { name: "Melodic Vox" }).click();
  await expect(page.getByTestId("browser-title")).toContainText("Melodic Vox");
  const title = await titles(page).nth(row).innerText();
  await titles(page).nth(row).dblclick();
  await expect(deckTitle(page)).toHaveText(title);
  await expect(player(page).getByRole("button", { name: "Play", exact: true })).toBeEnabled();
}

test("next loads the row after, and the last row goes round to the first", async ({ page }) => {
  await loadRow(page, 0);
  const second = await titles(page).nth(1).innerText();
  await skip(page, "Next track").click();
  await expect(deckTitle(page)).toHaveText(second);
  // A paused deck stays paused.
  await expect(player(page).getByRole("button", { name: "Play", exact: true })).toBeVisible();

  const count = await titles(page).count();
  const first = await titles(page).nth(0).innerText();
  await titles(page).nth(count - 1).dblclick();
  await expect(deckTitle(page)).toHaveText(await titles(page).nth(count - 1).innerText());
  await expect(player(page).getByRole("button", { name: "Play", exact: true })).toBeEnabled();
  await skip(page, "Next track").click();
  await expect(deckTitle(page)).toHaveText(first);
});

test("previous at the start loads the row before, the first going round to the last", async ({ page }) => {
  await loadRow(page, 1);
  const first = await titles(page).nth(0).innerText();
  await skip(page, "Previous track").click();
  await expect(deckTitle(page)).toHaveText(first);

  const count = await titles(page).count();
  const last = await titles(page).nth(count - 1).innerText();
  await expect(player(page).getByRole("button", { name: "Play", exact: true })).toBeEnabled();
  await skip(page, "Previous track").click();
  await expect(deckTitle(page)).toHaveText(last);
});

test("previous once a second has played goes back to the start, and again to the row before", async ({ page }) => {
  await loadRow(page, 1);
  const loaded = await deckTitle(page).innerText();
  const first = await titles(page).nth(0).innerText();
  await player(page).getByRole("button", { name: "Play", exact: true }).click();
  await expect.poll(() => elapsedSeconds(page), { timeout: 5_000 }).toBeGreaterThan(1.5);

  await skip(page, "Previous track").click();
  await expect(deckTitle(page)).toHaveText(loaded);
  // Still playing, from the top.
  await expect(player(page).getByRole("button", { name: "Pause", exact: true })).toBeVisible();
  await expect.poll(() => elapsedSeconds(page)).toBeLessThan(1);

  await skip(page, "Previous track").click();
  await expect(deckTitle(page)).toHaveText(first);
  // A playing deck plays on into the row it loads.
  await expect(player(page).getByRole("button", { name: "Pause", exact: true })).toBeVisible({ timeout: 10_000 });
});

test("an empty deck has nothing to skip from", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await expect(skip(page, "Previous track")).toBeDisabled();
  await expect(skip(page, "Next track")).toBeDisabled();
});
