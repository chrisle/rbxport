import { expect, test, type Locator, type Page } from "@playwright/test";

const positions = (pane: Locator) => pane.locator("[data-loaded]").evaluateAll(nodes => nodes.map(node =>
  node.closest('[role="row"]')?.querySelector('[data-col="trackNo"]')?.textContent));

async function layout(page: Page, name: string) {
  await page.getByRole("button", { name: "Layout", exact: true }).click();
  await page.getByRole("menuitemradio", { name, exact: true }).click();
}

test("confirmed loaded rows survive pause, hidden players and the independent browser", async ({ page }) => {
  await page.addInitScript(() => {
    const layout = JSON.stringify({ order: ["attr", "title", "preview", "artwork"], widths: {} });
    for (const context of ["collection", "playlist", "subBrowser"]) localStorage.setItem(`rbl.columns.v2.${context}`, layout);
  });
  const port = (Number(process.env.E2E_PORT) || 1420) + 1;
  await page.goto(`http://localhost:${port}/`);
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  const main = page.locator('[data-browser-pane="list"]').first();
  const cells = main.locator('[data-col="title"][role="gridcell"]');
  await cells.nth(1).click(); await page.keyboard.press("Enter");
  const markers = main.locator("[data-loaded]");
  await expect(markers).toHaveCount(1);
  await expect(markers.first()).toBeVisible();
  expect(await markers.first().evaluate(node => {
    const style = getComputedStyle(node); const box = node.getBoundingClientRect();
    return { width: box.width, height: box.height, colour: style.backgroundColor };
  })).toEqual({ width: 7, height: 8, colour: "rgb(150, 150, 150)" });
  await expect(markers.first().locator('xpath=ancestor::*[@role="row"]').locator('[data-col="trackNo"]')).toHaveText("2");
  await layout(page, "2 PLAYER");
  await cells.nth(3).click(); await page.keyboard.press("Shift+Enter");
  await expect(markers).toHaveCount(2);
  const b = page.getByRole("group", { name: "Deck B transport", exact: true });
  await b.getByRole("button", { name: "Play", exact: true }).click();
  await expect(markers).toHaveCount(2);
  await b.getByRole("button", { name: "Pause", exact: true }).click();
  await expect(markers).toHaveCount(2);
  await layout(page, "FULL BROWSER");
  await expect(markers).toHaveCount(2);
  await layout(page, "2 PLAYER");
  await expect(markers).toHaveCount(2);
  await page.getByRole("button", { name: "Sub-Browser Window", exact: true }).click();
  const sub = page.getByRole("region", { name: "Sub-Browser", exact: true });
  await sub.getByRole("treeitem").filter({ hasText: "Melodic Vox" }).first().click();
  await expect.poll(() => positions(main)).toEqual(["2", "4"]);
  await expect.poll(() => positions(sub)).toEqual(["2", "4"]);
  await page.getByRole("region", { name: "Preview player", exact: true }).getByRole("button", { name: "Eject", exact: true }).click();
  await expect(markers).toHaveCount(1);
  await expect.poll(() => positions(main)).toEqual(["4"]);
  await expect.poll(() => positions(sub)).toEqual(["4"]);
});
