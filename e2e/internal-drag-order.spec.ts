import { expect, test } from "@playwright/test";

for (const reverseSelection of [false, true]) {
  test(`cross-playlist drag preserves displayed order (reverse selection=${reverseSelection})`, async ({ page }) => {
    await page.route("**/src/main.tsx*", async route => {
      const response = await route.fetch();
      await route.fulfill({ response, body: `
        import { getBackend as dragOrderBackend } from "/src/ipc/client.ts";
        const backend = await dragOrderBackend();
        const all = await backend.openView({ source: {kind:"collection"}, sort:"trackNo", descending:false, query:"" });
        const tracks = await backend.viewIdsInRange(all.viewId, 0, 3);
        await backend.edits.createPlaylist("Generated drag source", "root");
        await backend.edits.createPlaylist("Generated drag target", "root");
        const tree = await backend.playlistTree();
        const source = tree.find(node => node.name === "Generated drag source");
        const target = tree.find(node => node.name === "Generated drag target");
        await backend.edits.addTracksToPlaylist(source.id, tracks, false);
        window.dragOrderFixture = { tracks, target: target.id, read: async () => {
          const view = await backend.openView({ source: {kind:"playlist",id:target.id}, sort:"trackNo", descending:false, query:"" });
          return view.len ? backend.viewIdsInRange(view.viewId, 0, view.len - 1) : [];
        } };
      ` + await response.text() });
    });
    await page.goto("/?writable=1");
    await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
    const source = page.getByRole("treeitem").filter({ hasText: /^\s*Generated drag source/ }).first();
    const target = page.getByRole("treeitem").filter({ hasText: /^\s*Generated drag target/ }).first();
    await source.click();
    await expect(page.getByTestId("browser-title")).toContainText("Generated drag source (4 Tracks)");
    const rows = page.locator('[role="gridcell"][data-col="title"]');
    await rows.nth(reverseSelection ? 2 : 0).click();
    await rows.nth(reverseSelection ? 0 : 2).click({ modifiers: ["ControlOrMeta"] });
    await expect(page.locator('[role="row"][aria-selected="true"]')).toHaveCount(2);
    const expected = await page.evaluate(() => {
      const fixture = (window as unknown as { dragOrderFixture: {tracks: string[]} }).dragOrderFixture;
      return [fixture.tracks[0], fixture.tracks[2]];
    });
    await rows.nth(0).dragTo(target);
    await expect(page.getByRole("contentinfo")).toContainText("Added 2 tracks to Generated drag target.");
    const actual = await page.evaluate(() => (window as unknown as { dragOrderFixture: {read(): Promise<string[]>} }).dragOrderFixture.read());
    expect(actual).toEqual(expected);
  });
}


for (const missingRank of [false, true]) {
  test(`a virtualized drop waits for exact source ranks (missing=${missingRank})`, async ({ page }) => {
    await page.route("**/src/main.tsx*", async route => {
      const response = await route.fetch();
      await route.fulfill({ response, body: `
        import { getBackend as virtualDragBackend } from "/src/ipc/client.ts";
        const backend = await virtualDragBackend();
        const all = await backend.openView({ source: {kind:"collection"}, sort:"trackNo", descending:false, query:"" });
        const tracks = await backend.viewIdsInRange(all.viewId, 0, 319);
        await backend.edits.createPlaylist("Virtual drag source", "root");
        await backend.edits.createPlaylist("Virtual drag target", "root");
        const tree = await backend.playlistTree();
        const source = tree.find(node => node.name === "Virtual drag source");
        const target = tree.find(node => node.name === "Virtual drag target");
        await backend.edits.addTracksToPlaylist(source.id, tracks, false);
        const rawOpen = backend.openView.bind(backend);
        const rawIds = backend.viewIdsInRange.bind(backend);
        const rawAdd = backend.edits.addTracksToPlaylist.bind(backend.edits);
        const sourceViews = new Set();
        let reads = 0, adds = 0, release;
        backend.openView = async spec => {
          const view = await rawOpen(spec);
          if (spec.source.kind === "playlist" && spec.source.id === source.id) sourceViews.add(view.viewId);
          return view;
        };
        backend.viewIdsInRange = async (...args) => {
          const ids = await rawIds(...args);
          if (sourceViews.has(args[0]) && args[1] === 0 && args[2] >= 319 && ++reads === 2) {
            return new Promise(resolve => { release = () => resolve(${missingRank} ? ids.slice(1) : ids); });
          }
          return ids;
        };
        backend.edits.addTracksToPlaylist = (...args) => { if (args[0] === target.id) adds++; return rawAdd(...args); };
        window.virtualDragFixture = { tracks, held: () => typeof release === "function", adds: () => adds, release: () => release(), read: async () => {
          const view = await rawOpen({ source: {kind:"playlist",id:target.id}, sort:"trackNo", descending:false, query:"" });
          return view.len ? rawIds(view.viewId, 0, view.len - 1) : [];
        } };
      ` + await response.text() });
    });
    await page.goto("/?writable=1");
    await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
    await page.getByRole("treeitem").filter({ hasText: /^\s*Virtual drag source/ }).first().click();
    await expect(page.getByTestId("browser-title")).toContainText("Virtual drag source (320 Tracks)");
    const row = page.locator('[role="gridcell"][data-col="title"]').first();
    await row.click();
    await page.keyboard.press("ControlOrMeta+A");
    await expect.poll(() => page.evaluate(() => (window as unknown as {virtualDragFixture:{held():boolean}}).virtualDragFixture.held())).toBe(true);
    await row.dragTo(page.getByRole("treeitem").filter({ hasText: /^\s*Virtual drag target/ }).first());
    const read = () => page.evaluate(async () => {
      const fixture = (window as unknown as {virtualDragFixture:{adds():number;read():Promise<string[]>;tracks:string[]}}).virtualDragFixture;
      return { adds: fixture.adds(), actual: await fixture.read(), expected: fixture.tracks };
    });
    expect(await read()).toMatchObject({ adds: 0, actual: [] });
    await page.evaluate(() => (window as unknown as {virtualDragFixture:{release():void}}).virtualDragFixture.release());
    if (missingRank) {
      await expect(page.getByRole("contentinfo")).toContainText("The track list changed. Start the drag again.");
      expect(await read()).toMatchObject({ adds: 0, actual: [] });
    } else {
      await expect(page.getByRole("contentinfo")).toContainText("Added 320 tracks to Virtual drag target.");
      const result = await read();
      expect(result.actual).toEqual(result.expected);
    }
  });
}


