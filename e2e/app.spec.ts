import { expect, test } from "@playwright/test";

/** Presses that take the detail waveform from any step out to the widest. */
const ZOOM_STEPS_COUNT = 8;

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("grid")).toBeVisible();
});

test("shows the library tree and a populated track table", async ({ page }) => {
  await expect(page.getByRole("treeitem", { name: /All Tracks/ })).toBeVisible();
  await expect(page.getByRole("treeitem", { name: /Melodic Vox/ })).toBeVisible();
  const rows = page.getByRole("row").filter({ has: page.getByRole("gridcell") });
  await expect.poll(() => rows.count()).toBeGreaterThan(5);
  await expect.poll(() => page.evaluate(() => performance.getEntriesByName("startup:first-rows-painted").length)).toBe(1);
});

test("virtualizes: only a window of rows is in the DOM", async ({ page }) => {
  await page.getByRole("treeitem", { name: /All Tracks/ }).click();
  const rendered = await page.getByRole("row").filter({ has: page.getByRole("gridcell") }).count();
  // 2000 tracks in the mock; a ~1130px window at 25px rows is well under 100.
  expect(rendered).toBeLessThan(100);
});

test("selects a row, and cmd-click extends the selection", async ({ page }) => {
  const rows = page.getByRole("row").filter({ has: page.getByRole("gridcell") });
  await rows.nth(1).click();
  await expect(rows.nth(1)).toHaveAttribute("aria-selected", "true");

  await rows.nth(3).click({ modifiers: ["ControlOrMeta"] });
  await expect(rows.nth(1)).toHaveAttribute("aria-selected", "true");
  await expect(rows.nth(3)).toHaveAttribute("aria-selected", "true");
  await expect(page.getByText(/Selected: 2 Tracks/)).toBeVisible();
});

test("shift-click selects a contiguous range", async ({ page }) => {
  const rows = page.getByRole("row").filter({ has: page.getByRole("gridcell") });
  await rows.nth(1).click();
  await rows.nth(5).click({ modifiers: ["Shift"] });
  await expect(page.getByText(/Selected: 5 Tracks/)).toBeVisible();
});

test("sorting a column orders the rows and toggles direction", async ({ page }) => {
  const header = page.getByRole("columnheader", { name: /Track Title/ });
  // Read the visible titles rather than just the first row: the unsorted order
  // can coincidentally start with the same track, which made an earlier version
  // of this test pass and fail for the wrong reasons.
  // By column key, not by position: columns can be reordered and hidden, so an
  // index into the row was only ever right for one particular layout.
  const titles = async () =>
    page.locator('[role="gridcell"][data-col="title"]')
        .evaluateAll((cells) => cells.slice(0, 12).map((c) => c.textContent ?? ""));

  await header.click();
  await expect(header).toHaveAttribute("data-sorted", "true");
  await expect.poll(async () => {
    const t = await titles();
    return t.length > 1 && t.every((v, i) => i === 0 || t[i - 1]!.localeCompare(v) <= 0);
  }).toBe(true);

  await header.click(); // descending
  await expect.poll(async () => {
    const t = await titles();
    return t.length > 1 && t.every((v, i) => i === 0 || t[i - 1]!.localeCompare(v) >= 0);
  }).toBe(true);
});

test("choosing a playlist swaps the view and its title", async ({ page }) => {
  await page.getByRole("treeitem", { name: /Hardstyle/ }).click();
  await expect(page.getByText(/^Hardstyle \(\d+ Tracks\)/)).toBeVisible();
});

test("scrolling loads further rows without leaving gaps", async ({ page }) => {
  await page.getByRole("treeitem", { name: /All Tracks/ }).click();
  const scroller = page.getByTestId("track-scroll");
  await scroller.evaluate((el) => { el.scrollTop = 10_000; });
  await expect
    .poll(async () =>
      scroller.locator('[role="gridcell"][data-col="title"]').first().innerText().catch(() => ""),
    )
    .not.toBe("");
});

test("analysed tracks draw a waveform, unanalysed ones stay blank", async ({ page }) => {
  // Every canvas that was drawn must have actual pixels; a blank one means the
  // fetch-and-render path silently failed, which is how this first shipped.
  //
  // Polled on the pixels rather than on the canvas count. A row's canvas
  // exists before its strip has been fetched and painted, so counting the
  // canvases and then reading them once is a race — and on a loaded runner it
  // lost, reading every canvas while it was still blank. Returning 0 while any
  // canvas is blank makes one poll cover both halves: enough rows drawn, and
  // none of them empty.
  await expect
    .poll(async () =>
      page.evaluate(() => {
        let drawn = 0;
        let blank = 0;
        for (const canvas of document.querySelectorAll<HTMLCanvasElement>(
          '[role="row"] canvas',
        )) {
          const ctx = canvas.getContext("2d");
          if (!ctx) continue;
          const data = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
          if (data.some((v) => v !== 0)) drawn++;
          else blank++;
        }
        return blank === 0 ? drawn : 0;
      }),
    )
    .toBeGreaterThan(5);
});

test("typing in the search box filters the list, and Escape clears it", async ({ page }) => {
  await page.goto("/");
  const search = page.getByRole("searchbox", { name: /search within/i });
  const title = page.getByTestId("browser-title");

  // A count appears only once the view has settled, so waiting for one is
  // waiting for the load.
  await expect(title).toContainText("Tracks)");
  const before = await title.textContent();

  await search.fill("Extended");
  // Rust does the filtering; the count in the title is what proves it landed.
  await expect(title).toContainText("Tracks)");
  await expect(title).not.toHaveText(before ?? "");

  await search.press("Escape");
  await expect(search).toHaveValue("");
  await expect(title).toHaveText(before ?? "");
});

test("the search shortcut puts the caret in the box from anywhere", async ({ page }) => {
  await page.goto("/");
  await page.getByTestId("track-scroll").click();

  // Asked of the page, not of `process.platform`. `detectPlatform` reads the
  // *browser's* navigator, and Playwright's WebKit says "Macintosh" wherever it
  // runs — so on a Linux runner the app wanted Command while the runner's
  // platform said Control, and only the webkit project failed.
  const mac = await page.evaluate(() =>
    /Mac|iPhone|iPad/.test(`${navigator.platform ?? ""} ${navigator.userAgent}`),
  );
  await page.keyboard.press(`${mac ? "Meta" : "Control"}+f`);

  const search = page.getByRole("searchbox", { name: /search within/i });
  await expect(search).toBeFocused();
});

test("a search that matches nothing empties the list without breaking it", async ({ page }) => {
  await page.goto("/");
  const search = page.getByRole("searchbox", { name: /search within/i });
  await search.fill("zzzzzzzzzz-no-such-track");

  await expect(page.getByTestId("browser-title")).toContainText("(0 Tracks)");
  // The table must still be there and still scrollable, not collapsed.
  await expect(page.getByTestId("track-scroll")).toBeVisible();
});

test("the column headers stay aligned with the rows when scrolled sideways", async ({ page }) => {
  // As a sibling above the scroller the header stayed put while the rows moved,
  // so every column sheared away from its own heading.
  await page.goto("/");
  const scroll = page.getByTestId("track-scroll");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");

  const columnLeft = async () => {
    const head = page.getByRole("row").first().getByText("BPM", { exact: true });
    const cell = await head.boundingBox();
    return cell?.x ?? 0;
  };

  const before = await columnLeft();
  await scroll.evaluate((el) => {
    el.scrollLeft = 300;
  });
  await expect
    .poll(async () => Math.round(await scroll.evaluate((el) => el.scrollLeft)))
    .toBeGreaterThan(0);

  const after = await columnLeft();
  // The heading has to travel with its column, not stay behind.
  expect(Math.round(before - after)).toBeGreaterThan(200);
});

test("the column header stays visible when scrolled down", async ({ page }) => {
  // Sticky, so moving with the rows sideways must not let it scroll away.
  await page.goto("/");
  const scroll = page.getByTestId("track-scroll");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");

  const heading = page.getByRole("row").first().getByText("BPM", { exact: true });
  const top = await heading.boundingBox();

  await scroll.evaluate((el) => {
    el.scrollTop = 2000;
  });
  await expect.poll(async () => (await heading.boundingBox())?.y ?? -1).toBeGreaterThan(0);

  const after = await heading.boundingBox();
  expect(Math.abs((after?.y ?? 0) - (top?.y ?? 0))).toBeLessThan(2);
});

test("a folder in the tree collapses and expands", async ({ page }) => {
  await page.goto("/");
  const tree = page.getByRole("tree");
  await expect(tree.getByRole("treeitem").first()).toBeVisible();

  // A folder the mock nests playlists under. Connected devices arrive later,
  // but in their own section, so they do not move this count.
  const folder = tree.getByRole("treeitem").filter({ hasText: "CURRENT" }).first();
  await expect(folder).toBeVisible();
  const before = await tree.getByRole("treeitem").count();

  await folder.getByRole("button").click();
  await expect.poll(async () => tree.getByRole("treeitem").count()).toBeLessThan(before);

  await folder.getByRole("button").click();
  await expect.poll(async () => tree.getByRole("treeitem").count()).toBe(before);
});

test("collapsing a folder does not change the selected playlist", async ({ page }) => {
  // The twisty and the row are different intentions; opening a folder must not
  // navigate.
  await page.goto("/");
  const title = page.getByTestId("browser-title");
  // Wait for the title to settle rather than for one appearance of a count:
  // the view opens asynchronously, so a single read can catch it mid-change.
  await expect(title).toContainText("Tracks)");
  let before = "";
  await expect
    .poll(async () => {
      const now = (await title.textContent()) ?? "";
      const stable = now === before;
      before = now;
      return stable;
    })
    .toBe(true);

  const folder = page.getByRole("treeitem").filter({ hasText: "CURRENT" }).first();
  await folder.getByRole("button").click();

  await expect(title).toHaveText(before);
});

test("the top bar carries what rekordbox's does, in its order", async ({ page }) => {
  // Read off a capture of 7.2.11 running: the gear, the level knob, the
  // two-channel output meter, the processor meter, then the clock. The knob
  // replaced a headphone button that rekordbox does not have there.
  await page.goto("/");
  const bar = page.getByRole("banner");
  await expect(bar.getByRole("button", { name: "Settings", exact: true })).toBeVisible();
  await expect(bar.getByRole("slider", { name: "Master level" })).toBeVisible();
  await expect(bar.getByRole("meter", { name: "Master output L" })).toBeVisible();
  await expect(bar.getByRole("meter", { name: "Master output R" })).toBeVisible();
  await expect(bar.getByRole("meter", { name: "Audio Dropout Meter" })).toBeVisible();

  // Deliberate divergences: no EXPORT dropdown, no layout or record buttons,
  // and neither the info button nor the "Professional" badge before the gear.
  await expect(page.getByText("EXPORT", { exact: true })).toHaveCount(0);
  await expect(bar.getByRole("button", { name: "Information" })).toHaveCount(0);
  await expect(bar.getByText("Professional")).toHaveCount(0);

  // Left to right in that order, with the clock last.
  const xs: number[] = [];
  for (const item of [
    bar.getByRole("button", { name: "Settings", exact: true }),
    bar.getByRole("slider", { name: "Master level" }),
    bar.getByRole("meter", { name: "Master output L" }),
    bar.getByRole("meter", { name: "Audio Dropout Meter" }),
    page.getByTestId("clock"),
  ]) {
    xs.push((await item.boundingBox())?.x ?? 0);
  }
  expect(xs).toEqual([...xs].sort((a, b) => a - b));
});

test("the level knob turns, and both meters are the measured size", async ({ page }) => {
  await page.goto("/");
  const bar = page.getByRole("banner");
  const knob = bar.getByRole("slider", { name: "Master level" });
  // The knob reads 0 to 10, and starts at 10: a decibel under full.
  await expect(knob).toHaveAttribute("aria-valuenow", "10");

  // Dragged, not clicked, and up is louder — so a drag down turns it down,
  // and the reading shows while it turns.
  const box = await knob.boundingBox();
  await page.mouse.move((box?.x ?? 0) + 9, (box?.y ?? 0) + 9);
  await page.mouse.down();
  await page.mouse.move((box?.x ?? 0) + 9, (box?.y ?? 0) + 69, { steps: 6 });
  await expect(knob).toHaveText(/^[0-9]$/);
  await page.mouse.up();
  await expect(knob).toHaveText("");
  await expect.poll(async () => Number(await knob.getAttribute("aria-valuenow")))
    .toBeLessThan(10);

  // The notch is distance-based: from 5, 60px reaches 10, then another
  // 85px releases to 11. Check either side without a timing assumption.
  await page.mouse.move((box?.x ?? 0) + 9, (box?.y ?? 0) + 9);
  await page.mouse.down();
  await page.mouse.move((box?.x ?? 0) + 9, (box?.y ?? 0) + 9 - 100, { steps: 6 });
  await expect(knob).toHaveAttribute("aria-valuenow", "10");
  await page.mouse.move((box?.x ?? 0) + 9, (box?.y ?? 0) + 9 - 150);
  await expect(knob).toHaveAttribute("aria-valuenow", "11");
  await page.mouse.up();

  // The meters are the capture's: 80pt over two channels, and 35pt alone.
  const vu = await bar.getByRole("meter", { name: "Master output L" }).boundingBox();
  const cpu = await bar.getByRole("meter", { name: "Audio Dropout Meter" }).boundingBox();
  const width = async (name: string) =>
    page.evaluate(
      (n) => Number.parseFloat(getComputedStyle(document.documentElement).getPropertyValue(n)),
      name,
    );
  expect(vu?.width).toBeCloseTo(await width("--s-top-vu-w"), 0);
  expect(cpu?.width).toBeCloseTo(await width("--s-top-cpu-w"), 0);
  expect(vu?.height).toBeCloseTo(await width("--s-top-meter-h"), 0);

  // Green, yellow, red by position rather than by level: the gradient is
  // painted across the whole track and revealed by clipping, so a loud
  // moment does not turn the quiet end of the bar red.
  const fill = bar.getByRole("meter", { name: "Master output L" }).locator("[data-mode] > span").first();
  const paint = await fill.evaluate((el) => ({
    image: getComputedStyle(el).backgroundImage,
    width: el.getBoundingClientRect().width,
    clip: getComputedStyle(el).clipPath,
  }));
  expect(paint.image).toContain("linear-gradient");
  expect(paint.width).toBeCloseTo(await width("--s-top-vu-w"), 0);
  expect(paint.clip).toContain("inset(");
});

test("the tree and the browser are separated by a black gutter", async ({ page }) => {
  // Black, and the token's width: the capture measures seven points — the tree
  // ends at 282pt, its scrollbar runs to 292 and the track list starts at 299 —
  // and it is drawn narrower than that by request. Painted the tree's own
  // colour, as it was, there was no visible gap at all.
  await page.goto("/");
  const splitter = page.getByRole("separator", { name: "Resize the library tree" });
  const box = await splitter.boundingBox();
  const width = await page.evaluate(() =>
    Number.parseFloat(
      getComputedStyle(document.documentElement).getPropertyValue("--s-tree-gutter-w"),
    ),
  );
  expect(box?.width).toBeCloseTo(width, 1);
  await expect(splitter).toHaveCSS("background-color", "rgb(0, 0, 0)");
});

test("the player's cue and play sit at the foot of the transport", async ({ page }) => {
  // Measured off design/reference/macos/playlist-player@2x.png: the skip
  // buttons are at the top of the column, then a gap, then the two circles.
  await page.goto("/");
  const player = await page.getByRole("region", { name: "Preview player" }).boundingBox();
  const cue = await page.getByRole("button", { name: "Cue", exact: true }).boundingBox();
  const play = await page
    .getByRole("region", { name: "Preview player" })
    .getByRole("button", { name: "Play", exact: true })
    .boundingBox();

  expect((cue?.y ?? 0) - (player?.y ?? 0)).toBeGreaterThan((player?.height ?? 0) / 2);
  expect(play?.y ?? 0).toBeGreaterThan(cue?.y ?? 0);
});

test("the tree can be resized by dragging the splitter", async ({ page }) => {
  await page.goto("/");
  const tree = page.getByRole("navigation", { name: "Library" });
  const splitter = page.getByRole("separator", { name: /resize the library tree/i });
  await expect(splitter).toBeVisible();

  const before = (await tree.boundingBox())?.width ?? 0;
  const handle = await splitter.boundingBox();
  await page.mouse.move((handle?.x ?? 0) + 2, (handle?.y ?? 0) + 100);
  await page.mouse.down();
  await page.mouse.move((handle?.x ?? 0) + 122, (handle?.y ?? 0) + 100, { steps: 8 });
  await page.mouse.up();

  await expect.poll(async () => (await tree.boundingBox())?.width ?? 0).toBeGreaterThan(before + 80);
});

