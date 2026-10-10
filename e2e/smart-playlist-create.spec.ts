import { expect, test, type Page } from "@playwright/test";

/**
 * Create New Intelligent Playlist, from the tree's right-click menus (#151).
 *
 * rekordbox 7.2.19 offers the row on the Playlists heading, on a folder, on a
 * playlist and on an intelligent playlist, between Create New Playlist and
 * Create New Folder (`BrowsePopupMenuManager::showTreeViewPopupMenu`
 * @0x100119568 and @0x10011b0b8). It opens the rule editor titled with the
 * same words, the list name "Untitled Intelligent List".
 */

async function menuOver(page: Page, name: string, menuName: string) {
  await page.getByRole("tree").first().getByRole("treeitem").filter({ hasText: name }).first().click({ button: "right" });
  const menu = page.getByRole("menu", { name: menuName });
  await expect(menu).toBeVisible();
  return menu;
}

test("every playlist menu offers Create New Intelligent Playlist between the other create rows", async ({ page }) => {
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  for (const [node, menuName] of [
    ["Playlists", "Playlists"],
    ["CURRENT", "Folder"],
    ["Melodic Vox", "Playlist"],
    ["Fresh 128s", "Playlist"],
  ] as const) {
    const menu = await menuOver(page, node, menuName);
    const labels = await menu.getByRole("menuitem").allInnerTexts();
    const at = labels.indexOf("Create New Intelligent Playlist");
    expect(at, node).toBeGreaterThan(0);
    expect(labels.slice(at - 1, at + 2), node).toEqual(["Create New Playlist", "Create New Intelligent Playlist", "Create New Folder"]);
    await expect(menu.getByRole("menuitem", { name: "Create New Intelligent Playlist" })).toBeEnabled();
    await page.keyboard.press("Escape");
    await expect(menu).toHaveCount(0);
  }
});

test("an intelligent playlist's menu opens with Edit the Intelligent Playlist", async ({ page }) => {
  await page.goto("/?writable=1");
  const menu = await menuOver(page, "Fresh 128s", "Playlist");
  await expect(menu.getByRole("menuitem").first()).toHaveText("Edit the Intelligent Playlist");
  await page.keyboard.press("Escape");
  const plain = await menuOver(page, "Melodic Vox", "Playlist");
  await expect(plain.getByRole("menuitem", { name: "Edit the Intelligent Playlist" })).toHaveCount(0);
});

test("Create New Intelligent Playlist makes one in the folder from the rule editor", async ({ page }) => {
  await page.goto("/?writable=1");
  const menu = await menuOver(page, "CURRENT", "Folder");
  await menu.getByRole("menuitem", { name: "Create New Intelligent Playlist" }).click();

  const dialog = page.getByRole("dialog", { name: "Create New Intelligent Playlist" });
  await expect(dialog).toBeVisible();
  const name = dialog.getByLabel("List name");
  await expect(name).toHaveValue("Untitled Intelligent List");
  await expect(name).toBeFocused();

  // OK waits for a condition with a value, as an empty rule says nothing.
  const condition = dialog.getByRole("group", { name: "Condition 1" });
  await expect(dialog.getByRole("button", { name: "OK" })).toBeDisabled();
  await name.fill("Big Room");
  await condition.getByLabel("Property", { exact: true }).selectOption("genre");
  await condition.getByLabel("Value", { exact: true }).fill("House");
  await dialog.getByRole("button", { name: "OK" }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole("contentinfo")).toContainText("Created Big Room.");

  // It is in the tree, under the folder, as an intelligent playlist whose
  // rule is the one written.
  const made = page.getByRole("tree").first().getByRole("treeitem").filter({ hasText: "Big Room" });
  await expect(made).toHaveCount(1);
  const edit = await menuOver(page, "Big Room", "Playlist");
  await edit.getByRole("menuitem", { name: "Edit the Intelligent Playlist" }).click();
  const editor = page.getByRole("dialog", { name: "Edit the Intelligent Playlist" });
  await expect(editor.getByLabel("List name")).toHaveValue("Big Room");
  const saved = editor.getByRole("group", { name: "Condition 1" });
  await expect(saved.getByLabel("Property", { exact: true })).toHaveValue("genre");
  await expect(saved.getByLabel("Value", { exact: true })).toHaveValue("House");
});

test("Cancel makes nothing, and the row is greyed while rekordbox holds the library", async ({ page }) => {
  await page.goto("/?writable=1");
  const menu = await menuOver(page, "Playlists", "Playlists");
  await menu.getByRole("menuitem", { name: "Create New Intelligent Playlist" }).click();
  const dialog = page.getByRole("dialog", { name: "Create New Intelligent Playlist" });
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole("treeitem").filter({ hasText: "Untitled Intelligent List" })).toHaveCount(0);

  // The mock without ?writable=1 stands in for a library rekordbox holds.
  await page.goto("/");
  const held = await menuOver(page, "CURRENT", "Folder");
  await expect(held.getByRole("menuitem", { name: "Create New Intelligent Playlist" })).toBeDisabled();
});
