import { expect, test } from "@playwright/test";

for (const kind of ["folder", "playlist"] as const) {
  test(`direct ${kind} export retains its failure and log access until another export`, async ({ page }) => {
    await page.goto("/?writable=1&tracks=4");
    await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
    await page.evaluate(async () => {
      const url = "/src/ipc/client.ts";
      const { getBackend } = await import(url) as typeof import("../src/ipc/client");
      const backend = await getBackend();
      let exports = 0;
      backend.exportPlaylist = () => ++exports === 1
        ? Promise.reject(new Error("Owned USB export failure: Ànne / &"))
        : new Promise(() => {});
      backend.openLogWindow = () => {
        document.documentElement.dataset.exportLogOpened = "yes";
        return Promise.resolve(true);
      };
    });
    const row = page.locator(`nav[data-browser-side="main"] [role="treeitem"][data-kind="${kind}"]`).first();
    const exportAgain = async () => {
      await row.click({ button: "right" });
      await page.getByRole("menuitem", { name: kind === "folder" ? "Export Folder" : "Export Playlist", exact: true }).hover();
      await page.getByRole("menuitem", { name: "TEST", exact: true }).click();
    };
    await page.clock.install();
    await exportAgain();
    const message = page.getByRole("alert").getByRole("button", { name: "Owned USB export failure: Ànne / &", exact: true });
    await expect(message).toBeVisible();
    await page.clock.fastForward(70_000);
    await expect(message).toBeVisible();
    await message.click();
    await expect(page.locator("html")).toHaveAttribute("data-export-log-opened", "yes");
    await expect(message).toBeVisible();
    await exportAgain();
    await expect(message).toHaveCount(0);
    await expect(page.getByRole("contentinfo")).toContainText("Writing to TEST…");
  });
}
