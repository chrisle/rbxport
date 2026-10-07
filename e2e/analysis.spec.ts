import { expect, test } from "@playwright/test";

test("analysis settings gate the selected batch and can be cancelled", async ({ page }) => {
  await page.goto("/?writable=1");
  const rows = page.getByRole("row").filter({ has: page.getByRole("gridcell") });
  await rows.nth(2).click();
  await rows.nth(4).click({ modifiers: ["Shift"] });
  await page.keyboard.press("Shift+Meta+A");
  const dialog = page.getByRole("dialog", { name: "Analysis Setting" });
  await expect(dialog).toContainText("3 tracks selected");
  await expect(page.getByRole("contentinfo")).not.toContainText("Analyzing ");
  await expect(dialog.getByRole("checkbox", { name: "Waveform", exact: true })).toBeChecked();
  await expect(dialog.getByRole("checkbox", { name: "BPM / Grid", exact: true })).toBeChecked();
  await expect(dialog.getByRole("checkbox", { name: "High precision analysis" })).toBeChecked();
  await dialog.getByRole("combobox", { name: "BPM Range" }).selectOption("98-195");
  await dialog.getByRole("checkbox", { name: "BPM / Grid", exact: true }).uncheck();
  await expect(dialog.getByRole("combobox", { name: "BPM Range" })).toBeDisabled();
  await dialog.getByRole("checkbox", { name: "KEY", exact: true }).uncheck();
  await dialog.getByRole("checkbox", { name: "Waveform", exact: true }).uncheck();
  await expect(dialog.getByRole("button", { name: "OK", exact: true })).toBeDisabled();
  await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole("contentinfo")).not.toContainText("Analyzing ");

  await page.keyboard.press("Shift+Meta+A");
  await expect(dialog).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
});

test("analysis settings submit a key-only batch", async ({ page }) => {
  await page.goto("/?writable=1");
  const rows = page.getByRole("row").filter({ has: page.getByRole("gridcell") });
  await rows.nth(2).click();
  await rows.nth(9).click({ modifiers: ["Shift"] });
  await page.keyboard.press("Shift+Meta+A");
  const dialog = page.getByRole("dialog", { name: "Analysis Setting" });
  await dialog.getByRole("checkbox", { name: "BPM / Grid", exact: true }).uncheck();
  await dialog.getByRole("checkbox", { name: "Waveform", exact: true }).uncheck();
  await dialog.getByRole("button", { name: "OK", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole("contentinfo")).toContainText("Analyzing ");
  await expect(page.getByRole("contentinfo").getByRole("button", { name: "Stop" })).toHaveCount(0, { timeout: 15_000 });
});

test("analysis settings submit a waveform-only batch", async ({ page }) => {
  await page.goto("/?writable=1");
  const rows = page.getByRole("row").filter({ has: page.getByRole("gridcell") });
  await rows.nth(3).click();
  await page.keyboard.press("Shift+Meta+A");
  const dialog = page.getByRole("dialog", { name: "Analysis Setting" });
  await dialog.getByRole("checkbox", { name: "BPM / Grid", exact: true }).uncheck();
  await dialog.getByRole("checkbox", { name: "KEY", exact: true }).uncheck();
  await dialog.getByRole("button", { name: "OK", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole("contentinfo")).toContainText("Analyzing ");
  await expect(page.getByRole("contentinfo").getByRole("button", { name: "Stop" })).toHaveCount(0, { timeout: 15_000 });
});
