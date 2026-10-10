import { expect, test } from "@playwright/test";

test("analysis settings gate the selected batch and can be cancelled", async ({ page }) => {
  await page.goto("/?writable=1");
  const rows = page.getByRole("row").filter({ has: page.getByRole("gridcell") });
  await rows.nth(2).click();
  await rows.nth(4).click({ modifiers: ["Shift"] });
  await page.keyboard.press("Shift+Meta+A");
  const dialog = page.getByRole("dialog", { name: "Analysis Setting" });
  await expect(dialog).toContainText("3 tracks selected");
  await expect(page.getByRole("contentinfo")).not.toContainText("Analyzing ");
  await expect(dialog.getByRole("checkbox", { name: "BPM / Grid", exact: true })).toBeChecked();
  await expect(dialog.getByRole("checkbox", { name: "High precision analysis" })).toBeChecked();
  await dialog.getByRole("combobox", { name: "BPM Range" }).selectOption("98-195");
  await dialog.getByRole("checkbox", { name: "BPM / Grid", exact: true }).uncheck();
  await expect(dialog.getByRole("combobox", { name: "BPM Range" })).toBeDisabled();
  await dialog.getByRole("checkbox", { name: "KEY", exact: true }).uncheck();
  await expect(dialog.getByRole("button", { name: "OK", exact: true })).toBeDisabled();
  await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole("contentinfo")).not.toContainText("Analyzing ");

  await page.keyboard.press("Shift+Meta+A");
  await expect(dialog).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
});

test("analysis settings submit a key-only batch", async ({ page }) => {
  await page.goto("/?writable=1");
  const rows = page.getByRole("row").filter({ has: page.getByRole("gridcell") });
  await rows.nth(2).click();
  await rows.nth(9).click({ modifiers: ["Shift"] });
  await page.keyboard.press("Shift+Meta+A");
  const dialog = page.getByRole("dialog", { name: "Analysis Setting" });
  await dialog.getByRole("checkbox", { name: "BPM / Grid", exact: true }).uncheck();
  await dialog.getByRole("button", { name: "OK", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole("contentinfo")).toContainText("Analyzing ");
  await expect(page.getByRole("contentinfo").getByRole("button", { name: "Stop" })).toHaveCount(0, { timeout: 15_000 });
});

test("the first-beat memory cue is a preference, not a per-batch choice", async ({ page }) => {
  await page.goto("/?writable=1");
  const rows = page.getByRole("row").filter({ has: page.getByRole("gridcell") });
  await rows.nth(2).click();
  const dialog = page.getByRole("dialog", { name: "Analysis Setting" });
  await page.keyboard.press("Shift+Meta+A");
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole("checkbox", { name: "Add memory cue at first beat" })).toHaveCount(0);
  await dialog.getByRole("button", { name: "Cancel", exact: true }).click();

  await page.getByRole("banner").getByRole("button", { name: "Settings" }).click();
  const preferences = page.getByRole("dialog", { name: "Preferences" });
  await preferences.getByRole("tab", { name: "Analysis" }).click();
  const preference = preferences.getByRole("region", { name: "Track Analysis" }).getByRole("switch", { name: "Add memory cue at first beat" });
  await expect(preference).not.toBeChecked();
  await preference.click();
  await expect(preference).toBeChecked();
});

test("a macOS Control-click menu analyses the whole selection (#135)", async ({ page }) => {
  await page.goto("/?writable=1");
  // Control-click is the secondary click only on macOS; elsewhere it toggles.
  test.skip(!(await page.evaluate(() => /Mac/.test(navigator.platform))), "macOS only");
  const rows = page.getByRole("row").filter({ has: page.getByRole("gridcell") });
  await rows.nth(2).click();
  await rows.nth(4).click({ modifiers: ["Shift"] });
  // Playwright sends what macOS does: a left press with Control, then contextmenu.
  await rows.nth(3).click({ modifiers: ["Control"] });
  await page.getByRole("menuitem", { name: "Analyze Track" }).click();
  const dialog = page.getByRole("dialog", { name: "Analysis Setting" });
  await expect(dialog).toContainText("3 tracks selected");
  await page.screenshot({ path: test.info().outputPath("control-click-analyse.png") });
  await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(rows.and(page.locator("[data-selected]"))).toHaveCount(3);
});