test("the tree cannot be dragged wide enough to squeeze out the track list", async ({ page }) => {
  await page.goto("/");
  const tree = page.getByRole("navigation", { name: "Library" });
  const splitter = page.getByRole("separator", { name: /resize the library tree/i });

  const handle = await splitter.boundingBox();
  await page.mouse.move((handle?.x ?? 0) + 2, (handle?.y ?? 0) + 100);
  await page.mouse.down();
  // Far past the right edge of the window.
  await page.mouse.move(3000, (handle?.y ?? 0) + 100, { steps: 10 });
  await page.mouse.up();

  const width = (await tree.boundingBox())?.width ?? 0;
  const viewport = page.viewportSize()?.width ?? 1280;
  expect(width).toBeLessThanOrEqual(viewport * 0.5 + 1);
  // And the table is still there and usable.
  await expect(page.getByTestId("track-scroll")).toBeVisible();
});

test("the header menu lists every column and toggles one on", async ({ page }) => {
  await page.goto("/");
  const header = page.getByRole("columnheader", { name: /BPM/ });
  await header.click({ button: "right" });

  const menu = page.getByRole("menu", { name: "Columns" });
  await expect(menu).toBeVisible();
  await expect(menu.getByRole("menuitem", { name: "Auto-size all columns" })).toBeVisible();
  // Transcribed from rekordbox's own menu.
  await expect(menu.getByRole("menuitemcheckbox")).toHaveCount(39);
  await expect(menu.getByRole("menuitemcheckbox", { name: "Track Title" })).toBeDisabled();
  await expect(menu.getByRole("menuitemcheckbox", { name: "Size", exact: true })).toBeEnabled();
  await expect(menu.getByRole("menuitemcheckbox", { name: "Cloud" })).toBeEnabled();
  await expect(menu.getByRole("menuitemcheckbox", { name: "Album", exact: true })).toBeEnabled();
  await expect(menu.locator('[role="menuitemcheckbox"]:disabled')).toHaveCount(1);

  await expect(page.getByRole("columnheader", { name: /^Genre/ })).toHaveCount(0);
  await menu.getByRole("menuitemcheckbox", { name: "Genre" }).click();
  await expect(page.getByRole("columnheader", { name: /^Genre/ })).toBeVisible();
});

test("detail columns display row metadata after being enabled", async ({ page }) => {
  const show = async (name: string) => {
    await page.getByRole("columnheader", { name: "Track Title" }).click({ button: "right" });
    await page.getByRole("menu", { name: "Columns" }).getByRole("menuitemcheckbox", { name, exact: true }).click();
  };
  await show("Size");
  await expect(page.locator('[role="gridcell"][data-col="size"]').first()).toContainText("MB");
  await show("File Type");
  await expect(page.locator('[role="gridcell"][data-col="fileType"]').first()).toHaveText("MP3");
  await show("Album Artist");
  await expect(page.locator('[role="gridcell"][data-col="albumArtist"]').first()).not.toBeEmpty();
});

test("DJ Play Count sorts numerically in both directions", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("columnheader", { name: "Track Title" }).click({ button: "right" });
  await page.getByRole("menu", { name: "Columns" })
    .getByRole("menuitemcheckbox", { name: "DJ Play Count", exact: true }).click();

  const header = page.getByRole("columnheader", { name: /^DJ Play Count/ });
  const values = page.locator('[role="gridcell"][data-col="djPlayCount"]');
  await header.click();
  await expect(header).toHaveAttribute("data-sorted", "true");
  await expect.poll(async () => {
    const visible = (await values.allInnerTexts()).map((value) => value === "" ? 0 : Number(value));
    return visible.every((value, index) => index === 0 || (visible[index - 1] ?? 0) <= value);
  }).toBe(true);

  await header.click();
  await expect(values.first()).toHaveText("6");
  await expect.poll(async () => {
    const visible = (await values.allInnerTexts()).map((value) => value === "" ? 0 : Number(value));
    return visible.every((value, index) => index === 0 || (visible[index - 1] ?? 0) >= value);
  }).toBe(true);
});

test("Color sorts in rekordbox's palette order, not by name", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("columnheader", { name: "Track Title" }).click({ button: "right" });
  await page.getByRole("menu", { name: "Columns" })
    .getByRole("menuitemcheckbox", { name: "Color", exact: true }).click();

  const palette = ["", "Pink", "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple"];
  const header = page.getByRole("columnheader", { name: /^Color/ });
  const values = page.locator('[role="gridcell"][data-col="color"]');
  await header.click();
  await expect(header).toHaveAttribute("data-sorted", "true");
  await expect(values.first()).toHaveText("");
  await expect.poll(async () => {
    const visible = (await values.allInnerTexts()).map((value) => palette.indexOf(value));
    return visible.every((value, index) => value >= 0 && (index === 0 || (visible[index - 1] ?? 0) <= value));
  }).toBe(true);

  await header.click();
  await expect(values.first()).toHaveText("Purple");
  await expect.poll(async () => {
    const visible = (await values.allInnerTexts()).map((value) => palette.indexOf(value));
    return visible.every((value, index) => value >= 0 && (index === 0 || (visible[index - 1] ?? 0) >= value));
  }).toBe(true);

  // rekordbox paints the colour's ball before its name (ListViewer::
  // paintColourColumn): an 8px circle at x 5 in rekordbox::cPurple.
  const ball = values.first().locator("[data-color]");
  await expect(ball).toHaveAttribute("data-color", "Purple");
  await expect(ball).toHaveCSS("background-color", "rgb(152, 8, 248)");
  await expect(ball).toHaveCSS("width", "8px");
  await expect(ball).toHaveCSS("border-radius", "50%");
  const [cellBox, ballBox] = [await values.first().boundingBox(), await ball.boundingBox()];
  expect((ballBox?.x ?? 0) - (cellBox?.x ?? 0)).toBe(5);
});

test("Track number sorts by the tag's number, apart from the # column", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("columnheader", { name: "Track Title" }).click({ button: "right" });
  await page.getByRole("menu", { name: "Columns" })
    .getByRole("menuitemcheckbox", { name: "Track number", exact: true }).click();

  const header = page.getByRole("columnheader", { name: /^Track number/ });
  const values = page.locator('[role="gridcell"][data-col="trackNumber"]');
  await header.click();
  await expect(header).toHaveAttribute("data-sorted", "true");
  await expect(values.first()).toHaveText("1");
  await expect.poll(async () => {
    const visible = (await values.allInnerTexts()).map(Number);
    return visible.every((value, index) => index === 0 || (visible[index - 1] ?? 0) <= value);
  }).toBe(true);

  await header.click();
  await expect(values.first()).toHaveText("12");
  await expect.poll(async () => {
    const visible = (await values.allInnerTexts()).map(Number);
    return visible.every((value, index) => index === 0 || (visible[index - 1] ?? 0) >= value);
  }).toBe(true);
});

test("column menu highlights each field as the pointer moves", async ({ page }) => {
  await page.getByRole("columnheader", { name: "Track Title" }).click({ button: "right" });
  const menu = page.getByRole("menu", { name: "Columns" });
  const size = menu.getByRole("menuitemcheckbox", { name: "Size", exact: true });
  const disc = menu.getByRole("menuitemcheckbox", { name: "Disc number" });
  await size.hover();
  await expect(size).toHaveCSS("background-color", "rgb(19, 115, 235)");
  await disc.hover();
  await expect(disc).toHaveCSS("background-color", "rgb(19, 115, 235)");
  await expect(size).toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
});

test("a column can be dragged wider", async ({ page }) => {
  await page.goto("/");
  const header = page.getByRole("columnheader", { name: /BPM/ });
  const before = (await header.boundingBox())?.width ?? 0;

  const grip = header.getByRole("separator", { name: /resize bpm/i });
  const box = await grip.boundingBox();
  await page.mouse.move((box?.x ?? 0) + 2, (box?.y ?? 0) + 5);
  await page.mouse.down();
  await page.mouse.move((box?.x ?? 0) + 82, (box?.y ?? 0) + 5, { steps: 6 });
  await page.mouse.up();

  await expect.poll(async () => (await header.boundingBox())?.width ?? 0).toBeGreaterThan(before + 50);
});

test("resizing a column does not also re-sort the table", async ({ page }) => {
  // The grip lives inside the heading, whose click sorts.
  await page.goto("/");
  const first = page.getByRole("row").nth(1);
  await expect(first).toBeVisible();
  const beforeText = await first.textContent();

  const grip = page.getByRole("columnheader", { name: /BPM/ }).getByRole("separator");
  const box = await grip.boundingBox();
  await page.mouse.move((box?.x ?? 0) + 2, (box?.y ?? 0) + 5);
  await page.mouse.down();
  await page.mouse.move((box?.x ?? 0) + 60, (box?.y ?? 0) + 5, { steps: 4 });
  await page.mouse.up();

  await expect(page.getByRole("row").nth(1)).toHaveText(beforeText ?? "");
});

test("a column can be dragged to a new position", async ({ page }) => {
  await page.goto("/");
  // `goto` resolves before React has rendered the table. Establish the
  // initial layout first so an empty pre-render snapshot cannot be compared
  // against the post-drag columns.
  await expect(page.getByRole("columnheader", { name: /BPM/ })).toBeVisible();
  const order = async () =>
    page.getByRole("columnheader").evaluateAll((h) => h.map((c) => c.textContent ?? ""));

  const before = await order();
  const bpm = page.getByRole("columnheader", { name: /BPM/ });
  const key = page.getByRole("columnheader", { name: /^Key/ });
  const from = await bpm.boundingBox();
  const to = await key.boundingBox();

  // Past the threshold, so this is a reorder and not a sort click.
  await page.mouse.move((from?.x ?? 0) + 20, (from?.y ?? 0) + 8);
  await page.mouse.down();
  await page.mouse.move((to?.x ?? 0) + 10, (to?.y ?? 0) + 8, { steps: 8 });
  await page.mouse.up();

  await expect.poll(order).not.toEqual(before);
  // Nothing lost, only moved.
  expect((await order()).sort()).toEqual([...before].sort());
});

test("the column layout survives a reload", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("columnheader", { name: /BPM/ }).click({ button: "right" });
  await page.getByRole("menu", { name: "Columns" })
    .getByRole("menuitemcheckbox", { name: "Genre" }).click();
  await expect(page.getByRole("columnheader", { name: /^Genre/ })).toBeVisible();

  await page.reload();
  await expect(page.getByRole("columnheader", { name: /^Genre/ })).toBeVisible();
});

test("the source rail switches which part of the library the tree shows", async ({ page }) => {
  await page.goto("/");
  const rail = page.getByRole("tablist", { name: "Library sources" });
  // Five: Playlists, Explorer, Devices, Histories, Tag List.
  // No Collection — All Tracks is at the top of the tree and always in
  // sight, so a button that scrolls to it is a shortcut to where you already
  // are.
  await expect(rail.getByRole("tab")).toHaveCount(5);
  await expect(rail.getByRole("tab", { name: "Collection" })).toHaveCount(0);

  // A filter, as rekordbox's is: the tree shows the lit section and nothing
  // else, so the playlists go when Devices is picked and come back with
  // Playlists. All Tracks belongs to the playlists section.
  await expect(page.getByRole("treeitem").filter({ hasText: "CURRENT" })).toBeVisible();

  await rail.getByRole("tab", { name: "Devices" }).click();
  await expect(rail.getByRole("tab", { name: "Devices" })).toHaveAttribute("aria-selected", "true");
  await expect(page.getByRole("treeitem", { name: /All Tracks/ })).toHaveCount(0);
  await expect(page.getByRole("treeitem").filter({ hasText: "CURRENT" })).toHaveCount(0);

  await rail.getByRole("tab", { name: "Playlists" }).click();
  await expect(page.getByRole("treeitem", { name: /All Tracks/ })).toBeVisible();
  await expect(page.getByRole("treeitem").filter({ hasText: "CURRENT" })).toBeVisible();
});

test("unfinished sources and playlist creation are hidden", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("tab", { name: "Related Tracks" })).toHaveCount(0);
  await page.getByRole("treeitem").filter({ hasText: "CURRENT" }).click({ button: "right" });
  await expect(page.getByRole("menuitem", { name: "Create New Intelligent Playlist" })).toHaveCount(0);
});

test("the Histories section opens on the sessions rekordbox recorded", async ({ page }) => {
  await page.goto("/");
  const tree = page.getByRole("navigation", { name: "Library" });
  const rail = page.getByRole("tablist", { name: "Library sources" });

  // Something to jump to, so the button is live rather than dimmed.
  const histories = rail.getByRole("tab", { name: "Histories" });
  await expect(histories).not.toHaveAttribute("data-empty", "true");

  // Not in the playlists section: the rail filters.
  const session = tree.getByRole("treeitem").filter({ hasText: "LINK HISTORY 2026-09-04" });
  await expect(session).toHaveCount(0);

  await histories.click();
  const heading = tree.getByRole("treeitem").filter({ hasText: /^Histories$/ });
  await expect(heading).toHaveAttribute("aria-selected", "true");
  // The sessions and nothing else: the playlists are in another section.
  await expect(tree.getByRole("treeitem").filter({ hasText: "CURRENT" })).toHaveCount(0);

  // The heading and the year arrive open, the month closed and under its
  // name rather than its number, as rekordbox files it.
  await expect(tree.getByRole("treeitem").filter({ hasText: /^2026$/ })).toBeVisible();
  const month = tree.getByRole("treeitem").filter({ hasText: /^September$/ });
  await expect(month).toBeVisible();
  await expect(session).toHaveCount(0);
  await month.getByRole("button").click();

  await session.click();
  // A session is a track source of its own, not the whole collection.
  await expect(page.getByTestId("browser-title")).toContainText("LINK HISTORY 2026-09-04");
});

test("a connected device appears under Devices", async ({ page }) => {
  // A missing Devices button reads as a broken app, and a dimmed one reads as
  // nothing plugged in — so with a stick connected it must be neither.
  await page.goto("/");
  const devices = page.getByRole("tablist", { name: "Library sources" })
    .getByRole("tab", { name: "Devices" });
  await expect(devices).toBeVisible();
  await expect(devices).not.toHaveAttribute("data-empty", "true");

  await devices.click();
  await expect(page.getByRole("treeitem", { name: /DJ STICK/ })).toBeVisible();
});

test("a device row reveals a safe eject button on hover", async ({ page }) => {
  await page.getByRole("tablist", { name: "Library sources" })
    .getByRole("tab", { name: "Devices" }).click();
  const stick = page.locator('[role="treeitem"][data-kind="device"]').filter({ hasText: "DJ STICK" });
  const other = page.locator('[role="treeitem"][data-kind="device"]').filter({ hasText: "TEST" });
  await expect(stick).toBeVisible();
  await other.click();
  const eject = stick.getByRole("button", { name: "Eject DJ STICK" });
  await expect(eject).toHaveCSS("opacity", "0");
  await stick.hover();
  await expect(eject).toHaveCSS("opacity", "1");
  await eject.click();
  await expect(stick).toHaveCount(0);
  await expect(other).toHaveAttribute("aria-selected", "true");
  await expect(page.getByRole("contentinfo")).toContainText("DJ STICK safely ejected.");
});

test("ejecting the selected device returns to the library", async ({ page }) => {
  await page.getByRole("tablist", { name: "Library sources" })
    .getByRole("tab", { name: "Devices" }).click();
  const stick = page.locator('[role="treeitem"][data-kind="device"]').filter({ hasText: "DJ STICK" });
  await stick.click();
  await expect(page.getByRole("region", { name: "Device DJ STICK" })).toBeVisible();
  await stick.hover();
  await stick.getByRole("button", { name: "Eject DJ STICK" }).click();
  await expect(page.getByRole("region", { name: "Device DJ STICK" })).toHaveCount(0);
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await expect(page.getByRole("contentinfo")).toContainText("DJ STICK safely ejected.");
});

test("a device shows what is on it, and a second write only syncs the difference", async ({ page }) => {
  // A stick with nothing on it: the DJ System switch that would give it an
  // empty database on open is off here, so the first write is a full export.
  await page.addInitScript(() => {
    window.localStorage.setItem("rbl.preferences", JSON.stringify({ djSystem: { createDatabaseFolders: false } }));
  });
  await page.goto("/");
  await page.getByRole("tablist", { name: "Library sources" })
    .getByRole("tab", { name: "Devices" })
    .click();
  await page.getByRole("treeitem", { name: /DJ STICK/ }).click();

  const panel = page.getByRole("region", { name: "Device DJ STICK" });
  await expect(panel).toBeVisible();
  await expect(panel).toContainText("No export on this device yet.");
  // The General tab's space table, in the units rekordbox prints.
  await expect(panel.getByRole("table")).toContainText("Total Space32.0 GB");
  await expect(panel.getByRole("table")).toContainText("Available Space24.0 GB");

  // First write: everything goes.
  await expect(panel.getByRole("button", { name: "Export" })).toBeVisible();
  await panel.getByRole("button", { name: "Export" }).click();
  await expect(page.getByRole("contentinfo")).toContainText(/DJ STICK: Updated \d+ tracks/);
  await expect(panel).toContainText("Only what changed will be copied.");

  // Second write to the same stick: nothing changed, so nothing is copied.
  await panel.getByRole("button", { name: "Sync" }).click();
  await expect(page.getByRole("contentinfo")).toContainText(/DJ STICK: Updated 0 tracks\. Skipped \d+ tracks \(no change\)/);
});

