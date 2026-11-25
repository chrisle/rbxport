import { expect, test } from "@playwright/test";

test("secondary device selections retain their colour owner without replacing the main selection", async ({ page }) => {
  await page.route("**/src/main.tsx*", async route => {
    const response = await route.fetch();
    // Presentation contract only: native device multi-details are not supported.
    // Keep the existing synthetic selection record separate from owner routing.
    const fixture = `
      import { getBackend as ownerFixture } from "/src/ipc/client.ts";
      const ownerBackend = await ownerFixture();
      ownerBackend.colorNames = async source => [source ? source.format === "deviceLibrary" ? "PDB owner" : "One owner" : "Master owner","Red","Orange","Yellow","Green","Aqua","Blue","Purple"];
      const firstRecord = await ownerBackend.trackDetails("100000");
      ownerBackend.selectionDetails = async tracks => ({ first: { ...firstRecord, id: tracks[0], color: "1" }, count: tracks.length, mixed: [] });
    `;
    await route.fulfill({ response, body: fixture + await response.text() });
  });
  await page.goto("/?writable");
  const main = page.getByTestId("track-scroll").first();
  const mainRows = main.getByRole("row").filter({ has: page.getByRole("gridcell") });
  await mainRows.nth(0).locator('[data-col="title"]').click();
  await mainRows.nth(1).locator('[data-col="title"]').click({ modifiers: ["Shift"] });
  await page.evaluate(() => (window as unknown as { __menu: (id: string) => void }).__menu("info"));
  const info = page.getByRole("complementary", { name: "Information" });
  await expect(info.locator('#info-field-color option[value="1"]')).toHaveText("Master owner");

  await page.getByRole("button", { name: "Sub-Browser Window" }).click();
  const secondary = page.getByRole("region", { name: "Sub-Browser" });
  await secondary.getByRole("tablist", { name: "Library sources" }).getByRole("tab", { name: "Devices" }).click();
  await secondary.getByRole("button", { name: "Expand TEST" }).dispatchEvent("mousedown");
  const playlists = secondary.locator('[role="treeitem"][data-kind="devicePlaylist"]').filter({ hasText: /^\s*Melodic Vox/ });
  const rows = secondary.getByRole("row").filter({ has: page.getByRole("gridcell") });
  for (const [index, label] of [[0, "PDB owner"], [1, "One owner"]] as const) {
    await playlists.nth(index).click();
    await expect(rows).toHaveCount(3);
    await rows.nth(0).locator('[data-col="title"]').click();
    await rows.nth(1).locator('[data-col="title"]').click({ modifiers: ["Shift"] });
    await expect(secondary.locator('[role="row"][aria-selected="true"]')).toHaveCount(2);
    await expect(main.locator('[role="row"][aria-selected="true"]')).toHaveCount(2);
    await expect(info.locator('#info-field-color option[value="1"]')).toHaveText(label);
    // Keyboard selection stays in the last focused secondary list as well.
    await page.keyboard.press("ArrowUp");
    await expect(secondary.locator('[role="row"][aria-selected="true"]')).toHaveCount(1);
    await page.keyboard.press("Shift+ArrowDown");
    await expect(secondary.locator('[role="row"][aria-selected="true"]')).toHaveCount(2);
    await expect(main.locator('[role="row"][aria-selected="true"]')).toHaveCount(2);
    await expect(info.locator('#info-field-color option[value="1"]')).toHaveText(label);
  }
  await mainRows.nth(0).locator('[data-col="title"]').click();
  await mainRows.nth(1).locator('[data-col="title"]').click({ modifiers: ["Shift"] });
  await expect(info.locator('#info-field-color option[value="1"]')).toHaveText("Master owner");
});
