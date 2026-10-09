/**
 * The hot cue pads and the HOT CUE list beside the deck.
 *
 * rekordbox's own arrangement: an empty pad sets `Hot Cue <letter>` at the
 * playhead (`Set Hot Cue A`, on `1`-`3` for the first three from the Export
 * key map), a set pad calls its cue, and each set row of the HOT CUE tab has
 * a ✕ (`Clear Hot Cue A`, on `command + 1`-`3`). The mock backend keeps cues
 * per track, so an edit here is a real round trip: written, announced,
 * refetched, and the pad, the badges on both waveforms, the list and the
 * browser row's CUE mark all redrawn from the one array.
 */
import { expect, test, type Page } from "@playwright/test";
import { enableTooltips } from "./helpers";

const player = (page: Page) => page.getByRole("region", { name: "Preview player" });
// The pads and the list's rows share a name, so each is found in its own box.
const pad = (page: Page, letter: string) =>
  player(page).locator('[aria-label="Hot cues"]').getByRole("button", { name: `Hot cue ${letter}`, exact: true });
const panel = (page: Page) => page.getByRole("complementary", { name: "Cue list" });
const listRow = (page: Page, letter: string) =>
  panel(page).getByRole("button", { name: `Hot cue ${letter}`, exact: true });
const clearButton = (page: Page, letter: string) =>
  panel(page).getByRole("button", { name: `Clear hot cue ${letter}` });
const overviewBadge = (page: Page, letter: string) =>
  page.getByTestId("player-overview").locator(`[data-cue="${letter}"]`);
const detailBadge = (page: Page, letter: string) =>
  page.getByTestId("player-detail").locator(`[data-cue="${letter}"]`);
/** The browser's attribute cell for the loaded row: CUE when the track has hot cues. */
const cueMark = (page: Page) => page.locator('[role="gridcell"][data-col="attr"]').nth(3);
const elapsed = (page: Page) => player(page).locator('[class*="elapsed"]');
/** The elapsed readout in seconds: `01:23.4` is 83.4. */
async function elapsedSeconds(page: Page): Promise<number> {
  const text = (await elapsed(page).textContent()) ?? "";
  const [, minutes, seconds] = /(\d+):(\d+(?:\.\d)?)/.exec(text) ?? [];
  return Number(minutes) * 60 + Number(seconds);
}
/** A list row's `mm:ss` in whole seconds. */
async function cueSeconds(page: Page, letter: string): Promise<number> {
  const [, minutes, seconds] = /(\d\d):(\d\d)/.exec((await listRow(page, letter).textContent()) ?? "") ?? [];
  return Number(minutes) * 60 + Number(seconds);
}
const playButton = (page: Page) => player(page).getByRole("button", { name: "Play", exact: true });
const pauseButton = (page: Page) => player(page).getByRole("button", { name: "Pause", exact: true });

/** Loads the fourth row — analysed, so it carries the mock's four hot cues. */
async function load(page: Page, query = "") {
  await page.goto(`/${query}`);
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();
  await expect(player(page).getByRole("button", { name: "Play", exact: true })).toBeEnabled();
  await panel(page).getByRole("tab", { name: "HOT CUE" }).click();
}

/** Runs the deck for a moment and pauses it, so the playhead is somewhere in the track. */
async function playAWhile(page: Page) {
  await player(page).getByRole("button", { name: "Play", exact: true }).click();
  await page.waitForTimeout(700);
  await player(page).getByRole("button", { name: "Pause", exact: true }).click();
}

