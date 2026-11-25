import { expect, test, type Page } from "@playwright/test";

async function seedImports(page: Page) {
    await page.route("**/src/main.tsx*", async route => {
      const response = await route.fetch();
      const fixture = `
        import { getBackend as folderBackend } from "/src/ipc/client.ts";
        const backend = await folderBackend();
        window.folderImports = [];
        const importFolder = backend.importFolderPlaylist.bind(backend);
        backend.importFolderPlaylist = (...args) => { window.folderImports.push(args); return importFolder(...args); };
        window.importedFolderNodes = () => backend.playlistTree();
      `;
      await route.fulfill({ response, body: fixture + await response.text() });
    });
}

for (const secondary of [false, true]) {
  test(`the ${secondary ? "secondary" : "main"} Explorer imports a folder as a named root playlist`, async ({ page }) => {
    await seedImports(page);
    await page.goto("/?writable");
    await expect(page.getByRole("treeitem", { name: /Melodic Vox/ }).first()).toBeVisible();
    if (secondary) await page.getByRole("button", { name: "Sub-Browser Window" }).click();
    const panel = secondary ? page.getByRole("region", { name: "Sub-Browser" }) : page.locator("main");
    const tree = secondary ? panel.getByRole("navigation", { name: "Library" }) : page.getByRole("navigation", { name: "Library" }).first();
    await tree.getByRole("tab", { name: "Explorer" }).click();
    const folder = tree.getByRole("treeitem", { name: /(^|\s)Music$/ });
    for (const name of ["Music", "Music (1)"]) {
      await folder.click({ button: "right" });
      const menu = page.getByRole("menu", { name: "Folder", exact: true });
      await expect(menu).toBeVisible();
      await menu.getByRole("menuitem", { name: "Import as Playlist", exact: true }).click();
      await expect(page.getByRole("contentinfo")).toContainText(`Made playlists: ${name}.`);
    }
    const result = await page.evaluate(async () => {
      const fixture = window as unknown as { folderImports: unknown[][]; importedFolderNodes: () => Promise<{ name: string; depth: number }[]> };
      return { calls: fixture.folderImports, nodes: await fixture.importedFolderNodes() };
    });
    expect(result.calls).toEqual([
      ["/Users/mock/Music", "root", undefined, null, true],
      ["/Users/mock/Music", "root", undefined, null, true],
    ]);
    expect(result.nodes.filter(node => ["Music", "Music (1)"].includes(node.name)).map(node => node.depth)).toEqual([1, 1]);
  });
}

test("an Explorer folder import stays disabled while the library is read-only", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("treeitem", { name: /Melodic Vox/ })).toBeVisible();
  const tree = page.getByRole("navigation", { name: "Library" });
  await tree.getByRole("tab", { name: "Explorer" }).click();
  await tree.getByRole("treeitem", { name: /(^|\s)Music$/ }).click({ button: "right" });
  await expect(page.getByRole("menuitem", { name: "Import as Playlist", exact: true })).toBeDisabled();
});


test("Library Protection greys an Explorer folder import before it reaches the importer", async ({ page }) => {
  await seedImports(page);
  await page.addInitScript(() => {
    const held = JSON.parse(localStorage.getItem("rbl.preferences") ?? "{}");
    localStorage.setItem("rbl.preferences", JSON.stringify({ ...held, advanced: { ...held.advanced, protectLibrary: true } }));
  });
  await page.goto("/?writable");
  await expect(page.getByRole("treeitem", { name: /Melodic Vox/ })).toBeVisible();
  const tree = page.getByRole("navigation", { name: "Library" });
  await tree.getByRole("tab", { name: "Explorer" }).click();
  await tree.getByRole("treeitem", { name: /(^|\s)Music$/ }).click({ button: "right" });
  await expect(page.getByRole("menuitem", { name: "Import as Playlist", exact: true })).toBeDisabled();
  expect(await page.evaluate(() => (window as unknown as { folderImports: unknown[] }).folderImports)).toEqual([]);
});
