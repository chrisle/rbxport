import { expect, test } from "@playwright/test";

interface HeightComparison {
  height: number;
  cssWidth: number;
  matches: boolean;
}
interface HeightFixture {
  __heightReads: number;
  __heightComparison: (canvas: HTMLCanvasElement) => Promise<HeightComparison>;
}

test("whole-artwork previews use native-height bitmaps and cache both sizes", async ({ page }) => {
  await page.addInitScript(() => {
    const layout = JSON.stringify({ order: ["title", "preview"], widths: { preview: 160 } });
    localStorage.setItem("rbl.columns.v2.collection", layout);
    localStorage.setItem("rbl.columns.v2.collection.full", layout);
    localStorage.setItem("rbl.preferences", JSON.stringify({ view: { previewCueMarkers: false } }));
  });
  await page.route("**/src/main.tsx*", async route => {
    const response = await route.fetch();
    const fixture = `
      import { getBackend as fixtureBackend } from "/src/ipc/client.ts";
      import { renderPreview as fixtureRender } from "/src/canvas/waveform.ts";
      const backend = await fixtureBackend();
      const data = new Uint8Array(3600);
      for (let i = 0; i < 1200; i++) {
        data[i * 3] = 31 + i % 40;
        data[i * 3 + 1] = 5 + i % 20;
        data[i * 3 + 2] = 3 + i % 10;
      }
      let target = null;
      window.__heightReads = 0;
      backend.trackWaveform = (id, kind) => {
        target ??= id;
        if (id === target && kind === "bands") window.__heightReads++;
        return Promise.resolve(data);
      };
      window.__heightComparison = async canvas => {
        const box = canvas.getBoundingClientRect();
        const rendered = await fixtureRender(data, box.width, box.height, devicePixelRatio, "3band", true);
        if (!rendered) throw new Error("The generated preview could not be rendered.");
        const fresh = document.createElement("canvas");
        fresh.width = canvas.width;
        fresh.height = canvas.height;
        fresh.getContext("2d").drawImage(rendered.bitmap, 0, 0, fresh.width, fresh.height);
        const actual = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data;
        const expected = fresh.getContext("2d").getImageData(0, 0, fresh.width, fresh.height).data;
        if ("close" in rendered.bitmap) rendered.bitmap.close();
        return {
          height: canvas.height,
          cssWidth: box.width,
          matches: actual.every((value, index) => value === expected[index]),
        };
      };
    `;
    await route.fulfill({ response, body: fixture + await response.text() });
  });
  await page.goto("/");
  await expect(page.getByTestId("browser-title").first()).toContainText("Tracks)");
  const canvas = page.locator("canvas[data-preview-wave]").first();
  const compare = () => canvas.evaluate((node: HTMLCanvasElement) =>
    (window as unknown as HeightFixture).__heightComparison(node));
  const reads = () => page.evaluate(() => (window as unknown as HeightFixture).__heightReads);

  await expect.poll(async () => (await compare()).matches).toBe(true);
  const partial = await compare();
  await page.getByTestId("artwork-mode-full").click();
  await expect.poll(async () => (await compare()).height).toBeGreaterThan(partial.height);
  await expect.poll(async () => (await compare()).matches).toBe(true);
  const full = await compare();
  expect(full.cssWidth).toBe(partial.cssWidth);
  const loaded = await reads();

  await page.getByTestId("artwork-mode-partial").click();
  await expect.poll(async () => (await compare()).height).toBe(partial.height);
  await expect.poll(async () => (await compare()).matches).toBe(true);
  await page.getByTestId("artwork-mode-full").click();
  await expect.poll(async () => (await compare()).height).toBe(full.height);
  await expect.poll(async () => (await compare()).matches).toBe(true);
  expect(await reads()).toBe(loaded);
});