test("a track loads into the player on a double-click, not a click", async ({ page }) => {
  await page.goto("/");
  const title = page.getByTestId("player-title");
  await expect(title).toHaveText("");

  const firstTitle = page.locator('[role="gridcell"][data-col="title"]').first();
  const text = await firstTitle.innerText();

  // Selecting and playing are different intentions: arrowing down a playlist
  // to see what is in it must not load every track on the way past.
  await firstTitle.click();
  await expect(page.locator('[role="row"][data-selected]')).toHaveCount(1);
  await expect(title).toHaveText("");

  await firstTitle.dblclick();
  await expect(title).toHaveText(text);
});

test("editing a comment does not also load the track", async ({ page }) => {
  // The comment cell opens its editor on a double-click, which would otherwise
  // reach the row underneath and start playback.
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="comment"]').first().dblclick();
  await expect(page.locator('[role="gridcell"][data-col="comment"] input')).toBeVisible();
  await expect(page.getByTestId("player-title")).toHaveText("");
});

test("a playlist numbers its rows in the order they are in", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.getByRole("treeitem").filter({ hasText: "Melodic Vox" }).first().click();

  // The position is what makes a playlist's order readable, so it is always
  // there — rekordbox does not offer it in the header menu either.
  const numbers = page.locator('[role="gridcell"][data-col="trackNo"]');
  await expect(numbers.first()).toHaveText("1");
  await expect(numbers.nth(1)).toHaveText("2");
  await expect(numbers.nth(2)).toHaveText("3");

  // And it cannot be turned off.
  await page.getByRole("columnheader", { name: "Track Title" }).click({ button: "right" });
  await expect(page.getByRole("menu")).toBeVisible();
  await expect(page.getByRole("menuitemcheckbox", { name: "#" })).toHaveCount(0);
});

test("the player draws the transport, disabled where there is no backend", async ({ page }) => {
  // In a browser there is no rbl:// scheme to stream from, so the transport is
  // drawn and inert: a player waiting for a backend, not an unfinished panel.
  await page.goto("/");
  const player = page.getByRole("region", { name: "Preview player" });
  await expect(player.getByRole("button", { name: "Play", exact: true })).toBeDisabled();
  await expect(player.getByRole("button", { name: "Cue", exact: true })).toBeDisabled();
  await expect(page.getByTestId("player-overview")).toBeVisible();
  await expect(page.getByTestId("player-detail")).toBeVisible();
});

test("the player shows a position and a total once a track is chosen", async ({ page }) => {
  await page.goto("/");
  await page.locator('[role="gridcell"][data-col="title"]').first().dblclick();
  // Time *remaining*, with the tenths in a smaller face — which is what
  // rekordbox shows in that slot, not position over total.
  await expect(page.getByTestId("player-time")).toHaveText(/^-\d?\d:\d\d\.\d$/);
});

test("the waveform is a seek target", async ({ page }) => {
  await page.goto("/");
  await page.locator('[role="gridcell"][data-col="title"]').first().dblclick();
  const overview = page.getByTestId("player-overview");
  // A progressbar, not a slider: it scrubs with the pointer and takes no keys,
  // so it is not a tab stop and never wears a focus ring.
  await expect(overview).toHaveAttribute("role", "progressbar");
  await expect(overview).not.toHaveAttribute("tabindex", /.*/);
  await expect(overview).toHaveAttribute("aria-valuenow", "0");
  // The maximum is the track's length, so the head has a scale to sit on.
  const max = await overview.getAttribute("aria-valuemax");
  expect(Number(max)).toBeGreaterThan(0);
});

test("play/pause leaves no focus ring behind on the waveform", async ({ page }) => {
  await page.goto("/");
  await page.locator('[role="gridcell"][data-col="title"]').first().dblclick();
  const overview = page.getByTestId("player-overview");
  await overview.click();
  // Clicking parks no focus, so the key that follows has nothing to ring. The
  // pointer alone never shows one; :focus-visible only turns on once a key
  // goes down, which is what made Space look like it drew the border.
  await page.keyboard.press("Space");
  await expect(overview).not.toBeFocused();
  expect(await overview.evaluate((el) => el.matches(":focus-visible"))).toBe(false);
  expect(await page.evaluate(() => document.querySelectorAll(":focus-visible").length)).toBe(0);
});

test("the gear opens Preferences, and Escape closes them", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("banner").getByRole("button", { name: "Settings" }).click();

  const dialog = page.getByRole("dialog", { name: "Preferences" });
  await expect(dialog).toBeVisible();
  // rekordbox's panes, less PLAN and CLOUD, which have nothing here.
  await expect(dialog.getByRole("tab", { name: "View", exact: true })).toHaveAttribute("aria-selected", "true");
  await expect(dialog.getByRole("tab", { name: "PLAN" })).toHaveCount(0);
  await expect(dialog.getByRole("tab", { name: "CLOUD" })).toHaveCount(0);

  // Advanced › Database holds real information, not placeholder rows.
  await dialog.getByRole("tab", { name: "Advanced" }).click();
  await expect(dialog.getByText("Database version")).toBeVisible();

  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
});

test("preferences are kept, and the key column follows the display format", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  const cell = page.locator('[role="gridcell"][data-col="key"]').first();
  const classic = (await cell.innerText()).trim();

  await page.getByRole("banner").getByRole("button", { name: "Settings" }).click();
  const dialog = page.getByRole("dialog", { name: "Preferences" });
  await dialog.getByRole("radio", { name: "Alphanumeric" }).click();
  // A Camelot code: a number and A or B.
  await expect(cell).toHaveText(/^\d{1,2}[AB]$/);
  await expect(cell).not.toHaveText(classic);

  // Kept across a reload, and Reset to defaults puts it back.
  await page.reload();
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await expect(page.locator('[role="gridcell"][data-col="key"]').first()).toHaveText(/^\d{1,2}[AB]$/);
  await page.getByRole("banner").getByRole("button", { name: "Settings" }).click();
  await page.getByRole("dialog", { name: "Preferences" }).getByRole("button", { name: "Reset to defaults" }).click();
  await expect(page.locator('[role="gridcell"][data-col="key"]').first()).toHaveText(classic);
});

test("Library Protection refuses edits the way a running rekordbox does", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.getByRole("banner").getByRole("button", { name: "Settings" }).click();
  const dialog = page.getByRole("dialog", { name: "Preferences" });
  await dialog.getByRole("tab", { name: "Advanced" }).click();
  await dialog.getByRole("tab", { name: "Browse", exact: true }).click();
  await dialog.getByRole("switch", { name: "Protect library edit." }).click();
  await page.keyboard.press("Escape");

  // The status bar says so, and a drop onto a playlist is refused rather than raced.
  await expect(page.getByRole("contentinfo")).toContainText(/read-only/i);
  const row = page.getByRole("row").filter({ has: page.getByRole("gridcell") }).first();
  const target = page.getByRole("treeitem").filter({ hasText: "Hardstyle" }).first();
  await row.dragTo(target);
  await expect(page.getByRole("contentinfo")).toContainText("Library Protection");
  await page.getByRole("contentinfo").getByRole("button", { name: "Open Preferences" }).click();
  await expect(dialog.getByRole("tab", { name: "Advanced", exact: true })).toHaveAttribute("aria-selected", "true");
  await expect(dialog.getByRole("tab", { name: "Browse", exact: true })).toHaveAttribute("aria-selected", "true");
  const protection = dialog.getByRole("switch", { name: "Protect library edit." });
  await expect(protection).toBeChecked();
  await protection.click();
  await dialog.getByRole("dialog", { name: "Library Protection" })
    .getByRole("button", { name: "Unlock anyway" })
    .click();
  await expect(protection).not.toBeChecked();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("contentinfo").getByRole("alert")).toHaveCount(0);
});

test("the Traffic Light lights the keys that go with the loaded track's", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  const keys = page.locator('[role="gridcell"][data-col="key"]');
  // Nothing loaded: nothing lit.
  await expect(page.locator('[role="gridcell"][data-col="key"][data-lit]')).toHaveCount(0);

  await page.locator('[role="gridcell"][data-col="title"]').first().dblclick();
  const loaded = (await keys.first().innerText()).trim();
  // The loaded track's own key is lit, and so are the ones around it on the wheel.
  await expect(keys.first()).toHaveAttribute("data-lit", "true");
  const lit = page.locator('[role="gridcell"][data-col="key"][data-lit]');
  await expect.poll(() => lit.count()).toBeGreaterThan(1);
  for (const text of await lit.allInnerTexts()) {
    expect(text.trim()).not.toBe("");
  }

  // The MASTER menu picks the deck. PLAYER B holds nothing, so nothing lights.
  const menu = page.getByRole("button", { name: "Traffic Light deck", exact: true });
  await expect(menu).toHaveText("MASTER");
  await menu.click();
  await expect(page.getByRole("menuitemradio", { name: "PLAYER A - Traffic Light" })).toBeVisible();
  await expect(page.getByRole("menuitemradio", { name: "PLAYER B - Traffic Light" })).toHaveCount(0);
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Layout" }).click();
  await page.getByRole("menuitemradio", { name: "2 PLAYER" }).click();
  await menu.click();
  await page.getByRole("menuitemradio", { name: "PLAYER B - Traffic Light" }).click();
  await expect(menu).toHaveText("PLAYER B");
  await expect(lit).toHaveCount(0);
  await page.getByRole("button", { name: "Layout" }).click();
  await page.getByRole("menuitemradio", { name: "1 PLAYER", exact: true }).click();
  await expect(menu).toHaveText("PLAYER A");
  await expect(keys.first()).toHaveAttribute("data-lit", "true");
  await menu.click();
  await page.getByRole("menuitemradio", { name: "PLAYER A - Traffic Light" }).click();
  await expect(keys.first()).toHaveAttribute("data-lit", "true");
  expect((await keys.first().innerText()).trim()).toBe(loaded);

  // Same Key in Preferences narrows it to the one key.
  await page.getByRole("banner").getByRole("button", { name: "Settings" }).click();
  const dialog = page.getByRole("dialog", { name: "Preferences" });
  await dialog.getByRole("combobox", { name: "Traffic Light" }).selectOption("same");
  await page.keyboard.press("Escape");
  for (const text of await lit.allInnerTexts()) {
    expect(text.trim()).toBe(loaded);
  }
});

test("the # column sorts a playlist by its own order, and back", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.getByRole("treeitem").filter({ hasText: "Melodic Vox" }).first().click();
  const titles = page.locator('[role="gridcell"][data-col="title"]');
  const trackNos = page.locator('[role="gridcell"][data-col="trackNo"]');
  await expect(page.getByTestId("browser-title")).toContainText("Melodic Vox");
  const first = (await titles.first().innerText()).trim();
  const second = (await titles.nth(1).innerText()).trim();

  // These are the entries' stored playlist positions, not visible row numbers.
  await expect(trackNos.first()).toHaveText("1");
  await expect(trackNos.nth(1)).toHaveText("2");

  // The order it opens in is its own, and the heading says so.
  const head = page.getByRole("columnheader", { name: /^#/ });
  await head.click();
  // Reversed: what was first is now last, and each entry keeps its stored
  // playlist position instead of being renumbered for the visible rows.
  await expect(titles.first()).not.toHaveText(first);
  await expect(titles.last()).toHaveText(first);
  await expect(trackNos.first()).toHaveText("30");
  await expect(trackNos.last()).toHaveText("1");
  await head.click();
  await expect(titles.first()).toHaveText(first);
  await expect(titles.nth(1)).toHaveText(second);
  await expect(trackNos.first()).toHaveText("1");
  await expect(trackNos.nth(1)).toHaveText("2");
});

test("the Layout tab hides the Explorer and shows playlist counts", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  const rail = page.getByRole("tablist", { name: "Library sources" });
  await rail.getByRole("tab", { name: "Explorer" }).click();
  await expect(page.locator('[role="treeitem"][data-kind="explorer"]')).toHaveCount(1);

  await page.getByRole("banner").getByRole("button", { name: "Settings" }).click();
  const dialog = page.getByRole("dialog", { name: "Preferences" });
  await dialog.getByRole("tab", { name: "Layout", exact: true }).click();
  await dialog.getByRole("checkbox", { name: "Explorer" }).click();
  await dialog.getByRole("checkbox", { name: /number of tracks in a playlist/ }).click();
  await page.keyboard.press("Escape");

  // Gone from the tree, and its rail button dims because it leads nowhere.
  await expect(page.locator('[role="treeitem"][data-kind="explorer"]')).toHaveCount(0);
  await expect(rail.getByRole("tab", { name: "Explorer" })).toHaveAttribute("data-empty", "true");
  await rail.getByRole("tab", { name: "Playlists" }).click();
  await expect(page.getByRole("treeitem").filter({ hasText: "Hardstyle" }).first()).toContainText(/\(\d+\)/);
});

test("settings can put the columns back", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("columnheader", { name: /BPM/ }).click({ button: "right" });
  await page.getByRole("menu", { name: "Columns" })
    .getByRole("menuitemcheckbox", { name: "Genre" }).click();
  await expect(page.getByRole("columnheader", { name: /^Genre/ })).toBeVisible();

  await page.getByRole("banner").getByRole("button", { name: "Settings" }).click();
  await page.getByRole("dialog", { name: "Preferences" }).getByRole("tab", { name: "Layout", exact: true }).click();
  await page.getByRole("button", { name: "Reset columns" }).click();
  await expect(page.getByRole("columnheader", { name: /^Genre/ })).toHaveCount(0);
});

test("tracks can be dragged from the browser onto a playlist", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");

  const row = page.getByRole("row").filter({ has: page.getByRole("gridcell") }).first();
  const target = page.getByRole("treeitem").filter({ hasText: "Hardstyle" }).first();

  await row.dragTo(target);

  // The status bar reports what happened rather than leaving it silent.
  await expect(page.getByRole("contentinfo")).toContainText(/Added \d+ track/);
});

test("a selection dragged onto a deck loads its first track, not the one under the hand", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  const rows = page.getByRole("row").filter({ has: page.getByRole("gridcell") });
  const topTitle = (await rows.nth(2).locator('[data-col="title"]').innerText()).trim();

  // Rows 3 to 5, then the drag starts on the last of them.
  await rows.nth(2).locator('[data-col="title"]').click();
  await rows.nth(4).locator('[data-col="title"]').click({ modifiers: ["Shift"] });
  await expect(page.getByText("Selected: 3 Tracks")).toBeVisible();
  await rows.nth(4).dragTo(page.getByRole("region", { name: "Preview player" }));

  // rekordbox loads the first of a dropped selection in list order.
  await expect(page.getByRole("region", { name: "Preview player" }).getByTestId("player-title")).toHaveText(topTitle);
});

test("a track dragged onto a deck loads there", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");

  // Two decks, so the second one can be loaded without touching the first.
  await page.getByRole("button", { name: "Layout" }).click();
  await page.getByRole("menuitemradio", { name: "2 PLAYER" }).click();
  const decks = page.getByRole("region", { name: /^Preview player/ });
  await expect(decks).toHaveCount(2);

  const rows = page.getByRole("row").filter({ has: page.getByRole("gridcell") });
  const second = rows.nth(1);
  const title = (await second.locator('[data-col="title"]').innerText()).trim();

  // Onto deck B, which the browser selection never loads: only a drop does.
  await second.dragTo(page.getByRole("region", { name: "Preview player B" }));
  await expect(
    page.getByRole("region", { name: "Preview player B" }).getByTestId("player-title"),
  ).toHaveText(title);
});

