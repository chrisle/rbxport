/**
 * PERFORMANCE mode's fixed-distance beat jumps: `Forward 4 beats`, `Backward
 * 8 bars` and the rest, once the Keyboard pane gives them a key, move the
 * head by that many beats at the deck's tempo, whatever size the jump menu
 * is on. They ship unbound, so the keys here are given the way the pane
 * stores them, in `rbl.preferences`: option + cursor right for `Forward 4
 * beats` and option + cursor left for `Backward 8 bars`, chords no preset
 * uses, so nothing else answers them.
 */
import { expect, test, type Page } from "@playwright/test";
import process from "node:process";

// The dev server's port, as `playwright.config.ts` picks it.
const port = Number(process.env.E2E_PORT) || 1420;

test.use({
  storageState: {
    cookies: [],
    origins: [{
      origin: `http://localhost:${port}`,
      localStorage: [{
        name: "rbl.preferences",
        value: JSON.stringify({
          advanced: { protectLibrary: false },
          keyboard: {
            preset: "export",
            overrides: {
              jumpForward4Beats: { key: "ArrowRight", altKey: true },
              jumpBackward8Bars: { key: "ArrowLeft", altKey: true },
            },
          },
        }),
      }],
    }],
  },
});

const player = (page: Page) => page.getByRole("region", { name: "Preview player" });
/** The elapsed readout in seconds: `01:23.4` is 83.4. */
async function elapsedSeconds(page: Page): Promise<number> {
  const text = (await player(page).locator('[class*="elapsed"]').textContent()) ?? "";
  const [, minutes, seconds] = /(\d+):(\d+(?:\.\d)?)/.exec(text) ?? [];
  return Number(minutes) * 60 + Number(seconds);
}
/** How long `beats` last at the deck's tempo, from its BPM readout once it has one. */
async function secondsOf(page: Page, beats: number): Promise<number> {
  const readout = player(page).getByTestId("player-bpm");
  await expect(readout).toHaveText(/^[1-9]\d*\.\d\d$/);
  const bpm = Number(await readout.textContent());
  expect(bpm).toBeGreaterThan(0);
  return (beats * 60) / bpm;
}
/** Loads the fourth row and arms the deck, so the arrows are the deck's rather than the browser's. */
async function load(page: Page) {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();
  await expect(player(page).getByRole("button", { name: "Play", exact: true })).toBeEnabled();
  // A click anywhere on the deck arms it; the title is somewhere that does not seek.
  await player(page).getByTestId("player-title").click();
  await expect(player(page)).toHaveAttribute("data-armed", "");
}
/** The readout shows tenths, so a landing is right to within one of them. */
async function expectHeadAt(page: Page, seconds: number) {
  await expect.poll(() => elapsedSeconds(page)).toBeCloseTo(seconds, 0);
  expect(Math.abs((await elapsedSeconds(page)) - seconds)).toBeLessThanOrEqual(0.11);
}

test("a bound fixed jump moves by its own beats, and ignores the jump menu's size", async ({ page }) => {
  await load(page);

  const fourBeats = await secondsOf(page, 4);
  const eightBars = await secondsOf(page, 32);
  const start = await elapsedSeconds(page);

  // Forward 4 beats, with the menu on its default 4Beats.
  await page.keyboard.press("Alt+ArrowRight");
  await expectHeadAt(page, start + fourBeats);

  // The menu set to 8Bars: the plain arrow follows it, the bound jump does not.
  await page.getByRole("button", { name: "Beat jump size" }).click();
  await page.getByRole("menu", { name: "Beat jump size" }).getByRole("menuitemradio", { name: "8Bars", exact: true }).click();
  await page.keyboard.press("ArrowRight");
  await expectHeadAt(page, start + fourBeats + eightBars);
  await page.keyboard.press("Alt+ArrowRight");
  await expectHeadAt(page, start + fourBeats * 2 + eightBars);

  // Backward 8 bars is 32 beats back.
  await page.keyboard.press("Alt+ArrowLeft");
  await expectHeadAt(page, start + fourBeats * 2);
});

test("a fixed jump left unbound answers no key", async ({ page }) => {
  await load(page);

  const start = await elapsedSeconds(page);
  // No preset binds `Forward 1/2 beat`, and the preferences above give it
  // nothing either: option + cursor up is nobody's key.
  await page.keyboard.press("Alt+ArrowUp");
  await page.waitForTimeout(200);
  expect(await elapsedSeconds(page)).toBe(start);
});
