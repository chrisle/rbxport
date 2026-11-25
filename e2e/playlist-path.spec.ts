import { expect, test, type Page } from "@playwright/test";

async function seedPaths(page: Page, delayed = false) {
  await page.route("**/src/main.tsx*", async route => {
      const response = await route.fetch();
      const fixture = `
        import { getBackend as pathFixture } from "/src/ipc/client.ts";
        const pathBackend = await pathFixture();
        for (const folder of ["Shows", "Archive"]) {
          await pathBackend.edits.createFolder(folder, "root");
          const parent = (await pathBackend.playlistTree()).find(node => node.name === folder);
          await pathBackend.edits.createPlaylist("Play", parent.id);
          const playlist = (await pathBackend.playlistTree()).find(node => node.name === "Play" && node.depth === parent.depth + 1 && !window.fixturePlaylists?.includes(node.id));
          window.fixturePlaylists = [...(window.fixturePlaylists ?? []), playlist.id];
          await pathBackend.edits.addTracksToPlaylist(playlist.id, ["100001", "100000", "100002", "100000"], true);
        }
        window.pathWrites = 0;
        for (const [name, edit] of Object.entries(pathBackend.edits)) {
          if (typeof edit === "function") pathBackend.edits[name] = (...args) => { window.pathWrites++; return edit(...args); };
        }
        const openedPaths = new Map();
        const openPathView = pathBackend.openView.bind(pathBackend);
        pathBackend.openView = async spec => { const view = await openPathView(spec); openedPaths.set(view.viewId, spec.source); return view; };
        const pathIds = pathBackend.viewIdsInRange.bind(pathBackend);
        pathBackend.viewIdsInRange = async (...args) => {
          const source = openedPaths.get(args[0]);
          if (window.delayPlaylistLocate && source?.kind === "playlist" && source.id === window.fixturePlaylists[0]) {
            await new Promise(done => { window.releasePlaylistLocate = done; });
          }
          return pathIds(...args);
        };

      `;
      await route.fulfill({ response, body: fixture + await response.text() });
    });

  if (delayed) await page.addInitScript(() => { (window as unknown as { delayPlaylistLocate: boolean }).delayPlaylistLocate = true; });
}

for (const secondary of [false, true]) {
  test(`the ${secondary ? "secondary" : "main"} header opens a containing playlist and restores the selected track`, async ({ page }) => {
    await seedPaths(page);
    await page.goto("/?writable");
    await expect(page.getByTestId("browser-title").first()).toContainText("Tracks)");
    if (secondary) await page.getByRole("button", { name: "Sub-Browser Window" }).click();
    else await page.getByRole("treeitem", { name: /All Tracks/ }).first().click();
    const panel = secondary ? page.getByRole("region", { name: "Sub-Browser" }) : page.locator("main");
    const list = secondary ? panel.getByTestId("track-scroll") : page.getByTestId("track-scroll").first();
    const paths = (secondary ? panel.getByTestId("browser-title") : page.getByTestId("browser-title").first()).locator("select");
    const rows = list.getByRole("row").filter({ has: page.getByRole("gridcell") });
    await expect(paths).toBeDisabled();
    const title = await rows.nth(0).locator('[data-col="title"]').textContent();
    await rows.nth(0).locator('[data-col="title"]').click();
    await expect(paths.locator("option").filter({ hasText: "Playlists / Shows / Play" })).toHaveCount(1);
    await expect(paths.locator("option").filter({ hasText: "Playlists / Archive / Play" })).toHaveCount(1);
    if (!secondary) {
      await page.evaluate(() => {
        const stored = JSON.parse(localStorage.getItem("rbl.preferences") ?? "{}");
        localStorage.setItem("rbl.preferences", JSON.stringify({ ...stored, view: { ...stored.view, locale: "de" } }));
        window.dispatchEvent(new Event("rbl-preferences"));
      });
      await expect(page.getByRole("columnheader", { name: /^Trackname/ })).toBeVisible();
      await expect(paths.locator('option').filter({ hasText: "Wiedergabelisten / Shows / Play" })).toHaveCount(1);
    }
    await paths.selectOption({ label: `${secondary ? "Playlists" : "Wiedergabelisten"} / Shows / Play` });
    await expect(list).toHaveAttribute("aria-rowcount", "4");
    await expect(rows.nth(1)).toHaveAttribute("aria-selected", "true");
    await expect(rows.nth(1).locator('[data-col="title"]')).toHaveText(title!);
    // Existing list selection is by content ID, so duplicate occurrences share it.
    await expect(rows.nth(3)).toHaveAttribute("aria-selected", "true");
    if (secondary) await expect(page.getByTestId("track-scroll").first()).not.toHaveAttribute("aria-rowcount", "4");
    // No click or hover on another row is needed to restore the selected track.
    await paths.selectOption("@collection");
    await expect(list).not.toHaveAttribute("aria-rowcount", "4");
    await expect(rows.nth(0)).toHaveAttribute("aria-selected", "true");
    await rows.nth(1).locator('[data-col="title"]').click({ modifiers: ["Shift"] });
    await expect(paths).toBeDisabled();
    expect(await page.evaluate(() => (window as unknown as { pathWrites: number }).pathWrites)).toBe(0);
  });
}

test("a delayed playlist locate cannot replace a newer browser selection", async ({ page }) => {
  await seedPaths(page, true);
  await page.goto("/?writable");
  await page.getByRole("treeitem", { name: /All Tracks/ }).first().click();
  const list = page.getByTestId("track-scroll").first();
  const rows = list.getByRole("row").filter({ has: page.getByRole("gridcell") });
  const paths = page.getByRole("combobox", { name: "Playlists", exact: true }).first();
  await rows.nth(0).locator('[data-col="title"]').click();
  await paths.selectOption({ label: "Playlists / Shows / Play" });
  await expect.poll(() => page.evaluate(() => typeof (window as unknown as { releasePlaylistLocate?: unknown }).releasePlaylistLocate)).toBe("function");
  await page.getByRole("treeitem", { name: /All Tracks/ }).first().click();
  await expect(list).not.toHaveAttribute("aria-rowcount", "4");
  await rows.nth(1).locator('[data-col="title"]').click();
  await page.evaluate(() => (window as unknown as { releasePlaylistLocate: () => void }).releasePlaylistLocate());
  await expect(rows.nth(1)).toHaveAttribute("aria-selected", "true");
  await expect(rows.nth(0)).not.toHaveAttribute("aria-selected", "true");
});
