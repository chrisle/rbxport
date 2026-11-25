import { expect, test, type Locator, type Page } from "@playwright/test";

const deck = (page: Page, b = false) => page.getByRole("region", { name: b ? "Preview player B" : "Preview player", exact: true });
async function elapsed(player: Locator) {
  const text = await player.locator('[class*="elapsed"]').textContent();
  const [, minutes, seconds] = /(\d+):(\d+(?:\.\d)?)/.exec(text ?? "") ?? [];
  return Number(minutes) * 60 + Number(seconds);
}
async function seek(player: Locator) {
  const overview = player.getByTestId("player-overview");
  const box = await overview.boundingBox();
  if (!box) throw new Error("Loaded overview is missing");
  await overview.click({ position: { x: box.width * .3, y: box.height / 2 } });
  await expect.poll(() => elapsed(player)).toBeGreaterThan(10);
  return elapsed(player);
}
async function layout(page: Page, name: string) {
  await page.getByRole("button", { name: "Layout", exact: true }).click();
  await page.getByRole("menuitemradio", { name, exact: true }).click();
}
async function open(page: Page) {
  await page.addInitScript(() => {
    localStorage.setItem("rbl.preferences", JSON.stringify({ advanced: { protectLibrary: false } }));
    localStorage.setItem("rbl.columns.v2.collection", JSON.stringify({ order: ["title", "rating", "preview"], widths: {} }));
  });
  const port = (Number(process.env.E2E_PORT) || 1420) + 1;
  await page.goto(`http://localhost:${port}/?missing=7&writable=1`);
  await page.getByRole("treeitem", { name: /All Tracks/ }).click();
  await expect(page.getByTestId("browser-title")).toContainText("All Tracks");
}
const title = (page: Page, at = 0) => page.locator('[data-col="title"][role="gridcell"]').nth(at);

test("Enter and double click restart the loaded track, but a missing load and layout remount do not", async ({ page }) => {
  await open(page);
  const row = title(page);
  await row.click(); await page.keyboard.press("Enter");
  const a = deck(page);
  await expect(a.getByTestId("player-title")).not.toHaveText("");
  await seek(a);
  await row.click(); await page.keyboard.press("Enter");
  await expect.poll(() => elapsed(a)).toBe(0);
  await seek(a);
  await row.dblclick();
  await expect.poll(() => elapsed(a)).toBe(0);
  const kept = await seek(a);
  const rowNode = row.locator('xpath=ancestor::*[@role="row"]');
  const label = await rowNode.locator('[role="radio"][aria-checked="false"]').first().getAttribute("aria-label");
  const rating = rowNode.getByRole("radio", { name: label ?? "", exact: true });
  await rating.click();
  await expect(rating).toBeChecked();
  await expect.poll(() => elapsed(a)).toBe(kept); // Metadata refresh is not Load.
  await title(page, 1).click(); await page.keyboard.press("Enter");
  await expect(page.getByRole("alert")).toContainText("Load error. The file could not be found.");
  await expect.poll(() => elapsed(a)).toBe(kept);
  await layout(page, "FULL BROWSER");
  await layout(page, "1 PLAYER");
  await expect.poll(() => elapsed(a)).toBe(kept);
});

test("explicit same-track loading on B keeps A's paused position", async ({ page }) => {
  await open(page); await layout(page, "2 PLAYER");
  const row = title(page);
  await row.click(); await page.keyboard.press("Enter");
  const a = deck(page); const b = deck(page, true);
  const kept = await seek(a);
  await row.click(); await page.keyboard.press("Shift+Enter");
  await expect(b.getByTestId("player-title")).not.toHaveText("");
  await seek(b);
  await row.click(); await page.keyboard.press("Shift+Enter");
  await expect.poll(() => elapsed(b)).toBe(0);
  await expect.poll(() => elapsed(a)).toBe(kept);
});
