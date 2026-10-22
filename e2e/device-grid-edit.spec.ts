import { expect, test, type Page } from "@playwright/test";

/**
 * The GRID panel over a track loaded from a stick's Devices tree (#660).
 *
 * rekordbox edits a device track's beat grid and saves it to the device
 * (`BeatGridAdjustment::saveBeatPosition` sends it to the device's library
 * and analysis files). The mock's TEST stick holds analysed tracks, so the
 * panel is live for them; the lock stays off, since rbxport keeps Analysis
 * Lock for the collection.
 */

const player = (page: Page) => page.getByRole("region", { name: "Preview player" });
const gridPanel = (page: Page) => player(page).getByRole("region", { name: "Beat grid" });
const button = (page: Page, label: string) => gridPanel(page).getByRole("button", { name: label, exact: true });
const bpmField = (page: Page) => page.getByTestId("grid-bpm");
const rows = (page: Page) => page.getByRole("row").filter({ has: page.getByRole("gridcell") });

async function loadFromStick(page: Page) {
  await page.addInitScript(() => localStorage.setItem("rbl.preferences", JSON.stringify({ view: { tooltips: true } })));
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.getByRole("tablist", { name: "Library sources" }).getByRole("tab", { name: "Devices" }).click();
  await page.getByRole("button", { name: "Expand TEST" }).dispatchEvent("mousedown");
  await page.locator('[role="treeitem"][data-kind="deviceAllTracks"]').first().click();
  await expect(rows(page)).toHaveCount(6);
  await rows(page).first().locator('[role="gridcell"][data-col="title"]').dblclick();
  await expect(player(page).getByRole("button", { name: "Play", exact: true })).toBeEnabled();
  await player(page).getByRole("tab", { name: "GRID" }).click();
  await expect(gridPanel(page)).toBeVisible();
}

test("a track from the Devices tree has its beat grid edited, undone and redone", async ({ page }) => {
  await loadFromStick(page);
  await expect(bpmField(page)).toHaveValue("128.00");
  await expect(button(page, "Double the tempo")).toBeEnabled();

  await button(page, "Double the tempo").click();
  await expect(bpmField(page)).toHaveValue("256.00");
  await button(page, "Undo the last grid edit").click();
  await expect(bpmField(page)).toHaveValue("128.00");
  await button(page, "Redo the last grid edit").click();
  await expect(bpmField(page)).toHaveValue("256.00");
});

test("a track from the Devices tree offers no Analysis Lock", async ({ page }) => {
  await loadFromStick(page);
  await expect(button(page, "Lock the grid")).toBeDisabled();
});
