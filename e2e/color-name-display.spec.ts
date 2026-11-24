import { expect, test } from "@playwright/test";

test("a stored colour label stays literal in German, including the Color choices", async ({ page }) => {
  await page.route("**/src/main.tsx", async route => {
    const response = await route.fetch();
    const fixture = `
      import { getBackend as colorFixture } from "/src/ipc/client.ts";
      const colorBackend = await colorFixture();
      colorBackend.colorNames = async () => ["Play","Red","Orange","Yellow","Green","Aqua","Blue","Purple"];
      const originalColorRows = colorBackend.fetchRows.bind(colorBackend);
      colorBackend.fetchRows = async (...args) => (await originalColorRows(...args)).map(row => ({ ...row, extra: { ...row.extra, color: 1 } }));
    `;
    await route.fulfill({ response, body: fixture + await response.text() });
  });
  await page.goto("/?writable");
  await page.getByRole("columnheader", { name: "Track Title" }).click({ button: "right" });
  await page.getByRole("menu", { name: "Columns" }).getByRole("menuitemcheckbox", { name: "Color", exact: true }).click();
  await page.keyboard.press("Escape");
  const cell = page.locator('[role="gridcell"][data-col="color"]').first();
  await expect(cell).toHaveText("Play");
  await page.evaluate(() => {
    const preferences = JSON.parse(localStorage.getItem("rbl.preferences") ?? "{}");
    localStorage.setItem("rbl.preferences", JSON.stringify({ ...preferences, view: { ...preferences.view, locale: "de" } }));
    window.dispatchEvent(new Event("rbl-preferences"));
  });
  await expect(page.locator("html")).toHaveAttribute("lang", "de");
  await expect(cell).toHaveText("Play");
  await cell.click();
  await cell.click();
  const editor = cell.getByRole("combobox");
  await expect(editor).toBeVisible();
  await expect(editor.locator('option[value="1"]')).toHaveText("Play");
  await editor.press("Escape");
  await expect(cell).toHaveText("Play");
});
