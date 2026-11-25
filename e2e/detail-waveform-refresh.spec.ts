import { expect, test } from "@playwright/test";

for (const hidden of [false, true]) {
  test(`detail cache observes analysis while ${hidden ? "the deck is hidden" : "another track is loaded"}`, async ({ page }) => {
    await page.route("**/src/main.tsx*", async route => {
      const response = await route.fetch();
      await route.fulfill({ response, body: `
        import { getBackend as fixtureBackend } from "/src/ipc/client.ts";
        const backend = await fixtureBackend();
        const originalWaveform = backend.trackWaveform;
        const originalSubscribe = backend.onAnalysisChanged;
        const listeners = new Set();
        let target = null, changed = false;
        window.__detailReads = 0;
        backend.trackWaveform = (id, kind, ...rest) => {
          if (kind !== "bandsDetail") return originalWaveform(id, kind, ...rest);
          target ??= id;
          if (id === target) window.__detailReads++;
          return Promise.resolve(new Uint8Array(30000).fill(id === target && changed ? 90 : 15));
        };
        backend.onAnalysisChanged = listener => {
          listeners.add(listener);
          const stop = originalSubscribe(listener);
          return () => { listeners.delete(listener); stop(); };
        };
        window.__changeDetail = () => { changed = true; for (const listener of listeners) listener(target); };
      ` + await response.text() });
    });
    await page.goto("/");
    await expect(page.getByTestId("browser-title").first()).toContainText("Tracks)");
    const row = page.locator('[role="gridcell"][data-col="title"]');
    await row.nth(3).dblclick();
    const player = page.getByRole("region", { name: "Preview player", exact: true });
    const canvas = player.getByTestId("player-detail").locator("canvas").first();
    await expect.poll(() => page.evaluate(() => (window as unknown as { __detailReads: number }).__detailReads)).toBe(1);
    const before = await canvas.evaluate((node: HTMLCanvasElement) => node.toDataURL());
    if (hidden) {
      await page.getByRole("button", { name: "Layout", exact: true }).click();
      await page.getByRole("menuitemradio", { name: "FULL BROWSER", exact: true }).click();
      await expect(player).toHaveCount(0);
    } else {
      await row.nth(4).dblclick();
    }
    await page.evaluate(() => (window as unknown as { __changeDetail: () => void }).__changeDetail());
    if (hidden) {
      await page.getByRole("button", { name: "Layout", exact: true }).click();
      await page.getByRole("menuitemradio", { name: "1 PLAYER", exact: true }).click();
    } else {
      await row.nth(3).dblclick();
    }
    await expect.poll(() => page.evaluate(() => (window as unknown as { __detailReads: number }).__detailReads)).toBe(2);
    await expect.poll(() => canvas.evaluate((node: HTMLCanvasElement) => node.toDataURL())).not.toBe(before);
    // A visible completion has one invalidation authority, followed by refresh.
    await page.evaluate(() => (window as unknown as { __changeDetail: () => void }).__changeDetail());
    await expect.poll(() => page.evaluate(() => (window as unknown as { __detailReads: number }).__detailReads)).toBe(3);
  });
}