test("a track dragged onto the deck loads it in every layout that draws one", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  const rows = page.getByRole("row").filter({ has: page.getByRole("gridcell") });
  const pick = async (n: number) => ({
    row: rows.nth(n),
    title: (await rows.nth(n).locator('[data-col="title"]').innerText()).trim(),
  });
  const choose = async (layout: string) => {
    await page.getByRole("button", { name: "Layout" }).click();
    await page.getByRole("menuitemradio", { name: layout }).click();
  };

  // 1 PLAYER: the only deck is A, and dragging to it must not depend on the
  // row being the selected one — the hand is on row 5, the selection stays
  // where it was.
  await choose("1 PLAYER");
  const deckA = page.getByRole("region", { name: "Preview player", exact: true });
  const first = await pick(5);
  await first.row.dragTo(deckA);
  await expect(deckA.getByTestId("player-title")).toHaveText(first.title);

  // SIMPLE PLAYER: the strip is the same deck A, and shows the sleeve as the
  // drop target while a row is in the air.
  await choose("SIMPLE PLAYER");
  const strip = page.getByTestId("simple-player");
  await expect(strip.getByTestId("simple-player-title")).toHaveText(first.title);
  const second = await pick(6);
  await second.row.hover();
  await page.mouse.down();
  await strip.hover();
  await expect(strip).toHaveAttribute("data-droppable", "true");
  await page.mouse.up();
  await expect(strip).not.toHaveAttribute("data-droppable");
  await expect(strip.getByTestId("simple-player-title")).toHaveText(second.title);

  // 2 PLAYER: each deck takes its own drop, and one does not disturb the other.
  await choose("2 PLAYER");
  const deckB = page.getByRole("region", { name: "Preview player B" });
  const third = await pick(7);
  await third.row.dragTo(deckB);
  await expect(deckB.getByTestId("player-title")).toHaveText(third.title);
  await expect(deckA.getByTestId("player-title")).toHaveText(second.title);
  const fourth = await pick(8);
  await fourth.row.dragTo(deckA);
  await expect(deckA.getByTestId("player-title")).toHaveText(fourth.title);
  await expect(deckB.getByTestId("player-title")).toHaveText(third.title);
});

test("the track menu loads a track into either player", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.getByRole("button", { name: "Layout" }).click();
  await page.getByRole("menuitemradio", { name: "2 PLAYER" }).click();

  const cell = page.locator('[role="gridcell"][data-col="title"]').first();
  const title = (await cell.innerText()).trim();
  await cell.click({ button: "right" });

  const menu = page.getByRole("menu", { name: "Track" });
  await expect(menu).toBeVisible();
  // rekordbox's own list, with Load live because there are players to load
  // into — it is greyed when the layout draws none.
  await menu.getByRole("menuitem", { name: "Load", exact: true }).hover();
  await menu.getByRole("menuitem", { name: "Load track to player 2" }).click();

  await expect(
    page.getByRole("region", { name: "Preview player B" }).getByTestId("player-title"),
  ).toHaveText(title);
});

test("clicking an empty deck loads the selected track, and selecting alone does not", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.getByRole("button", { name: "Layout" }).click();
  await page.getByRole("menuitemradio", { name: "2 PLAYER" }).click();

  const deckB = page.getByRole("region", { name: "Preview player B" });
  const row = page.getByRole("row").filter({ has: page.getByRole("gridcell") }).nth(3);
  const title = (await row.locator('[data-col="title"]').innerText()).trim();

  await row.click();
  // Arrowing through a playlist must not load forty tracks on the way past.
  await expect(deckB.getByTestId("player-title")).toHaveText("");

  await deckB.getByRole("button", { name: "Load the selected track" }).click();
  await expect(deckB.getByTestId("player-title")).toHaveText(title);
  // Loaded, the same sleeve is the eject button again.
  await expect(deckB.getByRole("button", { name: "Eject" })).toBeVisible();
});

test("only playlists offer themselves as a drop target", async ({ page }) => {
  // A folder holds playlists, so dropping tracks into one would have to invent
  // which playlist was meant.
  await page.goto("/");
  const row = page.getByRole("row").filter({ has: page.getByRole("gridcell") }).first();

  await row.hover();
  await page.mouse.down();
  await page.mouse.move(200, 400, { steps: 4 });

  const playlists = page.locator('[role="treeitem"][data-droppable]');
  await expect.poll(async () => playlists.count()).toBeGreaterThan(0);
  const folders = page.getByRole("treeitem").filter({ hasText: "CURRENT" });
  await expect(folders.first()).not.toHaveAttribute("data-droppable", "true");
  // Offering is not lighting up: while the pointer is over the list, no
  // playlist is marked, and none carries the outline every one of them used
  // to get for the whole drag.
  await expect(page.locator('[role="treeitem"][data-over]')).toHaveCount(0);
  const outlined = await playlists.evaluateAll((els) =>
    els.filter((el) => getComputedStyle(el).outlineStyle !== "none").length,
  );
  expect(outlined).toBe(0);
  await page.mouse.up();
});

test("only the playlist under a dragged track lights up, and it goes dark when the drag leaves", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  const row = page.getByRole("row").filter({ has: page.getByRole("gridcell") }).first();
  const playlists = page.locator('[role="treeitem"][data-kind="playlist"]');
  const first = playlists.nth(0);
  const second = playlists.nth(1);

  // Stepped moves, not `hover()`: a native drag only follows the pointer
  // through the moves the browser sees.
  const into = async (target: typeof first) => {
    const box = await target.boundingBox();
    await page.mouse.move((box?.x ?? 0) + 40, (box?.y ?? 0) + 10, { steps: 6 });
  };
  await row.hover();
  await page.mouse.down();
  await page.mouse.move(200, 400, { steps: 4 });
  await into(first);
  await expect(first).toHaveAttribute("data-over", "true");
  await expect(page.locator('[role="treeitem"][data-over]')).toHaveCount(1);
  await into(second);
  await expect(second).toHaveAttribute("data-over", "true");
  await expect(first).not.toHaveAttribute("data-over");
  await page.mouse.move(600, 600, { steps: 6 });
  await expect(page.locator('[role="treeitem"][data-over]')).toHaveCount(0);
  await page.mouse.up();
});

test("a track can be rated by clicking its stars", async ({ page }) => {
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");

  // Not the first row: it sits under the sticky column header.
  const stars = page.locator('[data-col="rating"] [role="radiogroup"]').nth(3);
  await stars.getByRole("radio", { name: "4 of 5" }).click();

  await expect(page.getByRole("contentinfo")).toContainText("Rated 4 of 5");
});

test("clicking the star already set clears the rating", async ({ page }) => {
  // The only way back to no rating, and how rekordbox behaves.
  await page.goto("/?writable=1");
  const stars = page.locator('[data-col="rating"] [role="radiogroup"]').nth(3);
  // The mock's ratings are deterministic but not zero, so pick a star the row
  // is not already on: clicking the current one clears rather than sets.
  const checked = await stars.locator('[aria-checked="true"]').count();
  const current = checked === 0
    ? 0
    : Number((await stars.locator('[aria-checked="true"]').getAttribute("aria-label"))?.[0] ?? 0);
  const target = current === 2 ? 4 : 2;

  await stars.getByRole("radio", { name: `${target} of 5` }).click();
  await expect(page.getByRole("contentinfo")).toContainText(`Rated ${target} of 5`);

  await stars.getByRole("radio", { name: `${target} of 5` }).click();
  await expect(page.getByRole("contentinfo")).toContainText("Rating cleared");
});

test("a comment is edited in place, and Escape abandons the edit", async ({ page }) => {
  await page.goto("/?writable=1");
  const cell = page.locator('[role="gridcell"][data-col="comment"]').nth(3);
  const before = await cell.innerText();

  await cell.dblclick();
  const field = page.getByRole("textbox", { name: "Comment" });
  await field.fill("changed my mind");
  await field.press("Escape");

  await expect(page.locator('[role="gridcell"][data-col="comment"]').nth(3)).toHaveText(before);
  await expect(page.getByRole("contentinfo")).not.toContainText("Comment saved");
});

test("a comment commits on Enter", async ({ page }) => {
  await page.goto("/?writable=1");
  const cell = page.locator('[role="gridcell"][data-col="comment"]').nth(3);
  await cell.dblclick();
  const field = page.getByRole("textbox", { name: "Comment" });
  await field.fill("5A - Am - 128");
  await field.press("Enter");

  await expect(page.getByRole("contentinfo")).toContainText("Comment saved");
});

test("a second single click edits a selected comment", async ({ page }) => {
  await page.goto("/?writable=1");
  const cell = page.locator('[role="gridcell"][data-col="comment"]').nth(3);
  await cell.click();
  await expect(cell.locator("input")).toHaveCount(0);
  await cell.click();
  // Non-title fields do not need to wait for the title's double-click-to-load
  // grace period; the second click opens the field in the same event turn.
  await expect(cell.locator("input")).toBeFocused({ timeout: 250 });
});

test("the metadata columns are typed over in the list, and Escape abandons", async ({ page }) => {
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");

  // Artist commits on Enter and the row shows it without a reload.
  const artist = page.locator('[role="gridcell"][data-col="artist"]').nth(3);
  await artist.dblclick();
  const field = page.getByRole("textbox", { name: "Artist" });
  await expect(field).toBeVisible();
  await field.fill("Edited Artist Name");
  await field.press("Enter");
  await expect(page.getByRole("contentinfo")).toContainText("Artist saved.");
  await expect(page.locator('[role="gridcell"][data-col="artist"]').nth(3))
    .toHaveText("Edited Artist Name");

  // Label abandons on Escape, and nothing is written. Album and Genre edit
  // the same way but are not columns the list shows by default.
  const label = page.locator('[role="gridcell"][data-col="label"]').nth(2);
  const was = await label.innerText();
  await label.dblclick();
  const labelField = page.getByRole("textbox", { name: "Label" });
  await labelField.fill("Should Not Stick");
  await labelField.press("Escape");
  await expect(page.locator('[role="gridcell"][data-col="label"]').nth(2)).toHaveText(was);
  await expect(page.getByRole("contentinfo")).not.toContainText("Label saved.");
});

test("the title cell still loads a track on double-click", async ({
  page,
}) => {
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();
  await expect(page.locator('[role="gridcell"][data-col="title"] input')).toHaveCount(0);
  // And it loaded, which is what the double click was for.
  await expect(page.getByTestId("player-title")).not.toHaveText("");
});

test("a second single click edits a playlist title", async ({ page }) => {
  await page.goto("/?writable=1");
  await page.getByRole("treeitem").filter({ hasText: "Melodic Vox" }).first().click();
  const cell = page.locator('[role="gridcell"][data-col="title"]').nth(3);
  await cell.click();
  await expect(cell.locator("input")).toHaveCount(0);
  await cell.click();
  const field = cell.locator("input");
  expect(await field.count()).toBe(0);
  await expect(field).toBeFocused();
  await field.fill("Edited Playlist Title");
  await field.press("Enter");
  await expect(cell).toHaveText("Edited Playlist Title");
  await expect(page.getByRole("contentinfo")).toContainText("Title saved.");
  await expect(page.getByTestId("player-title")).toHaveText("");

  await cell.click();
  await cell.locator("input").fill("Discard this title");
  await cell.locator("input").press("Escape");
  await expect(cell).toHaveText("Edited Playlist Title");

  await cell.dblclick();
  await expect(cell.locator("input")).toHaveCount(0);
  await expect(page.getByTestId("player-title")).toHaveText("Edited Playlist Title");
});

// Metadata has no committed-edit undo command; the Edit history belongs to
// beat-grid changes. Exercise the supported save/reopen contract here.
// TODO: add metadata undo coverage when that feature is implemented.
test("a committed title survives reopening its playlist", async ({ page }) => {
  await page.goto("/?writable=1");
  const playlist = page.getByRole("treeitem").filter({ hasText: "Melodic Vox" }).first();
  await playlist.click();
  await expect(page.getByTestId("browser-title")).toContainText("Melodic Vox");
  const cell = page.locator('[role="gridcell"][data-col="title"]').nth(3);
  await expect(cell).not.toBeEmpty();
  const original = await cell.innerText();
  const edited = `${original} (saved title test)`;

  await cell.click();
  await cell.click();
  const input = cell.locator("input");
  await expect(input).toBeFocused();
  await input.fill(edited);
  await input.press("Enter");
  await expect(input).toHaveCount(0);
  await expect(cell).toHaveText(edited);
  await expect(page.getByRole("contentinfo")).toContainText("Title saved.");

  // Reopening the playlist must read the saved value from the backend.
  await page.getByRole("treeitem").filter({ hasText: "All Tracks" }).first().click();
  await playlist.click();
  await expect(cell).toHaveText(edited);
});

test("the key column is not typed over: it is checked against the library's own", async ({
  page,
}) => {
  // A free-typed key would be refused after the fact, so it is the
  // information panel's list rather than a cell to type in.
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="key"]').nth(3).dblclick();
  await expect(page.locator('[role="gridcell"][data-col="key"] input')).toHaveCount(0);
});

test("editing follows rekordbox opening and closing without reloading", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  const cell = page.locator('[role="gridcell"][data-col="title"]').nth(3);
  await cell.click();
  await cell.click();
  const warning = page.getByRole("contentinfo").getByRole("alert");
  await expect(warning).toContainText("rekordbox is running");

  // Change the mock process state without remounting the app.
  await page.evaluate(() => history.replaceState(null, "", "/?writable=1"));
  await expect(page.getByRole("contentinfo")).not.toContainText("Library read-only");
  await expect(warning).toHaveCount(0);
  await cell.click();
  await expect(cell.locator("input")).toBeFocused();
  const originalTitle = await cell.locator("input").inputValue();
  await cell.locator("input").fill("Must not save after locking");

  await page.evaluate(() => history.replaceState(null, "", "/"));
  await expect(page.getByRole("contentinfo")).toContainText("Library read-only");
  await expect(cell.locator("input")).toHaveAttribute("readonly", "");
  await cell.locator("input").press("Enter");
  await expect(cell).toHaveText(originalTitle);
  await cell.click();
  await expect(cell.locator("input")).toHaveCount(0);
  await expect(warning).toContainText("rekordbox is running");
});

test("with the library held by rekordbox the cells are not editable at all", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="artist"]').nth(3).dblclick();
  await expect(page.locator('[role="gridcell"][data-col="artist"] input')).toHaveCount(0);
  const warning = page.getByRole("contentinfo").getByRole("alert");
  await expect(warning).toHaveText("Editing is locked while rekordbox is running. Quit rekordbox to enable editing.");
  await expect(warning).toHaveCount(0, { timeout: 12000 });

  const title = page.locator('[role="gridcell"][data-col="title"]').nth(3);
  await title.click();
  await title.click();
  await expect(title.locator("input")).toHaveCount(0);
  await expect(warning).toHaveText("Editing is locked while rekordbox is running. Quit rekordbox to enable editing.");
});

test("the player marks a track's cues on its waveforms", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();

  // The overview spans the whole track, so it holds every cue. The detail is a
  // window and holds only what falls inside it.
  const overview = page.getByTestId("player-overview");
  await expect.poll(async () => overview.locator('[data-cue]:not([data-cue=""])').count()).toBe(4);
  await expect(overview.locator('[data-cue=""]')).toHaveCount(1);
  // The detail starts at the top of the track, spanning its first 8%. The
  // mock's memory cue sits at 2% and its first hot cue at 12%, so exactly one
  // marker belongs there — which is what makes it a window rather than a
  // second copy of the overview.
  const detail = page.getByTestId("player-detail");
  await expect(detail.locator('[data-cue=""]')).toHaveCount(1);
  await expect(detail.locator('[data-cue]:not([data-cue=""])')).toHaveCount(0);
});

test("the Analysis pane says what Auto Analysis will do", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("banner").getByRole("button", { name: "Settings" }).click();
  const dialog = page.getByRole("dialog", { name: "Preferences" });
  await dialog.getByRole("tab", { name: "Analysis" }).click();

  const section = dialog.getByRole("region", { name: "Track Analysis" });
  const automatic = section.getByRole("switch", { name: "Automatic analysis" });
  await expect(automatic).not.toBeChecked();
  await automatic.click();
  await expect(automatic).toBeChecked();
});

test("a rating appears at once rather than waiting for the reload", async ({ page }) => {
  // A write makes the backend re-read the library; waiting for that before the
  // star fills in feels broken.
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");

  const stars = page.locator('[data-col="rating"] [role="radiogroup"]').nth(3);
  await stars.getByRole("radio", { name: "5 of 5" }).click();
  await expect(stars.getByRole("radio", { name: "5 of 5" })).toHaveAttribute("aria-checked", "true");
});

test("an edited comment shows before the backend catches up", async ({ page }) => {
  await page.goto("/?writable=1");
  const cell = page.locator('[role="gridcell"][data-col="comment"]').nth(3);
  await cell.dblclick();
  const field = page.getByRole("textbox", { name: "Comment" });
  await field.fill("shown at once");
  await field.press("Enter");

  await expect(page.locator('[role="gridcell"][data-col="comment"]').nth(3))
    .toHaveText("shown at once");
});

