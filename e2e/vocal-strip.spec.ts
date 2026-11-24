import { expect, test, type Locator } from "@playwright/test";

async function intensityBands(canvas: Locator) {
  return canvas.evaluate((element: HTMLCanvasElement) => {
    const context = element.getContext("2d");
    if (!context || !element.width || !element.height) return [];
    const pixels = context.getImageData(0, 0, element.width, element.height).data;
    const kinds = new Set<string>();
    for (let at = 3; at < pixels.length; at += 4) {
      const alpha = pixels[at] ?? 0;
      kinds.add(alpha === 0 ? "silent" : alpha === 255 ? "full" : "partial");
    }
    return [...kinds].sort();
  });
}

test("supported vocal intensities paint the overview and survive resizing", async ({ page }) => {
  const previewPort = (Number(process.env.E2E_PORT) || 1420) + 1;
  await page.goto(`http://localhost:${previewPort}/`);
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();
  const player = page.getByRole("region", { name: "Preview player", exact: true });
  await expect(player.getByTestId("player-title")).not.toHaveText("");
  const canvas = player.getByTestId("player-vocal").locator("canvas");
  await expect(canvas).toBeVisible();
  await expect.poll(() => intensityBands(canvas)).toEqual(["full", "partial", "silent"]);
  await page.setViewportSize({ width: 1500, height: 950 });
  await expect.poll(() => intensityBands(canvas)).toEqual(["full", "partial", "silent"]);
});