for (const keep of [false, true]) {
  test(`repeated selected content keeps visual order with existing duplicate choice (keep=${keep})`, async ({ page }) => {
    await page.route("**/src/main.tsx*", async route => {
      const response = await route.fetch();
      await route.fulfill({ response, body: `
        import { getBackend as repeatedDragBackend } from "/src/ipc/client.ts";
        const backend = await repeatedDragBackend();
        const all = await backend.openView({ source: {kind:"collection"}, sort:"trackNo", descending:false, query:"" });
        const ids = await backend.viewIdsInRange(all.viewId, 0, 2);
        const tracks = [ids[0], ids[2], ids[0], ids[1]];
        await backend.edits.createPlaylist("Repeated drag source", "root");
        await backend.edits.createPlaylist("Repeated drag target", "root");
        const tree = await backend.playlistTree();
        const source = tree.find(node => node.name === "Repeated drag source");
        const target = tree.find(node => node.name === "Repeated drag target");
        await backend.edits.addTracksToPlaylist(source.id, tracks, true);
        await backend.edits.addTracksToPlaylist(target.id, [ids[0]], false);
        const add = backend.edits.addTracksToPlaylist.bind(backend.edits);
        const writes = [];
        backend.edits.addTracksToPlaylist = (...args) => { if (args[0] === target.id) writes.push(args); return add(...args); };
        window.repeatedDrag = { ids, tracks, writes, read: async () => {
          const view = await backend.openView({ source: {kind:"playlist",id:target.id}, sort:"trackNo", descending:false, query:"" });
          return backend.viewIdsInRange(view.viewId, 0, view.len - 1);
        } };
      ` + await response.text() });
    });
    await page.goto("/?writable=1");
    await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
    await page.getByRole("treeitem").filter({ hasText: /^\s*Repeated drag source/ }).first().click();
    await expect(page.getByTestId("browser-title")).toContainText("Repeated drag source (4 Tracks)");
    const row = page.locator('[role="gridcell"][data-col="title"]').first();
    await row.click(); await page.keyboard.press("ControlOrMeta+A");
    await expect(page.locator('[role="row"][aria-selected="true"]')).toHaveCount(4);
    await row.dragTo(page.getByRole("treeitem").filter({ hasText: /^\s*Repeated drag target/ }).first());
    await page.getByRole("button", {name: keep ? "Add duplicates" : "Skip duplicates", exact:true}).click();
    await expect(page.getByRole("contentinfo")).toContainText(`Added ${keep ? 4 : 2} tracks to Repeated drag target.`);
    const result = await page.evaluate(async () => {
      const fixture = (window as unknown as {repeatedDrag:{ids:string[];tracks:string[];writes:[string,string[],boolean][];read():Promise<string[]>}}).repeatedDrag;
      return {actual:await fixture.read(), ids:fixture.ids, tracks:fixture.tracks, writes:fixture.writes};
    });
    expect(result.writes).toHaveLength(1);
    expect(result.writes[0]?.[1]).toEqual(result.tracks);
    expect(result.writes[0]?.[2]).toBe(keep);
    expect(result.actual).toEqual(keep ? [result.ids[0], ...result.tracks] : [result.ids[0],result.ids[2],result.ids[1]]);
  });
}
