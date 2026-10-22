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

test("each mode keeps its own columns, the whole artwork's opening with Summary (#575)", async ({ page }) => {
  // rekordbox 7.2.19 keeps a `-FullArtwork` header layout per table, whose
  // default puts Summary after the Artwork and hides Track Title, Artist,
  // BPM and Key [OBS static, kSmallArtworkHeaderDefault].
  await page.goto("/");
  const main = page.locator('[data-browser-pane="list"][data-browser-side="main"]');
  // A heading's accessible name ends with its resize grip's ("Resize ...").
  const heading = (name: string) => main.getByRole("columnheader", { name: new RegExp(`^${name}( Resize|$)`) });
  await expect(heading("Track Title")).toBeVisible();
  await expect(heading("Summary")).toHaveCount(0);

  await page.getByTestId("artwork-mode-full").click();
  await expect(heading("Summary")).toBeVisible();
  await expect(heading("Track Title")).toHaveCount(0);
  const summary = main.locator('[role="row"][aria-selected] [data-col="summary"]').first();
  await expect(summary).toBeVisible();
  // Two lines, the second a size smaller.
  const [title, line] = await summary.locator(":scope > span").evaluateAll((spans) =>
    spans.map((span) => Number.parseFloat(getComputedStyle(span).fontSize)));
  expect(line).toBeCloseTo((title ?? 0) - 2, 1);

  await page.getByTestId("artwork-mode-partial").click();
  await expect(heading("Track Title")).toBeVisible();
  await expect(heading("Summary")).toHaveCount(0);
});

test("the sub-browser has its own pair of artwork buttons (#575)", async ({ page }) => {
  // `SubTrackListHeader` makes both buttons and places them before its
  // search box; its mode is its own [OBS static, rekordbox 7.2.19].
  await page.goto("/");
  await page.getByRole("button", { name: "Sub-Browser Window" }).click();
  const panel = page.getByRole("region", { name: "Sub-Browser" });
  const subFull = panel.getByTestId("artwork-mode-full");
  await expect(subFull).toHaveAttribute("aria-pressed", "false");
  await subFull.click();
  await expect(subFull).toHaveAttribute("aria-pressed", "true");
  await expect(panel.getByRole("columnheader", { name: /^Summary( Resize|$)/ })).toBeVisible();
  // The main browser's stays where it was.
  const mainFull = page.locator('[data-browser-pane="list"][data-browser-side="main"]').getByTestId("artwork-mode-full");
  await expect(mainFull).toHaveAttribute("aria-pressed", "false");
});
