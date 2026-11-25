import { expect, test, type Page } from "@playwright/test";

async function fixture(page: Page) {
  await page.route("**/src/main.tsx*", async route => {
    const response = await route.fetch();
    const setup = `
      import { getBackend as repairBackend } from "/src/ipc/client.ts";
      const backend = await repairBackend();
      window.repairCalls = { sync: 0, writes: 0, requests: 0, acknowledgments: 0, sawOpen: false };
      window.repairUnavailable = true;
      window.repairExpired = false;
      backend.validateExportFiles = async () => [{ id: "101", title: "Repair fixture", path: "/missing/fixture.wav", playlists: [{ id: "1", path: "Selected playlist" }] }];
      backend.missingTracks = async () => ({ total: 1, tracks: [{ id: "101", title: "Repair fixture", artist: "Generated", album: "", path: "/missing/fixture.wav" }] });
      backend.openMissingFiles = async () => { window.repairCalls.requests++; if (window.repairUnavailable) throw new Error("fixture owner unavailable"); return false; };
      backend.onMissingFilesRequested = listener => { window.requestMissingFiles = listener; return () => { window.requestMissingFiles = null; }; };
      backend.missingFilesReady = async () => { window.repairCalls.acknowledgments++; window.repairCalls.sawOpen = document.querySelector('dialog[aria-labelledby="missing-file-manager-title"]')?.open === true; return !window.repairExpired; };
      const repairOriginalSync = backend.syncDevices.bind(backend);
      backend.syncDevices = (...args) => { window.repairCalls.sync++; return repairOriginalSync(...args); };
      for (const key of ["autoRelocate", "relocateTrack", "chooseRelocateFile"]) { const original = backend[key].bind(backend); backend[key] = (...args) => { window.repairCalls.writes++; return original(...args); }; }
    `;
    await route.fulfill({ response, body: setup + await response.text() });
  });
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
}

for (const protect of [false, true]) test(`missing export hands off to the real main manager with protection=${protect}`, async ({ page }) => {
  let release!: () => void;
  const held = new Promise<void>(resolve => { release = resolve; });
  let requested = false;
  await page.route("**/src/views/library/MissingFileManager.tsx*", async route => {
    const response = await route.fetch(); requested = true;
    await held; await route.fulfill({ response });
  });
  await fixture(page);
  await page.getByRole("button", { name: "Sync Manager", exact: true }).click();
  const manager = page.getByRole("dialog", { name: "Sync Manager", exact: true });
  await manager.getByRole("checkbox", { name: "Melodic Vox", exact: true }).check();
  await manager.getByRole("checkbox", { name: "DJ STICK", exact: true }).check();
  await manager.getByRole("button", { name: "SYNC", exact: true }).click();
  const missing = page.locator('dialog[aria-labelledby="missing-files-title"]');
  await expect(missing.getByText("Repair fixture", { exact: true })).toBeVisible();
  await missing.getByRole("button", { name: "Display All Missing Files", exact: true }).click();
  await expect(missing.getByRole("alert")).toContainText("Could not open Display All Missing Files");
  await expect(missing.getByRole("button", { name: "Cancel", exact: true })).toBeEnabled();
  await expect(missing.getByRole("button", { name: "Keep available copies and sync", exact: true })).toBeEnabled();
  await page.evaluate(protectedLibrary => {
    const state = JSON.parse(localStorage.getItem("rbl.preferences") ?? "{}");
    localStorage.setItem("rbl.preferences", JSON.stringify({ ...state, advanced: { ...state.advanced, protectLibrary: protectedLibrary } }));
    window.dispatchEvent(new StorageEvent("storage", { key: "rbl.preferences" }));
    (window as unknown as { repairUnavailable: boolean }).repairUnavailable = false;
  }, protect);
  await missing.getByRole("button", { name: "Display All Missing Files", exact: true }).click();
  await expect.poll(() => requested).toBe(true);
  await expect(missing).toBeVisible();
  await expect(missing.getByRole("button", { name: "Keep available copies and sync", exact: true })).toBeDisabled();
  release();
  const repair = page.getByRole("dialog", { name: "Missing File Manager", exact: true });
  await expect(repair).toBeVisible();
  await expect(repair.getByRole("gridcell", { name: "Repair fixture", exact: true })).toBeVisible();
  await expect(missing).toHaveCount(0);
  await expect(manager).toHaveCount(0);
  for (const name of ["Relocate", "Auto Relocate", "Delete"]) {
    const button = repair.getByRole("button", { name, exact: true });
    if (protect) await expect(button).toBeDisabled(); else await expect(button).toBeEnabled();
  }
  expect(await page.evaluate(() => (window as unknown as { repairCalls: { sync: number; writes: number } }).repairCalls)).toMatchObject({ sync: 0, writes: 0 });
  await repair.getByRole("button", { name: "OK", exact: true }).click();
  await expect(repair).toHaveCount(0);
});

