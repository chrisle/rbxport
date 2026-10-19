/**
 * The browser's artwork display pattern (#575): rekordbox's two buttons
 * beside the MASTER menu. The whole artwork doubles the row and makes the
 * Artwork cell a square; the choice survives a reload, as rekordbox keeps
 * `ArtworkStatus-TrackList` in its browse settings.
 */
import { expect, test, type Page } from "@playwright/test";

async function firstRowBoxes(page: Page) {
  const row = page.locator('[role="row"][aria-selected]').first();
  await expect(row).toBeVisible();
  const rowBox = await row.boundingBox();
  const cellBox = await row.locator('[data-col="artwork"]').boundingBox();
  expect(rowBox).not.toBeNull();
  expect(cellBox).not.toBeNull();
  return { row: rowBox!, cell: cellBox! };
}

test("the whole-artwork button doubles the rows, squares the Artwork cell, and is kept", async ({ page }) => {
  await page.goto("/");
  const partial = page.getByTestId("artwork-mode-partial");
  const full = page.getByTestId("artwork-mode-full");
  await expect(partial).toHaveAttribute("aria-pressed", "true");
  await expect(full).toHaveAttribute("aria-pressed", "false");
  await expect(partial).toHaveAttribute("aria-label", "Show the upper part of the artwork.");
  await expect(full).toHaveAttribute("aria-label", "Show the entire artwork.");
  const before = await firstRowBoxes(page);
  expect(before.cell.width).toBeGreaterThan(before.row.height);

  await full.click();
  await expect(full).toHaveAttribute("aria-pressed", "true");
  await expect.poll(async () => (await firstRowBoxes(page)).row.height).toBeCloseTo(before.row.height * 2, 0);
  const after = await firstRowBoxes(page);
  expect(after.cell.width).toBeCloseTo(after.row.height, 0);
  expect(after.cell.height).toBeCloseTo(after.row.height, 0);

  await page.reload();
  await expect(page.getByTestId("artwork-mode-full")).toHaveAttribute("aria-pressed", "true");
  expect((await firstRowBoxes(page)).row.height).toBeCloseTo(before.row.height * 2, 0);

  await page.getByTestId("artwork-mode-partial").click();
  await expect.poll(async () => (await firstRowBoxes(page)).row.height).toBeCloseTo(before.row.height, 0);
  // The stored layout's Artwork width comes back with the upper part.
  expect((await firstRowBoxes(page)).cell.width).toBeCloseTo(before.cell.width, 0);
});
