import { expect, type Page } from "@playwright/test";

/**
 * Tooltips default off; some assertions need the real text a hover shows.
 * Must run before the page navigates — it seeds the preference the same way
 * `grid-edit.spec.ts` does, rather than clicking through Settings each time.
 */
export async function enableTooltips(page: Page): Promise<void> {
  await page.addInitScript(() => localStorage.setItem("rbl.preferences", JSON.stringify({ view: { tooltips: true } })));
}

/** The main playlist chooser also contains other destinations as options. */
export async function expectPlaylistCaption(page: Page, caption: string | RegExp): Promise<void> {
  const chooser = page.getByTestId("browser-title").getByRole("combobox", { name: "Playlists", exact: true });
  await expect(chooser).toBeVisible();
  await expect(chooser.locator("option:checked")).toHaveText(caption);
}
