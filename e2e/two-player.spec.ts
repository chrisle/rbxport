/**
 * The 2 PLAYER layout against the capture it was measured from:
 * `docs/screenshots`, Screenshot 2026-09-08 at 3.00.12 PM (rekordbox 7.2.11,
 * 2x, both decks loaded). Every number is a `--s-player-dual-*` or `--s-mixer-*`
 * token with the pixel range it came from, read back from the running page
 * so the test and the app agree.
 *
 * What the capture has, and what these gate: a title row with the sleeve in
 * it and the sync buttons and readouts at its right; the overview the deck's
 * full width under it; the phrase bar; a control row where the one-deck
 * layout has its pad row — no CUE/LOOP–GRID tabs, no pads; and the detail
 * waveform taking the rest of the half, so the two decks' details meet at the
 * line between them. Deck B reads the other way up, and the two decks share
 * one transport column, one mixer strip and one zoom cluster.
 */
import { expect, test, type Locator, type Page } from "@playwright/test";

async function token(page: Page, name: string): Promise<number> {
  const value = await page.evaluate(
    (n) => getComputedStyle(document.documentElement).getPropertyValue(n),
    name,
  );
  return Number.parseFloat(value);
}

type Box = { x: number; y: number; width: number; height: number };
const box = async (locator: Locator): Promise<Box> =>
  (await locator.boundingBox()) ?? { x: 0, y: 0, width: 0, height: 0 };
const bottom = (b: Box) => b.y + b.height;
const right = (b: Box) => b.x + b.width;

/** Both decks loaded: the first row on A, the second on B by the track menu. */
async function twoPlayer(page: Page, query = "") {
  await page.goto(`/${query}`);
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.getByRole("button", { name: "Layout" }).click();
  await page.getByRole("menuitemradio", { name: "2 PLAYER" }).click();
  await page.locator('[role="gridcell"][data-col="title"]').first().dblclick();
  const cell = page.locator('[role="gridcell"][data-col="title"]').nth(1);
  await cell.click({ button: "right" });
  const menu = page.getByRole("menu", { name: "Track" });
  await menu.getByRole("menuitem", { name: "Load", exact: true }).hover();
  await menu.getByRole("menuitem", { name: "Load track to player 2" }).click();
  const a = page.getByRole("region", { name: "Preview player", exact: true });
  const b = page.getByRole("region", { name: "Preview player B" });
  await expect(b.getByTestId("player-title")).not.toHaveText("");
  return { a, b };
}

