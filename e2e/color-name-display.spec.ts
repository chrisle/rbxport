import { expect, test } from "@playwright/test";

test("a stored colour label stays literal in German, including the Color choices", async ({ page }) => {
  await page.route("**/src/main.tsx*", async route => {
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
  await expect(page.getByRole("columnheader", { name: /^Farbe/ })).toBeVisible();
  await expect(cell).toHaveText("Play");
  await cell.click();
  await cell.click();
  const editor = cell.getByRole("combobox");
  await expect(editor).toBeVisible();
  await expect(editor.locator('option[value="1"]')).toHaveText("Play");
  await editor.press("Escape");
  await expect(cell).toHaveText("Play");
});

test("a same-device multi-selection keeps that database's colour comments in Info", async ({ page }) => {
  await page.route("**/src/main.tsx*", async route => {
    const response = await route.fetch();
    const fixture = `
      import { getBackend as selectionFixture } from "/src/ipc/client.ts";
      const selectionBackend = await selectionFixture();
      selectionBackend.colorNames = async source => [source ? source.format === "deviceLibrary" ? "PDB label" : "One label" : "Master label","Red","Orange","Yellow","Green","Aqua","Blue","Purple"];
      const firstColorRecord = await selectionBackend.trackDetails("100000");
      selectionBackend.selectionDetails = async tracks => ({ first: { ...firstColorRecord, id: tracks[0], color: "1" }, count: tracks.length, mixed: [] });
    `;
    await route.fulfill({ response, body: fixture + await response.text() });
  });
  await page.goto("/?writable");
  await page.getByRole("tablist", { name: "Library sources" }).getByRole("tab", { name: "Devices" }).click();
  await page.getByRole("button", { name: "Expand TEST" }).dispatchEvent("mousedown");
  const playlists = page.locator('[role="treeitem"][data-kind="devicePlaylist"]').filter({ hasText: /^\s*Melodic Vox/ });
  const rows = page.getByRole("row").filter({ has: page.getByRole("gridcell") });
  const panel = page.getByRole("complementary", { name: "Information" });
  for (const [index, label] of [[0, "PDB label"], [1, "One label"]] as const) {
    await playlists.nth(index).click();
    await expect(rows).toHaveCount(3);
    await rows.nth(0).locator('[data-col="title"]').click();
    await rows.nth(1).locator('[data-col="title"]').click({ modifiers: ["Shift"] });
    await expect(page.locator('[role="row"][aria-selected="true"]')).toHaveCount(2);
    if (!await panel.isVisible()) await page.getByRole("button", { name: "Information", exact: true }).click();
    await panel.getByRole("tab", { name: "Info", exact: true }).click();
    await expect(panel.locator('#info-field-color option[value="1"]')).toHaveText(label);
  }
});
