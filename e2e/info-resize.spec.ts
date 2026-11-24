import { expect, test, type Locator, type Page } from "@playwright/test";

const width = async (element: Locator) => (await element.boundingBox())?.width ?? 0;
const info = (page: Page) => page.getByRole("complementary", { name: "Information" });
const handle = (page: Page) => page.getByRole("separator", { name: "Resize the information panel" });
const storedWidth = (page: Page) => page.evaluate(() =>
  (JSON.parse(localStorage.getItem("rbl.session") ?? "{}") as { infoWidth?: number | null }).infoWidth);

async function open(page: Page) {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)", { timeout: 15_000 });
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).click();
  await page.getByRole("toolbar", { name: "Browser panels" }).getByRole("button", { name: "Information", exact: true }).click();
  await expect(info(page)).toBeVisible();
}

async function drag(page: Page, dx: number) {
  const box = (await handle(page).boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2 + dx, box.y + box.height / 2, { steps: 5 });
  await page.mouse.up();
}

test("Information retains its measured default, resizes below 280px, and remembers keyboard adjustments (#860)", async ({ page }) => {
  await open(page);
  const defaultWidth = await width(info(page));
  const body = await width(page.getByTestId("body"));
  expect(defaultWidth).toBeCloseTo(Math.min(557, body * 0.33), 0);
  await drag(page, defaultWidth - 160);
  await expect.poll(() => width(info(page))).toBeCloseTo(160, 0);
  await handle(page).focus();
  await page.keyboard.press("Shift+ArrowLeft");
  await expect.poll(() => width(info(page))).toBeCloseTo(224, 0);
  await expect.poll(() => storedWidth(page)).toBe(224);
  await page.reload();
  await expect(info(page)).toBeVisible();
  await expect.poll(() => width(info(page))).toBeCloseTo(224, 0);
});

test("Information can exceed half the row while preserving list space and restores its request after a window resize (#860)", async ({ page }) => {
  await open(page);
  await drag(page, -4000);
  const wide = await width(info(page));
  expect(wide).toBeGreaterThan((await width(page.getByTestId("body"))) / 2);
  expect(await width(page.getByTestId("track-scroll").first())).toBeGreaterThanOrEqual(360);
  await page.setViewportSize({ width: 1000, height: 1130 });
  await expect.poll(() => width(info(page))).toBeLessThan(wide);
  await expect.poll(() => width(page.getByTestId("track-scroll").first())).toBeGreaterThanOrEqual(360);
  await page.setViewportSize({ width: 1800, height: 1130 });
  await expect.poll(() => width(info(page))).toBeCloseTo(wide, 0);

  const panels = page.getByRole("toolbar", { name: "Browser panels" });
  await panels.getByRole("button", { name: "Sub-Browser Window", exact: true }).click();
  await expect(page.getByRole("region", { name: "Sub-Browser", exact: true })).toBeVisible();
  await expect.poll(() => width(page.getByTestId("track-scroll").first())).toBeGreaterThanOrEqual(360);
  await panels.getByRole("button", { name: "My Tag", exact: true }).click();
  const myTag = page.getByRole("region", { name: "My Tag", exact: true });
  await expect(myTag).toBeVisible();
  const fixed = await page.evaluate(() => Number.parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--s-mytag-w")));
  expect(await width(myTag)).toBeCloseTo(fixed, 0);
  await expect(handle(page)).toBeHidden();
  await panels.getByRole("button", { name: "Information", exact: true }).click();
  await page.getByRole("banner").getByRole("button", { name: "Settings", exact: true }).click();
  const preferences = page.getByRole("dialog", { name: "Preferences" });
  await preferences.getByRole("tab", { name: "View", exact: true }).click();
  await preferences.getByRole("tab", { name: "Layout", exact: true }).click();
  await preferences.getByRole("button", { name: "Reset panel sizes", exact: true }).click();
  await preferences.getByRole("button", { name: "Close", exact: true }).click();
  await expect.poll(() => storedWidth(page)).toBeNull();
  expect(await width(info(page))).toBeCloseTo(Math.min(557, (await width(page.getByTestId("body"))) * 0.25), 0);
});
