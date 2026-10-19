import { expect, test, type Locator, type Page } from "@playwright/test";

/**
 * Exporting a chosen few tracks or one playlist by dropping it on a stick in
 * the Devices tree (#580), as rekordbox does: "Drag tracks from [Collection]
 * to [the device] in [Devices]" and "Drag a playlist to [Devices]"
 * (rekordbox 7.2.18 manual p. 108). The tree shows one section at a time, so
 * the collection is in the Sub-Browser and the stick in the main tree.
 */

test.use({ viewport: { width: 1800, height: 1130 } });

/** What an export to the TEST stick says once it is done. */
const exported = (page: Page) => page.getByText(/^TEST: Updated/);

/**
 * Drags `row` onto `target` with the drag events themselves:
 * `dragTo` aims at the row's middle, which a stick's row gives to its Eject
 * button.
 */
async function dropRow(page: Page, row: Locator, target: Locator) {
  const transfer = await page.evaluateHandle(() => new DataTransfer());
  await row.dispatchEvent("dragstart", { dataTransfer: transfer });
  await target.dispatchEvent("dragenter", { dataTransfer: transfer });
  await target.dispatchEvent("dragover", { dataTransfer: transfer });
  await target.dispatchEvent("drop", { dataTransfer: transfer });
  await row.dispatchEvent("dragend", { dataTransfer: transfer });
}

async function stickBesideCollection(page: Page, url: string) {
  await page.goto(url);
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.getByRole("button", { name: "Sub-Browser Window" }).click();
  const panel = page.getByRole("region", { name: "Sub-Browser" });
  await expect(panel).toBeVisible();
  await page.getByRole("tablist", { name: "Library sources" }).first().getByRole("tab", { name: "Devices" }).click();
  const stick = page.getByRole("tree").first().locator('[role="treeitem"][data-kind="device"]').filter({ hasText: "TEST" });
  await expect(stick).toBeVisible();
  return { panel, stick };
}

test("tracks dropped on a stick are exported to it, and only those", async ({ page }) => {
  const { panel, stick } = await stickBesideCollection(page, "/?writable=1");
  await panel.getByRole("tree").getByRole("treeitem").filter({ hasText: "All Tracks" }).first().click();
  const rows = panel.getByRole("grid").locator('[role="row"]:has([role="gridcell"])');
  await expect(rows.first()).toBeVisible();

  await dropRow(page, rows.first(), stick);
  await expect(exported(page)).toContainText("Updated 1 track.");
});

test("a playlist dropped on a stick is exported to it, and stays where it was", async ({ page }) => {
  const { panel, stick } = await stickBesideCollection(page, "/?writable=1");
  const playlist = panel.getByRole("tree").locator('[role="treeitem"][data-kind="playlist"]').filter({ hasText: "Melodic Vox" }).first();
  await expect(playlist).toHaveAttribute("draggable", "true");

  const depth = await playlist.getAttribute("style");
  await dropRow(page, playlist, stick);
  await expect(exported(page)).toContainText("Added 1 playlist.");
  await expect(page.getByText(/^Moved /)).toHaveCount(0);
  await expect(playlist).toHaveAttribute("style", depth ?? "");
});

test("a stick's own playlists take no tracks from the collection", async ({ page }) => {
  const { panel } = await stickBesideCollection(page, "/?writable=1");
  await page.getByRole("button", { name: "Expand TEST" }).dispatchEvent("mousedown");
  const onStick = page.locator('[role="treeitem"][data-kind="devicePlaylist"]').first();
  await expect(onStick).toBeVisible();
  await panel.getByRole("tree").getByRole("treeitem").filter({ hasText: "All Tracks" }).first().click();
  const rows = panel.getByRole("grid").locator('[role="row"]:has([role="gridcell"])');
  await dropRow(page, rows.first(), onStick);
  await page.waitForTimeout(500);
  await expect(exported(page)).toHaveCount(0);
});
