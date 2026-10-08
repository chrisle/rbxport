import { expect, test, type Page } from "@playwright/test";

/**
 * The six tabs a selected device opens, against the mock's TEST stick — the
 * rows read off the real one, so the lists here are the capture's lists.
 */
async function openTest(page: Page) {
  await page.goto("/");
  await page.getByRole("tablist", { name: "Library sources" }).getByRole("tab", { name: "Devices" }).click();
  await page.getByRole("treeitem", { name: /TEST/ }).click();
  const panel = page.getByRole("region", { name: "Device TEST" });
  await expect(panel).toBeVisible();
  return panel;
}

test("the strip has the six tabs and they switch", async ({ page }) => {
  const panel = await openTest(page);
  const strip = panel.getByRole("tablist", { name: "Device settings" });
  await expect(strip.getByRole("tab")).toHaveText([
    "General", "Category", "Sort", "Column", "Color", "My Settings",
  ]);
  await expect(strip.getByRole("tab", { name: "General" })).toHaveAttribute("aria-selected", "true");
  await expect(panel.getByLabel("Device Name")).toHaveValue("TEST");

  await strip.getByRole("tab", { name: "Category" }).click();
  await expect(strip.getByRole("tab", { name: "Category" })).toHaveAttribute("aria-selected", "true");
  await expect(panel.getByRole("listbox", { name: "Inactive Categories" })).toBeVisible();

  await strip.getByRole("tab", { name: "Column" }).click();
  await expect(panel.getByRole("combobox", { name: "Default right column" })).toHaveValue("");

  await strip.getByRole("tab", { name: "Color" }).click();
  await expect(panel.getByLabel("Color comment 1")).toHaveValue("Pink");

  await strip.getByRole("tab", { name: "My Settings" }).click();
  await expect(panel.getByText("My Settings are not available")).toBeVisible();
});

test("General shows the stick's display settings and the space table", async ({ page }) => {
  const panel = await openTest(page);
  // The TEST stick's DEVSETTING.DAT: 3Band, CENTER, Half, Classic.
  await expect(panel.getByRole("radio", { name: "3Band" })).toBeChecked();
  await expect(panel.getByRole("radio", { name: "CENTER" })).toBeChecked();
  await expect(panel.getByRole("radio", { name: "Half Waveform" })).toBeChecked();
  await expect(panel.getByRole("radio", { name: "Half Waveform" })).toBeDisabled();
  await expect(panel.getByRole("radio", { name: "Classic" })).toBeChecked();
  // Waveform Divisions is drawn and inert: its byte is not known.
  await expect(panel.getByRole("radio", { name: "TIMESCALE" })).toBeDisabled();

  const rows = panel.getByRole("table").getByRole("row");
  await expect(rows).toHaveText([
    /Total Space1,430\.3 GB/,
    /Available Space201\.9 GB/,
    /Device LibraryDevice Library, OneLibrary/,
  ]);

  // A change is written and survives leaving and coming back.
  await panel.getByRole("radio", { name: "RGB" }).check();
  await page.getByRole("treeitem", { name: /DJ STICK/ }).click();
  await page.getByRole("treeitem", { name: /TEST/ }).click();
  await expect(page.getByRole("region", { name: "Device TEST" }).getByRole("radio", { name: "RGB" })).toBeChecked();
});

test("General sets the two background colours separately", async ({ page }) => {
  const panel = await openTest(page);
  const oneLibrary = panel.getByRole("combobox", { name: "Background Color : OneLibrary" });
  const deviceLibrary = panel.getByRole("combobox", { name: "Background Color : Device Library" });
  await expect(oneLibrary).toBeEnabled();
  await expect(oneLibrary).toHaveValue("0");
  await expect(deviceLibrary).toHaveValue("0");

  await oneLibrary.selectOption({ label: "Purple" });
  await deviceLibrary.selectOption({ label: "Blue" });
  await page.getByRole("treeitem", { name: /DJ STICK/ }).click();
  await page.getByRole("treeitem", { name: /TEST/ }).click();
  const back = page.getByRole("region", { name: "Device TEST" });
  await expect(back.getByRole("combobox", { name: "Background Color : OneLibrary" })).toHaveValue("8");
  await expect(back.getByRole("combobox", { name: "Background Color : Device Library" })).toHaveValue("7");
});