test("deck A is laid out the way the capture measures it", async ({ page }) => {
  const { a } = await twoPlayer(page);
  // The region is the deck and the cue panel beside it; the deck's own right
  // edge is the panel's left.
  const region = await box(a);
  const deck = { ...region, width: region.width - (await token(page, "--s-player-right-w")) };
  expect(deck.height).toBeCloseTo((await token(page, "--s-player-dual-h")) / 2, 0);

  // The sleeve is in the title row: 36 square, 8 in and 6 down.
  const sleeve = await box(a.getByRole("button", { name: "Eject" }));
  expect(sleeve.width).toBeCloseTo(await token(page, "--s-player-dual-sleeve"), 0);
  expect(sleeve.height).toBeCloseTo(await token(page, "--s-player-dual-sleeve"), 0);
  expect(sleeve.x - deck.x).toBeCloseTo(await token(page, "--s-player-dual-sleeve-inset"), 0);
  expect(sleeve.y - deck.y).toBeCloseTo(await token(page, "--s-player-dual-sleeve-top"), 0);
  // The title starts the measured gap past the sleeve, the artist under it.
  const title = await box(a.getByTestId("player-title"));
  const artist = await box(a.getByTestId("player-artist"));
  expect(title.x - right(sleeve)).toBeCloseTo(await token(page, "--s-player-dual-title-gap"), 0);
  expect(artist.y).toBeGreaterThan(title.y);
  expect(artist.x).toBeCloseTo(title.x, 0);

  // KEY SYNC and BEAT SYNC at the row's right, 69x16 from 6 down, a measured
  // gap apart and 8.5 short of the deck's edge; MASTER under BEAT SYNC.
  const keySync = await box(a.getByRole("button", { name: "Key sync" }));
  const beatSync = await box(a.getByRole("button", { name: "Beat sync" }));
  const master = await box(a.getByRole("button", { name: "Sync master" }));
  const syncW = await token(page, "--s-player-dual-sync-w");
  const syncH = await token(page, "--s-player-dual-sync-h");
  for (const b of [keySync, beatSync, master]) {
    expect(b.width).toBeCloseTo(syncW, 0);
    expect(b.height).toBeCloseTo(syncH, 0);
  }
  expect(beatSync.y - deck.y).toBeCloseTo(await token(page, "--s-player-dual-sleeve-top"), 0);
  expect(right(deck) - right(beatSync)).toBeCloseTo(await token(page, "--s-player-dual-right-inset"), 0);
  expect(beatSync.x - right(keySync)).toBeCloseTo(await token(page, "--s-player-dual-sync-gap"), 0);
  expect(master.x).toBeCloseTo(beatSync.x, 0);
  expect(master.y - bottom(beatSync)).toBeCloseTo(await token(page, "--s-player-dual-sync-row-gap"), 0);
  // The readouts are in the second row, left of the key shift.
  const time = await box(a.getByTestId("player-time"));
  expect(time.y).toBeGreaterThan(bottom(keySync));
  expect(right(time)).toBeLessThan(keySync.x);

  // The overview: 27 tall, the measured way down the band that starts under
  // the 42pt title row, and running the deck's width less the 12pt inset.
  const overview = await box(a.getByTestId("player-overview"));
  const titleH = await token(page, "--s-player-dual-title-h");
  const inset = await token(page, "--s-player-inset");
  expect(overview.height).toBeCloseTo(await token(page, "--s-player-dual-overview-wave-h"), 0);
  expect(overview.y - deck.y).toBeCloseTo(
    titleH +
      (await token(page, "--s-player-dual-overview-top")) +
      (await token(page, "--s-player-vocal-h")),
    0,
  );
  expect(overview.x - deck.x).toBeCloseTo(inset, 0);
  expect(right(deck) - right(overview)).toBeCloseTo(inset, 0);

  // The phrase bar under the 51pt overview band, on the same edges.
  const phrase = await box(a.getByTestId("player-phrase"));
  expect(phrase.y - deck.y).toBeCloseTo(titleH + (await token(page, "--s-player-dual-overview-h")), 0);
  expect(phrase.height).toBeCloseTo(await token(page, "--s-player-phrase-h"), 0);
  expect(phrase.x).toBeCloseTo(overview.x, 0);
  expect(phrase.width).toBeCloseTo(overview.width, 0);

  // The control row a black point under the phrase bar, 26 tall, inset 3
  // from each edge, its buttons 18 tall and 4 down.
  const controls = await box(a.getByTestId("player-controls"));
  expect(controls.y - bottom(phrase)).toBeCloseTo(await token(page, "--s-player-dual-phrase-gap"), 0);
  expect(controls.height).toBeCloseTo(await token(page, "--s-player-dual-control-h"), 0);
  expect(controls.x - deck.x).toBeCloseTo(await token(page, "--s-player-dual-control-inset"), 0);
  expect(right(deck) - right(controls)).toBeCloseTo(await token(page, "--s-player-dual-control-inset"), 0);
  const memory = await box(a.getByRole("button", { name: "Set memory cue" }));
  expect(memory.height).toBeCloseTo(await token(page, "--s-player-dual-btn-h"), 0);
  expect(memory.width).toBeCloseTo(await token(page, "--s-player-dual-memory-w"), 0);
  expect(memory.y - controls.y).toBeCloseTo(await token(page, "--s-player-dual-control-pad"), 0);
  const q = await box(a.getByRole("button", { name: "Quantize" }));
  expect(right(deck) - right(q)).toBeCloseTo(await token(page, "--s-player-dual-right-inset"), 0);
  // MT and RST used to sit inline here; the deck tempo slider replaced them
  // (0bd2668, 2026-09-20), reached from the BPM readout instead.

  // The detail takes everything under the control row to the deck's bottom
  // edge, the deck's full width: no zoom column, no inset, nothing under it.
  const detail = await box(a.getByTestId("player-detail"));
  expect(detail.y).toBeCloseTo(bottom(controls), 0);
  expect(bottom(detail)).toBeCloseTo(bottom(deck), 0);
  expect(detail.x).toBeCloseTo(deck.x, 0);
  expect(detail.width).toBeCloseTo(deck.width, 0);
  expect(detail.height).toBeGreaterThan(70);
});

