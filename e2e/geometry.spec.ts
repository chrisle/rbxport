/**
 * The rendered app against the measurements taken from real rekordbox.
 *
 * Not a pixel diff. The mock's data is not the reference library's, and the
 * top bar deliberately diverges (no EXPORT dropdown, a settings gear instead),
 * so comparing images would fail for reasons that are not regressions.
 *
 * What *is* worth gating is that the app draws at the geometry the captures
 * were measured to. Every number below is a token with a recorded source, so
 * a failure here means either the layout drifted or a token changed without
 * the interface following it.
 */
import { expect, test, type Page } from "@playwright/test";

/** Reads a token from the running page, so the test and the app agree. */
async function token(page: Page, name: string): Promise<number> {
  const value = await page.evaluate(
    (n) => getComputedStyle(document.documentElement).getPropertyValue(n),
    name,
  );
  return Number.parseFloat(value);
}

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
});

test("the tree is the measured width", async ({ page }) => {
  // browseSetting.xml TreeView 319 including the rail.
  const expected = await token(page, "--s-tree-w");
  const box = await page.getByRole("navigation", { name: "Library" }).boundingBox();
  expect(box?.width).toBeCloseTo(expected, 0);
});

test("the source rail is the measured width", async ({ page }) => {
  // browseSetting.xml TreeShortcut w=57.
  const expected = await token(page, "--s-tree-rail-w");
  // The rail holds the sources tablist and, at its foot, the Sync button;
  // the measured width is the rail's, border included.
  const box = await page.getByRole("tablist", { name: "Library sources" }).locator("..").boundingBox();
  expect(box?.width).toBeCloseTo(expected, 0);
});

test("the player is the measured height", async ({ page }) => {
  // Measured at 231px by saturation banding, against the 275 an unsourced
  // token used to claim.
  const expected = await token(page, "--s-player-h");
  const box = await page.getByRole("region", { name: "Preview player" }).boundingBox();
  expect(box?.height).toBeCloseTo(expected, 0);
});

test("track rows sit on the measured pitch", async ({ page }) => {
  const expected = await token(page, "--s-row-height");
  const tops = await page
    .locator('[role="row"][aria-selected]')
    .evaluateAll((rows) => rows.slice(0, 6).map((r) => r.getBoundingClientRect().top));
  expect(tops.length).toBeGreaterThan(2);
  for (let i = 1; i < tops.length; i++) {
    expect(Math.round((tops[i] ?? 0) - (tops[i - 1] ?? 0))).toBe(Math.round(expected));
  }
});

test("the column header is the measured height", async ({ page }) => {
  const expected = await token(page, "--s-col-header-h");
  const box = await page.getByRole("columnheader").first().boundingBox();
  expect(box?.height).toBeCloseTo(expected, 0);
});

test("the status bar and top bar are the measured heights", async ({ page }) => {
  for (const [name, selector] of [
    ["--s-status-bar-h", page.getByRole("contentinfo")],
    ["--s-top-bar-h", page.getByRole("banner")],
  ] as const) {
    const expected = await token(page, name);
    const box = await selector.boundingBox();
    expect(box?.height, name).toBeCloseTo(expected, 0);
  }
});

test("default columns are the twelve the captured header menu ticks, behind the row number", async ({ page }) => {
  const headings = await page
    .getByRole("columnheader")
    .evaluateAll((h) => h.map((c) => c.textContent?.replace(/[↑↓]/g, "").trim() ?? ""));
  expect(headings).toEqual([
    // `#` leads and is not one of the twelve: it is a fixed column, absent
    // from the captured menu and from german.lang's column names alike.
    "#",
    "Preview", "Artwork", "Track Title", "Key", "BPM", "Time",
    "Rating", "Artist", "Comments", "Label", "Date Added", "Release Date",
  ]);
});

test("each default column is its measured width", async ({ page }) => {
  // Widths are rekordbox's own, from TableHeader-PlaylistTracks.
  const expected: Record<string, number> = {
    "Track Title": 387, Key: 73, BPM: 80, Time: 80, Rating: 101, Artist: 301,
    Comments: 210, Label: 128, Artwork: 80, Preview: 128,
  };
  for (const [label, width] of Object.entries(expected)) {
    const box = await page.getByRole("columnheader", { name: new RegExp(`^${label}`) }).boundingBox();
    expect(Math.round(box?.width ?? 0), label).toBe(width);
  }
});

