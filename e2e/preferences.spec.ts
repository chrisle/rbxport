import { expect, test, type Page } from "@playwright/test";

/**
 * The Preferences window's newer panes and sections: View › Color, the Beat
 * Count Display and waveform click on Display Type, Audio's own sections,
 * the full Keyboard map, and About. Each choice is checked where it lands —
 * the deck, the row, the engine call — not only in the window.
 */

async function open(page: Page, query = "") {
  await page.goto(`/${query}`);
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
}

async function prefs(page: Page) {
  await page.getByRole("banner").getByRole("button", { name: "Settings" }).click();
  const dialog = page.getByRole("dialog", { name: "Preferences" });
  await expect(dialog).toBeVisible();
  return dialog;
}

test("Rekordbox browse sync starts enabled and can be disabled", async ({ page }) => {
  await open(page);
  const dialog = await prefs(page);
  await dialog.getByRole("tab", { name: "Rekordbox" }).click();
  const sync = dialog.getByRole("switch", { name: "Keep browse settings synchronized" });
  await expect(sync).toBeChecked();
  await sync.uncheck();
  await page.reload();
  const again = await prefs(page);
  await again.getByRole("tab", { name: "Rekordbox" }).click();
  await expect(again.getByRole("switch", { name: "Keep browse settings synchronized" })).not.toBeChecked();
});

const player = (page: Page) => page.getByRole("region", { name: "Preview player" });

test("View VU Meter switches modes and persists the selection", async ({ page }) => {
  await open(page);
  const dialog = await prefs(page);
  const choices = dialog.getByRole("radiogroup", { name: "RBXport VU Meter", exact: true });
  await expect(choices.getByRole("radio", { name: "Normal (shows signal peaks, like rekordbox)", exact: true })).toBeChecked();
  await choices.getByRole("radio", { name: "Advanced (peak + RMS, inspired by FabFilter Pro-L 2)", exact: true }).click();
  await expect(page.getByRole("banner").locator('[role="meter"][data-mode="fabulous"]')).toHaveCount(2);
  await expect(page.getByRole("banner").getByTestId("vu-rms")).toHaveCount(2);
  await page.reload();
  const again = await prefs(page);
  await expect(again.getByRole("radio", { name: "Advanced (peak + RMS, inspired by FabFilter Pro-L 2)", exact: true })).toBeChecked();
  await again.getByRole("radio", { name: "Normal (shows signal peaks, like rekordbox)", exact: true }).click();
  await expect(page.getByRole("banner").locator('[role="meter"][data-mode="normal"]')).toHaveCount(2);
  await expect(page.getByRole("banner").getByTestId("vu-rms")).toHaveCount(0);
});

test("View › Key display format offers and saves the browser key sort", async ({ page }) => {
  await open(page);
  const dialog = await prefs(page);
  const section = dialog.getByRole("region", { name: "Key display format" });
  const sort = section.getByRole("radiogroup", { name: "Sort keys" });
  await expect(sort.getByRole("radio", { name: "Alphabetically — A, Ab, B, …" })).toBeChecked();
  await sort.getByRole("radio", { name: "Musically — Abm, B, Ebm, F#, Bbm, …" }).click();

  await page.reload();
  const again = await prefs(page);
  await expect(again.getByRole("radiogroup", { name: "Sort keys" })
    .getByRole("radio", { name: "Musically — Abm, B, Ebm, F#, Bbm, …" })).toBeChecked();
});

/** Loads the fourth row — analysed, with the mock's four hot cues and a memory cue. */
async function load(page: Page) {
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();
  await expect(player(page).getByRole("button", { name: "Play", exact: true })).toBeEnabled();
}

test("memory cues display their saved notes", async ({ page }) => {
  await open(page);
  await load(page);
  const cues = player(page).getByRole("complementary", { name: "Cue list" });
  await expect(cues).toContainText("136 BPM");
  await expect(cues).not.toContainText("CUE(Auto)");
});

test("clicking the tempo readout toggles the tempo slider", async ({ page }) => {
  await open(page);
  await load(page);
  const toggle = page.getByRole("button", { name: "Toggle tempo slider" });
  await expect(toggle).toHaveAttribute("aria-expanded", "false");
  await toggle.click();
  await expect(page.getByRole("slider", { name: "Tempo", exact: true })).toBeVisible();
  await toggle.click();
  await expect(page.getByRole("slider", { name: "Tempo", exact: true })).toHaveCount(0);

  await page.getByRole("button", { name: "Layout" }).click();
  await page.getByRole("menuitemradio", { name: "2 PLAYER" }).click();
  await toggle.first().click();
  await expect(page.getByRole("slider", { name: "Tempo", exact: true })).toHaveCount(2);
  await toggle.first().click();
  await expect(page.getByRole("slider", { name: "Tempo", exact: true })).toHaveCount(0);
});

