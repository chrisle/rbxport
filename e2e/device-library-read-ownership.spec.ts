import { expect, test } from "@playwright/test";

for (const result of ["success", "error"] as const) {
  test(`an older device read ${result} cannot replace a successful playlist rename`, async ({ page }) => {
    await page.goto("/?writable=1");
    await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
    await page.getByRole("tablist", { name: "Library sources" }).getByRole("tab", { name: "Devices" }).click();
    await page.getByRole("button", { name: "Expand TEST" }).click();
    await expect(page.locator('[data-kind="deviceLibrary"]')).toHaveCount(2);
    await expect(page.getByRole("tablist", { name: "Device settings" })).toBeVisible();
    await page.evaluate(async (outcome) => {
      const modulePath = "/src/ipc/client.ts";
      const backend = await (await import(modulePath) as typeof import("../src/ipc/client")).getBackend();
      const read = backend.deviceLibraries.bind(backend);
      const probe = { armed: true, held: false, settled: false, release: () => {} };
      backend.deviceLibraries = async (...args) => {
        // The mock returns detached DTOs, just as the native IPC reply does.
        const snapshot = await read(...args);
        if (!probe.armed) return snapshot;
        probe.armed = false;
        probe.held = true;
        return new Promise((resolve, reject) => {
          probe.release = () => {
            probe.settled = true;
            if (outcome === "error") reject(new Error("Obsolete device read failed"));
            else resolve(snapshot);
          };
        });
      };
      Object.assign(window, { deviceReadProbe: probe });
    }, result);
    await page.getByRole("button", { name: "Refresh", exact: true }).click();
    await expect.poll(() => page.evaluate(() => (window as unknown as { deviceReadProbe: { held: boolean } }).deviceReadProbe.held)).toBe(true);
    const original = page.locator('[data-kind="devicePlaylist"]').filter({ hasText: /^\s*NP3-TEST-MP3/ }).first();
    await original.click();
    await original.press("F2");
    const rename = page.getByRole("textbox", { name: "Rename NP3-TEST-MP3" });
    await rename.fill("Current device playlist");
    await rename.press("Enter");
    await expect(page.getByRole("contentinfo")).toContainText("Renamed to Current device playlist.");
    const current = page.locator('[data-kind="devicePlaylist"]').filter({ hasText: /^\s*Current device playlist/ });
    await expect(current).toHaveCount(1);
    await page.evaluate(() => (window as unknown as { deviceReadProbe: { release: () => void } }).deviceReadProbe.release());
    // A subsequent browser task observes the response's queued React update.
    await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
    await expect(current).toHaveCount(1);
    await expect(page.locator('[data-kind="deviceLibrary"]')).toHaveCount(2);
    await expect(page.getByRole("alert").filter({ hasText: "Obsolete device read failed" })).toHaveCount(0);
  });
}