test("the player is laid out the way the capture measures it", async ({ page }) => {
  // Every number here is measured off a 2x capture of rekordbox 7.2.11
  // running — see the `source` on each token in design/tokens/theme.json.
  const player = await page.getByRole("region", { name: "Preview player" }).boundingBox();
  expect(player?.height).toBe(279);

  const transport = await page.locator("section[aria-label='Preview player'] > div").first().boundingBox();
  expect(transport?.width).toBe(80);

  const side = await page.getByRole("complementary", { name: "Cue list" }).boundingBox();
  expect(side?.width).toBe(209);

  // The bands stack in rekordbox's order, top to bottom.
  const tops = await Promise.all(
    ["player-vocal", "player-overview", "player-phrase", "player-detail"].map(async (id) =>
      (await page.getByTestId(id).boundingBox())?.y ?? 0,
    ),
  );
  expect(tops).toEqual([...tops].sort((a, b) => a - b));
});

test("the overview waveform is the measured height, not the whole band", async ({ page }) => {
  // The capture has a 49pt band holding a 5pt vocal strip, a 30pt waveform and
  // a 3pt position bar. Stretching the waveform over the whole band drew it
  // half again as tall as rekordbox does.
  const wave = await page.getByTestId("player-overview").boundingBox();
  expect(wave?.height).toBeCloseTo(await token(page, "--s-player-overview-wave-h"), 1);

  const vocal = await page.getByTestId("player-vocal").boundingBox();
  const phrase = await page.getByTestId("player-phrase").boundingBox();
  const band = (phrase?.y ?? 0) - (vocal?.y ?? 0);
  expect(band).toBeCloseTo(await token(page, "--s-player-overview-h"), 0);
});

test("hot cues are badges on the overview, and the same badge centred lower on the detail", async ({ page }) => {
  // Measured off docs/screenshots: four hot cues draw four 11pt badges along
  // the top of the overview, letter inside, and no line through the waveform.
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();
  const overview = page.getByTestId("player-overview");
  // The marker itself is zero-width — the position is its left edge — so the
  // badge inside it is what there is to look at.
  const badge = overview.locator('[data-cue]:not([data-cue=""]) b').first();
  await expect(badge).toBeVisible();

  const size = await token(page, "--s-cue-badge");
  const box = await badge.boundingBox();
  expect(box?.width).toBeCloseTo(size, 1);
  expect(box?.height).toBeCloseTo(size, 1);

  // Hung from the top of the strip, not centred in it.
  const strip = await overview.boundingBox();
  expect((box?.y ?? 0) - (strip?.y ?? 0)).toBeCloseTo(0, 1);

  // Widening the detail window until the cue falls inside it draws it there
  // too: the same 11pt badge, 15pt under the band's top and centred on the
  // cue, as the user's crop of rekordbox has it. e2e/cue-badges.spec.ts
  // measures the rest of that marker.
  const out = page.getByRole("button", { name: "Zoom out", exact: true });
  for (let i = 0; i < 3; i++) await out.click();
  const inDetail = page.getByTestId("player-detail").locator('[data-cue]:not([data-cue=""]) b').first();
  await expect(inDetail).toBeVisible();
  const detail = await page.getByTestId("player-detail").boundingBox();
  const badgeInDetail = await inDetail.boundingBox();
  expect((badgeInDetail?.y ?? 0) - (detail?.y ?? 0)).toBeCloseTo(await token(page, "--s-cue-badge-top"), 1);
  expect(badgeInDetail?.width).toBeCloseTo(size, 1);
  expect(badgeInDetail?.height).toBeCloseTo(size, 1);
  // And over the grid, not behind a downbeat line.
  const layer = await inDetail.evaluate((e) =>
    getComputedStyle(e.parentElement as HTMLElement).zIndex,
  );
  expect(Number(layer)).toBeGreaterThan(0);
});

