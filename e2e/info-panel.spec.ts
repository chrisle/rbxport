import { expect, test, type Page } from "@playwright/test";
import { enableTooltips } from "./helpers";

/**
 * The information panel's three tabs against the mock backend.
 *
 * The panel opens the way the native View menu opens it; a browser has no
 * menu bar, so the mock exposes the listener as `window.__menu`.
 */
async function openPanel(page: Page, path = "/") {
  await page.goto(path);
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  // Not the first row: it sits under the sticky column header.
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).click();
  await page.waitForFunction(() => "__menu" in window);
  await page.evaluate(() => (window as unknown as { __menu: (id: string) => void }).__menu("info"));
  const panel = page.getByRole("complementary", { name: "Information" });
  await expect(panel).toBeVisible();
  return panel;
}

test("the panel has Summary, Info and Artwork tabs, and Summary is first", async ({ page }) => {
  const panel = await openPanel(page);
  const tabs = panel.getByRole("tablist", { name: "Information" });
  await expect(tabs.getByRole("tab")).toHaveText(["Summary", "Info", "Artwork"]);
  await expect(tabs.getByRole("tab", { name: "Summary" })).toHaveAttribute("aria-selected", "true");

  await tabs.getByRole("tab", { name: "Info" }).click();
  await expect(tabs.getByRole("tab", { name: "Info" })).toHaveAttribute("aria-selected", "true");
  await expect(panel.getByRole("tabpanel")).toHaveAttribute("id", "info-panel-info");

  await tabs.getByRole("tab", { name: "Artwork" }).click();
  await expect(panel.getByRole("tabpanel")).toHaveAttribute("id", "info-panel-artwork");
  await expect(panel.getByRole("button", { name: "Add Artwork" })).toBeDisabled();
  await expect(panel.getByRole("button", { name: "Delete Artwork" })).toBeDisabled();
});

