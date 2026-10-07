/**
 * A machine with no configured rekordbox library can create one, choose an
 * existing master.db, or quit. Nothing else is left over from a previous
 * session.
 */
import { expect, test } from "@playwright/test";

test("with no library the window asks to create one, and creating it loads the library", async ({ page }) => {
  await page.goto("/?nolibrary");

  const dialog = page.getByRole("dialog", { name: "No rekordbox Library" });
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("Create a new one or choose an existing master.db.");
  await expect(dialog).toContainText("/Pioneer/rekordbox/master.db");
  await expect(page.locator('[role="row"] [data-col="title"]')).toHaveCount(0);

  // Escape is not a way out: there is nothing to go back to.
  await page.keyboard.press("Escape");
  await expect(dialog).toBeVisible();

  await dialog.getByRole("button", { name: "Create New" }).click();
  await expect(dialog).toBeHidden();
  await expect
    .poll(async () => page.locator('[role="row"] [data-col="title"]').count(), { timeout: 10_000 })
    .toBeGreaterThan(0);
});

test("an existing library can be chosen instead of creating a local one", async ({ page }) => {
  await page.goto("/?nolibrary");

  const dialog = page.getByRole("dialog", { name: "No rekordbox Library" });
  await dialog.getByRole("button", { name: "Choose Existing…" }).click();
  await expect(dialog).toBeHidden();
  await expect
    .poll(async () => page.locator('[role="row"] [data-col="title"]').count(), { timeout: 10_000 })
    .toBeGreaterThan(0);
});

test("a library that is there never asks", async ({ page }) => {
  await page.goto("/");
  await expect
    .poll(async () => page.locator('[role="row"] [data-col="title"]').count(), { timeout: 10_000 })
    .toBeGreaterThan(0);
  await expect(page.getByRole("dialog", { name: "No rekordbox Library" })).toHaveCount(0);
});