test("a hot cue's badge, pad and panel chip all take the colour rekordbox draws for it", async ({ page }) => {
  // Extracted from rekordbox's getPadColor table: the four cues of the loaded
  // track read the same colour in the overview badge, pad row and HOT CUE chip.
  const drawn = [
    "rgb(48, 90, 255)", "rgb(80, 176, 242)", "rgb(16, 177, 118)", "rgb(60, 235, 80)",
    "rgb(155, 215, 35)", "rgb(225, 170, 0)", "rgb(255, 140, 0)", "rgb(245, 30, 140)",
    "rgb(170, 114, 255)",
  ];
  const player = page.getByRole("region", { name: "Preview player" });
  const overview = page.getByTestId("player-overview");
  const background = (selector: string) =>
    page.locator(selector).first().evaluate((e) => getComputedStyle(e).backgroundColor);

  // A track whose cues are not all the default green, or the badge and the
  // pad's own fallback would agree without a colour ever having been sent.
  const green = "rgb(60, 235, 80)";
  let markers: string[] = [];
  for (let row = 0; row < 8 && markers.length === 0; row++) {
    // An unanalysed row has no preview and no cues; the canvas is the tell.
    const title = page.locator('[role="gridcell"][data-col="title"]').nth(row);
    const analysed = await title.locator("xpath=..").locator('[data-col="preview"] canvas').count();
    if (analysed === 0) continue;
    await title.dblclick();
    await expect.poll(async () => overview.locator('[data-cue]:not([data-cue=""])').count()).toBe(4);
    const letters = await overview.locator('[data-cue]:not([data-cue=""])').evaluateAll((els) =>
      els.map((e) => e.getAttribute("data-cue") ?? ""),
    );
    const first = await background(`[data-testid="player-overview"] [data-cue="${letters[0]}"] b`);
    if (first !== green) markers = letters;
  }
  expect(markers).toHaveLength(4);
  await player.getByRole("tab", { name: "HOT CUE" }).click();
  for (const letter of markers) {
    const badge = await background(`[data-testid="player-overview"] [data-cue="${letter}"] b`);
    expect(drawn).toContain(badge);
    // The pad's inner square and the panel's letter chip are the same colour.
    // The pad row and the HOT CUE list both name their slots `Hot cue A`,
    // so each is found inside its own cluster.
    const pad = await player
      .locator('[aria-label="Hot cues"]')
      .getByRole("button", { name: `Hot cue ${letter}`, exact: true })
      .locator("span")
      .evaluate((e) => getComputedStyle(e).backgroundColor);
    expect(pad).toBe(badge);
    const chip = await page
      .getByRole("complementary", { name: "Cue list" })
      .getByRole("button", { name: `Hot cue ${letter}`, exact: true })
      .locator("span")
      .first()
      .evaluate((e) => getComputedStyle(e).backgroundColor);
    expect(chip).toBe(badge);
  }
});

test("every strip of the deck runs to the same edges", async ({ page }) => {
  // They disagreed: 16 on the left and 12 on the right for the title row and
  // the overview, 71 and 12 for the phrase, and nothing at all for the detail,
  // which therefore ran wider than everything above it.
  // The title starts where the waveform does, not where the sleeve does: it is
  // the strips that line up, and the sleeve sits to the left of all of them.
  const edges = await Promise.all(
    ["player-title", "player-overview", "player-phrase", "player-detail"].map(async (id) => {
      const box = await page.getByTestId(id).boundingBox();
      return { left: Math.round(box?.x ?? 0), right: Math.round((box?.x ?? 0) + (box?.width ?? 0)) };
    }),
  );
  // The detail is the exception on the left: its zoom controls float over it
  // rather than sitting in a column beside it, so it starts where the sleeve
  // does and the three strips above it start past the sleeve.
  const [title, overview, phrase, detail] = edges;
  expect(overview?.left).toBe(title?.left);
  expect(phrase?.left).toBe(title?.left);
  expect(detail?.left).toBeLessThan(title?.left ?? 0);

  // Every strip runs to the same right edge, which is the deck's own: nothing
  // is inset there, so the waveform reaches the edge of the panel.
  for (const edge of edges.slice(1)) {
    expect(edge.right).toBe(edges[1]?.right);
  }

  const player = await page.getByRole("region", { name: "Preview player" }).boundingBox();
  const margin = await token(page, "--s-player-margin");
  const panel = await token(page, "--s-player-right-w");
  const right = (player?.x ?? 0) + (player?.width ?? 0) - panel;
  expect(right - (edges[1]?.right ?? 0)).toBeCloseTo(margin, 0);
});

