import { expect, test } from "@playwright/test";

for (const side of ["main", "sub"] as const) {
  for (const surface of ["tree", "list"] as const) {
    test(`ordinary file import targets ${side} All Tracks ${surface}`, async ({ page }) => {
      await page.goto("/?writable=1&dropFixture=1");
      await expect(page.locator('nav[data-browser-side="main"] [role="treeitem"][data-kind="allTracks"]')).toBeVisible({ timeout: 30_000 });
      if (side === "sub") await page.getByRole("button", { name: "Sub-Browser Window" }).click();
      const tree = page.locator(`nav[data-browser-side="${side}"]`);
      const all = tree.locator('[role="treeitem"][data-kind="allTracks"]');
      await all.click();
      const browser = page.locator(`[data-browser-pane="list"][data-browser-side="${side}"]`);
      const grid = browser.getByTestId("track-scroll");
      const target = surface === "tree" ? all : grid;
      await expect(target).toBeVisible();
      const name = `All Tracks ${side} ${surface}`;
      const accepted = await target.evaluate((element, title) => {
        const transfer = new DataTransfer();
        transfer.items.add(new File([], `${title}.mp3`, { type: "audio/mpeg" }));
        Object.defineProperty(transfer.files[0], "path", { value: `/tmp/${title}.mp3` });
        const over = new DragEvent("dragover", { bubbles: true, cancelable: true, dataTransfer: transfer });
        element.dispatchEvent(over);
        element.dispatchEvent(new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer: transfer }));
        return over.defaultPrevented;
      }, name);
      expect(accepted).toBe(true);
      await expect(page.getByRole("contentinfo")).toContainText("Imported 1 of 1 files");
      const result = await page.evaluate(async title => {
        const moduleUrl = "/src/ipc/client.ts";
        const { getBackend } = await import(moduleUrl) as typeof import("../src/ipc/client");
        const backend = await getBackend();
        const view = await backend.openView({ source: { kind: "collection" }, sort: "title", descending: false, query: title });
        return backend.fetchRows(view.viewId, 0, 10, []);
      }, name);
      expect(result).toHaveLength(1);
      expect(result[0]!.title).toBe(name);
      await browser.getByRole("searchbox", { name: "Search within this track list" }).fill(name);
      await expect(browser.getByRole("row").filter({ has: page.getByRole("gridcell") })).toHaveCount(1);
      await expect(browser.locator('[data-col="title"]')).toContainText([name]);
    });
  }
}

// The fixture exposes App's actual native callback; it does not emulate AppKit,
// WebView2 or GTK payload delivery, filesystem resolution or import writes.
for (const scenario of [
  { side: "main", kind: "allTracks", surface: "list", writable: true },
  { side: "sub", kind: "allTracks", surface: "list", writable: true },
  { side: "sub", kind: "playlist", surface: "list", writable: true },
  { side: "sub", kind: "playlist", surface: "tree", writable: true },
  { side: "sub", kind: "history", surface: "list", writable: true },
  { side: "sub", kind: "histories", surface: "list", writable: true },
  { side: "sub", kind: "allTracks", surface: "list", writable: false },
] as const) {
  test(`native drop targets ${scenario.side} ${scenario.kind} ${scenario.surface} writable=${scenario.writable}`, async ({ page }) => {
    await page.route("**/src/ipc/client.ts*", async route => {
      const response = await route.fetch();
      const body = await response.text();
      const signature = /export function subscribeNativeFileDrops\(listener\) \{/;
      expect(body).toMatch(signature);
      await route.fulfill({ response, body: body.replace(signature,
        "$& window.nativeDrop642 = listener;") });
    });
    await page.goto(`/?${scenario.writable ? "writable=1&" : ""}dropFixture=1`);
    await expect(page.locator('nav[data-browser-side="main"] [role="treeitem"][data-kind="allTracks"]')).toBeVisible({ timeout: 30_000 });
    const mainTree = page.locator('nav[data-browser-side="main"]');
    await mainTree.getByRole("treeitem").filter({ hasText: "Melodic Vox" }).first().click();
    await page.getByRole("button", { name: "Sub-Browser Window" }).click();
    const sideTree = page.locator(`nav[data-browser-side="${scenario.side}"]`);
    if (scenario.kind === "history" || scenario.kind === "histories") await sideTree.getByRole("tab", { name: "Histories" }).click();
    const node = sideTree.locator(`[role="treeitem"][data-kind="${scenario.kind}"]`).nth(scenario.kind === "playlist" ? 1 : 0);
    await node.click();
    const expectedPlaylist = scenario.kind === "playlist" ? await node.getAttribute("data-file-drop-playlist") : null;
    if (scenario.kind === "playlist") expect(expectedPlaylist).not.toBeNull();
    const capture = await page.evaluate(async () => {
      const moduleUrl = "/src/ipc/client.ts";
      const { getBackend } = await import(moduleUrl) as typeof import("../src/ipc/client");
      const backend = await getBackend();
      const evidence = { writes: [] as string[], imports: 0 };
      const add = backend.edits.addTracksToPlaylist.bind(backend.edits);
      backend.edits.addTracksToPlaylist = (...args) => { evidence.writes.push(args[0]); return add(...args); };
      const find = backend.listImportFiles.bind(backend);
      backend.listImportFiles = (...args) => { evidence.imports++; return find(...args); };
      Object.assign(window, { nativeDropEvidence642: evidence });
      const view = await backend.openView({ source: { kind: "collection" }, sort: "title", descending: false, query: "" });
      return view.len;
    });
    const grid = page.locator(`[data-browser-pane="list"][data-browser-side="${scenario.side}"]`).getByTestId("track-scroll");
    const target = scenario.surface === "tree" ? node : grid;
    await target.evaluate(element => {
      const rect = element.getBoundingClientRect();
      (window as unknown as { nativeDrop642: (drop: { paths: string[]; x: number; y: number }) => void })
        .nativeDrop642({ paths: ["/tmp/Native Owner 642.mp3"], x: rect.left + rect.width / 2, y: rect.top + Math.min(20, rect.height / 2) });
    });
    const refused = !scenario.writable || scenario.kind === "history" || scenario.kind === "histories";
    await expect(page.getByRole("contentinfo")).toContainText(refused
      ? "Drop files or folders onto All Tracks or a playlist to import them."
      : expectedPlaylist ? "Added 1 track to" : "Imported 1 of 1 files");
    const result = await page.evaluate(async () => {
      const moduleUrl = "/src/ipc/client.ts";
      const { getBackend } = await import(moduleUrl) as typeof import("../src/ipc/client");
      const backend = await getBackend();
      const view = await backend.openView({ source: { kind: "collection" }, sort: "title", descending: false, query: "" });
      return { ...(window as unknown as { nativeDropEvidence642: { writes: string[]; imports: number } }).nativeDropEvidence642,
        count: view.len };
    });
    expect(result).toEqual({ writes: refused || expectedPlaylist === null ? [] : [expectedPlaylist], imports: refused ? 0 : 1,
      count: capture + (refused ? 0 : 1) });
  });
}
