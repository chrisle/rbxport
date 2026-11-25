import { expect, test } from "@playwright/test";

for (const mac of [true, false]) {
  test(`Performance 1 Deck B keys survive menu conflicts (${mac ? "macOS assignments" : "Windows fallback"})`, async ({ page }) => {
    await page.addInitScript(mac => {
      Object.defineProperty(navigator, "platform", { value: mac ? "MacIntel" : "Win32", configurable: true });
      Object.defineProperty(navigator, "userAgent", { value: mac ? "Macintosh" : "Windows NT 10.0", configurable: true });
      localStorage.setItem("rbl.preferences", JSON.stringify({ advanced: { protectLibrary: false }, keyboard: { preset: "performance1", overrides: {} } }));
    }, mac);
    await page.route("**/src/main.tsx*", async route => {
      const response = await route.fetch();
      await route.fulfill({ response, body: `
        import { getBackend as menuFixture } from "/src/ipc/client.ts";
        const menuBackend = await menuFixture();
        window.__menuPresets = [];
        menuBackend.setMenuLabels = async (_labels, performance1) => { window.__menuPresets.push(performance1); };
      ` + await response.text() });
    });
    await page.goto("/");
    await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
    await page.getByRole("button", { name: "Layout", exact: true }).click();
    await page.getByRole("menuitemradio", { name: "2 PLAYER", exact: true }).click();
    await page.locator('[role="gridcell"][data-col="title"]').nth(3).click();
    await page.keyboard.press("Shift+Enter");
    const b = page.getByRole("region", { name: "Preview player B", exact: true });
    await expect(page.getByRole("group", { name: "Deck B transport", exact: true }).getByRole("button", { name: "Play", exact: true })).toBeEnabled();
    await b.getByTestId("player-title").click();
    const modifier = mac ? "Meta" : "Control";
    for (const [key, beats] of [["7", "1"], ["8", "2"], ["9", "4"], ["0", "8"]]) {
      await page.keyboard.press(`${modifier}+${key}`);
      await expect(b).toBeVisible();
      await expect(b.getByRole("button", { name: "Exit loop", exact: true })).toHaveText(beats!);
    }
    await b.getByRole("button", { name: "Exit loop", exact: true }).click();
    await page.keyboard.press(`${modifier}+p`);
    await expect(b.locator('[class*="elapsed"]')).not.toHaveText("00:00.0");
    await page.keyboard.press(`${modifier}+o`);
    await expect(b.locator('[class*="elapsed"]')).toHaveText("00:00.0");
    await expect.poll(() => page.evaluate(() => (window as unknown as { __menuPresets: boolean[] }).__menuPresets.at(-1))).toBe(true);
    await page.evaluate(() => {
      const preferences = JSON.parse(localStorage.getItem("rbl.preferences")!);
      preferences.keyboard.preset = "export";
      localStorage.setItem("rbl.preferences", JSON.stringify(preferences));
      window.dispatchEvent(new StorageEvent("storage", { key: "rbl.preferences" }));
    });
    await expect.poll(() => page.evaluate(() => (window as unknown as { __menuPresets: boolean[] }).__menuPresets.at(-1))).toBe(false);
    if (!mac) {
      await page.keyboard.press("Control+7");
      await expect(b).toHaveCount(0);
    }
  });
}
