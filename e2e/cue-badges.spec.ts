/**
 * Cue markers on the detail waveform.
 *
 * Measured off the user's 2x crop of rekordbox 7.2.11's detail at a hot cue
 * (cue-badge.png; the sources on the `cueMarker*`, `cueBadgeTop` and
 * `cueLine*` tokens carry the numbers): a red triangle pointing down from
 * near the band's top for the memory cue rekordbox keeps beside every hot
 * cue, the lettered square in the cue's colour centred on the cue below it,
 * and a white line down through the waveform. Every number here is read from
 * the tokens, so a drift in either the stylesheet or the tokens fails it.
 */
import { expect, test, type Page } from "@playwright/test";

const player = (page: Page) => page.getByRole("region", { name: "Preview player" });
const detail = (page: Page) => page.getByTestId("player-detail");
const pad = (page: Page, letter: string) =>
  player(page).locator('[aria-label="Hot cues"]').getByRole("button", { name: `Hot cue ${letter}`, exact: true });

/** Reads a token from the running page, so the test and the app agree. */
async function token(page: Page, name: string): Promise<number> {
  const value = await page.evaluate(
    (n) => getComputedStyle(document.documentElement).getPropertyValue(n),
    name,
  );
  return Number.parseFloat(value);
}

/** A token's colour as the browser reports it, for comparing with computed styles. */
async function tokenColour(page: Page, name: string): Promise<string> {
  return page.evaluate((n) => {
    const probe = document.createElement("i");
    probe.style.color = `var(${n})`;
    document.body.append(probe);
    const colour = getComputedStyle(probe).color;
    probe.remove();
    return colour;
  }, name);
}

/** Loads the fourth row — analysed, so it carries the mock's four hot cues. */
async function load(page: Page, query = "") {
  await page.goto(`/${query}`);
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();
  await expect(player(page).getByRole("button", { name: "Play", exact: true })).toBeEnabled();
}

/** The marker for a hot cue on the detail: its position and badge. */
async function detailMarker(page: Page, letter: string) {
  const marker = detail(page).locator(`[data-cue="${letter}"]`);
  await expect(marker.locator("b")).toBeVisible();
  return marker.evaluate((el) => {
    const band = el.closest('[data-testid="player-detail"]')!.getBoundingClientRect();
    const at = el.getBoundingClientRect();
    const badge = el.querySelector("b")!;
    const box = badge.getBoundingClientRect();
    return {
      x: at.x - band.x,
      letter: badge.textContent,
      badge: {
        top: box.y - band.y, width: box.width, height: box.height, centre: box.x + box.width / 2 - band.x,
        background: getComputedStyle(badge).backgroundColor,
        colour: getComputedStyle(badge).color,
        fontSize: getComputedStyle(badge).fontSize,
        fontWeight: getComputedStyle(badge).fontWeight,
        radius: getComputedStyle(badge).borderRadius,
      },
      layer: getComputedStyle(el).zIndex,
    };
  });
}

test("each hot cue on the detail is its lettered square in its colour, centred on the cue", async ({ page }) => {
  await load(page);
  const size = await token(page, "--s-cue-badge");
  const top = await token(page, "--s-cue-badge-top");
  const letterColour = await tokenColour(page, "--c-cue-hot-text");
  const fontSize = await token(page, "--f-size-xs");

  for (const letter of ["A", "B", "C", "D"]) {
    // Calling the cue brings it under the playhead, inside the window.
    await pad(page, letter).click();
    const m = await detailMarker(page, letter);
    expect(m.letter, letter).toBe(letter);
    // 11pt square, no rounding, 15pt under the band's top, centred on the cue.
    expect(m.badge.width, letter).toBeCloseTo(size, 1);
    expect(m.badge.height, letter).toBeCloseTo(size, 1);
    expect(m.badge.radius, letter).toBe("0px");
    expect(m.badge.top, letter).toBeCloseTo(top, 1);
    expect(m.badge.centre, letter).toBeCloseTo(m.x, 1);
    // The letter: the measured face and colour, bold.
    expect(m.badge.colour, letter).toBe(letterColour);
    expect(Number.parseFloat(m.badge.fontSize), letter).toBeCloseTo(fontSize, 1);
    expect(Number(m.badge.fontWeight), letter).toBeGreaterThanOrEqual(700);
    // The same colour the overview paints this cue.
    const overview = await page
      .getByTestId("player-overview")
      .locator(`[data-cue="${letter}"] b`)
      .evaluate((e) => getComputedStyle(e).backgroundColor);
    expect(m.badge.background, letter).toBe(overview);
  }
  // That row's four are all the default green. A row whose cues are coloured
  // shows the colour reaching the detail rather than the fallback agreeing
  // with itself: the mock's coloured sets carry four different ones.
  const overview = page.getByTestId("player-overview");
  const green = await tokenColour(page, "--c-cue-hot");
  let letters: string[] = [];
  for (let row = 0; row < 12 && letters.length === 0; row++) {
    const title = page.locator('[role="gridcell"][data-col="title"]').nth(row);
    const analysed = await title.locator("xpath=..").locator('[data-col="preview"] canvas').count();
    if (analysed === 0) continue;
    await title.dblclick();
    await expect.poll(async () => overview.locator('[data-cue]:not([data-cue=""])').count()).toBe(4);
    const found = await overview.locator('[data-cue]:not([data-cue=""])').evaluateAll((els) =>
      els.map((e) => e.getAttribute("data-cue") ?? ""),
    );
    const first = await overview
      .locator(`[data-cue="${found[0]}"] b`)
      .evaluate((e) => getComputedStyle(e).backgroundColor);
    if (first !== green) letters = found;
  }
  expect(letters).toHaveLength(4);
  const colours = new Set<string>();
  for (const letter of letters) {
    await pad(page, letter).click();
    const m = await detailMarker(page, letter);
    const above = await overview
      .locator(`[data-cue="${letter}"] b`)
      .evaluate((e) => getComputedStyle(e).backgroundColor);
    expect(m.badge.background, letter).toBe(above);
    colours.add(m.badge.background);
  }
  expect(colours.size).toBe(4);
});

