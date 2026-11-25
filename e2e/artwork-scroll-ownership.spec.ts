import { expect, test, type Page } from "@playwright/test";

type ArtworkFixture = Window & {
  holdArtworkIds: () => void;
  artworkIdsPending: boolean;
  artworkOpens: number;
  releaseArtworkIds: () => Promise<void>;
};

async function holdArtworkLookup(page: Page, custom = false) {
  await page.route("**/src/main.tsx*", async route => {
    const response = await route.fetch();
    const fixture = `
      ${custom ? `
        import { defaultLayout as artworkDefaults, fullArtworkLayout as artworkFull, toggleColumn as artworkToggle } from "/src/lib/columns.ts";
        const customArtwork = artworkToggle(artworkFull(artworkDefaults(), true), "composer", "full");
        for (const context of ["collection", "subBrowser"]) localStorage.setItem("rbl.columns.v2." + context + ".full", JSON.stringify(customArtwork));
      ` : ""}
      import { getBackend as artworkFixtureBackend } from "/src/ipc/client.ts";
      const artworkBackend = await artworkFixtureBackend();
      await artworkBackend.edits.createPlaylist("Artwork destination", "root");
      const artworkPlaylist = (await artworkBackend.playlistTree()).find(node => node.name === "Artwork destination");
      await artworkBackend.edits.addTracksToPlaylist(artworkPlaylist.id, Array.from({ length: 80 }, (_, i) => String(100000 + i)), true);
      const artworkIds = artworkBackend.viewIdsInRange.bind(artworkBackend);
      const artworkOpen = artworkBackend.openView.bind(artworkBackend);
      window.artworkOpens = 0;
      artworkBackend.openView = async (...args) => {
        const view = await artworkOpen(...args);
        window.artworkOpens++;
        return view;
      };
      let holdNextArtworkIds = false;
      window.holdArtworkIds = () => { holdNextArtworkIds = true; window.artworkIdsPending = false; };
      artworkBackend.viewIdsInRange = async (...args) => {
        const ids = await artworkIds(...args);
        if (!holdNextArtworkIds) return ids;
        holdNextArtworkIds = false;
        window.artworkIdsPending = true;
        return new Promise(resolve => {
          window.releaseArtworkIds = async () => {
            resolve(ids);
            await new Promise(requestAnimationFrame);
            await new Promise(requestAnimationFrame);
            window.artworkIdsPending = false;
          };
        });
      };
    `;
    await route.fulfill({ response, body: fixture + await response.text() });
  });
}

for (const secondary of [false, true]) {
  for (const change of ["mode", "source", "sort", "current", "custom"] as const) {
    test(`${secondary ? "secondary" : "main"} artwork scroll lookup respects ${change} ownership`, async ({ page }) => {
      await holdArtworkLookup(page, change === "custom");
      await page.goto("/?writable");
      if (secondary) await page.getByRole("button", { name: "Sub-Browser Window" }).click();
      const owner = secondary ? page.getByRole("region", { name: "Sub-Browser" }) : page;
      const scope = secondary ? page.getByRole("region", { name: "Sub-Browser" }) : page.locator('[data-browser-pane="list"][data-browser-side="main"]');
      await owner.getByRole("treeitem", { name: /All Tracks/ }).first().click();
      const scroll = scope.getByTestId("track-scroll");
      const rows = scroll.locator('[role="row"][aria-selected]');
      await expect(rows.nth(5)).toBeVisible();
      await rows.nth(5).locator('[data-col="title"]').click();
      await expect(scroll.locator('[aria-selected="true"]')).toHaveCount(1);
      await page.evaluate(() => (window as unknown as ArtworkFixture).holdArtworkIds());
      await scope.getByTestId("artwork-mode-full").click();
      await expect.poll(() => page.evaluate(() => (window as unknown as ArtworkFixture).artworkIdsPending)).toBe(true);
      if (change === "mode") await scope.getByTestId("artwork-mode-partial").click();
      else if (change === "source") {
        await owner.getByRole("treeitem").filter({ hasText: /^\s*Artwork destination/ }).click();
        await expect(scope.getByTestId("browser-title")).toContainText("Artwork destination");
      } else if (change === "sort") {
        const opened = await page.evaluate(() => (window as unknown as ArtworkFixture).artworkOpens);
        await scope.getByRole("columnheader", { name: /^Time( Resize|$)/ }).click();
        await expect.poll(() => page.evaluate(() => (window as unknown as ArtworkFixture).artworkOpens)).toBeGreaterThan(opened);
        await page.evaluate(async () => { await new Promise(requestAnimationFrame); await new Promise(requestAnimationFrame); });
      }
      const before = await scroll.evaluate((element, retired) => {
        if (retired) {
          element.scrollTop = 73;
          element.scrollLeft = 31;
        }
        return { top: element.scrollTop, left: element.scrollLeft };
      }, change !== "current" && change !== "custom");
      if (change !== "current" && change !== "custom") expect(before.top).toBe(73);
      await page.evaluate(() => (window as unknown as ArtworkFixture).releaseArtworkIds());
      if (change === "current" || change === "custom") {
        const height = await scroll.evaluate(element => Number.parseInt(getComputedStyle(element).getPropertyValue("--s-row-height"), 10));
        await expect.poll(() => scroll.evaluate(element => element.scrollTop)).toBe(5 * height);
        await expect.poll(() => scroll.evaluate(element => element.scrollLeft)).toBe(0);
      } else {
        expect(await scroll.evaluate(element => ({ top: element.scrollTop, left: element.scrollLeft }))).toEqual(before);
      }
    });
  }
}