test("View › Color offers the three palettes and the two hot cue colours, and keeps them", async ({ page }) => {
  await open(page);
  const dialog = await prefs(page);
  await dialog.getByRole("tab", { name: "Color" }).click();

  // Appearance is drawn, and greyed: the light theme is not built.
  await expect(dialog.getByRole("radio", { name: "Dark" })).toBeChecked();
  await expect(dialog.getByRole("radio", { name: "Light" })).toBeDisabled();

  const palette = dialog.getByRole("radiogroup", { name: "Waveform color" });
  await expect(palette.getByRole("radio", { name: "3Band" })).toBeChecked();
  await palette.getByRole("radio", { name: "RGB" }).click();
  await dialog.getByRole("combobox", { name: "HOT CUE color" }).selectOption("cdj");
  await page.keyboard.press("Escape");

  await page.reload();
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  const again = await prefs(page);
  await again.getByRole("tab", { name: "Color" }).click();
  await expect(again.getByRole("radiogroup", { name: "Waveform color" }).getByRole("radio", { name: "RGB" })).toBeChecked();
  await expect(again.getByRole("combobox", { name: "HOT CUE color" })).toHaveValue("cdj");
});

test("HOT CUE color CDJ draws every pad and badge in the one green", async ({ page }) => {
  await open(page);
  await load(page);
  // The mock's fourth track has coloured hot cues: pad A carries its colour.
  const padA = player(page).locator('[aria-label="Hot cues"]').getByRole("button", { name: "Hot cue A", exact: true });
  await expect(padA).toHaveAttribute("style", /--cue-colour/);
  const badgeA = page.getByTestId("player-overview").locator('[data-cue="A"]');
  await expect(badgeA).toHaveAttribute("style", /--cue-colour/);

  const dialog = await prefs(page);
  await dialog.getByRole("tab", { name: "Color" }).click();
  await dialog.getByRole("combobox", { name: "HOT CUE color" }).selectOption("cdj");
  await page.keyboard.press("Escape");

  // The colour is gone from the style, so the stylesheet's green shows.
  await expect(padA).not.toHaveAttribute("style", /--cue-colour/);
  await expect(badgeA).not.toHaveAttribute("style", /--cue-colour/);
});

test("the waveform palette redraws the deck and the rows without a reload", async ({ page }) => {
  await open(page);
  await load(page);
  const overview = page.getByTestId("player-overview").locator("canvas").first();
  const pixels = () =>
    overview.evaluate((c: HTMLCanvasElement) => {
      const ctx = c.getContext("2d");
      if (!ctx) return "";
      // A row of pixels through the middle, as a string, to compare.
      return Array.from(ctx.getImageData(0, Math.floor(c.height / 2), c.width, 1).data).join(",");
    });
  await expect.poll(pixels).not.toBe("");
  const threeBand = await pixels();

  const dialog = await prefs(page);
  await dialog.getByRole("tab", { name: "Color" }).click();
  await dialog.getByRole("radiogroup", { name: "Waveform color" }).getByRole("radio", { name: "BLUE" }).click();
  await page.keyboard.press("Escape");
  await expect.poll(pixels).not.toBe(threeBand);
});

test("the Beat Count Display counts bars, or down to the next memory cue", async ({ page }) => {
  await open(page);
  await load(page);
  const bars = page.getByTestId("player-bars");
  await expect(bars).toHaveText(/^\d+\.\d Bars$/);

  const dialog = await prefs(page);
  await dialog.getByRole("radio", { name: "Count to the next MEMORY CUE (Beats)" }).click();
  // The mock's memory cue is at 2 % of the track: ahead of a fresh load.
  await expect(bars).toHaveText(/^-\d+Beats$/);
  await dialog.getByRole("radio", { name: "Count to the next MEMORY CUE (Bars)" }).click();
  await expect(bars).toHaveText(/^-\d+\.\d Bars$/);
  await dialog.getByRole("radio", { name: "Current Position (Bars)" }).click();
  await expect(bars).toHaveText(/^\d+\.\d Bars$/);
});