test("the control row stands in for the pad row: no tabs, no pads, the capture's buttons", async ({ page }) => {
  const { a, b } = await twoPlayer(page, "?writable=1");
  for (const deck of [a, b]) {
    // No CUE/LOOP–GRID tabs and no lettered pads in this layout.
    await expect(deck.getByRole("tab", { name: "CUE/LOOP" })).toHaveCount(0);
    await expect(deck.getByRole("tab", { name: "GRID" })).toHaveCount(0);
    await expect(deck.getByRole("button", { name: "Hot cue A" })).toHaveCount(0);
    // The row, left to right as the capture has it.
    const row = deck.getByTestId("player-controls");
    // The tempo step and MT/RST buttons that used to sit here moved into the
    // deck tempo slider (0bd2668, 2026-09-20), behind the BPM readout.
    await expect(row.getByRole("button")).toHaveText([
      "", "", "", "MEMORY", "AU", "MA", "‹", "4", "›", "", "", "Q",
    ]);
    // A loaded, analysed track can loop; with no loop yet, OUT has nothing to do.
    await expect(row.getByRole("button", { name: "Loop in" })).toBeEnabled();
    await expect(row.getByRole("button", { name: "Loop out" })).toBeDisabled();
    // Both mock rows this loads are analysed, with a grid to edit.
    await expect(row.getByRole("button", { name: "Shift the grid earlier" })).toBeEnabled();
    await expect(row.getByRole("button", { name: "Set memory cue" })).toBeEnabled();
    await expect(deck.getByTestId("player-bpm")).not.toHaveText("");
  }
  // The buttons are ordered left to right on the page as well as in the DOM.
  const xs: number[] = [];
  for (const button of await a.getByTestId("player-controls").getByRole("button").all()) {
    xs.push((await box(button)).x);
  }
  expect([...xs].sort((p, q) => p - q)).toEqual(xs);
});

test("the control row loops the chosen number of beats, and the steps resize it", async ({ page }) => {
  const { a } = await twoPlayer(page, "?writable=1");
  const row = a.getByTestId("player-controls");
  await row.getByRole("button", { name: "Shorter loop" }).click();
  await expect(row.getByRole("button", { name: "2 beat loop" })).toBeVisible();

  // AU: IN starts a two-beat loop from the head; the field and both ends light.
  await row.getByRole("button", { name: "Loop in" }).click();
  const exit = row.getByRole("button", { name: "Exit loop" });
  await expect(exit).toHaveAttribute("aria-pressed", "true");
  await expect(row.getByRole("button", { name: "Loop out" })).toBeEnabled();

  // A step changes the length of the playing loop, and the loop plays on.
  await row.getByRole("button", { name: "Longer loop" }).click();
  await expect(exit).toHaveText("4");

  // OUT with no IN waiting exits; the range stays for a RELOOP.
  await row.getByRole("button", { name: "Loop out" }).click();
  await expect(row.getByRole("button", { name: "4 beat loop" })).toHaveAttribute("aria-pressed", "false");
  await expect(row.getByRole("button", { name: "Loop out" })).toBeEnabled();

  // MA: IN and OUT by hand.
  const manual = row.getByRole("button", { name: "MA", exact: true });
  await manual.click();
  await expect(manual).toHaveAttribute("aria-pressed", "true");
  // IN waits for its OUT, lit.
  await row.getByRole("button", { name: "Loop in" }).click();
  await expect(row.getByRole("button", { name: "Loop in" })).toHaveAttribute("data-on", "true");
  // A change of mode drops the waiting IN.
  await row.getByRole("button", { name: "AU", exact: true }).click();
  await expect(row.getByRole("button", { name: "Loop in" })).not.toHaveAttribute("data-on");
  await manual.click();

  // OUT, with the head moved on from IN, plays the loop between them.
  await page.getByRole("group", { name: "Deck A transport" }).getByRole("button", { name: "Play", exact: true }).click();
  await row.getByRole("button", { name: "Loop in" }).click();
  await page.waitForTimeout(1_500);
  await row.getByRole("button", { name: "Loop out" }).click();
  await expect(exit).toHaveAttribute("aria-pressed", "true");
  await expect(a.getByTestId("player-overview").locator('[class*="loopBand"][data-active]')).toHaveCount(1);
});

