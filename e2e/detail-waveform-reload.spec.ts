import { expect, test } from "@playwright/test";

for (const initial of ["empty", "positive"] as const) {
  test(`same-track explicit load recovers ${initial} waveform`, async ({ page }) => {
    await page.route("**/src/main.tsx*", async route => {
      const response = await route.fetch();
      await route.fulfill({ response, body: `
        import { getBackend as waveformProbeBackend } from "/src/ipc/client.ts";
        const backend = await waveformProbeBackend();
        const originalWave = backend.trackWaveform.bind(backend);
        const originalLoad = backend.deckLoad.bind(backend);
        let available = ${initial === "positive"};
        const reads = [], loads = [];
        backend.trackWaveform = (id, kind, ...rest) => {
          if (kind !== "bandsDetail" && kind !== "bands") return originalWave(id, kind, ...rest);
          reads.push({ id, kind, available });
          return Promise.resolve(available ? new Uint8Array(30000).fill(90) : new Uint8Array());
        };
        backend.deckLoad = (...args) => { loads.push(args); return originalLoad(...args); };
        window.waveformReloadProbe = { reads, loads, appear: () => { available = true; } };
      ` + await response.text() });
    });
    await page.goto("/");
    await expect(page.getByTestId("browser-title").first()).toContainText("Tracks)");
    const row = page.locator('[role="gridcell"][data-col="title"]').nth(3);
    await row.dblclick();
    const player = page.getByRole("region", { name: "Preview player", exact: true });
    const canvas = player.getByTestId("player-detail").locator("canvas").first();
    const pixels = () => canvas.evaluate((node: HTMLCanvasElement) => node.toDataURL());
    const state = () => page.evaluate(() => {
      const probe = (window as unknown as { waveformReloadProbe: { reads: {id:string;kind:string;available:boolean}[]; loads: [string,string,number][] } }).waveformReloadProbe;
      return { reads: probe.reads.filter(read => read.id === probe.loads[0]?.[1]), loads: probe.loads.length };
    });
    await expect.poll(async () => (await state()).loads).toBe(1);
    await expect.poll(async () => (await state()).reads.filter(read => read.kind === "bandsDetail").length).toBe(1);
    const before = await pixels();
    const initialReads = (await state()).reads;
    await page.evaluate(() => (window as unknown as { waveformReloadProbe: { appear(): void } }).waveformReloadProbe.appear());
    // Ordinary selection/renders do not issue another read.
    await row.click();
    expect((await state()).reads.filter(read => read.kind === "bandsDetail")).toHaveLength(1);
    await page.keyboard.press("Enter");
    await expect.poll(async () => (await state()).loads).toBe(2);
    for (const kind of ["bands", "bandsDetail"]) {
      await expect.poll(async () => (await state()).reads.filter(read => read.kind === kind).length).toBe(initialReads.filter(read => read.kind === kind).length + (initial === "empty" ? 1 : 0));
    }
    if (initial === "empty") await expect.poll(pixels).not.toBe(before);
    else await expect.poll(pixels).toBe(before);
  });
}