test("a click on the enlarged waveform plays, and again pauses and sets the cue, unless off", async ({ page }) => {
  await open(page);
  await load(page);
  const detail = page.getByTestId("player-detail");
  const box = await detail.boundingBox();
  if (!box) throw new Error("no detail waveform");
  const play = player(page).getByRole("button", { name: "Play", exact: true });
  const pause = player(page).getByRole("button", { name: "Pause", exact: true });

  // A stopped deck: the click plays it from where the head is.
  await detail.click({ position: { x: box.width * 0.75, y: box.height / 2 } });
  await expect(pause).toBeVisible();
  // A second click, once the head has moved, pauses it and takes the head
  // as the cue point.
  await expect(page.getByTestId("player-overview")).not.toHaveAttribute("aria-valuenow", "0");
  await detail.click({ position: { x: box.width * 0.75, y: box.height / 2 } });
  await expect(play).toBeVisible();
  await expect(page.getByTestId("player-overview")).not.toHaveAttribute("aria-valuenow", "0");

  const dialog = await prefs(page);
  await dialog.getByRole("switch", { name: "Enable" }).click();
  await page.keyboard.press("Escape");
  const before = await page.getByTestId("player-overview").getAttribute("aria-valuenow");
  await detail.click({ position: { x: box.width * 0.25, y: box.height / 2 } });
  await page.waitForTimeout(300);
  await expect(play).toBeVisible();
  await expect(page.getByTestId("player-overview")).toHaveAttribute("aria-valuenow", before ?? "0");
});

test("Audio has its own sections for the rate, the buffer, the metronome and the limiter", async ({ page }) => {
  await open(page);
  const dialog = await prefs(page);
  await dialog.getByRole("tab", { name: "Audio" }).click();
  for (const name of ["Sample Rate", "Buffer size", "Metronome", "RBXport Master Limiter"]) {
    await expect(dialog.getByRole("heading", { name })).toBeVisible();
  }
  await expect(dialog.getByRole("combobox", { name: "Sample Rate" })).toHaveValue("48000");
  await expect(dialog.getByTestId("buffer-size")).toHaveText("512 samples (10.7 ms)");
  await dialog.getByRole("combobox", { name: "Sample Rate" }).selectOption("96000");
  await expect(dialog.getByTestId("buffer-size")).toHaveText("512 samples (5.3 ms)");
  await expect(dialog.getByRole("radiogroup", { name: "Metronome" }).getByRole("radio", { name: "Click Sound 02" })).toBeChecked();
  await expect(dialog.getByRole("radiogroup", { name: "Metronome volume" }).getByRole("radio", { name: "Large" })).toBeChecked();
  await expect(dialog.getByRole("switch", { name: "Enable limiter" })).toBeVisible();
});

test("the deck's metronome button is live once a track is loaded", async ({ page }) => {
  await open(page);
  // The GRID EDIT row lives behind the pad row's GRID tab.
  await player(page).getByRole("tab", { name: "GRID" }).click();
  const metronome = player(page).getByRole("button", { name: /^Metronome volume:/ });
  await expect(metronome).toBeDisabled();
  await load(page);
  await expect(metronome).toBeEnabled();
  for (const level of ["Low", "Medium", "High", "Low"]) {
    await metronome.click();
    await expect(metronome).toHaveAccessibleName(`Metronome volume: ${level}`);
  }
});