test("each kind of table remembers its own columns", async ({ page }) => {
  // rekordbox keys these by context in browseSetting.xml, not per playlist, so
  // browsing a second playlist does not start from scratch.
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");

  // Add Genre while a playlist is selected.
  await page.getByRole("columnheader", { name: /BPM/ }).click({ button: "right" });
  await page.getByRole("menu", { name: "Columns" })
    .getByRole("menuitemcheckbox", { name: "Genre" }).click();
  await expect(page.getByRole("columnheader", { name: /^Genre/ })).toBeVisible();

  // The collection is a different context and keeps the defaults.
  await page.getByRole("treeitem", { name: /All Tracks/ }).click();
  await expect(page.getByRole("columnheader", { name: /^Genre/ })).toHaveCount(0);

  // Back to a playlist and the change is still there.
  await page.getByRole("treeitem").filter({ hasText: "Melodic Vox" }).first().click();
  await expect(page.getByRole("columnheader", { name: /^Genre/ })).toBeVisible();
});

test("analysing a selection reports progress and can be stopped", async ({ page }) => {
  // Analysis writes to the library, so the mock has to say it is writable.
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");

  // Select a run of rows, then analyse them.
  const rows = page.getByRole("row").filter({ has: page.getByRole("gridcell") });
  await rows.nth(2).click();
  await rows.nth(9).click({ modifiers: ["Shift"] });
  await page.keyboard.press("Shift+Meta+A");
  await page.getByRole("dialog", { name: "Analysis Setting" }).getByRole("button", { name: "OK", exact: true }).click();

  const status = page.getByRole("contentinfo");
  await expect(status).toContainText(/Analyzing 8 tracks\s*\(\d+%\)/);
  await expect(status.getByRole("progressbar", { name: "Analysis progress" })).toBeVisible();
  await expect(status.getByRole("button", { name: "Stop" })).toBeVisible();

  await status.getByRole("button", { name: "Stop" }).click();
  // The running track finishes, then the run ends and the readout goes back.
  await expect(status.getByRole("button", { name: "Stop" })).toHaveCount(0);
});

test("a track that cannot be analysed does not stop the run", async ({ page }) => {
  // The mock fails every seventh track, so a long enough run hits one.
  await page.goto("/?writable=1");
  const rows = page.getByRole("row").filter({ has: page.getByRole("gridcell") });
  await rows.nth(0).click();
  await rows.nth(14).click({ modifiers: ["Shift"] });
  await page.keyboard.press("Shift+Meta+A");
  await page.getByRole("dialog", { name: "Analysis Setting" }).getByRole("button", { name: "OK", exact: true }).click();

  const status = page.getByRole("contentinfo");
  await expect(status).toContainText("failed", { timeout: 15_000 });
  // And it carried on rather than stopping there.
  await expect(status.getByRole("button", { name: "Stop" })).toHaveCount(0, { timeout: 15_000 });
});

test("settings has the LINK switch, and says why a browser cannot turn it on", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("banner").getByRole("button", { name: "Settings" }).click();
  const dialog = page.getByRole("dialog", { name: "Preferences" });
  await dialog.getByRole("tab", { name: "PRO DJ LINK" }).click();

  const section = page.getByRole("region", { name: "Link" });
  await expect(page.getByTestId("link-on")).toHaveCount(0);
  await expect(section.getByRole("columnheader")).toHaveText(["Interface", "Connection", "Adapter", "IP address"]);
  const wired = section.getByRole("row").filter({ hasText: "en11" });
  await expect(wired).toContainText("Wired");
  await expect(wired).toContainText("USB Ethernet");
  await section.getByRole("radio", { name: "en11", exact: true }).check();
  await expect(section.getByRole("radio", { name: "en11", exact: true })).toBeChecked();

  await section.getByRole("button", { name: "Connect to PRO DJ LINK" }).click();
  await expect(section).toContainText("browser has no access to the network");
  // A LINK that cannot be turned on offers no button; the reason stands in its place.
  await expect(section.getByRole("button", { name: "Connect to PRO DJ LINK" })).toHaveCount(0);
});

test("the automatic LINK switch sits at the right edge with left-aligned help", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("banner").getByRole("button", { name: "Settings" }).click();
  const dialog = page.getByRole("dialog", { name: "Preferences" });
  await dialog.getByRole("tab", { name: "PRO DJ LINK" }).click();

  const toggle = dialog.getByRole("switch", { name: "Auto-join LINK when available" });
  const label = toggle.locator("xpath=parent::label");
  const labelText = label.locator("span");
  const help = dialog.getByText("Turn on PRO DJ LINK automatically when a player or mixer is detected.");
  const [toggleBox, labelBox, textBox, helpBox] = await Promise.all([
    toggle.boundingBox(),
    label.boundingBox(),
    labelText.boundingBox(),
    help.boundingBox(),
  ]);

  expect(toggleBox).not.toBeNull();
  expect(labelBox).not.toBeNull();
  expect(textBox).not.toBeNull();
  expect(helpBox).not.toBeNull();
  expect(toggleBox?.x ?? 0).toBeGreaterThan(textBox?.x ?? 0);
  expect((toggleBox?.x ?? 0) + (toggleBox?.width ?? 0)).toBeCloseTo(
    (labelBox?.x ?? 0) + (labelBox?.width ?? 0),
    0,
  );
  expect(helpBox?.x).toBeCloseTo(textBox?.x ?? 0, 0);
});

test("the detail waveform shows a window, not the whole track again", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();

  const overview = page.getByTestId("player-overview");
  const detail = page.getByTestId("player-detail");

  // The overview holds every cue; the detail holds only those inside its
  // window, which is a small slice of the track.
  await expect.poll(async () => overview.locator('[data-cue]:not([data-cue=""])').count()).toBe(4);
  await expect
    .poll(async () => detail.locator('[data-cue]').count())
    .toBeLessThan(5);
});

test("the widest zoom draws bar lines only, not a picket fence of beats", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();

  const detail = page.getByTestId("player-detail");
  // The grey beats between the white downbeats, by the colour the capture
  // measures — the class names are hashed by the CSS modules build.
  const greys = async () =>
    (await detail.locator("span").evaluateAll((els) =>
      els.map((e) => getComputedStyle(e).backgroundColor),
    )).filter((colour) => colour === "rgb(76, 76, 76)").length;

  await expect.poll(greys).toBeGreaterThan(0);

  // Out to the last step. Past it the beats are a few pixels apart and the
  // downbeats that make the grid readable are lost among them.
  const out = page.getByRole("button", { name: "Zoom out", exact: true });
  for (let i = 0; i < ZOOM_STEPS_COUNT; i++) await out.click();
  await expect.poll(greys).toBe(0);

  // The bar lines stay: they are what says where the phrase is.
  const whites = await detail.locator("span").evaluateAll((els) =>
    els.filter((e) => getComputedStyle(e).backgroundColor === "rgb(255, 255, 255)").length,
  );
  expect(whites).toBeGreaterThan(0);

  // And one step back in, the beats return.
  await page.getByRole("button", { name: "Zoom in", exact: true }).click();
  await expect.poll(greys).toBeGreaterThan(0);
});

test("the detail waveform draws a beat grid with heavier downbeats", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();

  const detail = page.getByTestId("player-detail");
  // A window of 8% of a ~5 minute track at ~128 BPM is on the order of 50
  // beats: enough to be a grid, few enough to be readable.
  await expect.poll(async () => detail.locator("span").count()).toBeGreaterThan(10);

  // Every fourth marker is white where the others are grey, and each carries a
  // triangle at either end — measured: the red in rekordbox's grid is the
  // downbeat's head, not its line.
  const marks = await detail.locator("span").evaluateAll((els) =>
    els.map((e) => {
      const style = getComputedStyle(e);
      const head = getComputedStyle(e, "::before");
      return `${style.backgroundColor}|${head.borderTopColor}`;
    }),
  );
  expect(marks).toContain("rgb(255, 255, 255)|rgb(234, 51, 35)");
  expect(marks).toContain("rgb(76, 76, 76)|rgb(124, 124, 124)");

  // And they do not span the band: a marker starts 15px above the waveform,
  // head included, so its line starts the head's height and a point lower.
  const band = await detail.boundingBox();
  const marker = await detail.locator("span").first().boundingBox();
  const token = (name: string) =>
    page.evaluate((n) => Number.parseFloat(getComputedStyle(document.documentElement).getPropertyValue(n)), name);
  expect((marker?.y ?? 0) - (band?.y ?? 0)).toBeCloseTo(
    (await token("--s-wave-inset-top")) - 15 + (await token("--s-beat-head-h")) + 1, 0,
  );
  expect((band?.y ?? 0) + (band?.height ?? 0) - ((marker?.y ?? 0) + (marker?.height ?? 0)))
    .toBeGreaterThan(4);
});

test("the waveforms follow the window rather than stretching a fixed canvas", async ({ page }) => {
  await page.setViewportSize({ width: 1200, height: 900 });
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();

  const canvas = page.getByTestId("player-detail").locator("canvas");
  await expect(canvas).toBeVisible();
  const narrow = await canvas.evaluate((el: HTMLCanvasElement) => el.width);
  expect(narrow).toBeGreaterThan(0);

  // A canvas whose backing store does not grow is simply upscaled by CSS, and
  // the waveform goes soft on a wide window.
  await page.setViewportSize({ width: 1800, height: 1130 });
  await expect
    .poll(async () => canvas.evaluate((el: HTMLCanvasElement) => el.width))
    .toBeGreaterThan(narrow);
});

test("right-clicking a playlist or a folder starts no export", async ({ page }) => {
  // Both offer export (Export Playlist, Export Folder), but only from the
  // menu: opening it must not start one.
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");

  // The mock has no filesystem, so the picker resolves to nothing and the
  // status bar goes quiet again rather than claiming an export happened.
  const playlist = page.getByRole("treeitem").filter({ hasText: "Melodic Vox" }).first();
  await playlist.click({ button: "right" });
  await expect(page.getByRole("contentinfo")).not.toContainText("Exported");

  const folder = page.getByRole("treeitem").filter({ hasText: "CURRENT" }).first();
  await folder.click({ button: "right" });
  await expect(page.getByRole("contentinfo")).not.toContainText("Exporting");
});

test("the information window shows the focused track and closes again", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");

  // Closed by default, which is what browseSetting.xml records for the real
  // rekordbox — so the default view is the one the captures were taken of.
  const panel = page.getByRole("complementary", { name: "Information" });
  await expect(panel).toBeHidden();

  const title = await page.locator('[role="gridcell"][data-col="title"]').nth(3).innerText();
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();

  // Fired the way the native menu bar fires it; a browser has none.
  await page.evaluate(() => (window as unknown as { __menu: (id: string) => void }).__menu("info"));
  await expect(panel).toBeVisible();
  await expect(panel).toContainText(title);
  await expect(panel).toContainText("File Type");

  // It gives up its width to the browser rather than overlaying it.
  const browser = page.getByTestId("browser-title");
  await expect(browser).toBeVisible();

  // No close button: rekordbox's Information Window has none, and the menu
  // item that opened it toggles it shut.
  await page.evaluate(() => (window as unknown as { __menu: (id: string) => void }).__menu("info"));
  await expect(panel).toBeHidden();
});

test("the sub-browser keeps its own selection, separate from the main one", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");

  // Closed by default, as browseSetting.xml records for the real rekordbox.
  const sub = page.getByRole("region", { name: "Sub-Browser" });
  await expect(sub).toBeHidden();
  await page.evaluate(() => (window as unknown as { __menu: (id: string) => void }).__menu("sub"));
  await expect(sub).toBeVisible();

  // Pick a playlist in the main tree, and a different one in the sub-browser's.
  const main = page.getByRole("tree").first();
  await main.getByRole("treeitem").filter({ hasText: "Melodic Vox" }).first().click();
  await expect(page.getByTestId("browser-title").first()).toContainText("Melodic Vox");

  const subTree = sub.getByRole("tree");
  const other = subTree.getByRole("treeitem").filter({ hasText: "All Tracks" }).first();
  await other.click();

  // The point of a sub-browser: two selections at once. The main browser must
  // not have followed the sub-browser's click.
  await expect(page.getByTestId("browser-title").first()).toContainText("Melodic Vox");
  await expect(sub.getByTestId("browser-title")).toContainText("All Tracks");

  // Closed from the icon column at the right edge, which is where rekordbox
  // keeps the toggle; the panel has no close button of its own.
  await page.getByRole("button", { name: "Sub-Browser Window" }).click();
  await expect(sub).toBeHidden();
});

test("the last screen is drawn while the library is still being read", async ({ page }) => {
  // Reading 38,681 rows out of SQLCipher takes long enough to see. Rather than
  // an empty window for that time, the previous run's screen is drawn and
  // replaced the moment the real one arrives.
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.getByRole("treeitem").filter({ hasText: "Melodic Vox" }).first().click();
  const title = await page.locator('[role="gridcell"][data-col="title"]').first().innerText();
  // Let the session be written.
  await expect
    .poll(async () =>
      page.evaluate(() => {
        const stored = localStorage.getItem("rbl.session") ?? "{}";
        return (JSON.parse(stored) as { rows?: unknown[] }).rows?.length ?? 0;
      }),
    )
    .toBeGreaterThan(0);

  // Now start again with the library held back.
  await page.goto("/?slow=1");

  // The tree, the rows and the player are all there before the backend has
  // answered anything.
  await expect(page.getByRole("treeitem").filter({ hasText: "Melodic Vox" }).first()).toBeVisible();
  await expect(page.locator('[role="row"]').first()).toBeVisible();
  await expect(page.locator('[role="gridcell"][data-col="title"]').first()).toHaveText(title);
  await expect(page.getByRole("contentinfo")).toContainText("Loading the library…");

  // And the real data replaces it.
  await page.waitForFunction(() => "__libraryReady" in window);
  await page.evaluate(() => (window as unknown as { __libraryReady: () => void }).__libraryReady());
  await expect(page.getByRole("contentinfo")).not.toContainText("Loading the library…");
});

test("a library that is still loading arrives when it is ready", async ({ page }) => {
  // The backend loads on its own thread, so the first request can easily
  // arrive before there is anything to answer with. `?slow` holds the mock's
  // library back until it is released, which is that race made deliberate.
  await page.goto("/?slow=1");

  const status = page.getByRole("contentinfo");
  await expect(status).toContainText("Loading the library…");

  // The mock is built on the first backend call, so the release hook appears
  // a beat after the page does.
  await page.waitForFunction(() => "__libraryReady" in window);
  await page.evaluate(() => (window as unknown as { __libraryReady: () => void }).__libraryReady());

  // Without a retry on the ready event this stays on "Loading…" forever, which
  // is what a 38,681-track collection in a debug build actually did.
  await expect(status).not.toContainText("Loading the library…");
  await expect(page.locator('[role="row"]').first()).toBeVisible();
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
});

test("the interface comes back the way it was left", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");

  // Change three things: the playlist, the sort, and a panel.
  await page.getByRole("treeitem").filter({ hasText: "Melodic Vox" }).first().click();
  await page.getByRole("columnheader", { name: /BPM/ }).click();
  await page.waitForFunction(() => "__menu" in window);
  await page.evaluate(() => (window as unknown as { __menu: (id: string) => void }).__menu("info"));
  await expect(page.getByRole("complementary", { name: "Information" })).toBeVisible();

  await page.reload();

  // The window's own size and position are the shell's job; this is what is
  // inside it.
  await expect(page.getByTestId("browser-title")).toContainText("Melodic Vox");
  await expect(page.getByRole("columnheader", { name: /BPM/ })).toHaveAttribute("data-sorted", "true");
  await expect(page.getByRole("complementary", { name: "Information" })).toBeVisible();
});

test("a playlist that has gone since last time does not leave an empty window", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  // As if the remembered playlist had been deleted between runs.
  await page.evaluate(() =>
    localStorage.setItem("rbl.session", JSON.stringify({ selectedNodeId: "pl-gone-forever" })),
  );
  await page.reload();

  // Falls back to a real playlist rather than showing nothing.
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await expect(page.locator('[role="row"]').first()).toBeVisible();
});

test("CUE/LOOP and GRID swap which controls the pad row shows", async ({ page }) => {
  await page.goto("/");
  const player = page.getByRole("region", { name: "Preview player" });

  // CUE/LOOP is the hot cues and the memory transport.
  await expect(player.getByRole("button", { name: "Hot cue A", exact: true })).toBeVisible();
  await expect(player.getByRole("region", { name: "Beat grid" })).toBeHidden();

  await player.getByRole("tab", { name: "GRID" }).click();
  await expect(player.getByRole("tab", { name: "GRID" })).toHaveAttribute("aria-selected", "true");
  // GRID is a different set of buttons entirely, not the same row relabelled.
  await expect(player.getByRole("button", { name: "Double the tempo" })).toBeVisible();
  await expect(player.getByRole("button", { name: "Mark the downbeat here" })).toBeVisible();
  await expect(player.getByRole("button", { name: "Cut the phrase here" })).toHaveCount(0);
  await expect(player.getByRole("button", { name: "Hot cue A", exact: true })).toBeHidden();

  await player.getByRole("tab", { name: "CUE/LOOP" }).click();
  await expect(player.getByRole("button", { name: "Hot cue A", exact: true })).toBeVisible();
});

