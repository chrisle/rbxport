/**
 * The browser's chrome against the 2026-09-09 captures: the icon column at
 * the right edge, the search field in the header, the tree's and the list's
 * scrollbars, and the bars above and below the window's content.
 *
 * Every number is read from the tokens the captures were measured into, so a
 * failure means the layout drifted from a measurement, not that a test's
 * literal went stale.
 */
import { expect, test, type Locator, type Page } from "@playwright/test";

/** Reads a token from the running page, so the test and the app agree. */
async function token(page: Page, name: string): Promise<number> {
  const value = await page.evaluate(
    (n) => getComputedStyle(document.documentElement).getPropertyValue(n),
    name,
  );
  return Number.parseFloat(value);
}

async function style(target: Locator, prop: string): Promise<string> {
  return target.evaluate((el, p) => getComputedStyle(el).getPropertyValue(p), prop);
}

/** `rgb(r, g, b)` from a `#RRGGBB` token, the way getComputedStyle reports it. */
function rgb(hex: string): string {
  const n = Number.parseInt(hex.trim().slice(1), 16);
  return `rgb(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255})`;
}

async function colour(page: Page, name: string): Promise<string> {
  return rgb(
    await page.evaluate((n) => getComputedStyle(document.documentElement).getPropertyValue(n), name),
  );
}

// The capture's window, so the measurements are checked at the size they
// were made.
test.use({ viewport: { width: 1800, height: 1130 } });

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
});

const rail = (page: Page) => page.getByRole("toolbar", { name: "Browser panels" });

test("the icon column holds My Tag, Information and Sub-Browser only, centred in the measured width", async ({ page }) => {
  const buttons = rail(page).getByRole("button");
  await expect(buttons).toHaveCount(3);
  await expect(buttons.nth(0)).toHaveAccessibleName("My Tag");
  await expect(buttons.nth(1)).toHaveAccessibleName("Information");
  await expect(buttons.nth(2)).toHaveAccessibleName("Sub-Browser Window");

  const column = await rail(page).boundingBox();
  const info = await buttons.nth(1).boundingBox();
  const sub = await buttons.nth(2).boundingBox();
  const width = await token(page, "--s-right-rail-w");
  const box = await token(page, "--s-right-rail-button");
  const inset = await token(page, "--s-right-rail-inset");
  expect(column?.width).toBeCloseTo(width, 0);
  expect(info?.width).toBeCloseTo(box, 0);
  expect(info?.height).toBeCloseTo(box, 0);
  // Centred: 3pt either side of an 18pt box in a 24pt column.
  expect((info?.x ?? 0) - (column?.x ?? 0)).toBeCloseTo(inset, 0);
  expect((column?.x ?? 0) + (column?.width ?? 0) - (info?.x ?? 0) - (info?.width ?? 0)).toBeCloseTo(inset, 0);
  // On the measured pitch.
  expect((sub?.y ?? 0) - (info?.y ?? 0)).toBeCloseTo(await token(page, "--s-right-rail-pitch"), 0);
});

test("My Tag, Information and Sub-Browser sit centred as a group, not pinned where rekordbox draws them", async ({ page }) => {
  // 2026-09-20: rekordbox pins these around two boxes this app leaves out
  // (TODO.md, "Deliberate divergences"), which read as a gap rather than a
  // reason without them there — so the remaining ones are centred instead.
  // My Tag came back with its window (#539).
  const column = await rail(page).boundingBox();
  const myTag = await rail(page).getByRole("button", { name: "My Tag" }).boundingBox();
  const sub = await rail(page).getByRole("button", { name: "Sub-Browser Window" }).boundingBox();
  const groupTop = myTag?.y ?? 0;
  const groupBottom = (sub?.y ?? 0) + (sub?.height ?? 0);
  const groupMid = (groupTop + groupBottom) / 2;
  const columnMid = (column?.y ?? 0) + (column?.height ?? 0) / 2;
  expect(groupMid).toBeCloseTo(columnMid, 0);
});

test("a lit box fills with the capture's blue and bands out to the column's edge", async ({ page }) => {
  const info = rail(page).getByRole("button", { name: "Information" });
  await info.click();
  await expect(info).toHaveAttribute("aria-pressed", "true");
  expect(await style(info, "background-color")).toBe(await colour(page, "--c-rail-lit"));
  expect(await style(info, "border-top-color")).toBe(await colour(page, "--c-icon-on"));

  // The band is drawn with box-shadows of the fill colour; both must be there.
  const shadow = await style(info, "box-shadow");
  expect(shadow.split(await colour(page, "--c-rail-lit")).length - 1).toBe(2);
  await info.click();
  await expect(info).toHaveAttribute("aria-pressed", "false");
  expect(await style(info, "box-shadow")).toBe("none");
});

