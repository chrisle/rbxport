/**
 * The MEMORY cluster and the memory list beside the deck.
 *
 * rekordbox's own arrangement: `Set Memory Cue` stores the cue point the
 * transport's CUE set, `Call Previous/Next Memory Cue` move to the cue either
 * side of the playhead, `Delete Memory Cue` takes the one it is on, and each
 * row of the MEMORY tab has a ✕. On `M`, `B`, `N` and `X` from the Export key
 * map. The mock backend keeps cues per track, so an edit here is a real round
 * trip: written, announced, refetched, redrawn.
 */
import { expect, test, type Page } from "@playwright/test";
import { enableTooltips } from "./helpers";

const player = (page: Page) => page.getByRole("region", { name: "Preview player" });
const memoryRows = (page: Page) =>
  page.getByRole("complementary", { name: "Cue list" }).getByRole("button", { name: /^Delete memory cue \d\d:\d\d:\d\d\d$/ });
const elapsed = (page: Page) => player(page).locator('[class*="elapsed"]');
/** A MEMORY row's `mm:ss:mmm`, in seconds. */
const seconds = (time: string) => {
  const [m, s, ms] = time.split(":").map(Number) as [number, number, number];
  return m * 60 + s + ms / 1000;
};

/** Loads the fourth row — analysed, so it carries the mock's cues. */
async function load(page: Page, query = "") {
  await page.goto(`/${query}`);
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();
  await expect(player(page).getByRole("button", { name: "Play", exact: true })).toBeEnabled();
}

/** Runs the deck for a moment, pauses it, and sets the cue point there. */
async function cueSomewhereIn(page: Page) {
  await player(page).getByRole("button", { name: "Play", exact: true }).click();
  await page.waitForTimeout(700);
  await player(page).getByRole("button", { name: "Pause", exact: true }).click();
  // Paused away from the cue point, CUE sets it here (snapped to the beat).
  await page.keyboard.press("c");
}

test("with rekordbox holding the library, storing and deleting are refused and calling is not", async ({ page }) => {
  // The mock's library is read-only unless asked otherwise, which is the
  // state every other write is tested against.
  await enableTooltips(page);
  await load(page);
  const deck = player(page);
  await expect(page.getByRole("contentinfo")).toContainText("Library read-only");

  const store = deck.getByRole("button", { name: "Set memory cue" });
  await expect(store).toBeDisabled();
  await expect(store).toHaveAttribute("title", /read-only/);
  await expect(deck.getByRole("button", { name: "Delete memory cue", exact: true })).toBeDisabled();
  // A row is listed, and its ✕ is drawn but dead.
  await expect(memoryRows(page)).toHaveCount(1);
  await expect(memoryRows(page).first()).toBeDisabled();

  // Calling a cue moves the playhead and writes nothing, so it still works.
  await expect(deck.getByRole("button", { name: "Next memory cue" })).toBeEnabled();
  await expect(deck.getByRole("button", { name: "Previous memory cue" })).toBeEnabled();
  // And the key does nothing rather than something it should not.
  await page.keyboard.press("m");
  await expect(memoryRows(page)).toHaveCount(1);
});

test("MEMORY stores the cue point, and the list and both waveforms show it", async ({ page }) => {
  await load(page, "?writable=1");
  const deck = player(page);
  const overview = page.getByTestId("player-overview");
  await expect(overview.locator('[data-cue=""]')).toHaveCount(1);
  await expect(memoryRows(page)).toHaveCount(1);

  await cueSomewhereIn(page);
  await deck.getByRole("button", { name: "Set memory cue" }).click();

  // The row appears, in position order, and the overview marks it. No
  // reload: the table underneath keeps its rows.
  await expect(memoryRows(page)).toHaveCount(2);
  await expect(overview.locator('[data-cue=""]')).toHaveCount(2);
  const rows = page.getByRole("complementary", { name: "Cue list" }).getByText(/^\d\d:\d\d:\d\d\d$/);
  const times = await rows.allTextContents();
  expect(times).toHaveLength(2);
  expect(times[0]! < times[1]!).toBe(true);

  // Pressed again on the same point it stores nothing twice.
  await page.keyboard.press("m");
  await page.waitForTimeout(200);
  await expect(memoryRows(page)).toHaveCount(2);

  // ✕ in the cluster deletes the one under the playhead — the one just set —
  // and the marker goes with the row.
  await deck.getByRole("button", { name: "Delete memory cue", exact: true }).click();
  await expect(memoryRows(page)).toHaveCount(1);
  await expect(overview.locator('[data-cue=""]')).toHaveCount(1);
});