test("an empty pad sets the hot cue at the playhead, and its ✕ in the list clears it", async ({ page }) => {
  await enableTooltips(page);
  await load(page, "?writable=1");

  // The mock's track has A to D; E is the first empty pad.
  await expect(pad(page, "D")).toHaveAttribute("aria-pressed", "true");
  await expect(pad(page, "E")).toHaveAttribute("aria-pressed", "false");
  await expect(pad(page, "E")).toHaveAttribute("title", "Set Hot Cue E");
  await expect(listRow(page, "E")).toHaveAttribute("aria-disabled", "true");
  await expect(overviewBadge(page, "E")).toHaveCount(0);

  await playAWhile(page);
  await pad(page, "E").click();

  // Lit, listed with a time and a ✕, and badged on both waveforms.
  await expect(pad(page, "E")).toHaveAttribute("aria-pressed", "true");
  await expect(listRow(page, "E")).not.toHaveAttribute("aria-disabled", "true");
  await expect(listRow(page, "E")).toContainText(/\d\d:\d\d/);
  await expect(clearButton(page, "E")).toBeEnabled();
  await expect(overviewBadge(page, "E")).toHaveCount(1);
  await expect(detailBadge(page, "E")).toHaveCount(1);
  await expect(overviewBadge(page, "E").locator("b")).toHaveText("E");

  // A set pad calls its cue rather than setting over it: the list does not
  // change when it is pressed again from elsewhere in the track.
  await playAWhile(page);
  const before = await listRow(page, "E").textContent();
  await pad(page, "E").click();
  await page.waitForTimeout(200);
  await expect(listRow(page, "E")).toHaveText(before ?? "");

  // ✕ clears it, and the pad, the list and both waveforms let go together.
  await clearButton(page, "E").click();
  await expect(pad(page, "E")).toHaveAttribute("aria-pressed", "false");
  await expect(listRow(page, "E")).toHaveAttribute("aria-disabled", "true");
  await expect(clearButton(page, "E")).toHaveCount(0);
  await expect(overviewBadge(page, "E")).toHaveCount(0);
  await expect(detailBadge(page, "E")).toHaveCount(0);
});

test("the simple player's overview wears the badge too", async ({ page }) => {
  // The strip draws from the same cue array as the full deck, so a hot cue
  // set here is a badge there — the 9.03.41 PM capture shows them.
  await load(page, "?writable=1");
  await playAWhile(page);
  await pad(page, "E").click();
  await expect(overviewBadge(page, "E")).toHaveCount(1);

  await page.getByRole("button", { name: "Layout" }).click();
  await page.getByRole("menuitemradio", { name: "SIMPLE PLAYER" }).click();
  const strip = page.getByTestId("simple-player-overview");
  await expect(strip.locator('[data-cue="D"]')).toHaveCount(1);
  await expect(strip.locator('[data-cue="E"]')).toHaveCount(1);
});

test("the browser row's CUE mark follows the deck without a reload", async ({ page }) => {
  await load(page, "?writable=1");
  // The Attribute column is off by default; the header menu turns it on.
  await page.getByRole("columnheader", { name: /BPM/ }).click({ button: "right" });
  await page.getByRole("menu", { name: "Columns" }).getByRole("menuitemcheckbox", { name: "Attribute" }).click();
  await expect(cueMark(page)).toContainText("CUE");
  const rows = page.locator('[role="row"]');
  const rowCount = await rows.count();

  // Clearing all four hot cues empties the row's letters, and the mark goes.
  for (const letter of ["A", "B", "C", "D"]) {
    await clearButton(page, letter).click();
    await expect(clearButton(page, letter)).toHaveCount(0);
  }
  await expect(cueMark(page)).not.toContainText("CUE");
  // No reload: the table kept its rows.
  await expect(rows).toHaveCount(rowCount);

  // `1` is `Set Hot Cue A`; the mark comes back with the letter.
  await playAWhile(page);
  await page.keyboard.press("1");
  await expect(pad(page, "A")).toHaveAttribute("aria-pressed", "true");
  await expect(cueMark(page)).toContainText("CUE");
  // `command + 1` is `Clear Hot Cue A`.
  await page.keyboard.press("Meta+1");
  await expect(pad(page, "A")).toHaveAttribute("aria-pressed", "false");
  await expect(cueMark(page)).not.toContainText("CUE");
});

