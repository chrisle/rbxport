/**
 * The detail waveform scrolls on whole device pixels (#346).
 *
 * The scrolling layer is slid by a transform every frame. A fractional slide
 * makes the compositor resample it, so every one-pixel bar and grid line is
 * split over two columns at partial brightness, and the split changes every
 * frame: the waveform shimmers while it plays. The canvas inside it must also
 * be exactly as wide as the device pixels it covers, or it is squeezed onto
 * them and resampled anyway.
 */
import { expect, test, type Page } from "@playwright/test";

const player = (page: Page) => page.getByRole("region", { name: "Preview player", exact: true });

async function playFirstTrack(page: Page) {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').first().dblclick();
  const play = player(page).getByRole("button", { name: "Play", exact: true });
  await expect(play).toBeEnabled();
  await expect(player(page).getByTestId("player-detail").locator("canvas")).toBeVisible();
  await play.click();
  await expect(player(page).getByRole("button", { name: "Pause", exact: true })).toBeVisible();
}

/** The layer's slide on each of `count` frames, and the canvas's sizes. */
async function sample(page: Page, count: number) {
  return page.evaluate(async (frames: number) => {
    const detail = document.querySelector('[data-testid="player-detail"]');
    const layer = detail?.firstElementChild;
    const canvas = detail?.querySelector("canvas");
    if (!(layer instanceof HTMLElement) || !(canvas instanceof HTMLCanvasElement)) throw new Error("no detail layer");
    const slides: number[] = [];
    await new Promise<void>((done) => {
      const tick = () => {
        slides.push(Number(/translateX\((-?[\d.e-]+)px\)/.exec(layer.style.transform)?.[1] ?? Number.NaN));
        if (slides.length < frames) requestAnimationFrame(tick);
        else done();
      };
      requestAnimationFrame(tick);
    });
    return {
      slides,
      ratio: devicePixelRatio,
      backing: canvas.width,
      shown: canvas.getBoundingClientRect().width * devicePixelRatio,
    };
  }, count);
}

for (const deviceScaleFactor of [2, 1]) {
  test.describe(`at ${deviceScaleFactor}x`, () => {
    test.use({ deviceScaleFactor });

    test("the playing detail slides by whole device pixels over an unsqueezed canvas", async ({ page }) => {
      await playFirstTrack(page);
      const { slides, ratio, backing, shown } = await sample(page, 90);
      expect(ratio).toBe(deviceScaleFactor);
      // It is moving, so this is a scrolling layer and not one at rest on 0.
      expect(new Set(slides).size).toBeGreaterThan(10);
      for (const slide of slides) {
        expect(Number.isFinite(slide)).toBe(true);
        const device = slide * ratio;
        expect(Math.abs(device - Math.round(device))).toBeLessThan(1e-6);
      }
      expect(Math.abs(backing - shown)).toBeLessThanOrEqual(1);
    });
  });
}