test("Category lists what the stick has, greys the fixed items, and moves them all", async ({ page }) => {
  const panel = await openTest(page);
  await panel.getByRole("tab", { name: "Category" }).click();
  const inactive = panel.getByRole("listbox", { name: "Inactive Categories" });
  const active = panel.getByRole("listbox", { name: "Active Categories", exact: true });

  await expect(inactive.getByRole("option")).toHaveText([
    "BITRATE", "BPM", "COLOR", "FILE NAME", "GENRE", "HOT CUE BANK", "LABEL",
    "ORIGINAL ARTIST", "RATING", "REMIXER", "TIME", "YEAR",
  ]);
  await expect(active.getByRole("option")).toHaveText([
    "ARTIST", "ALBUM", "TRACK", "KEY", "PLAYLIST", "HISTORY", "SEARCH", "MATCHING", "FOLDER",
    "DATE ADDED",
  ]);
  for (const fixed of ["TRACK", "PLAYLIST", "HISTORY", "SEARCH", "FOLDER"]) {
    await expect(active.getByRole("option", { name: fixed, exact: true })).toHaveAttribute("data-fixed", "true");
  }
  // A fixed item can be picked and moved Up / Down, but not taken out.
  await active.getByRole("option", { name: "TRACK", exact: true }).click();
  await expect(active.getByRole("option", { name: "TRACK", exact: true })).toHaveAttribute("aria-selected", "true");
  await expect(panel.getByRole("button", { name: "Remove from Active Categories" })).toBeDisabled();
  await panel.getByRole("button", { name: "Up" }).click();
  await expect(active.getByRole("option").nth(1)).toHaveText("TRACK");
  await panel.getByRole("button", { name: "Down" }).click();
  await expect(active.getByRole("option").nth(2)).toHaveText("TRACK");

  // Right arrow: GENRE goes to the end of the Active list.
  await inactive.getByRole("option", { name: "GENRE" }).click();
  await panel.getByRole("button", { name: "Add to Active Categories" }).click();
  await expect(active.getByRole("option").last()).toHaveText("GENRE");
  await expect(inactive.getByRole("option", { name: "GENRE" })).toHaveCount(0);

  // Up moves it above DATE ADDED; Down puts it back.
  await panel.getByRole("button", { name: "Up" }).click();
  await expect(active.getByRole("option").nth(9)).toHaveText("GENRE");
  await panel.getByRole("button", { name: "Down" }).click();
  await expect(active.getByRole("option").last()).toHaveText("GENRE");

  // Left arrow: back to the Inactive list, in alphabetical order.
  await panel.getByRole("button", { name: "Remove from Active Categories" }).click();
  await expect(inactive.getByRole("option").nth(4)).toHaveText("GENRE");
  await expect(active.getByRole("option")).toHaveCount(10);
});

test("Sort greys DEFAULT and ALPHABET/TRACK NAME and keeps the stick's order", async ({ page }) => {
  const panel = await openTest(page);
  await panel.getByRole("tab", { name: "Sort" }).click();
  const active = panel.getByRole("listbox", { name: "Active Sort Options", exact: true });
  await expect(active.getByRole("option")).toHaveText([
    "DEFAULT", "ALPHABET/TRACK NAME", "ARTIST", "ALBUM", "BPM", "RATING", "KEY",
  ]);
  await expect(active.getByRole("option", { name: "DEFAULT" })).toHaveAttribute("data-fixed", "true");
  await expect(active.getByRole("option", { name: "ALPHABET/TRACK NAME" })).toHaveAttribute("data-fixed", "true");
  await expect(panel.getByRole("listbox", { name: "Inactive Sort Options" }).getByRole("option")).toHaveText([
    "BITRATE", "COLOR", "COMMENTS", "DATE ADDED", "DJ PLAY COUNT", "GENRE", "LABEL",
    "ORIGINAL ARTIST", "REMIXER", "TIME",
  ]);

  // The order is the stick's: moving KEY up puts it before RATING, and the
  // change is still there after switching tabs.
  await active.getByRole("option", { name: "KEY" }).click();
  await panel.getByRole("button", { name: "Up" }).click();
  await panel.getByRole("tab", { name: "General" }).click();
  await panel.getByRole("tab", { name: "Sort" }).click();
  await expect(panel.getByRole("listbox", { name: "Active Sort Options", exact: true }).getByRole("option")).toHaveText([
    "DEFAULT", "ALPHABET/TRACK NAME", "ARTIST", "ALBUM", "BPM", "KEY", "RATING",
  ]);
});

test("Color renames a comment on Enter", async ({ page }) => {
  const panel = await openTest(page);
  await panel.getByRole("tab", { name: "Color" }).click();
  const orange = panel.getByLabel("Color comment 3");
  await expect(orange).toHaveValue("Orange");
  await orange.fill("Peak time");
  await orange.press("Enter");
  await panel.getByRole("tab", { name: "General" }).click();
  await panel.getByRole("tab", { name: "Color" }).click();
  await expect(panel.getByLabel("Color comment 3")).toHaveValue("Peak time");
});

test("opening a stick with no library leaves it untouched until sync", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("tablist", { name: "Library sources" }).getByRole("tab", { name: "Devices" }).click();
  await page.getByRole("treeitem", { name: /DJ STICK/ }).click();
  const panel = page.getByRole("region", { name: "Device DJ STICK" });
  await expect(panel.getByLabel("Device Name")).toBeDisabled();
  await panel.getByRole("tab", { name: "Category" }).click();
  await expect(panel.getByRole("listbox", { name: "Inactive Categories" }).getByRole("option", { name: "GENRE" })).toHaveAttribute("aria-disabled", "true");
});

test("a stick with no library shows the rows but will not edit them", async ({ page }) => {
  // With the switch off nothing is created, and the rows stay read-only.
  await page.addInitScript(() => {
    window.localStorage.setItem("rbl.preferences", JSON.stringify({ djSystem: { createDatabaseFolders: false } }));
  });
  await page.goto("/");
  await page.getByRole("tablist", { name: "Library sources" }).getByRole("tab", { name: "Devices" }).click();
  await page.getByRole("treeitem", { name: /DJ STICK/ }).click();
  const panel = page.getByRole("region", { name: "Device DJ STICK" });
  await expect(panel.getByLabel("Device Name")).toBeDisabled();
  await panel.getByRole("tab", { name: "Category" }).click();
  const inactive = panel.getByRole("listbox", { name: "Inactive Categories" });
  await expect(inactive.getByRole("option", { name: "GENRE" })).toHaveAttribute("aria-disabled", "true");
  await inactive.getByRole("option", { name: "GENRE" }).click({ force: true });
  await expect(panel.getByRole("button", { name: "Add to Active Categories" })).toBeDisabled();
});
