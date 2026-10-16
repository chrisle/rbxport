import { expect, test } from "@playwright/test";

/**
 * The Log window (#484): the status bar's message is shown for a moment, and
 * clicking it opens the log, where the message stays. In a browser the log is
 * drawn over the main window, as the Report bug and Sync Manager windows are.
 */
test("clicking the status bar's error opens the log, which keeps the message", async ({ page }) => {
  // Read-only, so a double-click loads rather than edits; row 1 is missing.
  await page.goto("/?missing=7");
  await page.getByRole("treeitem", { name: /All Tracks/ }).click();
  const rows = page.getByRole("row").filter({ has: page.locator('[data-col="title"]') });
  await rows.nth(1).locator('[data-col="title"]').dblclick();
  const error = page.getByRole("contentinfo").getByRole("alert");
  await expect(error).toHaveText("Load error. The file could not be found.");

  await error.getByRole("button").click();
  const log = page.getByRole("dialog", { name: "Log", exact: true });
  const lines = log.getByRole("log", { name: "Log lines" });
  await expect(lines).toContainText("WARN rbxport::status: Load error. The file could not be found.");

  // Only errors and warnings, then only lines with the text in them.
  await log.getByRole("combobox").selectOption("warn");
  await expect(lines).not.toContainText("logging to the browser preview");
  await log.getByRole("searchbox", { name: "Filter the log" }).fill("could not be found");
  await expect(log).toContainText("1 of");
  await log.getByRole("searchbox", { name: "Filter the log" }).fill("no such text");
  await expect(lines).toContainText("No lines match.");

  await log.getByRole("button", { name: "Open log folder" }).click();
  await expect(log.getByRole("status")).toHaveText("Opening the log folder requires the desktop app.");
  await log.getByRole("button", { name: "Close" }).click();
  await expect(log).toHaveCount(0);
});