test("deck B reads the other way up, and its detail meets deck A's at the centre line", async ({ page }) => {
  const { a, b } = await twoPlayer(page);
  const deckA = await box(a);
  const deckB = await box(b);
  expect(deckB.y).toBeCloseTo(bottom(deckA), 0);
  expect(deckB.height).toBeCloseTo(deckA.height, 0);

  // Down deck B: detail, control row, overview, phrase bar, title row.
  const detail = await box(b.getByTestId("player-detail"));
  const controls = await box(b.getByTestId("player-controls"));
  const overview = await box(b.getByTestId("player-overview"));
  const phrase = await box(b.getByTestId("player-phrase"));
  const sleeve = await box(b.getByRole("button", { name: "Eject" }));
  const title = await box(b.getByTestId("player-title"));
  expect(detail.y).toBeCloseTo(deckB.y, 0);
  expect(controls.y).toBeCloseTo(bottom(detail), 0);
  expect(overview.y).toBeGreaterThan(bottom(controls));
  expect(phrase.y).toBeGreaterThan(bottom(overview));
  expect(title.y).toBeGreaterThan(bottom(phrase));
  // The overview the measured way under the control row — closer than deck
  // A's sits under its title — and the phrase bar the measured way over the
  // title row, whose sleeve hangs 5 above the deck's bottom edge.
  expect(overview.y - bottom(controls)).toBeCloseTo(
    (await token(page, "--s-player-dual-overview-top-b")) + (await token(page, "--s-player-vocal-h")),
    0,
  );
  expect(bottom(deckB) - bottom(sleeve)).toBeCloseTo(await token(page, "--s-player-dual-sleeve-bottom"), 0);
  expect(bottom(deckB) - bottom(phrase)).toBeCloseTo(
    (await token(page, "--s-player-dual-title-hb")) + (await token(page, "--s-player-dual-phrase-gap-b")),
    0,
  );
  // The sync buttons and readouts keep their row, at the bottom now.
  const beatSync = await box(b.getByRole("button", { name: "Beat sync" }));
  const master = await box(b.getByRole("button", { name: "Sync master" }));
  expect(beatSync.y).toBeGreaterThan(bottom(phrase));
  expect(master.y).toBeGreaterThan(beatSync.y);

  // The two details meet: deck A's ends where deck B's starts.
  const detailA = await box(a.getByTestId("player-detail"));
  expect(detail.y).toBeCloseTo(bottom(detailA), 0);
  // Deck B's detail is the one drawn upside down; its overlays hang from the
  // bottom of the band rather than the top.
  const bars = b.getByTestId("player-bars");
  // 4pt, not flush against the bottom edge when flipped (0bd2668, 2026-09-20).
  await expect(bars).toHaveCSS("bottom", "4px");
  const beat = b.getByTestId("player-detail").locator("span[style*='left']").first();
  const beatBox = await box(beat);
  const beatBoxA = await box(a.getByTestId("player-detail").locator("span[style*='left']").first());
  // Deck A's beat line starts the token's distance under its band's top;
  // deck B's ends the same distance above its band's bottom.
  const lineTop = (await token(page, "--s-beat-marker-top")) + (await token(page, "--s-beat-head-h")) + 1;
  expect(beatBoxA.y - detailA.y).toBeCloseTo(lineTop, 0);
  expect(bottom(detail) - bottom(beatBox)).toBeCloseTo(lineTop, 0);
  expect(beatBox.y - detail.y).toBeLessThan(beatBoxA.y - detailA.y);
});

