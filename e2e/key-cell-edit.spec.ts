import { expect, test } from "@playwright/test";

/**
 * The Key column edits in place from a list of the library's keys, since the
 * writer refuses a key it does not hold.
 */
test("a selected row's Key cell opens a list and saves the choice", async ({ page }) => {
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");

  const row = page.getByRole("row").nth(1);
  await row.click();
  const cell = row.locator('[data-col="key"]');
  const before = (await cell.textContent()) ?? "";
  await cell.dblclick();

  const list = row.getByRole("combobox", { name: "Key" });
  await expect(list).toBeVisible();
  const options = await list.locator("option").allTextContents();
  const next = options.find((o) => o !== "" && o !== before);
  expect(next).toBeDefined();
  await list.selectOption({ label: next as string });

  await expect(row.locator('[data-col="key"]')).toHaveText(next as string);
});