test("Summary shows the record the row does not carry", async ({ page }) => {
  const panel = await openPanel(page);
  const title = await page.locator('[role="gridcell"][data-col="title"]').nth(3).innerText();
  await expect(panel).toContainText(title);

  // Every row of the table, in the captured order. The values are the mock's
  // own — an MP3 or a WAV, depending which track is fourth — so each is
  // pinned by its units rather than by one track's numbers.
  const labels = panel.locator("dl").last().locator("dt");
  await expect(labels).toHaveText([
    "Time", "File Type", "Size", "Date Created", "Sample Rate", "Bitrate", "DJ Play Count", "Location",
  ]);
  await expect(panel.getByText(/^(MP3|WAV) File$/)).toBeVisible();
  await expect(panel.getByText("44100 Hz")).toBeVisible();
  await expect(panel.getByText(/^\d+ kbps$/)).toBeVisible();
  await expect(panel.getByText(/^\/Volumes\/MUSIC\//)).toBeVisible();
});

test("artwork hue only tints the empty-record placeholder", async ({ page }) => {
  const panel = await openPanel(page);
  const summarySleeve = panel.locator('[style*="--hue"]').first();
  const summaryRecord = summarySleeve.locator("svg");

  // Applying the filter to the sleeve itself also hue-rotates real artwork.
  // Only the fallback record should inherit the per-track tint.
  await expect(summaryRecord).toBeVisible();
  expect(await summarySleeve.evaluate((element) => getComputedStyle(element).filter)).toBe("none");
  expect(await summaryRecord.evaluate((element) => getComputedStyle(element).filter)).not.toBe("none");

  await panel.getByRole("tab", { name: "Artwork" }).click();
  const artworkWell = panel.locator('[style*="--hue"]').first();
  const artworkRecord = artworkWell.locator("svg");
  await expect(artworkRecord).toBeVisible();
  expect(await artworkWell.evaluate((element) => getComputedStyle(element).filter)).toBe("none");
  expect(await artworkRecord.evaluate((element) => getComputedStyle(element).filter)).not.toBe("none");
});

test("Info edits a title, and the row in the table follows", async ({ page }) => {
  const panel = await openPanel(page, "/?writable");
  await panel.getByRole("tab", { name: "Info" }).click();

  const field = panel.getByRole("textbox", { name: "Track Title" });
  await expect(field).toBeEnabled();
  await field.fill("Renamed For The Test (Extended Mix)");
  await field.press("Enter");

  await expect(page.getByRole("contentinfo")).toContainText("Track Title saved");
  await expect(page.locator('[role="gridcell"][data-col="title"]').nth(3))
    .toHaveText("Renamed For The Test (Extended Mix)");
  // And the Summary tab reads the new record.
  await panel.getByRole("tab", { name: "Summary" }).click();
  await expect(panel).toContainText("Renamed For The Test (Extended Mix)");
});

test("Info refuses a year that is not a number before the round trip", async ({ page }) => {
  const panel = await openPanel(page, "/?writable");
  await panel.getByRole("tab", { name: "Info" }).click();

  const year = panel.getByRole("textbox", { name: "Year" });
  const before = await year.inputValue();
  await year.fill("abc");
  await year.press("Enter");
  await expect(year).toHaveValue(before);
  await expect(page.getByRole("contentinfo")).not.toContainText("Year saved");

  await year.fill("2019");
  await year.press("Enter");
  await expect(page.getByRole("contentinfo")).toContainText("Year saved");
  await expect(year).toHaveValue("2019");
});

test("Escape puts a field back", async ({ page }) => {
  const panel = await openPanel(page, "/?writable");
  await panel.getByRole("tab", { name: "Info" }).click();
  const artist = panel.getByRole("textbox", { name: "Artist", exact: true });
  const before = await artist.inputValue();
  await artist.fill("changed my mind");
  await artist.press("Escape");
  await expect(artist).toHaveValue(before);
  await expect(page.getByRole("contentinfo")).not.toContainText("Artist saved");
});

test("the fields the writer will not take are read-only and say why", async ({ page }) => {
  await enableTooltips(page);
  const panel = await openPanel(page, "/?writable");
  await panel.getByRole("tab", { name: "Info" }).click();
  for (const name of ["Album Artist", "BPM", "Mix Name", "Message"]) {
    const box = panel.getByRole("textbox", { name, exact: true });
    await expect(box).toHaveAttribute("readonly", "");
    await expect(box).toHaveAttribute("title", /.+/);
  }
  // The two flags are drawn, read, and not toggled.
  const auto = panel.getByRole("checkbox", { name: "Allow to auto load HotCue on CDJ/XDJ" });
  await expect(auto).toBeChecked();
  await auto.click();
  await expect(auto).toBeChecked();
  await expect(panel.getByRole("checkbox", { name: "Publish track information" })).not.toBeChecked();
});

test("read-only mode greys the form and says so", async ({ page }) => {
  // The mock's default is what the real backend reports while rekordbox
  // holds the database.
  const panel = await openPanel(page);
  await panel.getByRole("tab", { name: "Info" }).click();
  await expect(panel.getByRole("status")).toContainText("read-only");
  await expect(panel.getByRole("textbox", { name: "Track Title" })).toBeDisabled();
  await expect(panel.getByRole("combobox", { name: "Key" })).toBeDisabled();
  await expect(panel.getByRole("radio", { name: "3 of 5" })).toBeDisabled();
});

test("the key is chosen from what the library holds", async ({ page }) => {
  const panel = await openPanel(page, "/?writable");
  await panel.getByRole("tab", { name: "Info" }).click();
  const key = panel.getByRole("combobox", { name: "Key" });
  await key.selectOption("Dm");
  await expect(page.getByRole("contentinfo")).toContainText("Key saved");
  await expect(page.locator('[role="gridcell"][data-col="key"]').nth(3)).toHaveText("Dm");
});

// ------------------------------------------------- several tracks (#112)

/**
 * Selects the fourth to sixth rows. rekordbox greys Summary for a multiple
 * selection and moves the panel to Info [OBS: rekordbox 7, Windows 11].
 */
async function selectThree(page: Page) {
  const titles = page.locator('[role="gridcell"][data-col="title"]');
  await titles.nth(3).click();
  await titles.nth(5).click({ modifiers: ["Shift"] });
}

test("several selected tracks grey Summary and move the panel to Info", async ({ page }) => {
  // Writable, so the greyed title is the selection's doing, not read-only's.
  const panel = await openPanel(page, "/?writable");
  const tabs = panel.getByRole("tablist", { name: "Information" });
  await expect(tabs.getByRole("tab", { name: "Summary" })).toHaveAttribute("aria-selected", "true");

  await selectThree(page);
  await expect(tabs.getByRole("tab", { name: "Summary" })).toBeDisabled();
  await expect(tabs.getByRole("tab", { name: "Info" })).toHaveAttribute("aria-selected", "true");
  // The Track Title box is greyed and blank: three titles are not one. The
  // other boxes still take an edit.
  const title = panel.getByRole("textbox", { name: "Track Title" });
  await expect(title).toBeDisabled();
  await expect(title).toHaveValue("");
  await expect(panel.getByRole("textbox", { name: "Artist", exact: true })).toBeEnabled();
  await expect(panel.getByRole("textbox", { name: "Composer" })).toBeEnabled();

  // One track again: Summary comes back, and the panel stays on Info.
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).click();
  await expect(tabs.getByRole("tab", { name: "Summary" })).toBeEnabled();
  await expect(tabs.getByRole("tab", { name: "Info" })).toHaveAttribute("aria-selected", "true");
  await expect(title).toBeEnabled();
});

test("a multiple selection never shows the loaded track's record", async ({ page }) => {
  // The report: with a track on the deck, selecting several others showed
  // the deck's track in the panel.
  const panel = await openPanel(page);
  const titles = page.locator('[role="gridcell"][data-col="title"]');
  await titles.nth(1).dblclick();
  const loaded = await titles.nth(1).innerText();
  await selectThree(page);
  await panel.getByRole("tab", { name: "Info" }).click();
  await expect(panel.getByRole("textbox", { name: "Track Title" })).toHaveValue("");
  await expect(panel.getByRole("textbox", { name: "Track Title" })).not.toHaveValue(loaded);
  // The record shown is the selection's: its first track's file type is not
  // printed anywhere, because Summary is not shown.
  await expect(panel.getByRole("tabpanel")).toHaveAttribute("id", "info-panel-info");
});

test("an edit with several tracks selected goes to every one of them", async ({ page }) => {
  const panel = await openPanel(page, "/?writable");
  await selectThree(page);
  const artist = panel.getByRole("textbox", { name: "Artist", exact: true });
  await artist.fill("Same Artist For All");
  await artist.press("Enter");
  await expect(page.getByRole("contentinfo")).toContainText("Artist saved");
  const artists = page.locator('[role="gridcell"][data-col="artist"]');
  for (const n of [3, 4, 5]) await expect(artists.nth(n)).toHaveText("Same Artist For All");
  await expect(artists.nth(2)).not.toHaveText("Same Artist For All");
  await expect(artists.nth(6)).not.toHaveText("Same Artist For All");
  // The shared value now shows in the box.
  await expect(artist).toHaveValue("Same Artist For All");

  await panel.getByRole("combobox", { name: "Key" }).selectOption("Dm");
  await expect(page.getByRole("contentinfo")).toContainText("Key saved");
  const keys = page.locator('[role="gridcell"][data-col="key"]');
  for (const n of [3, 4, 5]) await expect(keys.nth(n)).toHaveText("Dm");

  // One undo takes the whole edit back from all three.
  await page.evaluate(() => {
    (document.activeElement as HTMLElement | null)?.blur();
    (window as unknown as { __menu: (id: string) => void }).__menu("undo");
  });
  for (const n of [3, 4, 5]) await expect(keys.nth(n)).not.toHaveText("Dm");
  for (const n of [3, 4, 5]) await expect(artists.nth(n)).toHaveText("Same Artist For All");
});
