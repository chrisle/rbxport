import { expect, test } from "@playwright/test";

test("Preferences can select the only available library after startup could not load one", async ({ page }) => {
  const masterDb = "/Volumes/RECOVERY/PIONEER/Master/master.db";
  // Keep this a disposable unloaded-backend fixture. The real chooser and
  // Preferences window run unchanged; no installed settings or DB are used.
  await page.route("**/src/main.tsx", async (route) => {
    const response = await route.fetch();
    const fixture = `
      import { getBackend as fixtureBackend } from "/src/ipc/client.ts";
      const backend = await fixtureBackend();
      backend.databaseDrives = async () => [{ name: "RECOVERY", masterDb: ${JSON.stringify(masterDb)}, current: false }];
      backend.switchLibrary = async (path) => { window.__recoverySwitches = [...(window.__recoverySwitches ?? []), path]; };
    `;
    await route.fulfill({ response, body: fixture + await response.text() });
  });
  await page.goto("/?nolibrary&writable#preferences/advanced");
  const preferences = page.getByRole("dialog", { name: "Preferences" });
  await expect(preferences).toBeVisible();
  await preferences.getByRole("tab", { name: "Database", exact: true }).click();
  const drive = preferences.getByRole("combobox", { name: "Select a drive" });
  await expect(drive).toBeEnabled();
  await expect(drive).toHaveValue("");
  await expect(drive.locator("option")).toHaveText(["Select a drive", "RECOVERY"]);
  await drive.selectOption({ label: "RECOVERY" });
  await expect(drive).toHaveValue(masterDb);
  await expect(drive).toBeDisabled();
  expect(await page.evaluate(() => (window as unknown as { __recoverySwitches: string[] }).__recoverySwitches)).toEqual([masterDb]);
});
