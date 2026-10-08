/**
 * The beat loop of the one-deck layout. The two-deck layout's control row
 * drives the same loop with the same handlers: see `two-player.spec.ts`.
 */
import { expect, test, type Page } from "@playwright/test";

const player = (page: Page) => page.getByRole("region", { name: "Preview player" });
const band = (page: Page) =>
  page.getByTestId("player-overview").locator('[class*="loopBand"][data-active]');

async function load(page: Page) {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();
  await expect(player(page).getByRole("button", { name: "Play", exact: true })).toBeEnabled();
}

test("a step changes the length of a playing loop at once, with no exit and re-entry", async ({ page }) => {
  await load(page);
  const deck = player(page);
  await deck.getByRole("button", { name: "4 beat loop" }).click();
  const exit = deck.getByRole("button", { name: "Exit loop" });
  await expect(exit).toHaveText("4");
  const long = (await band(page).boundingBox())?.width ?? 0;
  expect(long).toBeGreaterThan(0);

  await deck.getByRole("button", { name: "Shorter loop" }).click();
  await expect(exit).toHaveText("2");
  await expect(exit).toHaveAttribute("aria-pressed", "true");
  await expect.poll(async () => (await band(page).boundingBox())?.width ?? 0).toBeCloseTo(long / 2, 0);

  await deck.getByRole("button", { name: "Longer loop" }).click();
  await deck.getByRole("button", { name: "Longer loop" }).click();
  await expect(exit).toHaveText("8");
  await expect.poll(async () => (await band(page).boundingBox())?.width ?? 0).toBeCloseTo(long * 2, 0);
});

test("with no loop playing, a step only sets the length of the next one", async ({ page }) => {
  await load(page);
  const deck = player(page);
  await deck.getByRole("button", { name: "Longer loop" }).click();
  await expect(deck.getByRole("button", { name: "8 beat loop" })).toHaveAttribute("aria-pressed", "false");
  await expect(band(page)).toHaveCount(0);
});