test("the shared column, strip and zoom are placed where the capture puts them", async ({ page }) => {
  const { a, b } = await twoPlayer(page);
  const deckA = await box(a);
  const pairTop = deckA.y;
  const pairBottom = bottom(await box(b));

  // The transport: deck A's controls measured down from the pair's top, deck
  // B's up from its bottom, the rings 32 across.
  const cueA = await box(page.getByRole("group", { name: "Deck A transport" }).getByRole("button", { name: "Cue" }));
  const playA = await box(page.getByRole("group", { name: "Deck A transport" }).getByRole("button", { name: /^(Play|Pause)$/ }));
  const jumpA = await box(page.getByRole("group", { name: "Deck A transport" }).getByRole("button", { name: "Beat jump back" }));
  const sizeA = await box(page.getByRole("group", { name: "Deck A transport" }).getByRole("button", { name: "Beat jump size" }));
  expect(cueA.width).toBeCloseTo(await token(page, "--s-mixer-jog-w"), 0);
  expect(cueA.y - pairTop).toBeCloseTo(await token(page, "--s-mixer-cue-top"), 0);
  expect(playA.y - pairTop).toBeCloseTo(await token(page, "--s-mixer-play-top"), 0);
  expect(jumpA.y - pairTop).toBeCloseTo(await token(page, "--s-mixer-jump-top"), 0);
  expect(jumpA.width).toBeCloseTo(await token(page, "--s-mixer-btn-w"), 0);
  expect(jumpA.height).toBeCloseTo(await token(page, "--s-mixer-btn-h"), 0);
  expect(sizeA.y - pairTop).toBeCloseTo(await token(page, "--s-mixer-size-top"), 0);
  expect(sizeA.width).toBeCloseTo(await token(page, "--s-mixer-size-w"), 0);
  expect(sizeA.height).toBeCloseTo(await token(page, "--s-mixer-size-h"), 0);
  const groupB = page.getByRole("group", { name: "Deck B transport" });
  const cueB = await box(groupB.getByRole("button", { name: "Cue" }));
  const playB = await box(groupB.getByRole("button", { name: /^(Play|Pause)$/ }));
  const jumpB = await box(groupB.getByRole("button", { name: "Beat jump back" }));
  const sizeB = await box(groupB.getByRole("button", { name: "Beat jump size" }));
  expect(pairBottom - bottom(playB)).toBeCloseTo(await token(page, "--s-mixer-play-bottom"), 0);
  expect(pairBottom - bottom(cueB)).toBeCloseTo(await token(page, "--s-mixer-cue-bottom"), 0);
  expect(pairBottom - bottom(sizeB)).toBeCloseTo(await token(page, "--s-mixer-size-bottom"), 0);
  expect(pairBottom - bottom(jumpB)).toBeCloseTo(await token(page, "--s-mixer-jump-bottom"), 0);
  // Deck B keeps CUE over PLAY and the jumps over the size box: the groups
  // mirror deck A's, and the order inside each group does not.
  expect(cueB.y).toBeLessThan(playB.y);
  expect(jumpB.y).toBeLessThan(sizeB.y);
  expect(sizeB.y).toBeLessThan(cueB.y);

  // DUAL CONTROL on the centre line, at the glyph's size.
  const dual = await box(page.getByRole("button", { name: "Dual control" }));
  expect(dual.y + dual.height / 2).toBeCloseTo((pairTop + pairBottom) / 2, 0);
  expect(dual.width).toBeCloseTo(await token(page, "--s-mixer-dual-w"), 0);

  // The strip a measured black column past the transport, the decks on its
  // far edge; its kill buttons the measured way in from each outer edge.
  const rail = await box(page.getByRole("group", { name: "Deck A transport" }));
  const mixer = await box(page.getByRole("group", { name: "Mixer" }));
  expect(mixer.x - right(rail)).toBeCloseTo(await token(page, "--s-mixer-gutter-w"), 0);
  expect(deckA.x).toBeCloseTo(right(mixer), 0);
  const highA = await box(page.getByRole("group", { name: "Mixer" }).getByRole("button", { name: "HIGH" }).first());
  const lowB = await box(page.getByRole("group", { name: "Mixer" }).getByRole("button", { name: "LOW" }).last());
  expect(highA.y - pairTop).toBeCloseTo(await token(page, "--s-mixer-bands-inset"), 0);
  expect(pairBottom - bottom(lowB)).toBeCloseTo(await token(page, "--s-mixer-bands-inset"), 0);

  // One zoom cluster for the pair, over the line where the details meet: +
  // and − a measured spread apart about it, and none inside either deck.
  await expect(a.getByRole("button", { name: "Zoom in" })).toHaveCount(0);
  await expect(b.getByRole("button", { name: "Zoom in" })).toHaveCount(0);
  const zoomIn = await box(page.getByRole("button", { name: "Zoom in" }));
  const zoomOut = await box(page.getByRole("button", { name: "Zoom out" }));
  const centre = (pairTop + pairBottom) / 2;
  const spread = await token(page, "--s-player-dual-zoom-spread");
  expect(zoomIn.y + zoomIn.height / 2).toBeCloseTo(centre - spread / 2, 0);
  expect(zoomOut.y + zoomOut.height / 2).toBeCloseTo(centre + spread / 2, 0);
  expect(zoomIn.x - deckA.x).toBeCloseTo((await token(page, "--s-player-dual-zoom-x")) - 1, 0);

  // Pressing it zooms both decks: fewer beats in both windows after a zoom in.
  const beats = (deck: Locator) =>
    deck.getByTestId("player-detail").locator("span[style*='left']").count();
  const beforeA = await beats(a);
  const beforeB = await beats(b);
  expect(beforeA).toBeGreaterThan(4);
  await page.getByRole("button", { name: "Zoom in" }).click();
  await expect.poll(() => beats(a)).toBeLessThan(beforeA);
  await expect.poll(() => beats(b)).toBeLessThan(beforeB);
});

