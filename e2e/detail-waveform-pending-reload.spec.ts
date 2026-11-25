import { expect, test } from "@playwright/test";

for (const scenario of ["load-empty", "load-positive", "selection-empty"] as const) {
  test(`pending detail read with ${scenario}`, async ({ page }) => {
    await page.route("**/src/main.tsx*", async route => {
      const response = await route.fetch();
      await route.fulfill({ response, body: `
        import { getBackend as pendingWaveBackend } from "/src/ipc/client.ts";
        const backend = await pendingWaveBackend();
        const originalWave = backend.trackWaveform.bind(backend);
        const originalLoad = backend.deckLoad.bind(backend);
        let available = ${scenario === "load-positive"};
        let target = null;
        let release;
        const reads = [], loads = [];
        backend.trackWaveform = (id, kind, ...rest) => {
          if (kind !== "bandsDetail") return originalWave(id, kind, ...rest);
          target ??= id;
          reads.push({ id, available });
          const bytes = available ? new Uint8Array(30000).fill(90) : new Uint8Array();
          if (reads.length === 1) return new Promise(resolve => { release = () => resolve(bytes); });
          return Promise.resolve(bytes);
        };
        backend.deckLoad = (...args) => { loads.push(args); return originalLoad(...args); };
        window.pendingWave = { reads, loads, appear: () => { available = true; }, release: () => release() };
      ` + await response.text() });
    });
    await page.goto("/");
    await expect(page.getByTestId("browser-title").first()).toContainText("Tracks)");
    const row = page.locator('[role="gridcell"][data-col="title"]').nth(3);
    await row.dblclick();
    const state = () => page.evaluate(() => {
      const probe = (window as unknown as { pendingWave: { reads: {id:string;available:boolean}[]; loads: unknown[] } }).pendingWave;
      return { reads: probe.reads, loads: probe.loads.length };
    });
    await expect.poll(async () => (await state()).loads).toBe(1);
    await expect.poll(async () => (await state()).reads.length).toBe(1);
    const player = page.getByRole("region", { name: "Preview player", exact: true });
    const canvas = player.getByTestId("player-detail").locator("canvas").first();
    const pixels = () => canvas.evaluate((node: HTMLCanvasElement) => node.toDataURL());
    const blank = await pixels();
    await page.evaluate(() => (window as unknown as { pendingWave: { appear(): void } }).pendingWave.appear());
    await row.click();
    if (scenario !== "selection-empty") {
      await page.keyboard.press("Enter");
      await expect.poll(async () => (await state()).loads).toBe(2);
    }
    expect((await state()).reads).toHaveLength(1);
    await page.evaluate(() => (window as unknown as { pendingWave: { release(): void } }).pendingWave.release());
    if (scenario === "load-empty") {
      await expect.poll(async () => (await state()).reads.length).toBe(2);
      await expect.poll(pixels).not.toBe(blank);
    } else if (scenario === "load-positive") {
      await expect.poll(pixels).not.toBe(blank);
      expect((await state()).reads).toHaveLength(1);
    } else {
      await page.waitForTimeout(100);
      expect((await state()).reads).toHaveLength(1);
      expect(await pixels()).toBe(blank);
      expect((await state()).loads).toBe(1);
    }
  });
}
