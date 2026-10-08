import { expect, test } from "@playwright/test";

/**
 * An XML import reports its progress on the status line and the line is not
 * auto-cleared while the import runs. The mock holds importXml until
 * `window.__finishImport()` so the line can be watched.
 */
test("XML import shows progress and keeps it until the import finishes", async ({ page }) => {
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.waitForFunction(() => "__menu" in window);
  await page.evaluate(() => (window as unknown as { __menu: (id: string) => void }).__menu("import-xml"));
  const note = page.getByText("Importing 1 of 3 tracks…");
  await expect(note).toBeVisible();
  // Longer than the 4 s a plain status note lives for.
  await page.waitForTimeout(4600);
  await expect(note).toBeVisible();
  await page.waitForFunction(() => "__finishImport" in window);
  await page.evaluate(() => (window as unknown as { __finishImport: () => void }).__finishImport());
  await expect(note).toBeHidden();
});
