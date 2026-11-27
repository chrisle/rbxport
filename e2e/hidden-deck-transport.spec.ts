/**
 * What a layout switch does to the deck it hides (#548).
 *
 * rekordbox pauses every deck but the first on the way into 1 PLAYER, SIMPLE
 * PLAYER and FULL BROWSER, and leaves it paused where it was when 2 PLAYER
 * comes back; deck 1 plays on through all three [OBS rekordbox 7.2.11
 * static, `PlayerComponent::PlayerPanelL::resized` @0x101d9b6fc; see
 * `docs/reference/layout-hidden-decks.md`].
 */
import { expect, test, type Locator, type Page } from "@playwright/test";

const deck = (page: Page, b = false) => page.getByRole("region", { name: b ? "Preview player B" : "Preview player", exact: true });
const transport = (page: Page, b = false) => page.getByRole("group", { name: b ? "Deck B transport" : "Deck A transport" });

async function elapsed(player: Locator) {
  const text = await player.locator('[class*="elapsed"]').textContent();
  const [, minutes, seconds] = /(\d+):(\d+(?:\.\d)?)/.exec(text ?? "") ?? [];
  return Number(minutes) * 60 + Number(seconds);
}

async function layout(page: Page, name: string) {
  await page.getByRole("button", { name: "Layout", exact: true }).click();
  await page.getByRole("menuitemradio", { name, exact: true }).click();
}

/** Both decks loaded in 2 PLAYER: the first row on A, the second on B. */
async function twoPlayer(page: Page) {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await layout(page, "2 PLAYER");
  await page.locator('[role="gridcell"][data-col="title"]').nth(0).dblclick();
  const cell = page.locator('[role="gridcell"][data-col="title"]').nth(1);
  await cell.click({ button: "right" });
  const menu = page.getByRole("menu", { name: "Track" });
  await menu.getByRole("menuitem", { name: "Load", exact: true }).hover();
  await menu.getByRole("menuitem", { name: "Load track to player 2" }).click();
  const a = deck(page);
  const b = deck(page, true);
  await expect(b.getByTestId("player-title")).not.toHaveText("");
  return { a, b };
}

for (const name of ["1 PLAYER", "SIMPLE PLAYER", "FULL BROWSER"]) {
  test(`${name} pauses deck B where it is and leaves deck A playing`, async ({ page }) => {
    const { a, b } = await twoPlayer(page);
    await transport(page).getByRole("button", { name: "Play", exact: true }).click();
    await transport(page, true).getByRole("button", { name: "Play", exact: true }).click();
    await expect.poll(() => elapsed(a)).toBeGreaterThan(0.5);
    await expect.poll(() => elapsed(b)).toBeGreaterThan(0.5);

    await layout(page, name);
    await page.waitForTimeout(1_000);
    await layout(page, "2 PLAYER");

    // B came back paused, still loaded, and does not move on.
    await expect(transport(page, true).getByRole("button", { name: "Play", exact: true })).toBeVisible();
    await expect(b.getByTestId("player-title")).not.toHaveText("");
    const held = await elapsed(b);
    expect(held).toBeGreaterThan(0.5);
    await page.waitForTimeout(700);
    expect(await elapsed(b)).toBe(held);

    // A was never touched.
    await expect(transport(page).getByRole("button", { name: "Pause", exact: true })).toBeVisible();
    const seen = await elapsed(a);
    await expect.poll(() => elapsed(a)).toBeGreaterThan(seen);
  });
}

test("switching between layouts that both show deck B does not pause it", async ({ page }) => {
  const { b } = await twoPlayer(page);
  await transport(page, true).getByRole("button", { name: "Play", exact: true }).click();
  await expect.poll(() => elapsed(b)).toBeGreaterThan(0.5);
  // The menu's current entry: choosing it again applies the same layout.
  await layout(page, "2 PLAYER");
  await expect(transport(page, true).getByRole("button", { name: "Pause", exact: true })).toBeVisible();
  const seen = await elapsed(b);
  await expect.poll(() => elapsed(b)).toBeGreaterThan(seen);
});
