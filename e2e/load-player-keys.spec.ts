/**
 * Enter loads the highlighted track onto Player 1 and Shift+Enter onto
 * Player 2 (#610). Player 2 is a deck only in the 2 PLAYER layout, and a
 * deck loads only when a track is highlighted.
 */
import { expect, test, type Page } from "@playwright/test";

const titleCell = (page: Page, n: number) => page.locator('[role="gridcell"][data-col="title"]').nth(n);
const deckA = (page: Page) => page.getByRole("region", { name: "Preview player", exact: true });
const deckB = (page: Page) => page.getByRole("region", { name: "Preview player B" });

async function open(page: Page, twoPlayer: boolean) {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  if (!twoPlayer) return;
  await page.getByRole("button", { name: "Layout" }).click();
  await page.getByRole("menuitemradio", { name: "2 PLAYER" }).click();
}

test("Shift+Enter loads the selected track onto Player 2, and Enter onto Player 1", async ({ page }) => {
  await open(page, true);
  await expect(deckB(page).getByTestId("player-title")).toHaveText("");

  const second = titleCell(page, 1);
  const title = (await second.textContent())?.trim() ?? "";
  await second.click();
  await page.keyboard.press("Shift+Enter");
  await expect(deckB(page).getByTestId("player-title")).toHaveText(title);
  await expect(deckA(page).getByTestId("player-title")).toHaveText("");

  const third = titleCell(page, 2);
  const other = (await third.textContent())?.trim() ?? "";
  await third.click();
  await page.keyboard.press("Enter");
  await expect(deckA(page).getByTestId("player-title")).toHaveText(other);
  await expect(deckB(page).getByTestId("player-title")).toHaveText(title);
});

test("Shift+Enter does nothing in the 1 PLAYER layout", async ({ page }) => {
  await open(page, false);
  await titleCell(page, 1).click();
  await page.keyboard.press("Shift+Enter");
  await expect(deckA(page).getByTestId("player-title")).toHaveText("");
});
