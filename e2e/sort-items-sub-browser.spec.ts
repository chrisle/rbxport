import { expect, test, type Locator, type Page } from "@playwright/test";

const original = ["Melodic Vox", "Hardstyle", "Drum and Bass", "Fresh 128s"];
const sorted = ["Drum and Bass", "Fresh 128s", "Hardstyle", "Melodic Vox"];
type Shell = { __menu?: (id: string) => void; __menuEnabled?: Record<string, boolean> };
const native = (page: Page, id: string) => page.evaluate(item => (window as unknown as Shell).__menu?.(item), id);
const enabled = (page: Page) => page.evaluate(() => (window as unknown as Shell).__menuEnabled?.["sort-items"] === true);
const tree = (page: Page, side: "main" | "sub") => page.locator(`[data-browser-pane="tree"][data-browser-side="${side}"]`);
const folder = (owner: Locator) => owner.getByRole("treeitem").filter({ hasText: "CURRENT" }).first();
async function children(owner: Locator) {
  const names = await owner.getByRole("treeitem").evaluateAll(rows => rows.map(row => row.querySelector("span[class*='label']")?.textContent ?? ""));
  return names.slice(names.indexOf("CURRENT") + 1, names.indexOf("USB"));
}
async function open(page: Page) {
  await page.addInitScript(() => localStorage.setItem("rbl.preferences", JSON.stringify({
    advanced: { protectLibrary: false },
    keyboard: { preset: "export", overrides: { sortItems: { key: "y", shiftKey: true, altKey: true } } },
  })));
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.getByRole("button", { name: "Sub-Browser Window", exact: true }).click();
  await expect(page.getByRole("region", { name: "Sub-Browser" })).toBeVisible();
}

for (const focus of ["tree", "list"] as const) {
  test(`secondary ${focus} Sort Items uses its folder and supports Undo`, async ({ page }) => {
    await open(page);
    const main = tree(page, "main");
    const sub = tree(page, "sub");
    await main.getByRole("treeitem").filter({ hasText: "Melodic Vox" }).first().click();
    await folder(sub).click();
    expect(await children(sub)).toEqual(original);
    if (focus === "list") {
      const list = page.getByRole("region", { name: "Sub-Browser" }).locator('[data-testid="track-scroll"]');
      await list.click({ position: { x: 12, y: 12 } });
    }
    await expect.poll(() => enabled(page)).toBe(true);
    await native(page, "sort-items");
    await expect(page.getByRole("contentinfo")).toContainText("Sorted CURRENT.");
    await expect.poll(() => children(sub)).toEqual(sorted);
    await native(page, "undo");
    await expect.poll(() => children(sub)).toEqual(original);
    await native(page, "redo");
    await expect.poll(() => children(sub)).toEqual(sorted);
    // Main still holds an ordinary playlist; sorting it is inapplicable.
    await main.getByRole("treeitem").filter({ hasText: "Hardstyle" }).first().click();
    await expect.poll(() => enabled(page)).toBe(false);
  });
}

test("secondary Sort Items key refuses its latent search, then sorts when cleared", async ({ page }) => {
  await open(page);
  const sub = tree(page, "sub");
  const search = sub.getByRole("searchbox", { name: "Search library tree" });
  await search.fill("CURRENT");
  await folder(sub).click();
  await page.keyboard.press("Shift+Alt+KeyY");
  await expect(page.getByRole("contentinfo")).toContainText("Playlist cannot be edited or deleted while searching.");
  await search.fill("");
  await search.blur();
  expect(await children(sub)).toEqual(original);
  await page.keyboard.press("Shift+Alt+KeyY");
  await expect(page.getByRole("contentinfo")).toContainText("Sorted CURRENT.");
  await expect.poll(() => children(sub)).toEqual(sorted);
});
