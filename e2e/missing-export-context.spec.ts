import { expect, test } from "@playwright/test";

test("missing export tracks retain distinct IDs and literal selected playlist labels in German", async ({ page }) => {
  await page.route("**/src/main.tsx*", async route => {
    const response = await route.fetch();
    const fixture = `
      import { getBackend as missingContextBackend } from "/src/ipc/client.ts";
      const backend = await missingContextBackend();
      window.contextSyncs = 0;
      backend.validateExportFiles = async () => [
        { id: "101", title: "Play", path: "/missing/same.wav", playlists: [{ id: "1", path: "Play" }, { id: "2", path: "Folder / Play" }] },
        { id: "102", title: "Play", path: "/missing/same.wav", playlists: [{ id: "3", path: "Play" }] },
      ];
      const contextSync = backend.syncDevices.bind(backend);
      backend.syncDevices = (...args) => { window.contextSyncs++; return contextSync(...args); };
    `;
    await route.fulfill({ response, body: fixture + await response.text() });
  });
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.evaluate(() => {
    const held = JSON.parse(localStorage.getItem("rbl.preferences") ?? "{}");
    localStorage.setItem("rbl.preferences", JSON.stringify({ ...held, view: { ...held.view, locale: "de" } }));
    window.dispatchEvent(new Event("rbl-preferences"));
  });
  // The catalogue must be loaded before testing data that itself matches a UI label.
  await expect(page.getByRole("columnheader", { name: /^Trackname/ })).toBeVisible();
  await page.getByRole("button", { name: /^(Sync Manager|Sync-Manager)$/ }).click();
  const manager = page.getByRole("dialog", { name: /^(Sync Manager|Sync-Manager)$/ });
  await manager.getByRole("checkbox", { name: "Melodic Vox", exact: true }).check();
  await manager.getByRole("checkbox", { name: "DJ STICK", exact: true }).check();
  await manager.getByRole("button", { name: "Synchronisation", exact: true }).click();
  const missing = page.locator('dialog[aria-labelledby="missing-files-title"]');
  await expect(missing).toBeVisible();
  await expect(missing.getByText("Folder / Play", { exact: true })).toBeVisible();
  await expect(missing.locator('[data-track-id]')).toHaveCount(2);
  await expect(missing.locator('[data-track-id="101"] strong')).toHaveText("Play");
  await expect(missing.locator('[data-track-id="102"] strong')).toHaveText("Play");
  await expect(missing.locator('[data-track-id="101"] [data-playlist-id]')).toHaveText(["Play", "Folder / Play"]);
  await expect(missing.locator('[data-track-id="102"] [data-playlist-id]')).toHaveText(["Play"]);
  expect(await page.evaluate(() => (window as unknown as { contextSyncs: number }).contextSyncs)).toBe(0);
  await missing.locator("button").first().click();
  await expect(missing).toHaveCount(0);
  expect(await page.evaluate(() => (window as unknown as { contextSyncs: number }).contextSyncs)).toBe(0);
  await manager.getByRole("button", { name: "Synchronisation", exact: true }).click();
  await expect(missing).toBeVisible();
  await missing.locator("button").last().click();
  await expect(missing).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => (window as unknown as { contextSyncs: number }).contextSyncs)).toBe(1);
});