test("a measured gap separates the player from the browser", async ({ page }) => {
  // The capture has the pad bar ending at 335pt and the track list starting at
  // 338pt, so three points of black sit between them. With the player and the
  // browser butted together the deck reads as part of the list.
  const player = await page.getByRole("region", { name: "Preview player" }).boundingBox();
  const body = await page.getByTestId("body").boundingBox();
  const gap = (body?.y ?? 0) - ((player?.y ?? 0) + (player?.height ?? 0));
  expect(gap).toBeCloseTo(await token(page, "--s-player-gutter-h"), 1);
});

test("each transport control sits where the capture measures it", async ({ page }) => {
  // Distances from the top of the player, in points, off the 2x capture. They
  // were wrong once because a flex `gap` and a per-item margin were both
  // applying, which put PLAY 34pt below CUE instead of the measured 10.5.
  const player = page.getByRole("region", { name: "Preview player" });
  const top = (await player.boundingBox())?.y ?? 0;
  for (const [name, want] of [
    ["Previous track", 36],
    ["Beat jump back", 82],
    ["Beat jump size", 110],
    ["Cue", 156],
    ["Play", 206.5],
  ] as const) {
    const box = await player.getByRole("button", { name, exact: true }).boundingBox();
    expect((box?.y ?? 0) - top, name).toBeCloseTo(want, 1);
  }
});

test("the pad row is built to the sizes the capture measures", async ({ page }) => {
  const player = page.getByRole("region", { name: "Preview player" });
  // Two stacked tabs, 80x24pt, filling the 48pt row between them.
  for (const name of ["CUE/LOOP", "GRID"]) {
    const box = await player.getByRole("tab", { name }).boundingBox();
    expect(box?.width, name).toBe(await token(page, "--s-pad-tab-w"));
    expect(box?.height, name).toBe(await token(page, "--s-pad-tab-h"));
  }

  // A pad is a dark well with a small bright square inside it when a cue is
  // set — not a filled button, which is what it was.
  const pad = player.getByRole("button", { name: "Hot cue A", exact: true });
  const padBox = await pad.boundingBox();
  expect(padBox?.width).toBe(await token(page, "--s-pad-cue-w"));
  expect(padBox?.height).toBe(await token(page, "--s-pad-cue-h"));
  const inner = await pad.locator("span").boundingBox();
  expect(inner?.height).toBe(await token(page, "--s-pad-cue-inner"));
  expect(inner?.height).toBeLessThan(padBox?.height ?? 0);
});

test("the player carries the controls a deck has", async ({ page }) => {
  const player = page.getByRole("region", { name: "Preview player" });
  for (const name of [
    "Previous track", "Next track",
    "Beat jump back", "Beat jump forward",
    "Zoom in", "Zoom out",
    "Cue", "Play",
    "Hot cue A", "Hot cue H",
  ]) {
    await expect(player.getByRole("button", { name, exact: true })).toBeVisible();
  }
  // The pad modes are stacked tabs, and the cue-list has its own three.
  await expect(player.getByRole("tab", { name: "CUE/LOOP" })).toBeVisible();
  await expect(player.getByRole("tab", { name: "GRID" })).toBeVisible();
  await expect(player.getByRole("group", { name: "Loop mode" })).toBeVisible();
  await expect(player.getByRole("tab", { name: "MEMORY" })).toBeVisible();
  await expect(player.getByRole("tab", { name: "HOT CUE" })).toBeVisible();
  await expect(player.getByRole("tab", { name: "INFO" })).toBeVisible();
});