test("Keyboard lists rekordbox's ten groups, with the unbuilt rows greyed", async ({ page }) => {
  await open(page);
  const dialog = await prefs(page);
  await dialog.getByRole("tab", { name: "Keyboard" }).click();
  const groups = dialog.locator('button[aria-expanded]');
  await expect(groups).toHaveText([
    "Browse", "Player A", "Player B", "General", "File", "View", "Track", "Playlist", "Help", "Link Export",
  ]);
  // Every group opens closed; Player A opened: Play/Pause works here, Time
  // Mode is rekordbox's alone.
  await expect(dialog.locator('[class*="keyRow"]')).toHaveCount(0);
  await dialog.getByRole("button", { name: "Player A" }).click();
  const playPause = dialog.locator('[class*="keyRow"]', { hasText: "Play/Pause" }).first();
  await expect(playPause).toContainText("spacebar");
  await expect(playPause).not.toHaveAttribute("data-dim");
  const timeMode = dialog.locator('[class*="keyRow"]', { hasText: "Time Mode" }).first();
  await expect(timeMode).toContainText(/^Time ModeT$/);
  await expect(timeMode).toHaveAttribute("data-dim", "");
  // A built row's key is a button: click it, press a key, and that is the key.
  const loopIn = dialog.locator('[class*="keyRow"]', { hasText: "Loop In" }).first();
  await expect(loopIn).not.toHaveAttribute("data-dim");
  await loopIn.getByRole("button", { name: "Loop In key" }).click();
  await page.keyboard.press("Shift+L");
  await expect(loopIn.getByRole("button", { name: "Loop In key" })).toHaveText("shift + L");
  // Player B's shift + L was Memory Cue 9; the key moved, so that row lost it.
  await dialog.getByRole("button", { name: "Player B" }).click();
  const cue9 = dialog.locator('[class*="keyRow"]', { hasText: "Memory Cue 9" }).nth(1);
  await expect(cue9.getByRole("button", { name: "Memory Cue 9 key" })).toHaveText("+");
  await dialog.getByRole("button", { name: "Reset to the preset" }).click();
  await expect(loopIn.getByRole("button", { name: "Loop In key" })).toHaveText("I");
  // A row rekordbox lists unbound has no key.
  await expect(dialog.locator('[class*="keyRow"]', { hasText: "1/64 Beat Loop" }).first()).toHaveText("1/64 Beat Loop");

  await dialog.getByRole("button", { name: "Link Export" }).click();
  await expect(dialog.getByText("Nothing is bound here in the Export preset.")).toBeVisible();
});

test("About groups version and update controls", async ({ page }) => {
  await open(page);
  const dialog = await prefs(page);
  await dialog.getByRole("tab", { name: "About" }).click();
  await expect(dialog.getByRole("heading", { name: "About" })).toHaveCount(0);
  await expect(dialog.getByText("Made by TRIODE with")).toBeVisible();
  await expect(dialog.getByText("in California", { exact: true })).toBeVisible();
  await expect(dialog.getByRole("heading", { name: "Support rbxport" })).toBeVisible();
  await expect(dialog.getByText("rbxport is free to use and independently developed by TRIODE.")).toBeVisible();
  await expect(dialog.getByRole("heading", { name: "rbxport", exact: true })).toBeVisible();
  await expect(dialog.getByRole("group", { name: "Author links" })).toBeVisible();
  const updates = dialog.getByRole("region", { name: "About", exact: true });
  await expect(dialog.getByTestId("about-version")).toHaveText("v0.4.0");
  await expect(updates.getByRole("button", { name: "Check for updates" })).toBeVisible();
  const auto = dialog.getByRole("switch", { name: "Download updates" });
  await expect(auto).toBeChecked();
  await expect(dialog.getByRole("combobox", { name: "Update frequency" })).toHaveValue("start");
  await auto.click();
  await expect(auto).not.toBeChecked();
  await expect(dialog.getByRole("combobox", { name: "Update frequency" })).toBeDisabled();
  await expect(dialog.getByRole("button", { name: "Check for updates" })).toBeEnabled();

  const advancedItem = dialog.locator("li").filter({ has: page.getByRole("tab", { name: "Advanced", exact: true }) });
  await expect(advancedItem.locator("+ li").getByRole("tab")).toHaveAccessibleName("Rekordbox");
  await dialog.getByRole("tab", { name: "Advanced", exact: true }).click();
  await dialog.getByRole("tab", { name: "Others" }).click();
  await expect(dialog.getByRole("switch", { name: "Download updates" })).toHaveCount(0);
});

