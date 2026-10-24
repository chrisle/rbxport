/**
 * A track goes on into its list's next row when it ends, where rekordbox's
 * EXPORT-mode player does: from a playlist in 1 PLAYER, from any list in
 * SIMPLE PLAYER. See `src/lib/continuousPlay.ts` for where that was read.
 */
import { expect, test, type Page } from "@playwright/test";

const player = (page: Page) => page.getByRole("region", { name: "Preview player", exact: true });
const titles = (page: Page) => page.locator('[role="gridcell"][data-col="title"]');

async function openPlaylist(page: Page) {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.getByRole("treeitem", { name: "Melodic Vox" }).click();
  await expect(page.getByTestId("browser-title")).toContainText("Melodic Vox");
}

/** Deck 1's PLAY: on the deck, or in its transport column in 2 PLAYER. */
const play = (page: Page, name = "Play") =>
  page.getByRole("region", { name: "Preview player", exact: true })
    .or(page.getByRole("group", { name: "Deck A transport" }))
    .getByRole("button", { name, exact: true });

/** Puts the head a breath before the end. */
async function seekNearEnd(page: Page) {
  const overview = await player(page).getByTestId("player-overview")
    .or(player(page).getByTestId("simple-player-overview")).boundingBox();
  if (!overview) throw new Error("no overview");
  await page.mouse.click(overview.x + overview.width - 1, overview.y + overview.height / 2);
}

/** Puts the head a breath before the end, then presses PLAY. */
async function playOutTheEnd(page: Page) {
  await seekNearEnd(page);
  await play(page).click();
}

test("the next row of the playlist loads and plays when the track ends", async ({ page }) => {
  await openPlaylist(page);
  const first = await titles(page).nth(0).innerText();
  const second = await titles(page).nth(1).innerText();
  await titles(page).nth(0).dblclick();
  const deck = player(page);
  await expect(deck.getByTestId("player-title")).toHaveText(first);
  await expect(play(page)).toBeEnabled();

  await playOutTheEnd(page);

  await expect(deck.getByTestId("player-title")).toHaveText(second, { timeout: 10_000 });
  await expect(play(page, "Pause")).toBeVisible();
});

for (const layout of ["1 PLAYER", "SIMPLE PLAYER"] as const) {
  test(`a selected group dropped on ${layout} continues after its first track`, async ({ page }) => {
    await openPlaylist(page);
    await page.getByRole("button", { name: "Layout" }).click();
    await page.getByRole("menuitemradio", { name: layout }).click();
    const rows = page.getByRole("row").filter({ has: page.getByRole("gridcell") });
    const first = await titles(page).nth(0).innerText();
    const second = await titles(page).nth(1).innerText();
    await rows.nth(0).click();
    await rows.nth(1).click({ modifiers: ["Shift"] });
    await expect(page.getByText("Selected: 2 Tracks")).toBeVisible();
    const deck = player(page);
    await rows.nth(1).dragTo(deck);
    const deckTitle = layout === "1 PLAYER" ? deck.getByTestId("player-title") : deck.getByTestId("simple-player-title");
    await expect(deckTitle).toHaveText(first);
    await play(page).click();
    const progress = deck.getByTestId("player-overview").or(deck.getByTestId("simple-player-overview"));
    // The title and transport can update before the async deck load finishes.
    // A moving head proves the engine is playing before the seek.
    await expect.poll(async () => Number(await progress.getAttribute("aria-valuenow"))).toBeGreaterThan(0);
    await seekNearEnd(page);

    await expect(deckTitle).toHaveText(second, { timeout: 10_000 });
    await expect(play(page, "Pause")).toBeVisible();
  });
}

test("the last row goes round to the first", async ({ page }) => {
  await openPlaylist(page);
  const count = await titles(page).count();
  const first = await titles(page).nth(0).innerText();
  // The playlist is short enough to be on screen whole.
  await titles(page).nth(count - 1).dblclick();
  const deck = player(page);
  await expect(deck.getByTestId("player-title")).toHaveText(await titles(page).nth(count - 1).innerText());
  await expect(play(page)).toBeEnabled();

  await playOutTheEnd(page);

  await expect(deck.getByTestId("player-title")).toHaveText(first, { timeout: 10_000 });
});

test("a track loaded from the collection stops at its end", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.getByRole("treeitem", { name: "All Tracks" }).click();
  await expect(page.getByTestId("browser-title")).toContainText("All Tracks");
  const loaded = await titles(page).nth(3).innerText();
  await titles(page).nth(3).dblclick();
  const deck = player(page);
  await expect(play(page)).toBeEnabled();

  await playOutTheEnd(page);

  await expect(play(page)).toBeVisible({ timeout: 10_000 });
  await page.waitForTimeout(1500);
  await expect(deck.getByTestId("player-title")).toHaveText(loaded);
});

test("in 2 PLAYER the deck stops at the end, as rekordbox's does", async ({ page }) => {
  await openPlaylist(page);
  await page.getByRole("button", { name: "Layout" }).click();
  await page.getByRole("menuitemradio", { name: "2 PLAYER" }).click();
  const loaded = await titles(page).nth(0).innerText();
  await titles(page).nth(0).dblclick();
  const deck = player(page);
  await expect(deck.getByTestId("player-title")).toHaveText(loaded);
  await expect(play(page)).toBeEnabled();

  await playOutTheEnd(page);

  await expect(play(page)).toBeVisible({ timeout: 10_000 });
  await page.waitForTimeout(1500);
  await expect(deck.getByTestId("player-title")).toHaveText(loaded);
});

test("in SIMPLE PLAYER a track from the collection goes on to the next row", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.getByRole("button", { name: "Layout" }).click();
  await page.getByRole("menuitemradio", { name: "SIMPLE PLAYER" }).click();
  await page.getByRole("treeitem", { name: "All Tracks" }).click();
  await expect(page.getByTestId("browser-title")).toContainText("All Tracks");
  const next = await titles(page).nth(4).innerText();
  await titles(page).nth(3).dblclick();
  await expect(play(page)).toBeEnabled();

  await playOutTheEnd(page);

  await expect(player(page).getByTestId("simple-player-title")).toHaveText(next, { timeout: 10_000 });
  await expect(play(page, "Pause")).toBeVisible();
});