test("a memory cue beside a hot cue is the red triangle over the badge, with no line of its own", async ({ page }) => {
  await load(page, "?writable=1");
  // Quantize off, so CUE and the new hot cue both land exactly on the
  // playhead rather than the beat beside it. Stopped mid-track, away from the
  // mock's memory cue near the start, CUE takes the playhead as the cue
  // point, MEMORY stores it, and the empty pad E sets a hot cue on the same
  // spot. (Calling a set pad would not do: it plays from the cue, as
  // rekordbox does, so the head does not stay on it.)
  await player(page).getByRole("button", { name: "Quantize" }).click();
  const overview = await page.getByTestId("player-overview").boundingBox();
  await page.mouse.click(
    (overview?.x ?? 0) + (overview?.width ?? 0) * 0.45,
    (overview?.y ?? 0) + (overview?.height ?? 0) / 2,
  );
  await expect(player(page).getByRole("button", { name: "Play", exact: true })).toBeVisible();
  await page.keyboard.press("c");
  await player(page).getByRole("button", { name: "Set memory cue" }).click();
  await pad(page, "E").click();

  const marker = detail(page).locator('[data-cue=""]');
  await expect(marker).toHaveCount(1);
  const hot = await detailMarker(page, "E");
  const memory = await marker.evaluate((el) => {
    const band = el.closest('[data-testid="player-detail"]')!.getBoundingClientRect();
    const head = el.querySelector("i")!.getBoundingClientRect();
    const line = getComputedStyle(el, "::before");
    return {
      x: el.getBoundingClientRect().x - band.x,
      head: {
        top: head.y - band.y, width: head.width, height: head.height, centre: head.x + head.width / 2 - band.x,
        colour: getComputedStyle(el.querySelector("i")!).backgroundColor,
        clip: getComputedStyle(el.querySelector("i")!).clipPath,
      },
      line: line.content,
      layer: getComputedStyle(el).zIndex,
    };
  });
  // On the hot cue, a 16x11pt red triangle pointing down from 8.25pt under
  // the band's top.
  expect(memory.x).toBeCloseTo(hot.x, 1);
  expect(memory.head.centre).toBeCloseTo(memory.x, 1);
  expect(memory.head.width).toBeCloseTo(await token(page, "--s-cue-marker-w"), 1);
  expect(memory.head.height).toBeCloseTo(await token(page, "--s-cue-marker-h"), 1);
  expect(memory.head.top).toBeCloseTo(await token(page, "--s-cue-marker-top"), 1);
  expect(memory.head.colour).toBe(await tokenColour(page, "--c-cue-head"));
  expect(memory.head.clip).toContain("50% 100%");
  // No line, and under the badge, which covers its point.
  expect(memory.line).toBe("none");
  expect(Number(hot.layer)).toBeGreaterThan(Number(memory.layer));
  expect(memory.head.top + memory.head.height).toBeGreaterThan(hot.badge.top);
});

test("the beat grid's heads start where they were measured, with the line under them", async ({ page }) => {
  await load(page);
  const beat = detail(page).locator('[class*="downbeat"]').first();
  await expect(beat).toBeVisible();
  const grid = await beat.evaluate((el) => {
    const band = el.closest('[data-testid="player-detail"]')!.getBoundingClientRect();
    const line = el.getBoundingClientRect();
    return {
      lineTop: line.y - band.y,
      lineBottom: band.bottom - line.bottom,
      headTop: Number.parseFloat(getComputedStyle(el, "::before").top),
      headH: Number.parseFloat(getComputedStyle(el, "::before").borderTopWidth),
    };
  });
  const headTop = await token(page, "--s-beat-marker-top");
  const headH = await token(page, "--s-beat-head-h");
  // The head is 26pt under the band's top; the line begins a point below it.
  expect(grid.lineTop + grid.headTop).toBeCloseTo(headTop, 1);
  expect(grid.headH).toBeCloseTo(headH, 1);
  expect(grid.lineTop).toBeCloseTo(headTop + headH + 1, 1);
  expect(grid.lineBottom).toBeCloseTo((await token(page, "--s-beat-marker-bottom")) + headH + 1, 1);
});
