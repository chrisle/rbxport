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

test("a memory loop called plays with the beat loop field unlit and its length unchanged", async ({ page }) => {
  // #352: the field is rekordbox's auto beat loop length. A called memory
  // loop of four beats is not the field's 32-beat loop, so the field stays
  // at 32 and unlit, as rekordbox's LoopController::updateLoopState draws it
  // once doSetCurrentCue has cleared the auto beat loop state.
  await page.goto("/?memoryLoops=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();
  const deck = player(page);
  await expect(deck.getByRole("button", { name: "Play", exact: true })).toBeEnabled();
  for (let i = 0; i < 3; i += 1) await deck.getByRole("button", { name: "Longer loop" }).click();
  await expect(deck.getByRole("button", { name: "32 beat loop" })).toHaveText("32");

  const list = page.getByRole("complementary", { name: "Cue list" });
  await list.locator('[role="button"][data-loop]').first().click();
  await expect(band(page)).toHaveCount(1);
  const field = deck.getByRole("button", { name: "Exit loop" });
  await expect(field).toHaveText("32");
  await expect(field).toHaveAttribute("aria-pressed", "false");

  // A step halves the memory loop and leaves the field's length alone.
  const long = (await band(page).boundingBox())?.width ?? 0;
  await deck.getByRole("button", { name: "Shorter loop" }).click();
  await expect.poll(async () => (await band(page).boundingBox())?.width ?? 0).toBeCloseTo(long / 2, 0);
  await expect(field).toHaveText("32");
  await expect(field).toHaveAttribute("aria-pressed", "false");

  // Left, and AU started from the field: that loop is the field's, and lights it.
  await field.click();
  await expect(band(page)).toHaveCount(0);
  await deck.getByRole("button", { name: "32 beat loop" }).click();
  await expect(deck.getByRole("button", { name: "Exit loop" })).toHaveAttribute("aria-pressed", "true");
});