test("the memory, hot cue and info tabs change the panel beside the deck", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();

  const panel = page.getByRole("complementary", { name: "Cue list" });
  // Memory cues and hot cues are the same rows told apart by a flag, so the
  // two tabs must genuinely list different things.
  // MEMORY prints its positions to the millisecond; the hot-cue list does not.
  await expect(panel.getByText(/^\d\d:\d\d:\d\d\d$/).first()).toBeVisible();
  await expect(panel.getByRole("button", { name: "Hot cue A" })).toHaveCount(0);

  await panel.getByRole("tab", { name: "HOT CUE" }).click();
  // Eight slots, always — an empty one is a slot you can fill, and hiding it
  // makes the list read as a shorter track. The tabs are role=tab, not button.
  await expect(panel.getByRole("button", { name: /^Hot cue [A-H]$/ })).toHaveCount(8);
  await expect(panel.getByRole("button", { name: /^Hot cue [A-H]$/, disabled: true })).toHaveCount(4);
  // A set row has its ✕; an empty one has nothing to clear.
  await expect(panel.getByRole("button", { name: /^Clear hot cue/ })).toHaveCount(4);
  await expect(panel.getByText(/^\d\d:\d\d:\d\d\d$/)).toHaveCount(0);

  await panel.getByRole("tab", { name: "INFO" }).click();
  // The file's kind, from the record — the manual's INFO tab has no BPM line.
  await expect(panel.getByText(/ File$/)).toBeVisible();
  await expect(panel.getByText("CUE(Auto)")).toHaveCount(0);
});

test("the playhead is moved by a transform, not by laying the strip out again", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();

  // Structural, because a browser build has no rbl:// scheme to play from and
  // so never advances: what this pins is that the head is driven by a
  // transform written from the frame loop. `left: %` moved it by laying the
  // strip out and painting it again on every tick.
  const head = page.getByTestId("player-head");
  await expect(head).toHaveCSS("left", "0px");
  expect(await head.evaluate((el) => getComputedStyle(el).transform)).not.toBe("none");

  // Same for the position bar under the overview: scaled, not resized.
  const fill = page.locator('[class*="scrubFill"]');
  await expect(fill).toHaveCSS("transform", "matrix(0, 0, 0, 1, 0, 0)");
});

test("the detail playhead is pinned to the middle, whatever the track does", async ({ page }) => {
  // The window moves under the head rather than the head across the window,
  // which is what a CDJ shows and what makes the head mean "here".
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();

  const detail = await page.getByTestId("player-detail").boundingBox();
  const head = await page.getByTestId("player-detail-head").boundingBox();
  expect((head?.x ?? 0) - (detail?.x ?? 0)).toBeCloseTo((detail?.width ?? 0) / 2, 0);
});

test("dragging the detail waveform scrubs, and dragging the overview seeks", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();

  // The mock backend keeps a deck, so this is the real behaviour and not just
  // the wiring: dragging right pulls earlier music in, so the clock goes back.
  const clock = page.getByTestId("player-time");
  const detail = await page.getByTestId("player-detail").boundingBox();
  // Far enough in that a drag backwards has somewhere to go. The deck ticks
  // ten times a second, so waiting on the button is not waiting on the clock.
  const start = await clock.textContent();
  await page.getByRole("button", { name: "Play", exact: true }).click();
  await expect(clock).not.toHaveText(start ?? "");
  await page.waitForTimeout(600);
  await page.getByRole("button", { name: "Pause", exact: true }).click();

  const before = await clock.textContent();
  await page.mouse.move((detail?.x ?? 0) + 200, (detail?.y ?? 0) + 20);
  await page.mouse.down();
  // Past the edge of the strip: pointer capture is what keeps a drag alive
  // out there, and this was a mousedown handler that gave up at the border.
  await page.mouse.move((detail?.x ?? 0) + 900, (detail?.y ?? 0) + 400, { steps: 10 });
  await page.mouse.up();
  await expect(clock).not.toHaveText(before ?? "");

  // The overview is the other kind: the head goes where the pointer is.
  const ovw = await page.getByTestId("player-overview").boundingBox();
  await page.mouse.move((ovw?.x ?? 0) + (ovw?.width ?? 0) * 0.75, (ovw?.y ?? 0) + 5);
  await page.mouse.down();
  await page.mouse.up();
  const head = await page.getByTestId("player-head").boundingBox();
  expect((head?.x ?? 0) - (ovw?.x ?? 0)).toBeCloseTo((ovw?.width ?? 0) * 0.75, -1);
});

test("a track can be scrubbed as soon as it is loaded, before it has ever played", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();
  const player = page.getByRole("region", { name: "Preview player", exact: true });
  await expect(player.getByRole("button", { name: "Play", exact: true })).toBeEnabled();

  // No Play. The hand goes straight to the overview, then to the detail.
  const clock = page.getByTestId("player-time");
  const start = await clock.textContent();
  const ovw = await page.getByTestId("player-overview").boundingBox();
  await page.mouse.move((ovw?.x ?? 0) + (ovw?.width ?? 0) * 0.5, (ovw?.y ?? 0) + 5);
  await page.mouse.down();
  await page.mouse.move((ovw?.x ?? 0) + (ovw?.width ?? 0) * 0.6, (ovw?.y ?? 0) + 5, { steps: 5 });
  await page.mouse.up();
  await expect(clock).not.toHaveText(start ?? "");
  const head = await page.getByTestId("player-head").boundingBox();
  expect((head?.x ?? 0) - (ovw?.x ?? 0)).toBeCloseTo((ovw?.width ?? 0) * 0.6, -1);

  const mid = await clock.textContent();
  const detail = await page.getByTestId("player-detail").boundingBox();
  await page.mouse.move((detail?.x ?? 0) + 200, (detail?.y ?? 0) + 20);
  await page.mouse.down();
  await page.mouse.move((detail?.x ?? 0) + 600, (detail?.y ?? 0) + 20, { steps: 10 });
  await page.mouse.up();
  await expect(clock).not.toHaveText(mid ?? "");
  // Still not playing: a drag is not a transport control.
  await expect(player.getByRole("button", { name: "Play", exact: true })).toBeVisible();
});

test("the waveform actually scrolls while a track plays", async ({ page }) => {
  // The thing that was broken: the layer moved ten times a second because it
  // was drawn from the tick. What it must do is move every frame.
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();

  const layer = page.locator('[class*="scroller"]');
  const at = async () => layer.evaluate((el) => getComputedStyle(el).transform);
  await page.getByRole("button", { name: "Play", exact: true }).click();
  const first = await at();
  await page.waitForTimeout(500);
  const second = await at();
  expect(second).not.toBe(first);

  // And it keeps moving between ticks: two reads a frame apart differ, which
  // they cannot if the layer is only repositioned when a tick arrives.
  const frames = await layer.evaluate(
    (el) =>
      new Promise<string[]>((resolve) => {
        const seen: string[] = [];
        const step = () => {
          seen.push(getComputedStyle(el).transform);
          if (seen.length < 6) requestAnimationFrame(step);
          else resolve(seen);
        };
        requestAnimationFrame(step);
      }),
  );
  expect(new Set(frames).size).toBeGreaterThan(1);
});

test("Q toggles quantize, and starts on the way a CDJ ships", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();

  const q = page.getByRole("button", { name: "Quantize" });
  await expect(q).toHaveAttribute("aria-pressed", "true");
  await q.click();
  await expect(q).toHaveAttribute("aria-pressed", "false");
  await q.click();
  await expect(q).toHaveAttribute("aria-pressed", "true");
});

test("the detail waveform scrolls by transform, not by a redraw a tick", async ({ page }) => {
  // The engine ticks ten times a second. Drawing the waveform from that made
  // the scroll step rather than move, once the playhead stopped moving itself.
  // What keeps it at frame rate is that the layer is drawn wider than the
  // strip and slid; `perf-budgets.json` player.waveformRedrawsPerSecond.
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();

  const strip = await page.getByTestId("player-detail").boundingBox();
  const layer = page.locator('[class*="scroller"]');
  await expect(layer).toHaveCSS("will-change", "transform");
  const drawn = await layer.boundingBox();
  expect(drawn?.width).toBeGreaterThan((strip?.width ?? 0) * 1.5);

  // And the canvas is drawn at the layer's width, not the strip's, so sliding
  // it never exposes an undrawn edge.
  const canvas = await layer.locator("canvas").boundingBox();
  expect(canvas?.width).toBeCloseTo(drawn?.width ?? 0, 0);
});

test("the deck takes the keyboard when it is clicked, and gives it back", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();

  const player = page.getByRole("region", { name: "Preview player" });
  const head = page.getByTestId("player-detail-head");
  const colour = async () => head.evaluate((el) => getComputedStyle(el).backgroundColor);
  const white = await colour();

  // Clicking the deck arms it: the playhead turns red, because the arrow keys
  // are about to move the track rather than the browser's cursor.
  await player.getByTestId("player-detail").click({ position: { x: 30, y: 10 } });
  await expect(player).toHaveAttribute("data-armed", "");
  expect(await colour()).not.toBe(white);

  // The arrows then beat-jump by the chosen size.
  const clock = page.getByTestId("player-time");
  const before = await clock.textContent();
  await page.keyboard.press("ArrowRight");
  await expect(clock).not.toHaveText(before ?? "");

  // Clicking the browser hands it back, and the playhead goes white again.
  await page.locator('[role="gridcell"][data-col="title"]').nth(5).click();
  await expect(player).not.toHaveAttribute("data-armed", "");
  expect(await colour()).toBe(white);
});

test("the beat jump size is a menu of every size, ticked at the one in use", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();

  const size = page.getByRole("button", { name: "Beat jump size" });
  await expect(size).toHaveText(/^4Beats/);

  // It was a cycle: six presses from Fine to 32Bars, and no way to see what
  // the sizes were without walking them.
  await size.click();
  const menu = page.getByRole("menu", { name: "Beat jump size" });
  // The tick sits in the item's own text, so these match rather than equal.
  await expect(menu.getByRole("menuitemradio")).toHaveText([
    /^Fine$/, /4Beats$/, /^8Beats$/, /^16Beats$/, /^8Bars$/, /^16Bars$/, /^32Bars$/,
  ]);
  await expect(
    menu.getByRole("menuitemradio", { name: "4Beats", exact: true }),
  ).toHaveAttribute("aria-checked", "true");

  await menu.getByRole("menuitemradio", { name: "16Bars", exact: true }).click();
  await expect(menu).toBeHidden();
  await expect(size).toHaveText(/^16Bars/);

  // And pressing the button again puts the menu away rather than reopening it.
  await size.click();
  await expect(page.getByRole("menu", { name: "Beat jump size" })).toBeVisible();
  await size.click();
  await expect(page.getByRole("menu", { name: "Beat jump size" })).toBeHidden();
});

test("the zoom controls float over the detail waveform rather than beside it", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();

  // They used to sit in a column as wide as the sleeve, which cost the strip
  // 55px of music to hold three controls.
  const detail = await page.getByTestId("player-detail").boundingBox();
  const zoom = await page.getByRole("button", { name: "Zoom in", exact: true }).boundingBox();
  expect(zoom?.x ?? 0).toBeGreaterThan(detail?.x ?? 0);
  expect((zoom?.x ?? 0) + (zoom?.width ?? 0)).toBeLessThan(
    (detail?.x ?? 0) + (detail?.width ?? 0),
  );

  // Pressing one zooms and does not also start a drag on the waveform under it.
  const before = await page.getByTestId("player-bars").textContent();
  await page.getByRole("button", { name: "Zoom out", exact: true }).click();
  expect(await page.getByTestId("player-bars").textContent()).toBe(before);
});

test("the play triangle is grey at rest and white on a lit ring", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();

  const colours = async (name: string) => {
    const button = page.getByRole("button", { name, exact: true });
    return button.evaluate((el) => {
      const glyph = el.querySelector("span") as HTMLElement;
      return {
        ring: getComputedStyle(el).borderTopColor,
        // The triangle is a border on a zero-sized box, so it takes the
        // button's colour through `currentColor`.
        triangle: getComputedStyle(glyph).borderLeftColor,
      };
    });
  };

  // Stopped: the resting ring the capture measures, and a grey triangle.
  expect(await colours("Play")).toEqual({
    ring: "rgb(54, 100, 42)",
    triangle: "rgb(160, 160, 160)",
  });

  await page.getByRole("button", { name: "Play", exact: true }).click();
  await expect(page.getByRole("button", { name: "Pause", exact: true })).toBeVisible();

  // Running: the ring lights and the triangle goes white. It stays a triangle
  // — the colour is what says the deck is running.
  expect(await colours("Pause")).toEqual({
    ring: "rgb(96, 211, 48)",
    triangle: "rgb(255, 255, 255)",
  });
});

test("hovering a running deck shows the pause it is about to do", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();

  // Which of the two glyphs the button is actually drawing.
  const shown = async (name: string) =>
    page.getByRole("button", { name, exact: true }).evaluate((el) =>
      [...el.querySelectorAll("span")]
        .map((g) => (getComputedStyle(g).display === "none" ? null : g.clientWidth === 0 ? "triangle" : "bars"))
        .filter(Boolean),
    );

  // Stopped, hovered: the triangle stays. It already says what a press does.
  await page.getByRole("button", { name: "Play", exact: true }).hover();
  expect(await shown("Play")).toEqual(["triangle"]);

  await page.getByRole("button", { name: "Play", exact: true }).click();
  const pause = page.getByRole("button", { name: "Pause", exact: true });
  await expect(pause).toBeVisible();

  // Running and hovered — the pointer is still on it after the click.
  expect(await shown("Pause")).toEqual(["bars"]);

  // Off the button, the triangle comes back.
  await page.getByTestId("player-title").hover();
  expect(await shown("Pause")).toEqual(["triangle"]);
});

test("the wheel over a waveform zooms it", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();

  // The layer is drawn across a fixed number of bars, so zooming changes how
  // much of the track one strip covers — which is what the beat spacing shows.
  const gaps = async () =>
    page.getByTestId("player-detail").locator('[class*="downbeat"]').count();
  const wide = await gaps();
  await page.getByTestId("player-detail").hover();
  await page.mouse.wheel(0, -120);
  await expect.poll(gaps).toBeLessThan(wide);
  // One step per gesture, whatever the delta: two events to pass the start.
  await page.mouse.wheel(0, 120);
  await page.mouse.wheel(0, 120);
  await expect.poll(gaps).toBeGreaterThan(wide);
});

test("the waveform zoom survives a restart", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks");
  const loadTrack = () =>
    page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();
  await loadTrack();

  // 12 → 8 → 4 → 2 → 1 → 0.5 bars, close enough to switch to PCM.
  const zoomIn = page.getByRole("button", { name: "Zoom in", exact: true });
  for (let i = 0; i < 5; i++) await zoomIn.click();
  await expect(page.getByTestId("player-detail")).toHaveAttribute("data-pcm");
  await expect.poll(() => page.evaluate(() => {
    const raw = localStorage.getItem("rbl.session") ?? "{}";
    return (JSON.parse(raw) as { waveformZoom?: { a?: number } }).waveformZoom?.a;
  })).toBe(0.5);

  // Tracks deliberately start empty after a restart; loading one into the
  // restored deck must nevertheless use the zoom it had before.
  await page.reload();
  await expect(page.getByTestId("browser-title")).toContainText("Tracks");
  await loadTrack();
  await expect(page.getByTestId("player-detail")).toHaveAttribute("data-pcm");
});

