/**
 * The My Tag configuration window (issue #539) against the mock backend:
 * opened from the icon column in the information window's place, a row per
 * category and per tag, ticks that follow the whole selection, Adding My
 * Tag, renaming and Delete — as rekordbox 7.2.14 draws and runs them.
 */
import { expect, test, type Page } from "@playwright/test";

/** Reads a token from the running page, so the test and the app agree. */
async function token(page: Page, name: string): Promise<number> {
  const value = await page.evaluate(
    (n) => getComputedStyle(document.documentElement).getPropertyValue(n),
    name,
  );
  return Number.parseFloat(value);
}

const rail = (page: Page) => page.getByRole("toolbar", { name: "Browser panels" });
const panel = (page: Page) => page.getByRole("region", { name: "My Tag" });
const titleCell = (page: Page, row: number) => page.locator('[role="gridcell"][data-col="title"]').nth(row);

test.beforeEach(async ({ page }) => {
  // Writable: the mock otherwise stands in for a library rekordbox holds.
  await page.goto("/?writable");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
});

test("the My Tag box opens the window in the information window's place", async ({ page }) => {
  const myTag = rail(page).getByRole("button", { name: "My Tag" });
  const info = rail(page).getByRole("button", { name: "Information" });
  await info.click();
  await expect(page.getByRole("complementary", { name: "Information" })).toBeVisible();

  await myTag.click();
  await expect(myTag).toHaveAttribute("aria-pressed", "true");
  await expect(info).toHaveAttribute("aria-pressed", "false");
  await expect(page.getByRole("complementary", { name: "Information" })).toHaveCount(0);
  await expect(panel(page)).toBeVisible();
  const box = await panel(page).boundingBox();
  expect(box?.width).toBeCloseTo(await token(page, "--s-mytag-w"), 0);

  // A row per category, then its tags, on the 24pt pitch.
  const situation = panel(page).getByRole("group", { name: "Situation" });
  await expect(situation.getByRole("listitem")).toHaveText(["Situation", "Peak", "Warm-up"]);
  const rows = await situation.getByRole("listitem").evaluateAll((items) => items.map((i) => i.getBoundingClientRect().height));
  for (const height of rows) expect(height).toBeCloseTo(await token(page, "--s-mytag-row-h"), 0);

  // With nothing selected the boxes are greyed.
  await expect(situation.getByRole("checkbox", { name: "Peak" })).toBeDisabled();

  await info.click();
  await expect(myTag).toHaveAttribute("aria-pressed", "false");
  await expect(panel(page)).toHaveCount(0);
});

test("a tick tags every selected track, and shows only what they all share", async ({ page }) => {
  await rail(page).getByRole("button", { name: "My Tag" }).click();
  const synth = panel(page).getByRole("checkbox", { name: "Synth" });
  await titleCell(page, 1).click();
  await expect(synth).toBeEnabled();
  await expect(synth).toHaveAttribute("aria-checked", "false");
  await synth.click();
  await expect(synth).toHaveAttribute("aria-checked", "true");

  // A second track without it: the two do not share it, so it is unticked.
  await titleCell(page, 2).click({ modifiers: ["Shift"] });
  await expect(synth).toHaveAttribute("aria-checked", "false");
  // Ticking it now tags both.
  await synth.click();
  await expect(synth).toHaveAttribute("aria-checked", "true");
  await titleCell(page, 2).click();
  await expect(synth).toHaveAttribute("aria-checked", "true");
  await titleCell(page, 1).click();
  await expect(synth).toHaveAttribute("aria-checked", "true");
  await synth.click();
  await expect(synth).toHaveAttribute("aria-checked", "false");
});

test("Adding My Tag makes My Comment, opens it for a name, and Delete takes it away", async ({ page }) => {
  await rail(page).getByRole("button", { name: "My Tag" }).click();
  const components = panel(page).getByRole("group", { name: "Components" });
  // The ⊕ sits on the category's last row.
  const add = components.getByRole("button", { name: "Create a new My Tag." });
  await expect(add).toHaveCount(1);
  await add.click();
  const name = components.getByRole("textbox", { name: "Name" });
  await expect(name).toBeFocused();
  await expect(name).toHaveValue("My Comment");
  await name.fill("Strings");
  await name.press("Enter");
  await expect(components.getByRole("listitem")).toHaveText(["Components", "Synth", "Vocal", "Strings"]);

  // The next one takes the base name again, since none is called that now.
  await add.click();
  await expect(components.getByRole("textbox", { name: "Name" })).toHaveValue("My Comment");
  await components.getByRole("textbox", { name: "Name" }).press("Escape");
  await expect(components.getByRole("listitem")).toHaveText(["Components", "Synth", "Vocal", "Strings", "My Comment"]);

  // A name the category already has is refused, and the row keeps its own.
  await components.getByRole("listitem").filter({ hasText: "My Comment" }).click();
  const again = components.getByRole("textbox", { name: "Name" });
  await again.fill("Synth");
  await again.press("Enter");
  await expect(page.getByText("An item with the same name already exists!")).toBeVisible();
  await expect(components.getByRole("listitem")).toHaveText(["Components", "Synth", "Vocal", "Strings", "My Comment"]);

  // Right-click offers Delete alone, and it deletes at once.
  await components.getByRole("listitem").filter({ hasText: "Strings" }).click({ button: "right" });
  const menu = page.getByRole("menu", { name: "My Tag" });
  await expect(menu.getByRole("menuitem")).toHaveText(["Delete"]);
  await menu.getByRole("menuitem", { name: "Delete" }).click();
  await expect(components.getByRole("listitem")).toHaveText(["Components", "Synth", "Vocal", "My Comment"]);
});

test("a library rekordbox holds leaves every My Tag control greyed", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await rail(page).getByRole("button", { name: "My Tag" }).click();
  await titleCell(page, 1).click();
  await expect(panel(page).getByRole("checkbox", { name: "Synth" })).toBeDisabled();
  await expect(panel(page).getByRole("button", { name: "Create a new My Tag." }).first()).toBeDisabled();
  await panel(page).getByRole("listitem").filter({ hasText: "Peak" }).click();
  await expect(panel(page).getByRole("textbox", { name: "Name" })).toHaveCount(0);
});

test("a category is renamed by clicking its name, and has no menu", async ({ page }) => {
  await rail(page).getByRole("button", { name: "My Tag" }).click();
  const heading = panel(page).getByRole("group", { name: "Situation" }).getByRole("listitem").first();
  await heading.click({ button: "right" });
  await expect(page.getByRole("menu", { name: "My Tag" })).toHaveCount(0);
  await heading.click();
  const name = panel(page).getByRole("textbox", { name: "Name" });
  await name.fill("When");
  await name.press("Enter");
  await expect(panel(page).getByRole("group", { name: "When" })).toBeVisible();
});
