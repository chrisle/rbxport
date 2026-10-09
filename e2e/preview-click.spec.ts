/**
 * A click on a row's waveform previews the track without loading it onto a
 * deck (#93): rekordbox's `PreviewComponent::clickWave`. The mock's preview
 * keeps time as the engine's does, so the playhead and the stop button are
 * what a browser can check; the sound is the backend's.
 */
import { expect, test, type Page } from "@playwright/test";

const player = (page: Page) => page.getByRole("region", { name: "Preview player" });
const waveform = (page: Page, row: number) =>
  page.locator('[role="gridcell"][data-col="preview"]').nth(row).locator("canvas");

test("clicking a row's waveform previews it without loading the deck", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  // The fourth row is analysed in the mock, so it has a waveform to click.
  const wave = waveform(page, 3);
  await expect(wave).toBeVisible();
  const box = await wave.boundingBox();
  if (!box) throw new Error("the waveform has no box");
  // Below the cue badges, halfway across.
  await page.mouse.click(box.x + box.width / 2, box.y + box.height - 2);

  const cell = page.locator('[role="gridcell"][data-col="preview"]').nth(3);
  const stop = cell.getByRole("button", { name: "Stop" });
  await expect(stop).toBeVisible();
  // On the left of the waveform, as rekordbox's is (#207).
  const stopBox = await stop.boundingBox();
  expect((stopBox?.x ?? 0) - box.x).toBeLessThan(box.width / 4);
  // The deck was not given the track.
  await expect(player(page).getByRole("button", { name: "Play", exact: true })).toBeDisabled();

  // The playhead starts near the middle and moves on.
  const head = cell.locator('[class*="previewHead"]');
  const at = () => head.evaluate((el) => new DOMMatrixReadOnly(getComputedStyle(el).transform).m41);
  const first = await at();
  expect(first).toBeGreaterThan(box.width * 0.4);
  await expect.poll(at).toBeGreaterThan(first);

  await stop.click();
  await expect(stop).toHaveCount(0);
});

/** Previews row `row` from halfway across its waveform; returns its stop button. */
async function preview(page: Page, row: number) {
  const box = await waveform(page, row).boundingBox();
  if (!box) throw new Error("the waveform has no box");
  await page.mouse.click(box.x + box.width / 2, box.y + box.height - 2);
  const stop = page.locator('[role="gridcell"][data-col="preview"]').nth(row).getByRole("button", { name: "Stop" });
  await expect(stop).toBeVisible();
  return stop;
}

// rekordbox stops its preview when a track is loaded onto a deck
// (`ListViewer::loadTrack`) and when a deck plays (`PreviewComponent::
// timerCallback`); rbxport left it playing under the deck (#242).
test("loading a track onto the deck stops the preview", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  const stop = await preview(page, 3);

  await page.locator('[role="gridcell"][data-col="title"]').nth(5).dblclick();
  await expect(player(page).getByRole("button", { name: "Play", exact: true })).toBeEnabled();
  await expect(stop).toHaveCount(0);
});

test("playing the deck stops the preview", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(5).dblclick();
  const play = player(page).getByRole("button", { name: "Play", exact: true });
  await expect(play).toBeEnabled();
  const stop = await preview(page, 3);

  await play.click();
  await expect(player(page).getByRole("button", { name: "Pause", exact: true })).toBeVisible();
  await expect(stop).toHaveCount(0);
});