test("the search field is the measured height in the measured header row, with the measured face and colours", async ({ page }) => {
  // The tree grew its own search field (2026-09-20); the browser's is the
  // second one in the DOM, the tree sitting to its left.
  const field = page.getByRole("search").last();
  const input = field.getByRole("searchbox");
  const box = await field.boundingBox();
  expect(box?.height).toBeCloseTo(await token(page, "--s-search-h"), 0);
  expect(box?.width).toBeCloseTo(await token(page, "--s-search-w"), 0);

  // A point of panel above and below it in the 26pt title row, and a point of
  // black under that before the column headings.
  const head = await page.getByTestId("browser-title").first().evaluate((el) => {
    const r = el.parentElement!.getBoundingClientRect();
    return { top: r.top, height: r.height };
  });
  const rowH = await token(page, "--s-browser-head-h");
  const gap = await token(page, "--s-browser-head-gap");
  expect(head.height).toBeCloseTo(rowH + gap, 0);
  expect((box?.y ?? 0) - head.top).toBeCloseTo((rowH - (box?.height ?? 0)) / 2, 0);

  // 20pt short of the pane's right edge.
  const pane = await page.getByTestId("browser-title").first().evaluate((el) => el.parentElement!.parentElement!.getBoundingClientRect().right);
  expect(pane - (box?.x ?? 0) - (box?.width ?? 0)).toBeCloseTo(await token(page, "--s-search-inset-right"), 0);

  // Black, no border, the placeholder face and grey.
  expect(await style(field, "background-color")).toBe(await colour(page, "--c-field"));
  expect(await style(field, "border-top-width")).toBe("0px");
  expect(Number.parseFloat(await style(input, "font-size"))).toBeCloseTo(await token(page, "--f-size-search"), 1);
  await expect(input).toHaveAttribute("placeholder", "Search within this track list");
});

test("the column headings are the measured height", async ({ page }) => {
  const box = await page.getByRole("columnheader").first().boundingBox();
  expect(box?.height).toBeCloseTo(await token(page, "--s-col-header-h"), 0);
});

// Playwright's headless Chromium runs with `--hide-scrollbars`, so a bar there
// has no width to measure; its styles can still be read. WebKit draws them.
// TODO: run these geometry checks in headed Chromium with visible scrollbars.
const HIDDEN_BARS = "headless Chromium hides its scrollbars; WebKit measures them";

test("the list's scrollbars are the measured column with the thumb inset in it, and the column grid still adds up", async ({ page, browserName }) => {
  test.skip(browserName === "chromium", HIDDEN_BARS);
  const scroller = page.getByTestId("track-scroll").first();
  const w = await token(page, "--s-scrollbar-w");
  // The whole collection overflows both ways, so both bars show at rest.
  const geometry = await scroller.evaluate((el: HTMLElement) => ({
    offsetW: el.offsetWidth,
    clientW: el.clientWidth,
    offsetH: el.offsetHeight,
    clientH: el.clientHeight,
    scrollW: el.scrollWidth,
    scrollH: el.scrollHeight,
  }));
  expect(geometry.scrollH).toBeGreaterThan(geometry.clientH);
  expect(geometry.scrollW).toBeGreaterThan(geometry.clientW);
  expect(geometry.offsetW - geometry.clientW).toBe(w);
  expect(geometry.offsetH - geometry.clientH).toBe(w);

  // The bars are drawn, not the platform's: the thumb is the measured grey
  // inset by the measured amount, with square-cornered black around it.
  const thumb = await scroller.evaluate((el) => {
    const s = getComputedStyle(el, "::-webkit-scrollbar-thumb");
    return { bg: s.backgroundColor, border: s.borderTopWidth, radius: s.borderTopLeftRadius };
  });
  expect(thumb.bg).toBe(await colour(page, "--c-scroll-thumb"));
  expect(Number.parseFloat(thumb.border)).toBeCloseTo(await token(page, "--s-scroll-thumb-inset"), 1);
  const track = await scroller.evaluate((el) => getComputedStyle(el, "::-webkit-scrollbar-track").backgroundColor);
  expect(track).toBe(await colour(page, "--c-scroll-track"));

  // The column header spans the scroller's client width plus the overflow —
  // the trailing `1fr` takes what the columns leave — so nothing is clipped
  // by the bar's 12pt.
  const header = await page.getByRole("row").first().boundingBox();
  expect(header?.width).toBeCloseTo(geometry.scrollW, 0);
});