test("the M and X keys are MEMORY and its ✕, and a row's ✕ deletes that row", async ({ page }) => {
  await load(page, "?writable=1");
  await cueSomewhereIn(page);

  await page.keyboard.press("m");
  await expect(memoryRows(page)).toHaveCount(2);
  await page.keyboard.press("x");
  await expect(memoryRows(page)).toHaveCount(1);

  // The first row's ✕ takes the mock's own memory cue, leaving nothing.
  await page.keyboard.press("m");
  await expect(memoryRows(page)).toHaveCount(2);
  await memoryRows(page).first().click();
  await expect(memoryRows(page)).toHaveCount(1);
  await memoryRows(page).first().click();
  await expect(memoryRows(page)).toHaveCount(0);
  await expect(page.getByTestId("player-overview").locator('[data-cue=""]')).toHaveCount(0);
});

test("M stores the cue point, and IN puts the cue point at the head, playing or paused", async ({ page }) => {
  // rekordbox 7.2.19: MEMORY stores the deck's current cue, never the play
  // position (`UiPlayer::eventMemoryCue`), and IN is Real-Time Cue, which
  // moves the current cue to the head (`UiPlayer::eventLoopIn`). So I then M
  // is how a playing deck gets a memory cue where it is.
  await load(page, "?writable=1");
  const deck = player(page);
  const times = () =>
    page.getByRole("complementary", { name: "Cue list" }).getByText(/^\d\d:\d\d:\d\d\d$/).allTextContents();
  await expect(memoryRows(page)).toHaveCount(1);
  const [own] = await times();

  // Playing away from the cue point: M alone stores the cue point, which is
  // the track's own memory cue, so nothing new is written.
  await deck.getByRole("button", { name: "Play", exact: true }).click();
  await page.waitForTimeout(1200);
  await page.keyboard.press("m");
  await page.waitForTimeout(200);
  await expect(memoryRows(page)).toHaveCount(1);

  // I while playing, then M: the new cue is where the head was (a second or
  // so in, before the track's own cue at 2%), and the deck plays on.
  await page.keyboard.press("i");
  await page.keyboard.press("m");
  await expect(memoryRows(page)).toHaveCount(2);
  await expect(deck.getByRole("button", { name: "Pause", exact: true })).toBeVisible();
  const playing = seconds((await times()).find((t) => t !== own)!);
  expect(playing).toBeGreaterThan(0.5);
  expect(playing).toBeLessThan(3);

  // Paused further on: I then M stores there too, and the deck stays paused.
  await page.waitForTimeout(1200);
  await deck.getByRole("button", { name: "Pause", exact: true }).click();
  await page.keyboard.press("i");
  await page.keyboard.press("m");
  await expect(memoryRows(page)).toHaveCount(3);
  const paused = (await times()).map(seconds).filter((t) => t !== seconds(own!) && t !== playing);
  expect(paused).toHaveLength(1);
  expect(paused[0]).toBeGreaterThan(playing + 0.5);
  await expect(deck.getByRole("button", { name: "Play", exact: true })).toBeVisible();
});

