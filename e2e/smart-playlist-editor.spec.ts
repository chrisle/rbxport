import { expect, test, type Page } from "@playwright/test";

/**
 * The intelligent playlist editor keeps focus where the user put it.
 *
 * #131, #215: its dropdowns opened and shut at once, and focus went back to
 * the list name. The editor focused the name every time App rendered it with
 * a new `onCancel`, and App renders on its own: when the window regains
 * focus it refreshes the devices and LINK status. A native dropdown loses
 * its popup when focus leaves it, so it shut. A window `focus` event stands
 * in here for whatever re-render the Windows WebView2 window gets while a
 * dropdown is open.
 */

async function openEditor(page: Page) {
  // The mock stands in for a library rekordbox holds unless told otherwise.
  await page.goto("/?writable=1");
  await page.getByRole("treeitem").filter({ hasText: "Fresh 128s" }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Edit the Intelligent Playlist" }).click();
  const dialog = page.getByRole("dialog", { name: "Edit the Intelligent Playlist" });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByLabel("List name")).toBeFocused();
  return dialog;
}

/** A window focus event, and time for what App does about it to render. */
async function refocusWindow(page: Page) {
  await page.evaluate(async () => {
    window.dispatchEvent(new Event("focus"));
    await new Promise((r) => setTimeout(r, 200));
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  });
}

test("a dropdown in the intelligent playlist editor keeps focus when the app renders again", async ({ page }) => {
  const dialog = await openEditor(page);
  const condition = dialog.getByRole("group", { name: "Condition 1" });

  for (const label of ["Property", "Operator"]) {
    const select = condition.getByLabel(label, { exact: true });
    await select.focus();
    await refocusWindow(page);
    await expect(select).toBeFocused();
  }

  // And the choice made in it is the one that stays.
  const property = condition.getByLabel("Property", { exact: true });
  await property.selectOption("genre");
  await refocusWindow(page);
  await expect(property).toHaveValue("genre");
});