test("the mixer strip is the measured width, and its buttons the measured size", async ({
  page,
}) => {
  // Scanned off the two-player capture: the strip x 72.5..120.5pt, the kill
  // buttons x 78.5..114.5pt and 15pt tall on a 20pt pitch.
  await page.getByRole("button", { name: "Layout" }).click();
  await page.getByRole("menuitemradio", { name: "2 PLAYER" }).click();
  const mixer = page.getByRole("group", { name: "Mixer" });
  await expect(mixer).toBeVisible();

  const box = await mixer.boundingBox();
  expect(box?.width).toBeCloseTo(await token(page, "--s-mixer-strip-w"), 0);

  const high = mixer.getByRole("button", { name: "HIGH" }).first();
  const mid = mixer.getByRole("button", { name: "MID" }).first();
  const band = await high.boundingBox();
  expect(band?.width).toBeCloseTo(await token(page, "--s-mixer-band-w"), 0);
  expect(band?.height).toBeCloseTo(await token(page, "--s-mixer-band-h"), 0);

  // The pitch is the button plus its gap, which is what the capture measures.
  const next = await mid.boundingBox();
  const pitch = await token(page, "--s-mixer-band-h") + await token(page, "--s-mixer-band-gap");
  expect((next?.y ?? 0) - (band?.y ?? 0)).toBeCloseTo(pitch, 0);

  const fader = mixer.getByRole("slider", { name: "Crossfader" });
  const travel = await fader.boundingBox();
  expect(travel?.height).toBeCloseTo(await token(page, "--s-mixer-fader-h"), 0);
  expect(travel?.width).toBeCloseTo(await token(page, "--s-mixer-fader-handle-w"), 0);
});

test("deck B reads bottom-up, and its waveforms are the mirror of deck A's", async ({ page }) => {
  // The two-deck layout mirrors the pair about the line between them: deck A's
  // title at the top of the pair and deck B's at the bottom, with the two
  // detail waveforms meeting in the middle.
  await page.getByRole("button", { name: "Layout" }).click();
  await page.getByRole("menuitemradio", { name: "2 PLAYER" }).click();
  // A track on each deck, so both draw their waveforms.
  await page.locator('[role="gridcell"][data-col="title"]').first().dblclick();
  const cell = page.locator('[role="gridcell"][data-col="title"]').nth(1);
  await cell.click({ button: "right" });
  const menu = page.getByRole("menu", { name: "Track" });
  await menu.getByRole("menuitem", { name: "Load", exact: true }).hover();
  await menu.getByRole("menuitem", { name: "Load track to player 2" }).click();

  const a = page.getByRole("region", { name: "Preview player" }).first();
  const b = page.getByRole("region", { name: "Preview player B" });
  await expect(b.getByTestId("player-title")).not.toHaveText("");
  const box = async (region: typeof a, testid: string) =>
    (await region.getByTestId(testid).boundingBox()) ?? { y: 0, height: 0 };

  // Deck A: title above its phrase bar. Deck B: title below its own.
  const titleA = await box(a, "player-title");
  const phraseA = await box(a, "player-phrase");
  const titleB = await box(b, "player-title");
  const phraseB = await box(b, "player-phrase");
  expect(titleA.y).toBeLessThan(phraseA.y);
  expect(titleB.y).toBeGreaterThan(phraseB.y);

  // And the whole of deck B sits below the whole of deck A.
  expect(titleB.y).toBeGreaterThan(titleA.y);

  // Its detail waveform is drawn upside down, which is the only part of a
  // deck that can be mirrored rather than reordered; its overview is not —
  // the capture draws deck B's overview the same way up as deck A's.
  const transform = (region: typeof a, testid: string) =>
    region
      .getByTestId(testid)
      .locator("canvas")
      .first()
      .evaluate((el) => getComputedStyle(el).transform);
  const isUpright = (value: string) => value === "none" || value === "matrix(1, 0, 0, 1, 0, 0)";
  expect(await transform(b, "player-detail")).toBe("matrix(1, 0, 0, -1, 0, 0)");
  expect(isUpright(await transform(b, "player-overview"))).toBe(true);
  expect(isUpright(await transform(a, "player-detail"))).toBe(true);
});

test("hiding the full-waveform phrase bar keeps the detail waveform and its height", async ({ page }) => {
  // #179: omitting the phrase bar let the grid's auto-placement shift every
  // later row up one track, so the detail landed in a fixed-height track.
  const on = await page.getByTestId("player-detail").boundingBox();
  expect(on?.height).toBeGreaterThan(20);

  await page.addInitScript(() =>
    localStorage.setItem("rbl.preferences", JSON.stringify({ view: { phraseFull: false } })),
  );
  await page.reload();
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await expect(page.getByTestId("player-phrase")).toHaveCount(0);
  const off = await page.getByTestId("player-detail").boundingBox();
  expect(off?.height).toBeCloseTo(on?.height ?? 0, 0);
});