test("with rekordbox holding the library, setting and clearing are refused and calling is not", async ({ page }) => {
  // The mock's library is read-only unless asked otherwise.
  await enableTooltips(page);
  await load(page);
  await expect(page.getByRole("contentinfo")).toContainText("Library read-only");

  // An empty pad is dead, and says why.
  await expect(pad(page, "E")).toBeDisabled();
  await expect(pad(page, "E")).toHaveAttribute("title", /read-only/);
  // A set pad still calls its cue, and its ✕ is drawn but dead.
  await expect(pad(page, "A")).toBeEnabled();
  await expect(pad(page, "A")).not.toHaveAttribute("title", /read-only/);
  await expect(clearButton(page, "A")).toBeDisabled();
  await expect(clearButton(page, "A")).toHaveAttribute("title", /read-only/);

  // Calling moves the playhead to the cue's time, which the list shows, and
  // plays from there. The list rounds down to the second and the deck runs
  // on, so the head is checked to be within the second after the cue.
  const time = await cueSeconds(page, "A");
  const nearA = async () => {
    const at = await elapsedSeconds(page);
    return at >= time && at < time + 2;
  };
  await pad(page, "D").click();
  await expect.poll(nearA).toBe(false);
  await pad(page, "A").click();
  await expect.poll(nearA).toBe(true);
  // The key calls it too, and clears nothing.
  await pad(page, "D").click();
  await expect.poll(nearA).toBe(false);
  await page.keyboard.press("1");
  await expect.poll(nearA).toBe(true);
  await page.keyboard.press("Meta+1");
  await page.waitForTimeout(200);
  await expect(pad(page, "A")).toHaveAttribute("aria-pressed", "true");
});

test("calling a set hot cue from pause plays from the cue", async ({ page }) => {
  // rekordbox 7 manual, EXPORT mode, "Calling and playing saved hot cue
  // points" (p.102): "Select a hot cue point. Playback starts from the
  // selected hot cue point." Only the Gate Cue preference, which rbx does
  // not have, makes a call from pause hold rather than play.
  await load(page);
  await expect(playButton(page)).toBeVisible();
  const time = await cueSeconds(page, "B");

  await pad(page, "B").click();
  await expect(pauseButton(page)).toBeVisible();
  const started = await elapsedSeconds(page);
  expect(started).toBeGreaterThanOrEqual(time);
  expect(started).toBeLessThan(time + 2);
  // And it runs: the head moves on from the cue.
  await expect.poll(() => elapsedSeconds(page)).toBeGreaterThan(started);

  // A playing deck carries on from a call, rather than stopping on it.
  await pad(page, "A").click();
  await page.waitForTimeout(300);
  await expect(pauseButton(page)).toBeVisible();

  // The list's rows are the same call.
  await pauseButton(page).click();
  await expect(playButton(page)).toBeVisible();
  await listRow(page, "B").click();
  await expect(pauseButton(page)).toBeVisible();
});

test("setting an empty pad from pause leaves the deck stopped", async ({ page }) => {
  await load(page, "?writable=1");
  await playAWhile(page);
  await expect(playButton(page)).toBeVisible();
  await pad(page, "E").click();
  await expect(pad(page, "E")).toHaveAttribute("aria-pressed", "true");
  await page.waitForTimeout(300);
  await expect(playButton(page)).toBeVisible();
});

test("with Q on, a call on a playing deck waits for the beat and keeps it", async ({ page }) => {
  // rekordbox 7.2.19, EXPORT mode: QuantizedCueBehavior::doHotCueLaunch ->
  // moveToCueAndPlayWithWait. The deck plays on to the next quantize step and
  // jumps to the cue there, so the beat runs on through the jump
  // [OBS static, parity/issue-126]. Q is on by default.
  await load(page);
  await expect(player(page).getByRole("button", { name: "Quantize" })).toHaveAttribute("aria-pressed", "true");
  await pad(page, "B").click();
  await expect(pauseButton(page)).toBeVisible();
  await page.waitForTimeout(600);

  // The head, read every few ms from just before the press of A.
  const heads = page.evaluate(async () => {
    const read = (window as unknown as { __deckSeconds: () => { a: number; beatA: number } }).__deckSeconds;
    const out: { a: number; beat: number; at: number }[] = [];
    const end = performance.now() + 1800;
    while (performance.now() < end) {
      const { a, beatA } = read();
      out.push({ a, beat: beatA, at: performance.now() });
      await new Promise((done) => setTimeout(done, 4));
    }
    return out;
  });
  await page.waitForTimeout(100);
  await pad(page, "A").click();
  const all = await heads;
  const beat = all[0]!.beat;
  expect(beat).toBeGreaterThan(0.2);
  // The jump: the one step that moves far, from B's region to A's cue.
  const jump = all.findIndex((sample, n) => n > 0 && Math.abs(sample.a - all[n - 1]!.a) > 0.3);
  expect(jump).toBeGreaterThan(0);
  const before = all[jump - 1]!.a;
  const after = all[jump]!.a;
  // Where it left and where it landed are the same place in a beat: the
  // jump waited for the step instead of cutting the beat short. On the mock
  // this comes to under a millisecond; a jump at the press is off by
  // wherever in the beat the press fell.
  const phase = (((before - after) % beat) + beat) % beat;
  expect(Math.min(phase, beat - phase)).toBeLessThan(0.02);
  // And it plays on from the cue.
  await expect(pauseButton(page)).toBeVisible();
});