test("an empty sleeve shows the record in every layout, and the row's too", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  const choose = async (layout: string) => {
    await page.getByRole("button", { name: "Layout" }).click();
    await page.getByRole("menuitemradio", { name: layout }).click();
  };
  // The record is one asset: `design/icons/ui/record.svg`, a disc 86% of its
  // square with a hole 24% across, drawn in the same colour on the same well
  // wherever a sleeve has no artwork. Its path is what says it is that asset.
  const RECORD = "M50 7a43 43 0 1 0 0 86 43 43 0 0 0 0-86zm0 31a12 12 0 1 0 0 24 12 12 0 0 0 0-24z";
  const record = async (sleeve: Locator) => {
    const svg = sleeve.locator("svg").first();
    await expect(svg).toBeVisible();
    expect(await svg.locator("path").getAttribute("d")).toBe(RECORD);
    const disc = await box(svg);
    const well = await box(sleeve);
    expect(disc.width).toBeCloseTo(well.width, 0);
    expect(disc.height).toBeCloseTo(well.height, 0);
    const colours = await svg.evaluate((el) => ({
      disc: getComputedStyle(el).color,
      well: getComputedStyle(el.parentElement as HTMLElement).backgroundColor,
    }));
    return colours;
  };
  const same = (a: { disc: string; well: string }, b: { disc: string; well: string }) => {
    expect(a.disc).toBe(b.disc);
    expect(a.well).toBe(b.well);
  };

  await choose("1 PLAYER");
  const one = await record(page.getByRole("button", { name: "Load the selected track" }));
  await choose("SIMPLE PLAYER");
  const simple = await record(page.getByRole("button", { name: "Load the selected track" }));
  same(one, simple);
  await choose("2 PLAYER");
  const sleeves = page.getByRole("button", { name: "Load the selected track" });
  await expect(sleeves).toHaveCount(2);
  same(one, await record(sleeves.first()));
  same(one, await record(sleeves.last()));

  // And the track list's cell without artwork draws the same record.
  const cell = page.locator('[role="gridcell"][data-col="artwork"]').first();
  expect(await cell.locator("svg path").getAttribute("d")).toBe(RECORD);
  const row = await cell.locator("svg").evaluate((el) => getComputedStyle(el).color);
  expect(row).toBe(one.disc);

  // Loading a track that has no artwork keeps the record under the eject.
  await page.locator('[role="gridcell"][data-col="title"]').first().dblclick();
  const loaded = page.getByRole("button", { name: "Eject" }).first();
  expect(await loaded.locator("svg").first().locator("path").getAttribute("d")).toBe(RECORD);
});