test("the tree's scrollbar is the same column, and the tree's width is unchanged by it", async ({ page, browserName }) => {
  test.skip(browserName === "chromium", HIDDEN_BARS);
  // The tree ends at its last row, so it only scrolls when the pane is
  // shorter than the list: a short window makes it so.
  await page.setViewportSize({ width: 1440, height: 420 });
  const nodes = page.getByRole("tree").first();
  const w = await token(page, "--s-scrollbar-w");
  const geometry = await nodes.evaluate((el: HTMLElement) => ({
    offsetW: el.offsetWidth,
    clientW: el.clientWidth,
    scrollH: el.scrollHeight,
    clientH: el.clientHeight,
  }));
  expect(geometry.scrollH).toBeGreaterThan(geometry.clientH);
  expect(geometry.offsetW - geometry.clientW).toBe(w);
  const thumb = await nodes.evaluate((el) => getComputedStyle(el, "::-webkit-scrollbar-thumb").backgroundColor);
  expect(thumb).toBe(await colour(page, "--c-scroll-thumb"));

  // The tree pane is still the token's width: the bar comes out of the
  // nodes' client width, not the pane's.
  const tree = await page.getByRole("navigation", { name: "Library" }).first().boundingBox();
  expect(tree?.width).toBeCloseTo(await token(page, "--s-tree-w"), 0);
});

test("a short list's horizontal scrollbar runs along the panel's foot, not under the last row", async ({ page, browserName }) => {
  // The mock's shortest playlist: 20 rows, half the pane.
  await page.getByRole("treeitem", { name: "Tech House", exact: true }).first().click();
  await expect(page.getByTestId("browser-title").first()).toContainText("Tech House (20 Tracks)");

  const scroller = page.getByTestId("track-scroll").first();
  const geometry = await scroller.evaluate((el: HTMLElement) => {
    const box = el.getBoundingClientRect();
    const panel = el.parentElement!.getBoundingClientRect();
    return {
      bottom: box.bottom,
      panelBottom: panel.bottom,
      // Rows end well above the bar.
      scrollH: el.scrollHeight,
      clientH: el.clientHeight,
      offsetH: el.offsetHeight,
      overflowsSideways: el.scrollWidth > el.clientWidth,
    };
  });
  expect(geometry.bottom).toBeCloseTo(geometry.panelBottom, 0);
  expect(geometry.scrollH).toBe(geometry.clientH);
  expect(geometry.overflowsSideways).toBe(true);
  // The horizontal bar's box: the scroller's last 12pt, ending at the panel.
  test.skip(browserName === "chromium", HIDDEN_BARS);
  const barTop = geometry.bottom - (geometry.offsetH - geometry.clientH);
  expect(geometry.bottom - barTop).toBeCloseTo(await token(page, "--s-scrollbar-w"), 0);
  expect(barTop).toBeCloseTo(geometry.panelBottom - (await token(page, "--s-scrollbar-w")), 0);
});

test("the title bar and the status bar carry the measured greys, and the status text the measured face", async ({ page }) => {
  const title = page.getByTestId("title-bar");
  expect(await style(title, "background-color")).toBe(await colour(page, "--c-titlebar"));
  expect(await style(title, "color")).toBe(await colour(page, "--c-titlebar-text"));
  expect(Number.parseFloat(await style(title, "font-size"))).toBeCloseTo(await token(page, "--f-size-window-title"), 1);

  const status = page.getByRole("contentinfo");
  const box = await status.boundingBox();
  expect(box?.height).toBeCloseTo(await token(page, "--s-status-bar-h"), 0);
  expect(await style(status, "background-color")).toBe(await colour(page, "--c-black"));
  expect(await style(status, "color")).toBe(await colour(page, "--c-status-text"));
  expect(Number.parseFloat(await style(status, "font-size"))).toBeCloseTo(await token(page, "--f-size-status"), 1);
  expect(await style(status, "font-weight")).toBe("700");
  expect(Number.parseFloat(await style(status, "padding-right"))).toBeCloseTo(await token(page, "--s-status-inset-right"), 1);
});

test("the top bar's right-hand items keep their places without the info button and the badge", async ({ page }) => {
  // The clock sits at the right edge and the gear hangs off the clock, so a
  // one-digit hour moves the gear by one tabular digit. Pinned to the
  // two-digit hour the capture was taken at.
  await page.clock.setFixedTime(new Date("2026-09-09T12:01:00"));
  await page.goto("/");
  const bar = page.getByRole("banner");
  await expect(bar.getByRole("button", { name: "Information" })).toHaveCount(0);
  await expect(bar.getByText("Professional")).toHaveCount(0);
  // Measured before the two were removed: the gear's box 243.7pt from the
  // right edge, the clock 14. Hanging off the right edge, they do not move.
  const right = async (target: Locator) => {
    const b = await target.boundingBox();
    return 1800 - (b?.x ?? 0) - (b?.width ?? 0);
  };
  expect(await right(bar.getByRole("button", { name: "Settings", exact: true }))).toBeCloseTo(243.7, 0);
  expect(await right(page.getByTestId("clock"))).toBeCloseTo(14, 0);
});
