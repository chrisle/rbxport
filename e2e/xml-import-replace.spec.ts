import { expect, test, type Page } from "@playwright/test";

/**
 * Issue #152: importing an xml whose lists already stand in the library asks
 * rekordbox's OK/Cancel question under the title "Import" [OBS static,
 * rekordbox 7.2.19 `TreeViewer::treeMessageImportPlaylistFromBridge`
 * @0x101569848]. Cancel imports nothing; OK goes on with the import. The mock
 * asks when `window.__xmlSameNamed` names lists, answers with
 * `window.__confirmAnswer`, and sets `window.__xmlImportStarted` once it
 * imports.
 */
type Page$ = {
  __menu: (id: string) => void;
  __xmlSameNamed?: string[];
  __xmlImportStarted?: boolean;
  __confirmAnswer?: boolean;
  __confirmed?: string[];
  __confirmTitles?: (string | null)[];
  __confirmLabels?: (string | null)[];
  __finishImport?: () => void;
};

const QUESTION = "One or several lists with the same name already exist.\nDo you want to replace them with the one you're importing?";

async function importXml(page: Page, answer: boolean) {
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.waitForFunction(() => "__menu" in window);
  await page.evaluate((ok) => {
    const w = window as unknown as Page$;
    w.__xmlSameNamed = ["Sets"];
    w.__confirmAnswer = ok;
    w.__menu("import-xml");
  }, answer);
  await page.waitForFunction(() => ((window as unknown as Page$).__confirmed ?? []).length > 0);
  return page.evaluate(() => {
    const w = window as unknown as Page$;
    return { asked: w.__confirmed, titles: w.__confirmTitles, labels: w.__confirmLabels };
  });
}

test("Cancel on the replace question imports nothing", async ({ page }) => {
  const { asked, titles, labels } = await importXml(page, false);
  expect(asked).toEqual([QUESTION]);
  expect(titles).toEqual(["Import"]);
  expect(labels).toEqual(["OK/Cancel"]);
  await expect(page.getByText("Importing 1 of 3 tracks…")).toHaveCount(0);
  expect(await page.evaluate(() => (window as unknown as Page$).__xmlImportStarted ?? false)).toBe(false);
});

test("OK on the replace question goes on with the import", async ({ page }) => {
  const { asked } = await importXml(page, true);
  expect(asked).toEqual([QUESTION]);
  await expect(page.getByText("Importing 1 of 3 tracks…")).toBeVisible();
  expect(await page.evaluate(() => (window as unknown as Page$).__xmlImportStarted)).toBe(true);
  await page.waitForFunction(() => "__finishImport" in window);
  await page.evaluate(() => (window as unknown as Page$).__finishImport?.());
});