test("the player's ≡ opens rekordbox's own menu, and its choices are the View preferences", async ({ page }) => {
  await page.setViewportSize({ width: 1100, height: 750 });
  await open(page);
  await load(page);
  await player(page).getByRole("button", { name: "Player menu" }).click();
  const menu = page.getByRole("menu", { name: "Player menu" });
  await expect(menu).toBeVisible();
  await expect(menu.getByRole("menuitem")).toHaveText([
    "Change waveform color", "Analyze Track", "Beat Count Display", "Export Track", "Export Loop As WAV",
    "Active Loop Playback", "Click on the waveform for PLAY and CUE",
  ]);
  // Export Track lists the mock's sticks; Export Loop As WAV waits for a
  // loop; Active Loop Playback is greyed for good (its column's "on" value
  // has never been seen).
  await expect(menu.getByRole("menuitem", { name: "Export Track" })).toBeEnabled();
  for (const greyed of ["Export Loop As WAV", "Active Loop Playback"]) {
    await expect(menu.getByRole("menuitem", { name: greyed })).toBeDisabled();
  }
  // Analysis writes to the library, which the mock holds read-only here.
  await expect(menu.getByRole("menuitem", { name: "Analyze Track" })).toBeDisabled();

  for (const name of ["Beat Count Display", "Export Track", "Click on the waveform for PLAY and CUE", "Change waveform color"]) {
    await menu.getByRole("menuitem", { name, exact: true }).hover();
    const submenu = page.getByRole("menu", { name, exact: true });
    await expect(submenu).toBeVisible();
    const bounds = await submenu.boundingBox();
    expect(bounds).not.toBeNull();
    expect(bounds!.x).toBeGreaterThanOrEqual(8);
    expect(bounds!.y).toBeGreaterThanOrEqual(8);
    expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(1092);
    expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(742);
  }

  // The submenu ticks what is in force, and choosing changes the preference.
  await menu.getByRole("menuitem", { name: "Change waveform color" }).hover();
  const colours = page.getByRole("menu", { name: "Change waveform color" });
  await expect(colours.getByRole("menuitemradio", { name: "3Band" })).toHaveAttribute("aria-checked", "true");
  await colours.getByRole("menuitemradio", { name: "BLUE" }).click();
  await expect(menu).toBeHidden();
  const dialog = await prefs(page);
  await dialog.getByRole("tab", { name: "Color" }).click();
  await expect(dialog.getByRole("radiogroup", { name: "Waveform color" }).getByRole("radio", { name: "BLUE" })).toBeChecked();
  await page.keyboard.press("Escape");
});

// Typing a BPM directly and dragging the readout to open a fader popup were
// TempoField's; the deck tempo slider that replaced it (0bd2668, 2026-09-20)
// is covered by "the deck tempo slider cycles ranges, resets, and can be
// hidden" below. What is left here, and unique to this test, is the key
// shift, which that redesign did not touch.
test("a BPM's key can be shifted a semitone either way, independent of tempo", async ({ page }) => {
  await open(page);
  await load(page);
  const deck = player(page);
  const bpm = deck.getByTestId("player-bpm");
  const resting = await bpm.innerText();

  const shift = deck.getByTestId("key-shift");
  await expect(shift).toHaveCount(0);
  await deck.getByRole("button", { name: "Key up a semitone" }).click();
  await expect(shift).toHaveText("+1");
  await deck.getByRole("button", { name: "Key down a semitone" }).click();
  await deck.getByRole("button", { name: "Key down a semitone" }).click();
  await expect(shift).toHaveText("-1");
  // The BPM is untouched by the key.
  await expect(bpm).toHaveText(resting);
});

test("the deck tempo slider cycles ranges, resets, and can be hidden", async ({ page }) => {
  await page.addInitScript(() => {
    if (sessionStorage.getItem("e2e.tempoSliderSeeded")) return;
    const preferences = JSON.parse(localStorage.getItem("rbl.preferences") ?? "{}");
    preferences.view = { ...preferences.view, tempoSlider: true };
    localStorage.setItem("rbl.preferences", JSON.stringify(preferences));
    sessionStorage.setItem("e2e.tempoSliderSeeded", "1");
  });
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();
  const deck = page.getByRole("region", { name: "Preview player", exact: true });
  const panel = deck.getByRole("complementary", { name: "Tempo slider" });
  const range = panel.getByRole("button", { name: "Tempo range", exact: true });
  for (const value of ["±6", "±10", "±16", "WIDE", "±6"]) {
    await expect(range).toHaveText(value);
    await range.click();
  }
  const slider = panel.getByRole("slider", { name: "Tempo", exact: true });
  const reset = panel.getByRole("button", { name: "Reset tempo" });
  await expect(reset).toHaveAttribute("aria-pressed", "true");
  await expect(reset).toHaveCSS("background-color", "rgb(102, 221, 66)");
  await expect(slider).toHaveAttribute("aria-disabled", "true");
  await slider.click({ force: true, position: { x: 5, y: 5 } });
  await expect(slider).toHaveAttribute("aria-valuenow", "0");
  await reset.click();
  await expect(slider).toHaveAttribute("aria-disabled", "false");
  const master = panel.getByRole("button", { name: "Master tempo", exact: true });
  if (await master.getAttribute("aria-pressed") !== "true") await master.click();
  await expect(master).toHaveCSS("color", "rgb(227, 49, 34)");
  const rangeBox = (await range.boundingBox())!;
  const masterBox = (await master.boundingBox())!;
  const sliderBox = (await slider.boundingBox())!;
  const resetBox = (await reset.boundingBox())!;
  expect(rangeBox.y + rangeBox.height).toBeLessThanOrEqual(masterBox.y);
  expect(masterBox.y + masterBox.height).toBeLessThanOrEqual(sliderBox.y);
  expect(resetBox.x + resetBox.width).toBeLessThan(sliderBox.x);
  expect(Math.abs(resetBox.y + resetBox.height / 2 - sliderBox.y - sliderBox.height / 2)).toBeLessThan(1);
  await page.mouse.move(sliderBox.x + sliderBox.width / 2, sliderBox.y + sliderBox.height / 2);
  await page.mouse.down();
  await page.mouse.move(sliderBox.x + sliderBox.width / 2, sliderBox.y + sliderBox.height * 0.75);
  const readout = page.locator("output").filter({ hasText: /[+-]\d+\.\d+%/ });
  await expect(readout).toBeVisible();
  await page.mouse.up();
  await expect(readout).toHaveCount(0);
  await reset.click();
  await expect(slider).toHaveAttribute("aria-valuenow", "0");
  await reset.click();
  await slider.focus();
  await slider.press("ArrowDown");
  await expect(slider).toHaveAttribute("aria-valuenow", "0.1");
  await panel.getByRole("button", { name: "Reset tempo" }).click();
  await expect(slider).toHaveAttribute("aria-valuenow", "0");
  await page.evaluate(() => {
    const value = JSON.parse(localStorage.getItem("rbl.preferences") ?? "{}");
    value.view = { ...value.view, tempoSlider: false };
    localStorage.setItem("rbl.preferences", JSON.stringify(value));
  });
  await page.reload();
  await expect(panel).toHaveCount(0);
});