// rekordbox 7.2.14, Auto Analysis on, unanalysed tracks in Collection: at
// launch it shows Analysis Setting with "Auto Analysis is starting." and
// OK/Cancel, BPM / Grid ticked and greyed, and no selection count (#207).
test("Auto Analysis asks at launch before analysing never-analysed tracks", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("rbl.preferences", JSON.stringify({ analysis: { auto: true } })));
  await page.goto("/?writable=1");
  const dialog = page.getByRole("dialog", { name: "Analysis Setting" });
  await expect(dialog).toContainText("Auto Analysis is starting.");
  await expect(dialog).not.toContainText("selected");
  await expect(dialog).not.toContainText("will be overwritten");
  const bpmGrid = dialog.getByRole("checkbox", { name: "BPM / Grid", exact: true });
  await expect(bpmGrid).toBeChecked();
  await expect(bpmGrid).toBeDisabled();
  await expect(dialog.getByRole("checkbox", { name: "KEY", exact: true })).toBeEnabled();
  await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole("contentinfo")).not.toContainText("Analyzing ");

  // Asked again at the next launch, and OK starts the run.
  await page.reload();
  await expect(dialog).toContainText("Auto Analysis is starting.");
  await dialog.getByRole("button", { name: "OK", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole("contentinfo")).toContainText("Analyzing ");
});

test("Auto Analysis does not ask when it is off or the library is read-only", async ({ page }) => {
  const rows = page.getByRole("row").filter({ has: page.getByRole("gridcell") });
  const dialog = page.getByRole("dialog", { name: "Analysis Setting" });
  await page.goto("/?writable=1");
  await expect(rows.first()).toBeVisible();
  await page.waitForTimeout(500);
  await expect(dialog).toHaveCount(0);

  await page.addInitScript(() => localStorage.setItem("rbl.preferences", JSON.stringify({ analysis: { auto: true } })));
  await page.goto("/");
  await expect(rows.first()).toBeVisible();
  await page.waitForTimeout(500);
  await expect(dialog).toHaveCount(0);
});

// rekordbox 7.2.19 (static): loading a never-analysed Collection track onto a
// deck asks with Analysis Setting ("Note: You can change this setting in
// [Preferences] > [Analysis].", "Don't ask me again", OK only) and analyses
// it, whether Auto Analysis is on or not; the deck then draws its waveform
// (#330). Before, the deck stayed black until the track was loaded again.
test("loading a never-analysed track analyses it and the deck draws its waveform", async ({ page }) => {
  // Twenty mock tracks: the unanalysed ones among them are not the mock's
  // every-seventh track that fails to analyse.
  await page.goto("/?writable=1&tracks=20");
  const titles = page.locator('[role="gridcell"][data-col="title"]');
  // An analysed row has a preview canvas; wait for the first ones to show.
  await expect(titles.nth(3).locator("xpath=..").locator('[data-col="preview"] canvas')).toHaveCount(1);
  const unanalysed: number[] = [];
  for (let row = 0; row < 20 && unanalysed.length < 2; row++) {
    const canvases = await titles.nth(row).locator("xpath=..").locator('[data-col="preview"] canvas').count();
    if (canvases === 0) unanalysed.push(row);
  }
  expect(unanalysed).toHaveLength(2);
  const [first = 0, second = 0] = unanalysed;
  const overview = page.getByTestId("player-overview");
  const dialog = page.getByRole("dialog", { name: "Analysis Setting" });

  await titles.nth(first).dblclick();
  await expect(dialog).toContainText("Note: You can change this setting in [Preferences] > [Analysis].");
  await expect(dialog).not.toContainText("selected");
  await expect(dialog).not.toContainText("Auto Analysis is starting.");
  const bpmGrid = dialog.getByRole("checkbox", { name: "BPM / Grid", exact: true });
  await expect(bpmGrid).toBeChecked();
  await expect(bpmGrid).toBeDisabled();
  await expect(dialog.getByRole("button", { name: "Cancel", exact: true })).toHaveCount(0);
  await expect(overview.locator("canvas")).toHaveCount(0);
  await dialog.getByRole("checkbox", { name: "Don't ask me again" }).check();
  await dialog.getByRole("button", { name: "OK", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  // The deck that was loaded black draws the waveform once it is analysed.
  await expect(overview.locator("canvas")).toHaveCount(1, { timeout: 15_000 });

  // Not asked again: the next never-analysed track is analysed straight away.
  await titles.nth(second).dblclick();
  await expect(page.getByRole("contentinfo")).toContainText("Analyzing ");
  await expect(dialog).toHaveCount(0);
  await expect(overview.locator("canvas")).toHaveCount(1, { timeout: 15_000 });
});

test("loading a never-analysed track into a read-only library asks nothing", async ({ page }) => {
  await page.goto("/");
  const titles = page.locator('[role="gridcell"][data-col="title"]');
  await expect(titles.nth(3).locator("xpath=..").locator('[data-col="preview"] canvas')).toHaveCount(1);
  let row = 0;
  while (row < 30 && (await titles.nth(row).locator("xpath=..").locator('[data-col="preview"] canvas').count()) > 0) row++;
  await titles.nth(row).dblclick();
  await expect(page.getByRole("region", { name: "Preview player" }).getByRole("button", { name: "Play", exact: true })).toBeEnabled();
  await page.waitForTimeout(300);
  await expect(page.getByRole("dialog", { name: "Analysis Setting" })).toHaveCount(0);
});
