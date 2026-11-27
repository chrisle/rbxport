import { expect, test, type Page } from "@playwright/test";

const player = (page: Page) => page.getByRole("region", { name: "Preview player", exact: true });

/**
 * How far across the deck's overview the waveform has been drawn: the
 * rightmost canvas column with anything in it, as a fraction of the width.
 * Zero before the deck has a waveform at all.
 */
function drawnTo(page: Page): Promise<number> {
  return page.evaluate(() => {
    const canvas = document.querySelector<HTMLCanvasElement>('[data-testid="player-overview"] canvas');
    const context = canvas?.getContext("2d");
    if (!canvas || !context || canvas.width === 0 || canvas.height === 0) return 0;
    const { data, width, height } = context.getImageData(0, 0, canvas.width, canvas.height);
    for (let x = width - 1; x >= 0; x--) {
      for (let y = 0; y < height; y++) {
        if ((data[(y * width + x) * 4 + 3] ?? 0) > 0) return (x + 1) / width;
      }
    }
    return 0;
  });
}

test("a track loaded without analysis is analysed and its waveform drawn on the deck from left to right", async ({ page }) => {
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  const rows = page.locator('[role="row"]');
  // The rows on screen with no tempo, which are the unanalysed ones.
  const unanalysed = await rows.evaluateAll((all) => all.flatMap((row, i) =>
    row.querySelector('[data-col="bpm"]')?.textContent === "" ? [i] : []));
  expect(unanalysed.length, "the mock library has unanalysed tracks on screen").toBeGreaterThan(1);

  // The mock fails every seventh track on purpose, before drawing anything,
  // so load them in turn until one starts to draw.
  let loaded = -1;
  for (const at of unanalysed) {
    await rows.nth(at).locator('[data-col="title"]').dblclick();
    await expect(player(page).getByTestId("player-title")).toHaveText(await rows.nth(at).locator('[data-col="title"]').innerText());
    const started = Date.now();
    while (Date.now() - started < 1_000 && await drawnTo(page) === 0) await page.waitForTimeout(25);
    if (await drawnTo(page) > 0) {
      loaded = at;
      break;
    }
  }
  expect(loaded, "a loaded track started drawing").toBeGreaterThanOrEqual(0);
  const bpm = rows.nth(loaded).locator('[data-col="bpm"]');

  const seen: number[] = [];
  await expect.poll(async () => {
    const reached = await drawnTo(page);
    seen.push(reached);
    return reached;
  }, { intervals: [40], timeout: 5_000 }).toBeGreaterThan(0.9);
  // A part, then more: the deck showed it part-drawn on the way, and the
  // drawing only ever moved to the right.
  expect(seen.some((reached) => reached > 0.05 && reached < 0.8), `drawn to ${seen.map((r) => r.toFixed(2)).join(", ")}`).toBe(true);
  expect(seen.every((reached, i) => i === 0 || reached >= (seen[i - 1] ?? 0) - 0.01)).toBe(true);
  await page.screenshot({ path: test.info().outputPath("deck-drawn.png") });

  // The analysis then lands: the row has a tempo, and the deck keeps its waveform.
  await expect(bpm).not.toHaveText("");
  await expect.poll(() => drawnTo(page)).toBeGreaterThan(0.9);
});

test("a protected deck distinguishes missing analysis from a missing detail palette", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("rbl.preferences", JSON.stringify({
    advanced: { protectLibrary: true }, analysis: { auto: false }, view: { waveformColor: "3band" },
  })));
  await page.route("**/src/main.tsx*", async (route) => {
    const response = await route.fetch();
    await route.fulfill({ response, body: `
      import { installDeckWaveformFixture } from "/e2e/deck-waveform-fixture.ts";
      window.deckWaveformFixture = await installDeckWaveformFixture();
    ` + await response.text() });
  });
  await page.goto("/?writable=1");
  const rows = page.getByRole("row").filter({ has: page.getByRole("gridcell") });
  await expect(rows.nth(7)).toBeVisible();
  const examples = await rows.evaluateAll((all) => all.flatMap(row => {
    const title = row.querySelector('[data-col="title"]')?.textContent?.trim();
    const tempo = row.querySelector('[data-col="bpm"]')?.textContent?.trim();
    return title && tempo !== undefined ? [{ title, analysed: tempo !== "" }] : [];
  }));
  const unanalysed = examples.find(row => !row.analysed);
  const [complete, partial] = examples.filter(row => row.analysed);
  if (!unanalysed || !complete || !partial) throw Error("Expected one unanalysed and two analysed fixture tracks");
  const load = async (title: string) => {
    await rows.getByRole("gridcell", { name: title, exact: true }).dblclick();
    await expect(player(page).getByTestId("player-title")).toHaveText(title);
  };
  const detail = player(page).getByTestId("player-detail").locator("canvas");
  const painted = () => detail.evaluate((canvas: HTMLCanvasElement) => {
    const pixels = canvas.getContext("2d")?.getImageData(0, 0, canvas.width, canvas.height).data;
    return pixels?.some((value, index) => index % 4 === 3 && value !== 0) ?? false;
  });
  const reads = () => page.evaluate(() => window.deckWaveformFixture.detailReads);

  await load(unanalysed.title);
  await expect(detail).toHaveCount(0);
  await load(complete.title);
  await expect.poll(painted).toBe(true);

  await page.evaluate((title) => window.deckWaveformFixture.omitPwv7(title), partial.title);
  await load(partial.title);
  await expect.poll(reads).toEqual(["bandsDetail"]);
  await expect.poll(() => drawnTo(page)).toBeGreaterThan(0);
  await page.evaluate(() => window.deckWaveformFixture.deliverMissingPwv7());
  expect(await painted(), "Three Band stays blank after the detail response settles").toBe(false);
  expect(await reads()).toEqual(["bandsDetail"]);

  await page.getByRole("banner").getByRole("button", { name: "Settings" }).click();
  const preferences = page.getByRole("dialog", { name: "Preferences" });
  await preferences.getByRole("tab", { name: "Color" }).click();
  await preferences.getByRole("radio", { name: "RGB" }).click();
  await page.keyboard.press("Escape");
  await expect.poll(painted).toBe(true);
  expect(await reads()).toEqual(["bandsDetail", "colourDetail"]);
  expect(await page.evaluate(() => window.deckWaveformFixture.analysisCalls)).toBe(0);
});