test("Analysis describes the selected mode", async ({ page }) => {
  await open(page);
  const dialog = await prefs(page);
  await dialog.getByRole("tab", { name: "Analysis", exact: true }).click();
  const mode = dialog.getByRole("combobox", { name: "Analysis mode", exact: true });
  await mode.selectOption("rekordbox");
  await expect(dialog.getByText("Normal mode with a 70–180 BPM range", { exact: false })).toBeVisible();
  await mode.selectOption("rbxport");
  await expect(dialog.getByText("Aligns beats to kick drums", { exact: false })).toBeVisible();
  await expect(dialog.getByText("Normal mode with a 70–180 BPM range", { exact: false })).toHaveCount(0);
});

test("Show BPM changes controls waveform annotations and persists", async ({page}) => {
  await open(page, "?writable=1");
  await load(page);
  const overview = page.getByTestId("player-overview");
  const bounds = await overview.boundingBox();
  if (!bounds) throw new Error("Missing overview");
  await page.mouse.click(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2);
  await player(page).getByRole("tab", {name:"GRID", exact:true}).click();
  await player(page).getByRole("button", {name:"Adjust beats from here", exact:true}).click();
  await player(page).getByRole("button", {name:"Double the tempo", exact:true}).click();
  await expect(page.getByTestId("overview-tempo")).not.toHaveCount(0);
  const dialog = await prefs(page);
  await dialog.getByRole("tab", {name:"Layout", exact:true}).click();
  const toggle = dialog.getByRole("checkbox", {name:"Show BPM changes", exact:true});
  await expect(toggle).toBeChecked();
  await toggle.click();
  await expect(page.getByTestId("overview-tempo")).toHaveCount(0);
  await expect(page.getByTestId("cue-tempo")).toHaveCount(0);
  await toggle.click();
  await expect(page.getByTestId("overview-tempo")).not.toHaveCount(0);
  await toggle.click();
  await page.reload();
  const reopened = await prefs(page);
  await reopened.getByRole("tab", {name:"Layout", exact:true}).click();
  await expect(reopened.getByRole("checkbox", {name:"Show BPM changes", exact:true})).not.toBeChecked();
});

test("Browse › FontSize and Line Space apply to the playlist tree", async ({ page }) => {
  await page.addInitScript(() =>
    localStorage.setItem("rbl.preferences", JSON.stringify({ view: { browseFontSize: 4, browseLineSpace: 4 } })));
  await open(page);
  const node = page.getByRole("treeitem").first();
  const { height, size } = await node.evaluate((el) => {
    const css = getComputedStyle(el);
    return { height: parseFloat(css.height), size: parseFloat(css.fontSize) };
  });
  const base = await page.evaluate(() => parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--f-size-ui-base")) || 0);
  expect(height).toBe(33);
  if (base) expect(size).toBeCloseTo(base * 1.3, 1);
});
