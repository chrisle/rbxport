import { expect, test, type Page } from "@playwright/test";

const quantize = (page: Page) => page.getByRole("region", { name: "Preview player", exact: true })
  .getByRole("button", { name: "Quantize" });

async function quantizeState(page: Page) {
  await expect(quantize(page)).toHaveAttribute("aria-pressed", "false");
  await expect(quantize(page)).not.toHaveAttribute("data-on");
  await expect.poll(() => page.evaluate(() =>
    JSON.parse(localStorage.getItem("rbl.deckQuantize.v1") ?? "{}") as { a?: boolean },
  )).toMatchObject({ a: false });
  return {
    pressed: await quantize(page).getAttribute("aria-pressed"),
    on: await quantize(page).getAttribute("data-on"),
    saved: await page.evaluate(() => JSON.parse(localStorage.getItem("rbl.deckQuantize.v1") ?? "{}") as { a?: boolean }),
  };
}

test("Q and the Quantize button leave the same visible and saved deck state", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();
  await expect(quantize(page)).toHaveAttribute("aria-pressed", "true");

  await page.keyboard.press("q");
  const fromShortcut = await quantizeState(page);

  await page.evaluate(() => localStorage.removeItem("rbl.deckQuantize.v1"));
  await page.reload();
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();
  await expect(quantize(page)).toHaveAttribute("aria-pressed", "true");

  await quantize(page).click();
  expect(await quantizeState(page)).toEqual(fromShortcut);
});

test("Q and the Quantize button also agree before a track loads", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await expect(quantize(page)).toHaveAttribute("aria-pressed", "true");

  await page.keyboard.press("q");
  const fromShortcut = await quantizeState(page);

  await page.evaluate(() => localStorage.removeItem("rbl.deckQuantize.v1"));
  await page.reload();
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await expect(quantize(page)).toHaveAttribute("aria-pressed", "true");

  await quantize(page).click();
  expect(await quantizeState(page)).toEqual(fromShortcut);
});
