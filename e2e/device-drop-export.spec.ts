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

test("a collection playlist track dropped onto a USB playlist in the main tree is added", async ({ page }) => {
  const { panel } = await stickBesideCollection(page, "/?writable=1&tracks=4");
  await page.getByRole("button", { name: "Expand TEST" }).dispatchEvent("mousedown");
  const onStick = page.locator('[role="treeitem"][data-kind="devicePlaylist"]').first();
  await expect(onStick).toBeVisible();
  await panel.getByRole("tree").locator('[role="treeitem"][data-kind="playlist"]').filter({ hasText: "Melodic Vox" }).first().click();
  const rows = panel.getByRole("grid").locator('[role="row"]:has([role="gridcell"])');
  await dropRow(page, rows.first(), onStick);
  await expect(page.getByRole("contentinfo")).toContainText("Added to NP3-TEST-MP3.");
  await onStick.click();
  await expect(page.getByRole("grid").first().locator('[role="row"]:has([role="gridcell"])')).toHaveCount(3);
});

test("a collection playlist track dropped onto a USB playlist in the sub-browser is added", async ({ page }) => {
  await page.goto("/?writable=1&tracks=4");
  await expect(page.getByTestId("browser-title")).toContainText("Melodic Vox");
  await page.getByRole("button", { name: "Sub-Browser Window" }).click();
  const panel = page.getByRole("region", { name: "Sub-Browser" });
  await expect(panel).toBeVisible();
  await panel.getByRole("tablist", { name: "Library sources" }).getByRole("tab", { name: "Devices" }).click();
  await panel.getByRole("button", { name: "Expand TEST" }).dispatchEvent("mousedown");
  const onStick = panel.locator('[role="treeitem"][data-kind="devicePlaylist"]').first();
  await expect(onStick).toBeVisible();
  const rows = page.getByRole("grid").first().locator('[role="row"]:has([role="gridcell"])');
  await expect(rows.first()).toBeVisible();

  const transfer = await page.evaluateHandle(() => new DataTransfer());
  await rows.first().dispatchEvent("dragstart", { dataTransfer: transfer });
  await expect(onStick).toHaveAttribute("data-device-track-drop", "true");
  await onStick.dispatchEvent("dragenter", { dataTransfer: transfer });
  await onStick.dispatchEvent("dragover", { dataTransfer: transfer });
  await expect(onStick).toHaveAttribute("data-over", "true");
  await onStick.dispatchEvent("drop", { dataTransfer: transfer });
  await rows.first().dispatchEvent("dragend", { dataTransfer: transfer });
  await expect(page.getByRole("contentinfo")).toContainText("Added to NP3-TEST-MP3.");
  await onStick.click();
  await expect(panel.getByRole("grid").locator('[role="row"]:has([role="gridcell"])')).toHaveCount(3);
  const otherLibrary = panel.locator('[role="treeitem"][data-kind="devicePlaylist"]').nth(3);
  await otherLibrary.click();
  await expect(panel.getByRole("grid").locator('[role="row"]:has([role="gridcell"])')).toHaveCount(2);
  await onStick.click();
  await expect(panel.getByRole("grid").locator('[role="row"]:has([role="gridcell"])')).toHaveCount(3);
});

for (const side of ["main", "sub"] as const) {
  test(`a real HTML drag is accepted by the ${side} USB playlist in split view`, async ({ page }) => {
    await page.addInitScript(() => {
      const evidence = { accepted: [] as string[], payloads: [] as string[] };
      Object.assign(window, { deviceDropEvidence: evidence });
      document.addEventListener("dragover", (event) => {
        const row = (event.target as Element).closest('[data-kind="devicePlaylist"]');
        if (row && event.defaultPrevented) {
          evidence.accepted.push(row.closest('[data-browser-side]')?.getAttribute("data-browser-side") ?? "");
        }
      });
      document.addEventListener("drop", (event) => {
        if ((event.target as Element).closest('[data-kind="devicePlaylist"]')) {
          evidence.payloads.push(event.dataTransfer?.getData("text/plain") ?? "");
        }
      });

    });
    await page.goto("/?writable=1&tracks=4");
    await expect(page.getByTestId("browser-title")).toContainText("Melodic Vox");
    await page.getByRole("button", { name: "Sub-Browser Window" }).click();
    const panel = page.getByRole("region", { name: "Sub-Browser" });
    const targetTree = page.locator(`nav[data-browser-side="${side}"]`);
    await targetTree.getByRole("tab", { name: "Devices" }).click();
    await targetTree.getByRole("button", { name: "Expand TEST" }).click();
    const onStick = targetTree.locator('[role="treeitem"][data-kind="devicePlaylist"]').first();
    await expect(onStick).toBeVisible();
    if (side === "main") {
      await panel.getByRole("tree").locator('[data-kind="playlist"]').filter({ hasText: "Melodic Vox" }).first().click();
    }
    const sourceGrid = side === "main" ? panel.getByRole("grid") : page.getByRole("grid").first();
    const row = sourceGrid.locator('[role="row"]:has([role="gridcell"])').first();
    // Hold a native browser drag over the target before releasing, so its
    // offered/highlighted state is observed rather than racing a quick drop.
    await row.locator('[data-col="title"]').hover({ position: { x: 8, y: 8 } });
    await page.mouse.down();
    try {
      await onStick.hover({ position: { x: 100, y: 10 } });
      await onStick.hover({ position: { x: 110, y: 10 } });
      await expect(onStick).toHaveAttribute("data-device-track-drop", "true");
      await expect(onStick).toHaveAttribute("data-over", "true");
    } finally {
      await page.mouse.up();
    }
    await expect(page.getByRole("contentinfo")).toContainText("Added to NP3-TEST-MP3.");
    const evidence = await page.evaluate(() => (window as unknown as {
      deviceDropEvidence: { accepted: string[]; payloads: string[] };
    }).deviceDropEvidence);
    expect(evidence.accepted).toContain(side);
    expect(evidence.payloads).toHaveLength(1);
    expect(evidence.payloads[0]).toMatch(/^\d+$/);
    await onStick.click();
    const targetGrid = side === "main" ? page.getByRole("grid").first() : panel.getByRole("grid");
    await expect(targetGrid.locator('[role="row"]:has([role="gridcell"])')).toHaveCount(3);
    const membership = await page.evaluate(async (payloads) => {
      const moduleUrl = "/src/ipc/client.ts";
      const { getBackend } = await import(moduleUrl) as typeof import("../src/ipc/client");
      const backend = await getBackend();
      const path = (await backend.listDevices()).find((device) => device.name === "TEST")!.path;
      const mapped = await backend.devicePlaylistCollectionTracks(path, "deviceLibrary", payloads);
      const tracks = mapped.filter((id): id is string => id !== null);
      return {
        mapped,
        target: await backend.devicePlaylistDuplicates(path, "deviceLibrary", "3", tracks),
        other: await backend.devicePlaylistDuplicates(path, "oneLibrary", "3", tracks),
      };
    }, evidence.payloads);
    expect(membership.mapped).toHaveLength(1);
    expect(membership.mapped[0]).not.toBeNull();
    expect(membership.target).toEqual(membership.mapped);
    expect(membership.other).toEqual([]);
    const otherLibrary = targetTree.locator('[data-kind="devicePlaylist"]').nth(3);
    await otherLibrary.click();
    await expect(targetGrid.locator('[role="row"]:has([role="gridcell"])')).toHaveCount(2);
    await onStick.click();
    await expect(targetGrid.locator('[role="row"]:has([role="gridcell"])')).toHaveCount(3);
  });
}
