/**
 * Which library the window works on, the way rekordbox decides it, with no
 * screens rekordbox does not have.
 *
 * A machine with no library at all asks to create one or quit. A library set
 * to a drive that is not connected gets rekordbox's own question, "Cannot
 * find Master Database…", Yes or No, and never a library made on the missing
 * drive. Libraries on other drives are chosen where rekordbox chooses them:
 * Preferences › Advanced › Database › Database management.
 */
import { expect, test, type Page } from "@playwright/test";

const rows = '[role="row"] [data-col="title"]';

test("with no library the window asks to create one, and creating it loads the library", async ({ page }) => {
  await page.goto("/?nolibrary");

  const dialog = page.getByRole("dialog", { name: "No rekordbox Library" });
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("Would you like to create a new database?");
  await expect(dialog).toContainText("/Pioneer/rekordbox/master.db");
  await expect(dialog.getByRole("button")).toHaveText(["Create", "Quit"]);
  await expect(page.locator(rows)).toHaveCount(0);

  // Escape is not a way out: there is nothing to go back to.
  await page.keyboard.press("Escape");
  await expect(dialog).toBeVisible();

  await dialog.getByRole("button", { name: "Create" }).click();
  await expect(dialog).toBeHidden();
  await expect.poll(async () => page.locator(rows).count(), { timeout: 10_000 }).toBeGreaterThan(0);
});

test("a library on a drive that is not connected asks rekordbox's question and never creates there", async ({ page }) => {
  await page.goto("/?libraryunavailable");

  const dialog = page.getByRole("dialog", { name: /Cannot find Master Database\./ });
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("Launch RBXport after connecting a drive where Master Database is stored.");
  await expect(dialog).toContainText("Do you want to open Master Database in the default drive?");
  await expect(dialog.getByRole("button")).toHaveText(["Yes", "No"]);
  await expect(page.locator(rows)).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(dialog).toBeVisible();

  // Yes, then OK at "Location of Master Database will be changed to the
  // default drive" (the mock confirms): the default folder is empty there,
  // so the next question is whether to make a library in it.
  await dialog.getByRole("button", { name: "Yes" }).click();
  const create = page.getByRole("dialog", { name: "No rekordbox Library" });
  await expect(create).toBeVisible();
  await expect(create).toContainText("/Users/you/Library/Pioneer/rekordbox/master.db");
  await create.getByRole("button", { name: "Create" }).click();
  await expect(create).toBeHidden();
  await expect.poll(async () => page.locator(rows).count(), { timeout: 10_000 }).toBeGreaterThan(0);
});

test("a library that is there never asks", async ({ page }) => {
  await page.goto("/");
  await expect.poll(async () => page.locator(rows).count(), { timeout: 10_000 }).toBeGreaterThan(0);
  await expect(page.getByRole("dialog", { name: "No rekordbox Library" })).toHaveCount(0);
  await expect(page.getByRole("dialog", { name: /Cannot find Master Database/ })).toHaveCount(0);
});

async function databaseManagement(page: Page, query: string) {
  await page.goto(`/${query}`);
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.getByRole("banner").getByRole("button", { name: "Settings" }).click();
  const dialog = page.getByRole("dialog", { name: "Preferences" });
  await dialog.getByRole("tab", { name: "Advanced", exact: true }).click();
  await dialog.getByRole("tab", { name: "Database" }).click();
  return dialog.getByRole("region", { name: "Database management" });
}

test("Database management lists the drives holding a library and switches between them", async ({ page }) => {
  const section = await databaseManagement(page, "?drivelibrary&writable");
  await expect(section).toContainText("Select a drive");
  const drive = section.getByRole("combobox", { name: "Select a drive" });
  await expect(drive.locator("option")).toHaveText(["Macintosh HD", "DJ SSD"]);
  await expect(drive).toBeEnabled();
  await drive.selectOption({ label: "DJ SSD" });
  await expect(drive).toHaveValue("/Volumes/DJ SSD/PIONEER/Master/master.db");
});

test("Database management is greyed out with only the default drive", async ({ page }) => {
  const section = await databaseManagement(page, "?writable");
  const drive = section.getByRole("combobox", { name: "Select a drive" });
  await expect(drive.locator("option")).toHaveText(["Macintosh HD"]);
  await expect(drive).toBeDisabled();
});

test("Database management cannot switch while rekordbox runs", async ({ page }) => {
  // The mock without ?writable is rekordbox running.
  const section = await databaseManagement(page, "?drivelibrary");
  await expect(section.getByRole("combobox", { name: "Select a drive" })).toBeDisabled();
});
