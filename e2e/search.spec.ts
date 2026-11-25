import { expect, test } from "@playwright/test";
import { TRACK_SEARCH_OPTIONS } from "../src/lib/search";

test("track search offers the reference scopes and searches the selected field", async ({ page }) => {
  await page.goto("/");
  // Past the remount once the session is restored, which would drop the text.
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.getByRole("treeitem", { name: "All Tracks", exact: true }).click();
  const input = page.getByRole("searchbox", { name: "Search within this track list", exact: true }).first();
  await input.fill("ARTBAT");
  await expect(page.getByTestId("browser-title")).not.toContainText("(0 Tracks)");
  await page.getByRole("button", { name: "Track search scope", exact: true }).first().click();
  const menu = page.getByRole("menu", { name: "Track search scope" });
  await expect(menu.getByRole("menuitemradio")).toHaveText(TRACK_SEARCH_OPTIONS.map(option => `${option.value === "all" ? "✓" : ""}${option.label}`));
  await menu.getByRole("menuitemradio", { name: "Artist", exact: true }).click();
  await expect(input).toHaveValue("ARTBAT");
  await expect(page.getByTestId("browser-title")).not.toContainText("(0 Tracks)");
  await page.getByRole("button", { name: "Track search scope", exact: true }).first().click();
  await menu.getByRole("menuitemradio", { name: "Track Title", exact: true }).click();
  await expect(page.getByTestId("browser-title")).toContainText("(0 Tracks)");
  await page.getByRole("button", { name: "Track search scope", exact: true }).first().click();
  await expect(menu.getByRole("menuitemradio", { name: "Track Title", exact: true })).toHaveAttribute("aria-checked", "true");
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
  await expect(input).toBeFocused();
  await expect(input).toHaveValue("ARTBAT");
});

test("tree search scopes to a kind of row and supports keyboard selection", async ({ page }) => {
  // Intelligent playlist was a fourth option here until the feature that
  // creates and edits them was hidden as unfinished (851ed28, 2026-09-20).
  await page.goto("/");
  await page.getByRole("button", { name: "Tree search scope", exact: true }).click();
  const menu = page.getByRole("menu", { name: "Tree search scope" });
  await expect(menu.getByRole("menuitemradio")).toHaveText(["✓All", "Playlist", "Folder"]);
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  const input = page.getByRole("searchbox", { name: "Search library tree", exact: true });
  await expect(input).toBeFocused();
  await input.fill("a");
  await page.getByRole("button", { name: "Tree search scope", exact: true }).click();
  await expect(menu.getByRole("menuitemradio", { name: "Folder", exact: true })).toHaveAttribute("aria-checked", "true");
  const browserTitle = page.getByTestId("browser-title");
  await expect(browserTitle.getByRole("combobox", { name: "Playlists", exact: true })).toBeDisabled();
  await browserTitle.click();
  await expect(menu).toHaveCount(0);
});