test("a late native request is retired after the actual main dialog loads", async ({ page }) => {
  let release!: () => void;
  const held = new Promise<void>(resolve => { release = resolve; });
  let requested = false;
  await page.route("**/src/views/library/MissingFileManager.tsx*", async route => {
    const response = await route.fetch(); requested = true;
    await held; await route.fulfill({ response });
  });
  await fixture(page);
  await page.evaluate(() => (window as unknown as { requestMissingFiles: (id: string) => void }).requestMissingFiles("42"));
  await expect.poll(() => requested).toBe(true);
  expect(await page.evaluate(() => (window as unknown as { repairCalls: { acknowledgments: number } }).repairCalls.acknowledgments)).toBe(0);
  await page.evaluate(() => { (window as unknown as { repairExpired: boolean }).repairExpired = true; });
  release();
  await expect.poll(() => page.evaluate(() => (window as unknown as { repairCalls: { acknowledgments: number } }).repairCalls.acknowledgments)).toBe(1);
  expect(await page.evaluate(() => (window as unknown as { repairCalls: { sawOpen: boolean } }).repairCalls.sawOpen)).toBe(true);
  await expect(page.getByRole("dialog", { name: "Missing File Manager", exact: true })).toHaveCount(0);
  expect(await page.evaluate(() => (window as unknown as { repairCalls: { sync: number; writes: number } }).repairCalls)).toMatchObject({ sync: 0, writes: 0 });
});

test("a failed repair module leaves the actual missing prompt usable", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.route("**/src/views/library/MissingFileManager.tsx*", route => route.abort());
  await fixture(page);
  await page.getByRole("button", { name: "Sync Manager", exact: true }).click();
  const manager = page.getByRole("dialog", { name: "Sync Manager", exact: true });
  await manager.getByRole("checkbox", { name: "Melodic Vox", exact: true }).check();
  await manager.getByRole("checkbox", { name: "DJ STICK", exact: true }).check();
  await manager.getByRole("button", { name: "SYNC", exact: true }).click();
  const missing = page.locator('dialog[aria-labelledby="missing-files-title"]');
  await page.evaluate(() => { (window as unknown as { repairUnavailable: boolean }).repairUnavailable = false; });
  await missing.getByRole("button", { name: "Display All Missing Files", exact: true }).click();
  await expect(missing.getByRole("alert")).toContainText("Could not open Display All Missing Files");
  await expect(missing.getByRole("button", { name: "Keep available copies and sync", exact: true })).toBeEnabled();
  await expect(page.getByTestId("browser-title")).toBeVisible();
  expect(errors).toEqual([]);
  expect(await page.evaluate(() => (window as unknown as { repairCalls: { sync: number; writes: number } }).repairCalls)).toMatchObject({ sync: 0, writes: 0 });
  await missing.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(missing).toHaveCount(0);
});
