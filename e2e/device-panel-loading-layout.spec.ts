import { expect, test, type Page } from "@playwright/test";

async function holdDevicePanel(page: Page) {
  let release = () => {};
  const ready = new Promise<void>((resolve) => { release = resolve; });
  await page.route("**/src/views/devices/DevicePanel.tsx", async (route) => {
    await ready;
    await route.continue();
  });
  return release;
}

async function openDevice(page: Page, side: "main" | "sub") {
  const tree = page.locator(`nav[data-browser-side="${side}"]`);
  await tree.getByRole("tab", { name: "Devices" }).click();
  await tree.getByRole("button", { name: "Expand TEST" }).click();
  await expect(tree.locator('[data-kind="deviceLibrary"]')).toHaveCount(2);
  return tree;
}

for (const width of [1100, 1800]) {
  for (const secondary of [false, true]) {
    test(`device panel reserves its workspace at ${width}px with secondary=${String(secondary)}`, async ({ page }) => {
      await page.setViewportSize({ width, height: 1130 });
      const release = await holdDevicePanel(page);
      try {
        await page.goto("/?writable=1");
        await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
        if (secondary) await page.getByRole("button", { name: "Sub-Browser Window" }).click();
        await openDevice(page, "main");
        const slot = page.getByTestId("body").locator(":scope > :nth-child(3)");
        const before = await slot.boundingBox();
        expect(before).not.toBeNull();
        release();
        await expect(page.getByRole("tablist", { name: "Device settings" })).toBeVisible();
        const after = await slot.boundingBox();
        expect(after).toEqual(before);
      } finally {
        release();
      }
    });
  }
}

test("a secondary device drag survives the main panel becoming ready after press", async ({ page }) => {
  const release = await holdDevicePanel(page);
  try {
    await page.goto("/?writable=1");
    await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
    await page.getByRole("button", { name: "Sub-Browser Window" }).click();
    const sourceTree = await openDevice(page, "sub");
    const targetTree = await openDevice(page, "main");
    await sourceTree.locator('[data-kind="deviceAllTracks"]').first().click();
    const sourceRows = page.getByRole("region", { name: "Sub-Browser" })
      .getByRole("grid").locator('[role="row"]:has([role="gridcell"])');
    await expect(sourceRows).toHaveCount(6);
    const source = sourceRows.first().locator('[data-col="title"]');
    const target = targetTree.locator('[data-kind="devicePlaylist"]')
      .filter({ hasText: /^\s*Melodic Vox/ }).first();
    await source.hover({ position: { x: 8, y: 8 } });
    await page.mouse.down();
    try {
      release();
      await expect(page.getByRole("tablist", { name: "Device settings" })).toBeVisible();
      await target.hover({ position: { x: 100, y: 10 } });
      await target.hover({ position: { x: 110, y: 10 } });
      await expect(target).toHaveAttribute("data-device-track-drop", "true");
      await expect(target).toHaveAttribute("data-over", "true");
    } finally {
      await page.mouse.up();
    }
    await expect(page.getByRole("contentinfo")).toContainText("Added to Melodic Vox.");
    await target.click();
    await expect(page.getByRole("grid").first().locator('[role="row"]:has([role="gridcell"])')).toHaveCount(4);
  } finally {
    release();
  }
});