test("◀ and ▶ call the memory cue either side of the playhead", async ({ page }) => {
  await load(page, "?writable=1");
  const deck = player(page);
  await cueSomewhereIn(page);
  await page.keyboard.press("m");
  await expect(memoryRows(page)).toHaveCount(2);

  // Two memory cues now; their positions from the list, to the second.
  const listed = await page
    .getByRole("complementary", { name: "Cue list" })
    .getByText(/^\d\d:\d\d:\d\d\d$/)
    .allTextContents();
  const [first, second] = listed.map((t) => t.slice(0, 5)) as [string, string];

  // The playhead is on the cue just stored, which is the earlier of the two:
  // CUE snapped it to a beat under a second in, and the mock's own sits at
  // 2% of the track. ▶ from there is the second; before the first, ◀ stays.
  await expect(elapsed(page)).toContainText(first);
  await deck.getByRole("button", { name: "Previous memory cue" }).click();
  await expect(elapsed(page)).toContainText(first);
  await deck.getByRole("button", { name: "Next memory cue" }).click();
  await expect(elapsed(page)).toContainText(second);
  // Past the last, ▶ stays put.
  await deck.getByRole("button", { name: "Next memory cue" }).click();
  await expect(elapsed(page)).toContainText(second);

  // ◀ walks back the same way, on the key as on the button.
  await page.keyboard.press("b");
  await expect(elapsed(page)).toContainText(first);
  await page.keyboard.press("n");
  await expect(elapsed(page)).toContainText(second);

  // Called, the cue is the cue point: C from here previews rather than
  // setting a new one, so the list does not grow when M follows.
  await page.keyboard.press("m");
  await page.waitForTimeout(200);
  await expect(memoryRows(page)).toHaveCount(2);
});

test("the MEMORY list is ten boxes whether the track has cues or none", async ({ page }) => {
  await load(page);
  const list = page.getByRole("complementary", { name: "Cue list" });
  const boxes = list.locator('[class*="cueRow"]');
  // A track with cues: its rows, then blank boxes to make ten. The blanks
  // carry a dimmed ✕ that is drawn, not a control.
  const withCues = await memoryRows(page).count();
  expect(withCues).toBeGreaterThan(0);
  await expect(boxes).toHaveCount(10);
  await expect(list.locator("[data-blank]")).toHaveCount(10 - withCues);
  await expect(list.locator("[data-blank] button")).toHaveCount(0);

  // A track with none: the same ten boxes, all blank, not an empty panel.
  for (let i = 0; i < 12 && (await memoryRows(page).count()) > 0; i++) {
    await page.locator('[role="gridcell"][data-col="title"]').nth(i).dblclick();
    await page.waitForTimeout(100);
  }
  await expect(memoryRows(page)).toHaveCount(0);
  await expect(boxes).toHaveCount(10);
  await expect(list.locator("[data-blank]")).toHaveCount(10);
});

test("memory cues draw red triangles in the overview and playlist preview", async ({page}) => {
  await load(page, "?writable=1");
  const head = page.getByTestId("player-overview").locator('[data-band="overview"][data-cue=""] i').first();
  await expect(head).toBeVisible();
  await expect(head).toHaveCSS("width", "13px");
  await expect(head).toHaveCSS("height", "10px");
  await expect(head).toHaveCSS("clip-path", "polygon(0px 0px, 100% 0px, 50% 100%)");
  const canvas = page.locator('[role="gridcell"][data-col="preview"]').nth(3).locator("canvas");
  await expect.poll(() => canvas.evaluate((c: HTMLCanvasElement) => {
    const context = c.getContext("2d");
    const pixel = context?.getImageData(Math.round(c.width * 0.02), 1, 1, 1).data;
    return pixel ? Array.from(pixel).slice(0, 3) : [];
  })).toEqual([234, 51, 35]);
  await memoryRows(page).first().click();
  await expect(head).toHaveCount(0);
  await expect.poll(() => canvas.evaluate((c: HTMLCanvasElement) => {
    const pixel = c.getContext("2d")?.getImageData(Math.round(c.width * 0.02), 1, 1, 1).data;
    return pixel ? Array.from(pixel).slice(0, 3) : [];
  })).not.toEqual([234, 51, 35]);
});
