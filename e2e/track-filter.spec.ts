/**
 * The Track Filter: toggled from the browser header, narrowing done in the
 * backend, RST putting everything back.
 *
 * Against the mock, whose filter semantics mirror `rbl-index`'s. The counts
 * come from the browser title, which is the backend's own row count for the
 * view — the frontend never has the rows to count.
 */
import { expect, test, type Page } from "@playwright/test";

/** The row count out of "Name (N Tracks)". */
async function trackCount(page: Page): Promise<number> {
  const title = page.getByTestId("browser-title");
  await expect(title).toContainText("Tracks");
  const text = (await title.textContent()) ?? "";
  return Number(/\((\d+) Tracks\)/.exec(text)?.[1] ?? "-1");
}

test("the header button shows the bar and hides it again, and the choice survives a reload", async ({ page }) => {
  await page.goto("/");
  const toggle = page.getByTestId("filter-toggle");
  const bar = page.getByTestId("track-filter");
  await expect(bar).toHaveCount(0);
  await expect(toggle).toHaveAttribute("aria-pressed", "false");

  await toggle.click();
  await expect(bar).toBeVisible();
  await expect(toggle).toHaveAttribute("aria-pressed", "true");
  // The eight columns of the capture: BPM, KEY, RATING, COLOR and four tags.
  await expect(bar.getByRole("checkbox")).toHaveCount(8);
  await expect(bar.getByTestId("filter-tag-column")).toHaveCount(4);
  // The bar sits between the header and the column header.
  const barBox = await bar.boundingBox();
  const head = await page.getByRole("columnheader").first().boundingBox();
  expect(barBox).not.toBeNull();
  expect(head).not.toBeNull();
  expect(head!.y).toBeGreaterThanOrEqual(barBox!.y + barBox!.height - 1);

  await page.reload();
  await expect(page.getByTestId("track-filter")).toBeVisible();

  await page.getByTestId("filter-toggle").click();
  await expect(page.getByTestId("track-filter")).toHaveCount(0);
});

test("ticking BPM and picking a value narrows the list; RST clears it", async ({ page }) => {
  await page.goto("/");
  const all = await trackCount(page);
  expect(all).toBeGreaterThan(0);

  await page.getByTestId("filter-toggle").click();
  const bar = page.getByTestId("track-filter");
  // The value lists come from the backend, so they take a moment to fill.
  const bpmList = bar.getByRole("listbox").first();
  await expect(bpmList.getByRole("option").nth(1)).toBeVisible();

  // Ticked with `All` picked: nothing changes.
  await bar.getByRole("checkbox", { name: "BPM" }).click();
  await expect(page.getByTestId("browser-title")).toContainText(`(${all} Tracks)`);

  // A value: the list narrows to that whole BPM.
  const first = bpmList.getByRole("option").nth(1);
  const picked = (await first.textContent()) ?? "";
  await first.click();
  await expect(first).toHaveAttribute("aria-selected", "true");
  await expect
    .poll(() => trackCount(page))
    .toBeLessThan(all);
  const narrowed = await trackCount(page);
  expect(narrowed).toBeGreaterThan(0);
  // Every row on screen carries the picked BPM.
  const cells = page.locator('[role="gridcell"][data-col="bpm"]');
  await expect(cells.first()).toBeVisible();
  for (const text of await cells.allTextContents()) {
    expect(text.startsWith(picked)).toBe(true);
  }

  // A second column narrows further: rating, five stars only.
  await bar.getByRole("checkbox", { name: "Rating" }).click();
  await bar.getByRole("option", { name: "5 of 5" }).click();
  await expect.poll(() => trackCount(page)).toBeLessThan(narrowed);

  // A colour too.
  await bar.getByRole("checkbox", { name: "Color" }).click();
  await bar.getByRole("option", { name: "Red" }).click();
  const withColour = await trackCount(page);
  expect(withColour).toBeLessThan(narrowed);

  // RST: every column unticked and the whole list back.
  await bar.getByRole("button", { name: "Reset" }).click();
  await expect.poll(() => trackCount(page)).toBe(all);
  for (const box of await bar.getByRole("checkbox").all()) {
    await expect(box).toHaveAttribute("aria-checked", "false");
  }
});

test("hiding the bar puts the whole list back", async ({ page }) => {
  await page.goto("/");
  const all = await trackCount(page);
  await page.getByTestId("filter-toggle").click();
  const bar = page.getByTestId("track-filter");
  await bar.getByRole("checkbox", { name: "Rating" }).click();
  await bar.getByRole("option", { name: "0 of 5" }).click();
  await expect.poll(() => trackCount(page)).toBeLessThan(all);

  await page.getByTestId("filter-toggle").click();
  await expect.poll(() => trackCount(page)).toBe(all);
});

test("the ± list is greyed until a deck is loaded", async ({ page }) => {
  await page.goto("/");
  await page.getByTestId("filter-toggle").click();
  const bar = page.getByTestId("track-filter");
  const pct = bar.getByRole("listbox").nth(1);
  await expect(pct).toHaveAttribute("aria-disabled", "true");
  // Picking a BPM gives the tolerance something to centre on without a deck.
  await bar.getByRole("listbox").first().getByRole("option").nth(1).click();
  await expect(pct).not.toHaveAttribute("aria-disabled", "true");
  await bar.getByRole("option", { name: "All" }).first().click();
  await expect(pct).toHaveAttribute("aria-disabled", "true");

  // So does loading a track: the MASTER PLAYER then has a BPM. Done with the
  // bar closed, because at the test viewport the bar leaves the list one row
  // tall and the first row sits under the sticky column header.
  await page.getByTestId("filter-toggle").click();
  await page.locator('[role="gridcell"][data-col="title"]').first().dblclick();
  await page.getByTestId("filter-toggle").click();
  await expect(page.getByTestId("track-filter").getByRole("listbox").nth(1)).not.toHaveAttribute(
    "aria-disabled",
    "true",
  );
});

test("a My Tag column's chips can be picked, and OR widens what AND narrows", async ({ page }) => {
  await page.goto("/");
  const all = await trackCount(page);
  await page.getByTestId("filter-toggle").click();
  const bar = page.getByTestId("track-filter");
  const column = bar.getByTestId("filter-tag-column").first();
  await expect(column.getByRole("option").first()).toBeVisible();

  await column.getByRole("checkbox").click();
  await column.getByRole("option", { name: "Peak" }).click();
  await expect(column.getByRole("option", { name: "Peak" })).toHaveAttribute("aria-selected", "true");
  const peak = await trackCount(page);
  expect(peak).toBeGreaterThan(0);
  expect(peak).toBeLessThan(all);

  // The mock holds Peak on some tracks and Warm-up on none: AND with it matches
  // nothing, OR leaves the Peak tracks.
  await column.getByRole("option", { name: "Warm-up" }).click({ modifiers: ["ControlOrMeta"] });
  await expect(page.getByTestId("browser-title")).toContainText("(0 Tracks)");
  await column.getByRole("button", { name: "OR" }).click();
  await expect(page.getByTestId("browser-title")).toContainText(`(${peak} Tracks)`);

  await bar.getByRole("button", { name: "Reset" }).click();
  await expect(page.getByTestId("browser-title")).toContainText(`(${all} Tracks)`);
});