test("wheel crossing PCM returns to the same PWV7 canvas as ordinary zoom", async ({ page }) => {
  const load = async () => {
    await page.goto("/");
    await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
    await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();
    await expect(page.getByTestId("player-detail")).toBeVisible();
  };
  // A compact pixel digest lets this prove the rendered normal canvas is the
  // same after a wheel passes through PCM, without a fragile screenshot.
  const digest = () => page.getByTestId("player-detail").locator("canvas").evaluate((canvas: HTMLCanvasElement) => {
    const data = canvas.getContext("2d")?.getImageData(0, 0, canvas.width, canvas.height).data;
    if (!data?.length) return "";
    let hash = 2_166_136_261;
    for (let i = 0; i < data.length; i += 16) hash = Math.imul(hash ^ (data[i] ?? 0), 16_777_619);
    return `${canvas.width}x${canvas.height}:${hash >>> 0}`;
  });

  await load();
  // 12 → 8 → 4 → 2 → 1: a normal PWV7 one-bar view.
  const inButton = page.getByRole("button", { name: "Zoom in", exact: true });
  for (let i = 0; i < 4; i++) await inButton.click();
  await expect.poll(digest).not.toBe("");
  const expected = await digest();

  // The zoom survives a reload, so the wheel run has to be put back on the
  // default 12 bars the button run started from.
  await page.evaluate(() => {
    const session = JSON.parse(localStorage.getItem("rbl.session") ?? "{}") as { waveformZoom?: unknown };
    delete session.waveformZoom;
    localStorage.setItem("rbl.session", JSON.stringify(session));
  });
  await load();
  await expect(page.getByTestId("player-detail")).not.toHaveAttribute("data-pcm");
  const detail = page.getByTestId("player-detail");
  // The fifth wheel-in reaches PCM at 1/2 bar; one wheel-out returns to the
  // exact same 1-bar PWV7 state used above. Dispatch them together, as a
  // rapid physical scroll can cross the boundary before React paints between
  // native wheel events.
  await detail.evaluate((element) => {
    for (let i = 0; i < 5; i++) {
      element.dispatchEvent(new WheelEvent("wheel", { bubbles: true, cancelable: true, deltaY: -120 }));
    }
    element.dispatchEvent(new WheelEvent("wheel", { bubbles: true, cancelable: true, deltaY: 120 }));
  });
  await expect.poll(digest).toBe(expected);
});

test("the title bar carries the name in the middle of the window", async ({ page }) => {
  // macOS draws its own title beside the traffic lights at its own size; the
  // window uses an overlay title bar so this one can sit in the middle.
  await page.goto("/");
  const bar = page.getByTestId("title-bar");
  const name = bar.locator('[class*="appName"]');
  await expect(name).toHaveText("rbxport");
  const strip = await bar.boundingBox();
  const text = await name.boundingBox();
  const centre = (strip?.x ?? 0) + (strip?.width ?? 0) / 2;
  expect((text?.x ?? 0) + (text?.width ?? 0) / 2).toBeCloseTo(centre, 0);
});

test("the title bar reads out what the app is costing", async ({ page }) => {
  // A browser cannot see its own process, so what this pins is that every
  // figure is labelled and that an unavailable one is a dash rather than a
  // zero — a zero would claim the app is resident in no memory at all.
  await page.goto("/");
  const cost = page.getByTestId("app-cost");
  for (const label of ["AUDIO", "RAM", "FPS"]) {
    await expect(cost).toContainText(label);
  }
  // GPU was always a dash: macOS accounts it per process only to root.
  await expect(cost).not.toContainText("GPU");
  await expect(cost).toContainText("RAM —");
  // FPS is measured in the window itself, so it arrives even here.
  await expect(cost).not.toContainText("FPS —");
});

test("clicking the artwork ejects the track from the deck", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();
  await expect(page.getByTestId("player-title")).not.toHaveText("");

  // The sleeve is the eject button, as it is on a CDJ's screen.
  await page.getByRole("button", { name: "Eject" }).click();
  await expect(page.getByTestId("player-title")).toHaveText("");
  // And the deck goes back to being a deck with nothing in it.
  await expect(page.getByRole("button", { name: "Cue", exact: true })).toBeDisabled();
});

test("the settings window offers the audio output, and says when there is none", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.getByRole("banner").getByRole("button", { name: "Settings" }).click();
  await page.getByRole("dialog", { name: "Preferences" }).getByRole("tab", { name: "Audio" }).click();

  const section = page.getByRole("region", { name: "Audio output" });
  await expect(section).toBeVisible();
  // In a browser there is no engine and so nothing to choose between, and the
  // panel says that rather than drawing an empty picker.
  await expect(section).toContainText("no audio engine behind it");
  await expect(section.getByRole("combobox")).toHaveCount(0);
});

test("beat sync belongs to two decks, and pulls the follower to the master", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  // One deck has nothing to sync to and nothing to be master of.
  await expect(page.getByRole("button", { name: "Beat sync" })).toHaveCount(0);

  await page.getByRole("button", { name: "Layout" }).click();
  await page.getByRole("menuitemradio", { name: "2 PLAYER" }).click();

  // Two tracks at different tempos: 127 on deck A and 129 on deck B.
  await page.locator('[role="gridcell"][data-col="title"]').first().dblclick();
  const cell = page.locator('[role="gridcell"][data-col="title"]').nth(1);
  await cell.click({ button: "right" });
  const menu = page.getByRole("menu", { name: "Track" });
  await menu.getByRole("menuitem", { name: "Load", exact: true }).hover();
  await menu.getByRole("menuitem", { name: "Load track to player 2" }).click();

  const a = page.getByRole("region", { name: "Preview player" }).first();
  const b = page.getByRole("region", { name: "Preview player B" });

  // Deck A is master to begin with, so its own BEAT SYNC has nothing to do.
  await expect(a.getByRole("button", { name: "Sync master" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect(a.getByRole("button", { name: "Beat sync" })).toBeDisabled();
  await expect(b.getByRole("button", { name: "Beat sync" })).toBeEnabled();

  // Syncing B pulls it to A's tempo: 127 against its own 129.
  const bpmB = b.getByTestId("player-bpm");
  const before = Number(await bpmB.innerText());
  await b.getByRole("button", { name: "Beat sync" }).click();
  await expect.poll(async () => Number(await bpmB.innerText())).not.toBe(before);
  const leader = Number(await a.getByTestId("player-bpm").innerText());
  expect(Number(await bpmB.innerText())).toBeCloseTo(leader, 0);

  // And master moves: whichever deck holds it, the other is the one that syncs.
  await b.getByRole("button", { name: "Sync master" }).click();
  await expect(b.getByRole("button", { name: "Beat sync" })).toBeDisabled();
  await expect(a.getByRole("button", { name: "Beat sync" })).toBeEnabled();
});

test("BEAT SYNC stays lit and follows the master's tempo until RST or MASTER ends it", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.getByRole("button", { name: "Layout" }).click();
  await page.getByRole("menuitemradio", { name: "2 PLAYER" }).click();
  await page.locator('[role="gridcell"][data-col="title"]').first().dblclick();
  const cell = page.locator('[role="gridcell"][data-col="title"]').nth(1);
  await cell.click({ button: "right" });
  const menu = page.getByRole("menu", { name: "Track" });
  await menu.getByRole("menuitem", { name: "Load", exact: true }).hover();
  await menu.getByRole("menuitem", { name: "Load track to player 2" }).click();

  const a = page.getByRole("region", { name: "Preview player" }).first();
  const b = page.getByRole("region", { name: "Preview player B" });
  const sync = b.getByRole("button", { name: "Beat sync" });
  const bpmA = a.getByTestId("player-bpm");
  const bpmB = b.getByTestId("player-bpm");
  // What deck B's own file runs at, read rather than written down: which
  // track sits second is the mock's business, not this test's.
  const bpmBAtRest = await bpmB.innerText();

  // The BPM readout reveals both decks' tempo sliders at once — it is one
  // shared preference, not per deck. Deck A's RST starts locked, guarding
  // against an accidental nudge, so its first click only unlocks it.
  await bpmA.click();
  const sliderA = a.getByRole("slider", { name: "Tempo" });
  const sliderB = b.getByRole("slider", { name: "Tempo" });
  const resetA = a.getByRole("button", { name: "Reset tempo" });
  const resetB = b.getByRole("button", { name: "Reset tempo" });
  await resetA.click();

  // Lit once pressed, and the follower's own tempo steps are not its to take.
  await sync.click();
  await expect(sync).toHaveAttribute("aria-pressed", "true");
  await expect(sliderB).toHaveAttribute("aria-disabled", "true");
  await expect.poll(async () => Number(await bpmB.innerText())).toBeCloseTo(
    Number(await bpmA.innerText()),
    0,
  );

  // Nudging the master moves the follower with it.
  const leaderBefore = Number(await bpmA.innerText());
  await sliderA.press("ArrowDown");
  await expect.poll(async () => Number(await bpmA.innerText())).toBeGreaterThan(leaderBefore);
  await expect.poll(async () => Number(await bpmB.innerText())).toBeCloseTo(
    Number(await bpmA.innerText()),
    0,
  );

  // RST puts the follower back at its file's speed and takes it off sync —
  // deck B's RST was never clicked before, so the first click only unlocks
  // it and the second actually resets.
  await resetB.click();
  await resetB.click();
  await expect(sync).toHaveAttribute("aria-pressed", "false");
  await expect(bpmB).toHaveText(bpmBAtRest);
  await expect(sliderB).toHaveAttribute("aria-disabled", "true");

  // Synced again, then made master: a master follows nobody, so its light goes out.
  await sync.click();
  await expect(sync).toHaveAttribute("aria-pressed", "true");
  await b.getByRole("button", { name: "Sync master" }).click();
  await expect(sync).toBeDisabled();
  await expect(sync).toHaveAttribute("aria-pressed", "false");

  // MT is the engine's key lock, held per deck.
  const mt = a.getByRole("button", { name: "Master tempo" });
  await mt.click();
  await expect(mt).toHaveAttribute("aria-pressed", "true");
  await expect(b.getByRole("button", { name: "Master tempo" })).toHaveAttribute("aria-pressed", "false");
});

test("the tempo control moves the deck's BPM, and MT and RST are real", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  // A track, so there is a BPM to move.
  await page.locator('[role="gridcell"][data-col="title"]').first().dblclick();

  const deck = page.getByRole("region", { name: "Preview player" }).first();
  const bpm = deck.getByTestId("player-bpm");
  const at = async () => Number(await bpm.innerText());
  const resting = await at();
  expect(resting).toBeGreaterThan(0);

  // The BPM readout reveals the tempo slider; RST starts locked, guarding
  // against an accidental nudge, so the first click only unlocks it.
  await bpm.click();
  const reset = deck.getByRole("button", { name: "Reset tempo" });
  const slider = deck.getByRole("slider", { name: "Tempo" });
  await reset.click();
  await expect(slider).toHaveAttribute("aria-disabled", "false");

  // A tenth of a percent a press, which is the step a CDJ's fine setting uses.
  await slider.press("ArrowDown");
  await expect.poll(at).toBeGreaterThan(resting);
  await slider.press("ArrowUp");
  await slider.press("ArrowUp");
  await expect.poll(at).toBeLessThan(resting);

  // RST puts it back, and locks the slider again once there is nothing to reset.
  await expect(reset).toBeEnabled();
  await reset.click();
  await expect.poll(at).toBe(resting);
  await expect(slider).toHaveAttribute("aria-disabled", "true");

  // MT is a toggle: the key stays put while the speed changes.
  const mt = deck.getByRole("button", { name: "Master tempo" });
  await expect(mt).toHaveAttribute("aria-pressed", "false");
  await mt.click();
  await expect(mt).toHaveAttribute("aria-pressed", "true");
});

test("dual control links what both decks are showing", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  const dual = page.getByRole("button", { name: "Dual control" });
  // One deck has nothing to link to, so the button is not drawn at all.
  await expect(dual).toHaveCount(0);

  await page.getByRole("button", { name: "Layout" }).click();
  await page.getByRole("menuitemradio", { name: "2 PLAYER" }).click();
  await expect(dual).toBeVisible();
  await expect(dual).toHaveAttribute("aria-pressed", "false");

  // Both decks need a track for their zoom readouts to say anything.
  await page.locator('[role="gridcell"][data-col="title"]').first().dblclick();
  const cell = page.locator('[role="gridcell"][data-col="title"]').nth(1);
  await cell.click({ button: "right" });
  const menu = page.getByRole("menu", { name: "Track" });
  await menu.getByRole("menuitem", { name: "Load", exact: true }).hover();
  await menu.getByRole("menuitem", { name: "Load track to player 2" }).click();

  const sizes = page.getByRole("button", { name: "Beat jump size" });
  await expect(sizes).toHaveCount(2);
  await expect(sizes.first()).toHaveText(/4Beats/);
  await expect(sizes.last()).toHaveText(/4Beats/);

  // Off: one deck's size moves and the other stays where it was.
  await sizes.first().click();
  await page.getByRole("menu", { name: "Beat jump size" }).getByRole("menuitemradio", { name: "16Beats" }).click();
  await expect(sizes.first()).toHaveText(/16Beats/);
  await expect(sizes.last()).toHaveText(/4Beats/);

  // On: the pair share one, so setting either sets both.
  await dual.click();
  await expect(dual).toHaveAttribute("aria-pressed", "true");
  await sizes.last().click();
  await page.getByRole("menu", { name: "Beat jump size" }).getByRole("menuitemradio", { name: "8Bars" }).click();
  await expect(sizes.first()).toHaveText(/8Bars/);
  await expect(sizes.last()).toHaveText(/8Bars/);
});

test("dual control is remembered across a restart", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.getByRole("button", { name: "Layout" }).click();
  await page.getByRole("menuitemradio", { name: "2 PLAYER" }).click();
  const dual = page.getByRole("button", { name: "Dual control" });
  await expect(dual).toHaveAttribute("aria-pressed", "false");
  await dual.click();
  await expect(dual).toHaveAttribute("aria-pressed", "true");

  // The layout is restored too, so the same button comes back still on.
  await page.reload({ waitUntil: "domcontentloaded" });
  await expect(page.getByTestId("browser-title")).toContainText("Tracks");
  await expect(dual).toHaveAttribute("aria-pressed", "true");

  // Switching it off is remembered as well.
  await dual.click();
  await expect(dual).toHaveAttribute("aria-pressed", "false");
  await page.reload({ waitUntil: "domcontentloaded" });
  await expect(page.getByTestId("browser-title")).toContainText("Tracks");
  await expect(dual).toHaveAttribute("aria-pressed", "false");
});

test("the mixer belongs to the two-deck layouts and to nothing else", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  const mixer = page.getByRole("group", { name: "Mixer" });
  // One deck: no mixer and no crossfader, as rekordbox has it — and so a deck
  // nobody has touched a fader for plays at the level of its file.
  await expect(mixer).toHaveCount(0);

  await page.getByRole("button", { name: "Layout" }).click();
  await page.getByRole("menuitemradio", { name: "2 PLAYER" }).click();
  await expect(mixer).toBeVisible();

  // Three kill buttons a deck, and a gain knob each.
  await expect(mixer.getByRole("button")).toHaveCount(6);
  await expect(mixer.getByRole("slider", { name: /gain/i })).toHaveCount(2);

  // A kill is a toggle, not a momentary: it stays on until it is pressed again.
  const low = mixer.getByRole("button", { name: "LOW" }).first();
  await expect(low).toHaveAttribute("aria-pressed", "false");
  await low.click();
  await expect(low).toHaveAttribute("aria-pressed", "true");
  await low.click();
  await expect(low).toHaveAttribute("aria-pressed", "false");

  // The crossfader starts in the middle, where both decks are heard whole.
  const fader = mixer.getByRole("slider", { name: "Crossfader" });
  await expect(fader).toHaveAttribute("aria-valuenow", "0.5");
  await fader.press("ArrowDown");
  await expect(fader).toHaveAttribute("aria-valuenow", "0.55");
  await fader.dblclick();
  await expect(fader).toHaveAttribute("aria-valuenow", "0.5");
});

