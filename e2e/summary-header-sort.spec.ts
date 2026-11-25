import { expect, test, type Locator, type Page } from "@playwright/test";

async function fixture(page: Page) {
  await page.route("**/src/main.tsx*", async route => {
    const response = await route.fetch();
    await route.fulfill({ response, body: `
      import { getBackend as summaryBackend } from "/src/ipc/client.ts";
      import { defaultLayout, fullArtworkLayout } from "/src/lib/columns.ts";
      for (const context of ["collection", "playlist", "subBrowser"]) localStorage.setItem("rbl.columns.v2." + context + ".full", JSON.stringify(fullArtworkLayout(defaultLayout(), false)));
      const backend = await summaryBackend();
      await backend.edits.createPlaylist("Summary order", "root");
      const playlist = (await backend.playlistTree()).find(node => node.name === "Summary order");
      const ids = ["100000", "100001", "100002"];
      for (let i = 0; i < ids.length; i++) {
        for (const [field, values] of [["title", ["Zulu", "Alpha", "Mike"]], ["artist", ["Alpha", "Zulu", "Mike"]], ["bpm", ["130", "110", "120"]], ["key", ["Am", "Cm", "Bm"]]]) await backend.edits.setTrackField([ids[i]], field, values[i]);
      }
      await backend.edits.addTracksToPlaylist(playlist.id, ids, true);
    ` + await response.text() });
  });
}
const pane = (page: Page, sub: boolean) => page.locator(`[data-browser-pane="list"][data-browser-side="${sub ? "sub" : "main"}"]`);
const summary = (scope: Locator) => scope.getByRole("columnheader", { name: /^Summary( \(| Resize|$)/ });
const titles = (scope: Locator) => scope.getByTestId("track-scroll").locator('[data-col="summary"] > span:first-child');
async function choose(page: Page, scope: Locator, label: string) {
  await summary(scope).scrollIntoViewIfNeeded();
  await page.evaluate(async () => { await new Promise(requestAnimationFrame); await new Promise(requestAnimationFrame); });
  await summary(scope).click();
  const menu = page.getByRole("menu", { name: "Summary", exact: true });
  await expect(menu.getByRole("menuitem")).toHaveText(["Track Title", "Artist", "BPM", "Key"]);
  await menu.getByRole("menuitem", { name: label, exact: true }).click();
}
async function open(page: Page, sub: boolean) {
  await page.goto("/?writable");
  if (sub) await page.getByRole("button", { name: "Sub-Browser Window" }).click();
  const owner = sub ? page.getByRole("region", { name: "Sub-Browser" }) : page.locator('[data-browser-pane="tree"][data-browser-side="main"]');
  await owner.getByRole("treeitem").filter({ hasText: /^\s*Summary order/ }).click();
  const scope = pane(page, sub);
  await scope.getByTestId("artwork-mode-full").click();
  await expect(titles(scope)).toHaveText(["Zulu", "Alpha", "Mike"]);
  return scope;
}
for (const sub of [false, true]) {
  test(`${sub ? "secondary" : "main"} Summary choices share canonical order and transfer one indicator`, async ({ page }) => {
    await fixture(page);
    const scope = await open(page, sub);
    for (const [label, expected] of [["Track Title", ["Alpha", "Mike", "Zulu"]], ["Artist", ["Zulu", "Mike", "Alpha"]], ["BPM", ["Alpha", "Mike", "Zulu"]], ["Key", ["Zulu", "Mike", "Alpha"]]] as const) {
      await choose(page, scope, label);
      await expect(summary(scope)).toContainText(`Summary (${label})`);
      await expect(scope.locator('[role="columnheader"][data-sorted]')).toHaveCount(1);
      await expect(summary(scope)).toHaveAttribute("data-sorted", "true");
      await expect(summary(scope).locator("svg")).toHaveCount(1);
      await expect(titles(scope)).toHaveText(expected);
      await choose(page, scope, label);
      await expect(titles(scope)).toHaveText([...expected].reverse());
      await choose(page, scope, label);
      await expect(titles(scope)).toHaveText(["Zulu", "Alpha", "Mike"]);
      await expect(summary(scope)).not.toHaveAttribute("data-sorted");
    }
    await choose(page, scope, "Artist");
    await summary(scope).click();
    await page.keyboard.press("Escape");
    await expect(page.getByRole("menu", { name: "Summary", exact: true })).toHaveCount(0);
    await expect(titles(scope)).toHaveText(["Zulu", "Mike", "Alpha"]);
    await scope.getByRole("columnheader", { name: /^Artist( Resize|$)/ }).click();
    await expect(summary(scope)).not.toHaveAttribute("data-sorted");
    await expect(scope.getByRole("columnheader", { name: /^Artist( Resize|$)/ })).toHaveAttribute("data-sorted", "true");
    await expect(titles(scope)).toHaveText(["Zulu", "Mike", "Alpha"]);
    await choose(page, scope, "Artist");
    await scope.getByTestId("artwork-mode-partial").click();
    await expect(scope.getByRole("columnheader", { name: /^Artist( Resize|$)/ })).toHaveAttribute("data-sorted", "true");
  });
  test(`${sub ? "secondary" : "main"} Summary musical Key, menu retirement and independent pane ownership`, async ({ page }) => {
    await page.addInitScript(() => localStorage.setItem("rbl.preferences", JSON.stringify({ advanced: { protectLibrary: false }, view: { keySort: "musical" } })));
    await fixture(page);
    const scope = await open(page, sub);
    const mainIndicators = await pane(page, false).locator('[role="columnheader"][data-sorted]').allTextContents();
    await choose(page, scope, "Key");
    await expect(titles(scope)).toHaveText(["Alpha", "Zulu", "Mike"]);
    await expect(scope.locator('[role="columnheader"][data-sorted]')).toHaveCount(1);
    await scope.getByRole("columnheader", { name: /^Key( Resize|$)/ }).click();
    await expect(scope.getByRole("columnheader", { name: /^Key( Resize|$)/ })).toHaveAttribute("data-sorted", "true");
    await expect(summary(scope)).not.toHaveAttribute("data-sorted");
    await choose(page, scope, "Artist");
    if (sub) {
      await expect(pane(page, false).locator('[role="columnheader"][data-sorted]')).toHaveText(mainIndicators);
      await page.getByRole("button", { name: "Sub-Browser Window" }).click();
      await page.getByRole("button", { name: "Sub-Browser Window" }).click();
      await expect(summary(pane(page, true))).toContainText("Summary (Artist)");
    }
    await summary(scope).click();
    await scope.getByTestId("artwork-mode-partial").click();
    await expect(page.getByRole("menu", { name: "Summary", exact: true })).toHaveCount(0);
    await scope.getByTestId("artwork-mode-full").click();
    await summary(scope).click();
    const tree = sub ? page.getByRole("region", { name: "Sub-Browser" }) : page.locator('[data-browser-pane="tree"][data-browser-side="main"]');
    await tree.getByRole("treeitem", { name: /All Tracks/ }).first().click();
    await expect(page.getByRole("menu", { name: "Summary", exact: true })).toHaveCount(0);
  });
  test(`${sub ? "secondary" : "main"} Summary sort restores and ordinary choice clears its saved origin`, async ({ page }) => {
    await fixture(page);
    let scope = await open(page, sub);
    await choose(page, scope, "Artist");
    await choose(page, scope, "Artist");
    await page.reload();
    scope = pane(page, sub);
    await expect(summary(scope)).toContainText("Summary (Artist)");
    await expect(summary(scope)).toHaveAttribute("data-sorted", "true");
    await scope.getByRole("columnheader", { name: /^Artist( Resize|$)/ }).click();
    await page.reload();
    await expect(summary(pane(page, sub))).not.toHaveAttribute("data-sorted");
    await expect(summary(pane(page, sub))).not.toContainText("(Artist)");
  });
}