/**
 * BEAT SYNC and Q on: deck B stays on the master's beat. Both decks hold the
 * same track, so "on the beat" is a distance between the two heads of whole
 * beats. The mock counts frames, so this reads them, not the screen.
 */
test("a synced deck with Q on stays on the master's beat after jumps and in a loop", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.getByRole("button", { name: "Layout" }).click();
  await page.getByRole("menuitemradio", { name: "2 PLAYER" }).click();
  const first = page.locator('[role="gridcell"][data-col="title"]').first();
  await first.dblclick();
  await first.click({ button: "right" });
  const menu = page.getByRole("menu", { name: "Track" });
  await menu.getByRole("menuitem", { name: "Load", exact: true }).hover();
  await menu.getByRole("menuitem", { name: "Load track to player 2" }).click();
  const b = page.getByRole("region", { name: "Preview player B" });
  await expect(b.getByTestId("player-title")).not.toHaveText("");

  /** How far B is from A's beat, in seconds: 0 is on it. */
  const offBeat = () =>
    page.evaluate(() => {
      const { a, b, beat } = (window as unknown as {
        __deckSeconds: () => { a: number; b: number; beat: number };
      }).__deckSeconds();
      const into = (((a - b) % beat) + beat) % beat;
      return Math.min(into, beat - into);
    });

  // A is the master by default and Q is on: B follows.
  await b.getByRole("button", { name: "Beat sync" }).click();
  await expect(b.getByRole("button", { name: "Beat sync" })).toHaveAttribute("aria-pressed", "true");
  const play = page.getByRole("button", { name: "Play", exact: true });
  await play.first().click();
  await page.waitForTimeout(700);
  await play.first().click();
  await expect.poll(offBeat, { timeout: 3000 }).toBeLessThan(0.012);

  // FINE jumps on the master take it off B's beat; the lock brings B back.
  const sizes = page.getByRole("button", { name: "Beat jump size" });
  await sizes.first().click();
  await page.getByRole("menu", { name: "Beat jump size" }).getByRole("menuitemradio", { name: "Fine" }).click();
  const forward = page.getByRole("button", { name: "Beat jump forward" });
  for (let n = 0; n < 8; n++) await forward.first().click();
  await expect.poll(offBeat, { timeout: 3000 }).toBeLessThan(0.012);

  // A FINE jump on B lands back on the beat.
  await sizes.last().click();
  await page.getByRole("menu", { name: "Beat jump size" }).getByRole("menuitemradio", { name: "Fine" }).click();
  for (let n = 0; n < 8; n++) await forward.last().click();
  await expect.poll(offBeat, { timeout: 3000 }).toBeLessThan(0.012);

  // A hand-made loop on B, from Player B's LOOP IN and LOOP OUT keys. IN and
  // OUT go on whole beats, so B is on the beat through each repeat.
  await page.keyboard.press("Shift+I");
  await page.waitForTimeout(900);
  await page.keyboard.press("Shift+O");
  await expect.poll(() => page.evaluate(() => (window as unknown as {
    __deckSeconds: () => { looping: boolean };
  }).__deckSeconds().looping)).toBe(true);
  for (let n = 0; n < 5; n++) {
    await page.waitForTimeout(400);
    await expect.poll(offBeat, { timeout: 1000 }).toBeLessThan(0.012);
  }
});