test("the layout switch draws one deck, two, a short one, or none", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  const decks = page.getByRole("region", { name: /^Preview player/ });
  const choose = async (label: string) => {
    await page.getByRole("button", { name: "Layout" }).click();
    await page.getByRole("menuitemradio", { name: label }).click();
  };
  await expect(decks).toHaveCount(1);

  await choose("2 PLAYER");
  await expect(decks).toHaveCount(2);

  // The simple player is one strip: no pad row, no cue list, no detail.
  await choose("SIMPLE PLAYER");
  await expect(decks).toHaveCount(1);
  await expect(page.getByRole("tab", { name: "CUE/LOOP" })).toBeHidden();
  await expect(page.getByTestId("player-detail")).toHaveCount(0);
  const short = await decks.first().boundingBox();

  await choose("1 PLAYER");
  await expect(page.getByRole("tab", { name: "CUE/LOOP" })).toBeVisible();
  const full = await decks.first().boundingBox();
  expect(short?.height).toBeLessThan(full?.height ?? 0);

  // Full Browser puts the deck away, and the deck alone. Set the rest of the
  // screen up first — a playlist open, a track playing, the list scrolled and
  // a row selected — so the switch there and back can be seen to keep it.
  await page.getByRole("treeitem", { name: /All Tracks/ }).click();
  await expect(page.getByTestId("browser-title")).toContainText("All Tracks");
  const scroller = page.getByTestId("track-scroll");
  await scroller.evaluate((el) => { el.scrollTop = 600; });
  await expect.poll(() => scroller.evaluate((el) => el.scrollTop)).toBe(600);
  // A row inside the viewport, not the nth rendered one: the rows mounted
  // above the fold (the overscan) are rendered too, and double-clicking one
  // of those scrolls it into view — to the top, when the list is short
  // enough — which would undo the scroll being checked below.
  const frame = await scroller.boundingBox();
  const titles = page.locator('[role="gridcell"][data-col="title"]');
  let row = titles.nth(0);
  for (let i = 0, n = await titles.count(); i < n; i += 1) {
    const box = await titles.nth(i).boundingBox();
    if (frame && box && box.y > frame.y + 30 && box.y + box.height < frame.y + frame.height) {
      row = titles.nth(i);
      break;
    }
  }
  const loaded = await row.innerText();
  await row.dblclick();
  await expect(page.getByTestId("player-title")).toHaveText(loaded);
  await page.keyboard.press(" ");
  await expect(page.getByRole("button", { name: "Pause", exact: true })).toBeVisible();
  // "-M:SS.t", in seconds, so the readout can be compared across the switch.
  const remaining = async () => {
    const text = await page.getByTestId("player-time").innerText();
    const [, minutes, seconds] = /^-(\d+):(\d\d\.\d)$/.exec(text.trim()) ?? [];
    return Number(minutes) * 60 + Number(seconds);
  };
  const before = await remaining();
  await expect.poll(remaining).toBeLessThan(before);
  const left = await remaining();
  // Read back rather than assumed, so a scroll the double-click made of its
  // own is accounted for. What matters is that the switch adds none.
  const scrolled = await scroller.evaluate((el) => el.scrollTop);
  expect(scrolled).toBeGreaterThan(0);

  await choose("FULL BROWSER");
  await expect(decks).toHaveCount(0);
  // The browser takes everything under the top bar down to the status bar:
  // no deck, no gutter, and no strip left over where the deck row was.
  const top = await page.evaluate(() => {
    const style = getComputedStyle(document.documentElement);
    return parseFloat(style.getPropertyValue("--s-title-bar-h")) + parseFloat(style.getPropertyValue("--s-top-bar-h"));
  });
  const body = await page.getByTestId("body").boundingBox();
  const status = await page.locator("footer").boundingBox();
  const viewport = page.viewportSize();
  expect(body?.y).toBe(top);
  expect((body?.y ?? 0) + (body?.height ?? 0)).toBe(status?.y);
  expect((status?.y ?? 0) + (status?.height ?? 0)).toBe(viewport?.height);
  // Nothing else moved: the playlist, the scroll and the selection are where
  // they were.
  await expect(page.getByTestId("browser-title")).toContainText("All Tracks");
  expect(await scroller.evaluate((el) => el.scrollTop)).toBe(scrolled);
  await expect(page.locator('[role="row"][aria-selected="true"]')).toContainText(loaded);

  // And the deck comes back still playing the same track from where it had
  // got to, rather than reloaded and stopped at the start.
  await choose("1 PLAYER");
  await expect(decks).toHaveCount(1);
  await expect(page.getByTestId("player-title")).toHaveText(loaded);
  await expect(page.getByRole("button", { name: "Pause", exact: true })).toBeVisible();
  await expect.poll(remaining).toBeLessThan(left);
  expect(await scroller.evaluate((el) => el.scrollTop)).toBe(scrolled);
  await expect(page.locator('[role="row"][aria-selected="true"]')).toContainText(loaded);

  await choose("FULL BROWSER");
  await expect(decks).toHaveCount(0);
  // And it survives a restart, like the rest of the screen.
  await page.reload();
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await expect(page.getByRole("region", { name: /^Preview player/ })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Layout" })).toBeVisible();
  await page.getByRole("button", { name: "Layout" }).click();
  await expect(page.getByRole("menuitemradio", { name: "FULL BROWSER" })).toHaveAttribute("aria-checked", "true");
});

test("the simple player is one strip, the way rekordbox draws it", async ({ page }) => {
  // Measured off the SIMPLE PLAYER capture (docs/screenshots, 2026-09-09
  // 9.03.41 PM, 2x): PLAY, the sleeve, the title over the overview with its
  // hot cue badges, the artist, the two times, key, BPM, the rating — and
  // nothing of the full deck's rail, detail waveform or cue list.
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  const row = page.locator('[role="gridcell"][data-col="title"]').nth(3);
  const title = (await row.textContent()) ?? "";
  await row.dblclick();

  // Set the deck running first: the switch must not stop or reload it.
  const clock = page.getByTestId("player-time");
  const start = await clock.textContent();
  await page.getByRole("button", { name: "Play", exact: true }).click();
  await expect(clock).not.toHaveText(start ?? "");

  await page.getByRole("button", { name: "Layout" }).click();
  await page.getByRole("menuitemradio", { name: "SIMPLE PLAYER" }).click();

  const strip = page.getByTestId("simple-player");
  await expect(strip).toBeVisible();
  await expect(page.getByRole("region", { name: /^Preview player/ })).toHaveCount(1);

  // Still running, and still the same track.
  await expect(strip.getByRole("button", { name: "Pause", exact: true })).toBeVisible();
  await expect(page.getByTestId("simple-player-title")).toHaveText(title);
  const simpleClock = page.getByTestId("simple-player-time");
  await expect(simpleClock).toHaveText(/^-\d?\d:\d\d\.\d$/);
  const shown = await simpleClock.textContent();
  await expect(simpleClock).not.toHaveText(shown ?? "");

  // What the strip holds.
  await expect(page.getByTestId("simple-player-artist")).not.toHaveText("");
  await expect(page.getByTestId("simple-player-key")).not.toHaveText("");
  await expect(page.getByTestId("simple-player-bpm")).toHaveText(/^\d+\.\d\d$/);
  await expect(page.getByTestId("simple-player-stars").locator("svg")).toHaveCount(5);
  const overview = page.getByTestId("simple-player-overview");
  await expect(overview).toHaveAttribute("role", "progressbar");
  // Four lettered badges and the memory cue's head beside the first.
  await expect(overview.locator('[data-cue]:not([data-cue=""])')).toHaveCount(4);
  await expect(overview.locator('[data-cue=""]')).toHaveCount(1);
  await expect(page.getByTestId("simple-player-head")).toBeAttached();
  await expect(page.getByTestId("simple-player-cue-point")).toBeAttached();
  await expect(strip.getByRole("button", { name: "Eject" })).toBeVisible();

  // What it does not: the full deck's rail, waveforms and panels.
  await expect(strip.getByRole("button", { name: "Cue", exact: true })).toHaveCount(0);
  await expect(strip.getByRole("button", { name: /Beat jump/ })).toHaveCount(0);
  await expect(page.getByTestId("player-detail")).toHaveCount(0);
  await expect(page.getByTestId("player-phrase")).toHaveCount(0);
  await expect(page.getByRole("tab", { name: "CUE/LOOP" })).toHaveCount(0);
  await expect(page.getByRole("complementary", { name: "Cue list" })).toHaveCount(0);

  // The measured geometry: a 59pt strip (1pt black over a 58pt row), the
  // browser 4pt under it (a point wider than the capture measured, by
  // request), the 40pt ring centred in a 58pt column, a 48pt sleeve 8pt on
  // from it, and the overview 16.5pt tall from 30pt down.
  const box = await strip.boundingBox();
  expect(box?.height).toBeCloseTo(59, 0);
  const body = await page.getByTestId("body").boundingBox();
  expect((body?.y ?? 0) - (box?.y ?? 0)).toBeCloseTo(63, 0);
  const play = await strip.getByRole("button", { name: "Pause", exact: true }).boundingBox();
  expect(play?.width).toBeCloseTo(40, 0);
  expect((play?.x ?? 0) - (box?.x ?? 0)).toBeCloseTo(9, 0);
  expect((play?.y ?? 0) - (box?.y ?? 0)).toBeCloseTo(10, 0);
  const sleeve = await strip.getByRole("button", { name: "Eject" }).boundingBox();
  expect(sleeve?.width).toBeCloseTo(48, 0);
  expect((sleeve?.x ?? 0) - (box?.x ?? 0)).toBeCloseTo(66, 0);
  expect((sleeve?.y ?? 0) - (box?.y ?? 0)).toBeCloseTo(6, 0);
  const wave = await overview.boundingBox();
  expect(wave?.height).toBeCloseTo(16.5, 0);
  expect((wave?.x ?? 0) - (box?.x ?? 0)).toBeCloseTo(119, 0);
  expect((wave?.y ?? 0) - (box?.y ?? 0)).toBeCloseTo(31, 0);
  const rating = await strip.getByLabel("Rating").boundingBox();
  expect(rating?.width).toBeCloseTo(206, 0);
  expect((box?.x ?? 0) + (box?.width ?? 0) - ((rating?.x ?? 0) + (rating?.width ?? 0))).toBeCloseTo(3, 0);
  // The overview stops 13pt short of the rating box; the readouts run to it.
  expect((rating?.x ?? 0) - ((wave?.x ?? 0) + (wave?.width ?? 0))).toBeCloseTo(13, 0);
  const bpm = await page.getByTestId("simple-player-bpm").boundingBox();
  expect((bpm?.x ?? 0) + (bpm?.width ?? 0)).toBeCloseTo(rating?.x ?? 0, 0);

  // Back to the full deck, still playing the same track.
  await page.getByRole("button", { name: "Layout" }).click();
  await page.getByRole("menuitemradio", { name: "1 PLAYER" }).click();
  await expect(page.getByTestId("player-detail")).toBeVisible();
  await expect(page.getByRole("button", { name: "Pause", exact: true })).toBeVisible();
  await expect(page.getByTestId("player-title")).toHaveText(title);
});

test("hovering the sleeve shows what clicking it does", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();

  // A sleeve does not look like a button, so the eject glyph is what says it
  // is one. Opacity rather than visibility: it fades rather than appearing.
  const sleeve = page.getByRole("button", { name: "Eject" });
  const glyph = sleeve.locator("svg").last();
  const shown = async () => Number(await glyph.evaluate((el) => getComputedStyle(el).opacity));
  expect(await shown()).toBe(0);
  await sleeve.hover();
  await expect.poll(shown).toBe(1);

  // Ejected, the same square is the load button instead, and the glyph that
  // said "eject" is gone with the track it belonged to.
  await sleeve.click();
  await expect(sleeve).toHaveCount(0);
  const empty = page.getByRole("button", { name: "Load the selected track" });
  await expect(empty).toBeVisible();
  // Live, because the double-click that loaded the track also selected it.
  await expect(empty).toBeEnabled();
  await expect(empty.locator("svg")).toHaveCount(1);
});

test("the deck answers rekordbox's own keys", async ({ page }) => {
  // Transcribed from the Export preset in rekordbox's KeyMappings, not chosen.
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();

  // Space plays and pauses.
  await page.keyboard.press(" ");
  await expect(page.getByRole("button", { name: "Pause", exact: true })).toBeVisible();
  await page.keyboard.press(" ");
  await expect(page.getByRole("button", { name: "Play", exact: true })).toBeVisible();

  // Q toggles quantize.
  const q = page.getByRole("button", { name: "Quantize" });
  await expect(q).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.press("q");
  await expect(q).toHaveAttribute("aria-pressed", "false");

  // F10, F11 and F12 are the three cue lists.
  for (const [key, tab] of [["F11", "HOT CUE"], ["F12", "INFO"], ["F10", "MEMORY"]] as const) {
    await page.keyboard.press(key);
    await expect(page.getByRole("tab", { name: tab })).toHaveAttribute("aria-selected", "true");
  }

  // And none of them fire into the search box.
  await page.getByPlaceholder(/Search/).first().click();
  await page.keyboard.press("q");
  await expect(q).toHaveAttribute("aria-pressed", "false");
});

test("right-clicking a track opens rekordbox's own menu", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).click({ button: "right" });

  const menu = page.getByRole("menu", { name: "Track" });
  await expect(menu).toBeVisible();
  // Its own list, in its own order — greyed entries and all, because a menu
  // half the length of the real one is a menu people have to relearn later.
  // Analysis writes to the library, which the mock holds read-only unless
  // asked otherwise, so here it is greyed as every write is.
  await expect(menu.getByRole("menuitem", { name: "Analyze Track" })).toBeDisabled();
  await expect(menu.getByRole("menuitem", { name: "Get Info from iTunes" })).toBeDisabled();
  // No cloud library behind this app, so the row is absent rather than greyed.
  await expect(menu.getByRole("menuitem", { name: "Cloud Library Sync" })).toHaveCount(0);
  // Live, because the default layout draws a player to load into. With none
  // — Full Browser — it is the greyed arrow rekordbox draws.
  await expect(menu.getByRole("menuitem", { name: "Load", exact: true })).toBeEnabled();
  await expect(menu.getByRole("menuitem", { name: "Show information" })).toBeEnabled();
  // Not in a playlist, so there is nothing to remove it from.
  await expect(menu.getByRole("menuitem", { name: "Remove from Playlist" })).toBeDisabled();

  await page.keyboard.press("Escape");
  await expect(menu).toBeHidden();
});

test("right-clicking the tree opens the folder menu", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  const playlist = page.getByRole("treeitem").filter({ hasText: "Melodic Vox" }).first();
  await playlist.click({ button: "right" });

  const menu = page.getByRole("menu", { name: "Playlist" });
  await expect(menu).toBeVisible();
  // A playlist says playlist and a folder says folder, as rekordbox does.
  await expect(menu.getByRole("menuitem", { name: "Export Playlist" })).toBeVisible();
  await expect(menu.getByRole("menuitem", { name: "Delete Playlist" })).toBeVisible();

  // Reading is offered; writing is not, because the mock's library is
  // read-only — the same refusal the rest of the app makes when rekordbox is
  // holding the database.
  await expect(menu.getByRole("menuitem", { name: "Export Playlist" })).toBeEnabled();
  await expect(menu.getByRole("menuitem", { name: "Create New Playlist" })).toBeDisabled();
  await expect(menu.getByRole("menuitem", { name: "Delete Playlist" })).toBeDisabled();

  // Add Artwork writes too, so it is refused here; Add To Shortcut is the
  // window's own and always live; the one rekordbox has that this does not
  // is greyed either way.
  await expect(menu.getByRole("menuitem", { name: "Add Artwork" })).toBeDisabled();
  await expect(menu.getByRole("menuitem", { name: "Add To Shortcut" })).toBeEnabled();
  await expect(menu.getByRole("menuitem", { name: "Playlist display setting" })).toBeDisabled();
});

test("Add To Shortcut puts the playlist on the rail, and the shortcut's own menu deletes it", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  const playlist = page.getByRole("treeitem").filter({ hasText: "Melodic Vox" }).first();
  await playlist.click({ button: "right" });
  await page.getByRole("menu", { name: "Playlist" }).getByRole("menuitem", { name: "Add To Shortcut" }).click();

  // The rail takes the playlist as a button of its own, opening it.
  const shortcut = page.getByRole("list", { name: "Shortcuts" }).getByRole("listitem", { name: "Melodic Vox" });
  await expect(shortcut).toBeVisible();
  await shortcut.click();
  await expect(page.getByTestId("browser-title")).toContainText("Melodic Vox");

  // rekordbox 7.2.11 keeps "Add To Shortcut" on the playlist's menu once it
  // is one; the shortcut goes from its own menu, whose one row is Delete
  // Shortcut.
  await playlist.click({ button: "right" });
  const menu = page.getByRole("menu", { name: "Playlist" });
  await expect(menu.getByRole("menuitem", { name: "Add To Shortcut" })).toBeVisible();
  await expect(menu.getByRole("menuitem", { name: "Remove from Shortcut" })).toHaveCount(0);
  await page.keyboard.press("Escape");
  await shortcut.click({ button: "right" });
  const own = page.getByRole("menu", { name: "Shortcut" });
  await expect(own.getByRole("menuitem")).toHaveText(["Delete Shortcut"]);
  await own.getByRole("menuitem", { name: "Delete Shortcut" }).click();
  await expect(shortcut).toHaveCount(0);
});

test("errors go to the status bar in red, not over the deck", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");

  // The player used to print its own failures across the pad row and the tree
  // beneath it. Nothing in the deck says anything now.
  await expect(page.getByRole("region", { name: "Preview player" }).getByRole("alert"))
    .toHaveCount(0);

  // A refusal lands in the footer, in red. A browser has no Finder, so the
  // menu item that asks for one is a real failure rather than a staged one.
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Show in Finder" }).click();

  const footer = page.getByRole("contentinfo");
  const alert = footer.getByRole("alert");
  await expect(alert).toBeVisible();
  const colour = await alert.evaluate((el) => getComputedStyle(el).color);
  const red = await page.evaluate(() =>
    getComputedStyle(document.documentElement).getPropertyValue("--c-error").trim(),
  );
  // The token, as a colour the browser has resolved.
  const expected = await page.evaluate((hex) => {
    const probe = document.createElement("span");
    probe.style.color = hex;
    document.body.append(probe);
    const resolved = getComputedStyle(probe).color;
    probe.remove();
    return resolved;
  }, red);
  expect(colour).toBe(expected);
});