test("with Q on, a call inside a one-beat loop leaves the loop and lands on the cue", async ({ page }) => {
  // rekordbox 7.2.19: doHotCueLaunch leaves a playing loop at the press
  // (CueBehavior::doExitLoop) and moveToCueAndPlayWithWait fires no later
  // than the loop's old out point [OBS static, parity/issue-126]. Timed to
  // the next step alone, the call never came: the deck wrapped first.
  type Read = () => { a: number; beatA: number; loopingA: boolean };
  const read = () => page.evaluate(() => (window as unknown as { __deckSeconds: Read }).__deckSeconds());
  await load(page);
  // Where A is: called from pause, the deck starts from it. The first new
  // reading is the cue, give or take a few milliseconds of play.
  const start = (await read()).a;
  const moved = page.evaluate(async (from) => {
    const now = (window as unknown as { __deckSeconds: Read }).__deckSeconds;
    const end = performance.now() + 2000;
    while (performance.now() < end) {
      const { a } = now();
      if (a !== from) return a;
      await new Promise((done) => setTimeout(done, 2));
    }
    return from;
  }, start);
  await pad(page, "A").click();
  const cueA = await moved;
  await expect(pauseButton(page)).toBeVisible();
  await pad(page, "B").click();
  await page.waitForTimeout(700);
  const deck = player(page);
  await deck.getByRole("button", { name: "4 beat loop" }).click();
  await deck.getByRole("button", { name: "Shorter loop" }).click();
  await deck.getByRole("button", { name: "Shorter loop" }).click();
  await expect(deck.getByRole("button", { name: "Exit loop" })).toHaveText("1");
  await page.waitForTimeout(700);
  expect((await read()).loopingA).toBe(true);

  const heads = page.evaluate(async () => {
    const now = (window as unknown as { __deckSeconds: Read }).__deckSeconds;
    const out: { a: number; looping: boolean }[] = [];
    const end = performance.now() + 1500;
    while (performance.now() < end) {
      const { a, loopingA } = now();
      out.push({ a, looping: loopingA });
      await new Promise((done) => setTimeout(done, 4));
    }
    return out;
  });
  await page.waitForTimeout(100);
  await pad(page, "A").click();
  const all = await heads;
  const beat = (await read()).beatA;
  // The jump: the one step that moves further than a loop wraps.
  const jump = all.findIndex((sample, n) => n > 0 && Math.abs(sample.a - all[n - 1]!.a) > beat * 1.5);
  expect(jump).toBeGreaterThan(0);
  // It lands on A, not a loop length before it, and the loop is left.
  expect(Math.abs(all[jump]!.a - cueA)).toBeLessThan(0.1);
  expect(all.at(-1)!.looping).toBe(false);
  // And it plays on from the cue, past where the old loop would wrap it.
  expect(all.at(-1)!.a).toBeGreaterThan(all[jump]!.a + beat);
  await expect(pauseButton(page)).toBeVisible();
});

test("right-clicking a set pad opens the cue colour menu and recolours it (#272)", async ({ page }) => {
  await load(page);
  await pad(page, "A").click({ button: "right" });
  const menu = page.getByRole("menu", { name: "Hot cue color" });
  await expect(menu).toBeVisible();
  await menu.getByRole("button", { name: "Color 6" }).click();
  await expect(menu).toBeHidden();
});
