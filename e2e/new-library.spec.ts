/**
 * A machine with no library to open asks what to do and nothing else: no
 * error in the status bar, no rows left over from a previous session.
 *
 * With nothing configured it offers libraries on connected drives, a
 * master.db picked by hand, or a new library. With a library configured on a
 * drive that is not connected it asks for the drive, as rekordbox does, and
 * never offers to make a library there.
 */
import { expect, test } from "@playwright/test";

const rows = '[role="row"] [data-col="title"]';

test("with no library the window asks to create one, and creating it loads the library", async ({ page }) => {
  await page.goto("/?nolibrary");

  const dialog = page.getByRole("dialog", { name: "No rekordbox Library" });
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("RBXport could not find a rekordbox library on this computer.");
  await expect(dialog).toContainText("/Pioneer/rekordbox/master.db");
  await expect(dialog).toContainText("Preferences > Advanced > Database management");
  await expect(page.locator(rows)).toHaveCount(0);

  // There is no way around it: Escape leaves it open.
  await page.keyboard.press("Escape");
  await expect(dialog).toBeVisible();

  await dialog.getByRole("button", { name: "Create New" }).click();
  await expect(dialog).toBeHidden();
  await expect.poll(async () => page.locator(rows).count(), { timeout: 10_000 }).toBeGreaterThan(0);
});

test("a library on a connected drive is listed first and opens", async ({ page }) => {
  await page.goto("/?nolibrary&drivelibrary");

  const dialog = page.getByRole("dialog", { name: "No rekordbox Library" });
  const drives = dialog.getByRole("region", { name: "Libraries on connected drives" });
  await expect(drives).toContainText("/Volumes/DJ SSD/PIONEER/Master/master.db");
  await drives.getByRole("button", { name: "Open the library on DJ SSD" }).click();
  await expect(dialog).toBeHidden();
  await expect.poll(async () => page.locator(rows).count(), { timeout: 10_000 }).toBeGreaterThan(0);
});

test("an existing library can be chosen instead of creating a local one", async ({ page }) => {
  await page.goto("/?nolibrary");

  const dialog = page.getByRole("dialog", { name: "No rekordbox Library" });
  await expect(dialog.getByRole("region", { name: "Libraries on connected drives" })).toHaveCount(0);
  await dialog.getByRole("button", { name: "Choose master.db…" }).click();
  await expect(dialog).toBeHidden();
  await expect.poll(async () => page.locator(rows).count(), { timeout: 10_000 }).toBeGreaterThan(0);
});

test("a library on a drive that is not connected asks for the drive and never creates there", async ({ page }) => {
  await page.goto("/?libraryunavailable");

  const dialog = page.getByRole("dialog", { name: "Cannot Find Library" });
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("rekordbox is set to use a library that cannot be found.");
  await expect(dialog).toContainText("/Volumes/DJ SSD/PIONEER/Master/master.db");
  await expect(dialog.getByRole("button", { name: "Create New" })).toHaveCount(0);
  await expect(page.locator(rows)).toHaveCount(0);

  await dialog.getByRole("button", { name: "Try Again" }).click();
  await expect(dialog.getByRole("alert")).toContainText("still not there");
  await expect(dialog).toBeVisible();

  await dialog.getByRole("button", { name: "Create in Default Location" }).click();
  await expect(dialog).toBeHidden();
  await expect.poll(async () => page.locator(rows).count(), { timeout: 10_000 }).toBeGreaterThan(0);
});

test("connecting the drive and trying again opens the library", async ({ page }) => {
  await page.goto("/?libraryunavailable&drivelibrary");

  const dialog = page.getByRole("dialog", { name: "Cannot Find Library" });
  await dialog.getByRole("button", { name: "Try Again" }).click();
  await expect(dialog).toBeHidden();
  await expect.poll(async () => page.locator(rows).count(), { timeout: 10_000 }).toBeGreaterThan(0);
});

test("a library that is there never asks", async ({ page }) => {
  await page.goto("/");
  await expect.poll(async () => page.locator(rows).count(), { timeout: 10_000 }).toBeGreaterThan(0);
  await expect(page.getByRole("dialog", { name: "No rekordbox Library" })).toHaveCount(0);
  await expect(page.getByRole("dialog", { name: "Cannot Find Library" })).toHaveCount(0);
});
