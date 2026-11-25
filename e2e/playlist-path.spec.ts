import { expect, test, type Page } from "@playwright/test";

async function seedPaths(page: Page, delayed = false, delayedOpen = false) {
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
        window.mutateContainingPlaylist = async operation => {
          const playlist = window.fixturePlaylists[0];
          if (operation === "remove") return pathBackend.edits.removeTracksFromPlaylist(playlist, ["100000"]);
          if (operation === "add") return pathBackend.edits.addTracksToPlaylist(playlist, ["100000"], true);
          if (operation === "rename") return pathBackend.edits.renamePlaylist(playlist, "Changed");
          const parent = (await pathBackend.playlistTree()).find(node => node.name === "Archive" && node.kind === "folder");
          return pathBackend.edits.movePlaylist(playlist, parent.id);
        };
        const openedPaths = new Map();
        const openPathView = pathBackend.openView.bind(pathBackend);
        pathBackend.openView = async spec => {
          if (window.delayPlaylistOpen && spec.source.kind === "playlist" && spec.source.id === window.fixturePlaylists[0]) {
            window.pendingPlaylistOpen = true;
            await new Promise(done => { window.releasePlaylistOpen = done; });
            window.pendingPlaylistOpen = false;
          }
          const view = await openPathView(spec); openedPaths.set(view.viewId, spec.source); return view;
        };
        const pathIds = pathBackend.viewIdsInRange.bind(pathBackend);
        pathBackend.viewIdsInRange = async (...args) => {
          const source = openedPaths.get(args[0]);
          if (window.delayPlaylistLocate && source?.kind === "playlist" && source.id === window.fixturePlaylists[0]) {
            await new Promise(done => { window.releasePlaylistLocate = done; });
          }
          const ids = await pathIds(...args);
          if (window.pendingPlaylistOpen && source?.kind === "collection") window.keyboardDuringOpen = true;
          return ids;
        };

      `;
      await route.fulfill({ response, body: fixture + await response.text() });
    });

  if (delayedOpen) await page.addInitScript(() => { (window as unknown as { delayPlaylistOpen: boolean }).delayPlaylistOpen = true; });
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

test("a selection made while the containing playlist opens is not overwritten", async ({ page }) => {
  await seedPaths(page, false, true);
  await page.goto("/?writable");
  await page.getByRole("treeitem", { name: /All Tracks/ }).first().click();
  const list = page.getByTestId("track-scroll").first();
  const rows = list.getByRole("row").filter({ has: page.getByRole("gridcell") });
  const paths = page.getByTestId("browser-title").first().locator("select");
  await rows.nth(0).locator('[data-col="title"]').click();
  const newer = await rows.nth(1).locator('[data-col="title"]').textContent();
  await paths.selectOption({ label: "Playlists / Shows / Play" });
  await expect.poll(() => page.evaluate(() => typeof (window as unknown as { releasePlaylistOpen?: unknown }).releasePlaylistOpen)).toBe("function");
  await paths.evaluate(element => element.blur());
  await page.keyboard.press("ArrowDown");
  await expect.poll(() => page.evaluate(() => (window as unknown as { keyboardDuringOpen?: boolean }).keyboardDuringOpen)).toBe(true);
  await page.evaluate(() => (window as unknown as { releasePlaylistOpen: () => void }).releasePlaylistOpen());
  await expect(list).toHaveAttribute("aria-rowcount", "4");
  await expect(rows.nth(0)).toHaveAttribute("aria-selected", "true");
  await expect(rows.nth(0).locator('[data-col="title"]')).toHaveText(newer!);
  await expect(rows.nth(1)).not.toHaveAttribute("aria-selected", "true");
});

test("Collection navigation respects a hidden All Tracks node", async ({ page }) => {
  await seedPaths(page);
  await page.goto("/?writable");
  await page.getByRole("treeitem", { name: /All Tracks/ }).first().click();
  const list = page.getByTestId("track-scroll").first();
  const rows = list.getByRole("row").filter({ has: page.getByRole("gridcell") });
  const paths = page.getByTestId("browser-title").first().locator("select");
  await rows.nth(0).locator('[data-col="title"]').click();
  await paths.selectOption({ label: "Playlists / Shows / Play" });
  await expect(list).toHaveAttribute("aria-rowcount", "4");
  await page.evaluate(() => {
    const stored = JSON.parse(localStorage.getItem("rbl.preferences") ?? "{}");
    localStorage.setItem("rbl.preferences", JSON.stringify({ ...stored, view: { ...stored.view, allTracks: false } }));
    window.dispatchEvent(new StorageEvent("storage", { key: "rbl.preferences" }));
  });
  await expect(page.locator('[role="treeitem"][data-kind="allTracks"]')).toHaveCount(0);
  await paths.selectOption("@collection");
  await expect(list).not.toHaveAttribute("aria-rowcount", "4");
  await expect(page.locator('[role="treeitem"][data-kind="collection"]').first()).toHaveAttribute("aria-selected", "true");
});

test("containing paths refresh after membership and tree edits without changing selection", async ({ page }) => {
  await seedPaths(page);
  await page.goto("/?writable");
  await page.getByRole("treeitem", { name: /All Tracks/ }).first().click();
  const list = page.getByTestId("track-scroll").first();
  const rows = list.getByRole("row").filter({ has: page.getByRole("gridcell") });
  const paths = page.getByTestId("browser-title").first().locator("select");
  await rows.nth(0).locator('[data-col="title"]').click();
  const path = (label: string) => paths.locator("option").filter({ hasText: label });
  await expect(path("Playlists / Shows / Play")).toHaveCount(1);
  const mutate = (operation: string) => page.evaluate(op => (window as unknown as { mutateContainingPlaylist: (op: string) => Promise<unknown> }).mutateContainingPlaylist(op), operation);
  await mutate("remove");
  await expect(path("Playlists / Shows / Play")).toHaveCount(0);
  await expect(path("Playlists / Archive / Play")).toHaveCount(1);
  await expect(rows.nth(0)).toHaveAttribute("aria-selected", "true");
  await mutate("add");
  await expect(path("Playlists / Shows / Play")).toHaveCount(1);
  await mutate("rename");
  await expect(path("Playlists / Shows / Changed")).toHaveCount(1);
  await mutate("move");
  await expect(path("Playlists / Shows / Changed")).toHaveCount(0);
  await expect(path("Playlists / Archive / Changed")).toHaveCount(1);
  await expect(rows.nth(0)).toHaveAttribute("aria-selected", "true");
});
