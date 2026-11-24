import { expect, test, type Page } from "@playwright/test";

type Target = { id: string; name: string };
type Shell = { __menu?: (id: string) => void; __menuTargets?: Record<string, Target[]>; __menuEnabled?: Record<string, boolean> };
const rows = (page: Page) => page.getByRole("row").filter({ has: page.getByRole("gridcell") });
const status = (page: Page) => page.getByRole("contentinfo");
const native = (page: Page, id: string) => page.evaluate(item => (window as unknown as Shell).__menu?.(item), id);
const enabled = (page: Page, id: string) => page.evaluate(item => (window as unknown as Shell).__menuEnabled?.[item] === true, id);
async function target(page: Page, menu: string, name: string) {
  const find = () => page.evaluate(({ menu, name }) => (window as unknown as Shell).__menuTargets?.[menu]?.find(item => item.name.includes(name)), { menu, name });
  await expect.poll(find).toBeTruthy();
  const found = await find();
  if (!found) throw new Error("Native destination did not arrive");
  return found;
}
async function open(page: Page) {
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
}

test("native Add To Playlist uses the track selection and the chosen live destination", async ({ page }) => {
  await open(page);
  await page.getByRole("treeitem").filter({ hasText: "Hardstyle" }).first().click({ button: "right" });
  await page.getByRole("menu", { name: "Playlist", exact: true }).getByRole("menuitem", { name: "Create New Playlist", exact: true }).click();
  await expect(status(page)).toContainText("Created New playlist.");
  const row = rows(page).nth(1);
  const title = (await row.locator('[data-col="title"]').innerText()).trim();
  await row.click();
  const list = await target(page, "add-to-playlist", "New playlist");
  await expect.poll(() => enabled(page, "add-to-playlist")).toBe(true);
  await native(page, `add-to-playlist:${list.id}`);
  await expect(status(page)).toContainText("Added 1 track to New playlist.");
  await page.getByRole("treeitem").filter({ hasText: "New playlist" }).first().click();
  await expect(rows(page)).toHaveCount(1);
  await expect(rows(page).first().locator('[data-col="title"]')).toContainText(title);
});

test("native Track and Playlist exports use existing device export paths", async ({ page }) => {
  await open(page);
  await rows(page).nth(8).click();
  const stick = await target(page, "export-track", "TEST");
  await expect.poll(() => enabled(page, "export-track")).toBe(true);
  await native(page, `export-track:${stick.id}`);
  await expect(status(page)).toContainText("TEST: Updated");
  await page.getByRole("treeitem").filter({ hasText: "Melodic Vox" }).first().click();
  const playlistStick = await target(page, "export-playlist", "TEST");
  await expect.poll(() => enabled(page, "export-playlist")).toBe(true);
  await native(page, `export-playlist:${playlistStick.id}`);
  await expect(status(page)).toContainText("TEST: Updated");
  await expect(status(page)).toContainText("Added 1 playlist");
});

test("a configured Import Playlist key follows the native device selection", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("rbl.preferences", JSON.stringify({
    advanced: { protectLibrary: false },
    keyboard: { preset: "export", overrides: { importPlaylist: { key: "i", altKey: true } } },
  })));
  await open(page);
  const sources = page.getByRole("tablist", { name: "Library sources" });
  await sources.getByRole("tab", { name: "Devices" }).click();
  await page.getByRole("button", { name: "Expand TEST" }).dispatchEvent("mousedown");
  const list = page.locator('[role="treeitem"][data-kind="devicePlaylist"]').filter({ hasText: "Melodic Vox" }).nth(1);
  await list.click();
  await expect.poll(() => enabled(page, "import-playlist")).toBe(true);
  await page.keyboard.press("Alt+i");
  await expect(status(page)).toContainText("Imported Melodic Vox.");
  await sources.getByRole("tab", { name: "Playlists" }).click();
  await expect(page.locator('[role="treeitem"][data-kind="playlist"]').filter({ hasText: "Melodic Vox" })).toHaveCount(2);
});
