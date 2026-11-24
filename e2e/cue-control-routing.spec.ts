import { expect, test, type Page } from "@playwright/test";

const player = (page: Page) => page.getByRole("region", { name: "Preview player" });
const trackRows = (page: Page) => page.getByRole("row").filter({ has: page.getByRole("gridcell") });
const pad = (page: Page, letter: string) => player(page).locator('[aria-label="Hot cues"]').getByRole("button", { name: `Hot cue ${letter}`, exact: true });

async function start(page: Page) {
  await page.addInitScript(() => localStorage.setItem("rbl.preferences", JSON.stringify({ view: { tooltips: true } })));
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
}

async function assertCueWritesBlocked(page: Page) {
  await expect(player(page).getByRole("button", { name: "Play", exact: true })).toBeEnabled();
  await expect(pad(page, "E")).toBeDisabled();
  await expect(pad(page, "E")).toHaveAttribute("title", "That track is not in the library.");
  const store = player(page).getByRole("button", { name: "Set memory cue", exact: true });
  await expect(store).toBeDisabled();
  await expect(store).toHaveAttribute("title", "That track is not in the library.");
  // Shortcuts go through the hooks too, so a disabled button cannot be bypassed.
  await page.keyboard.press("3");
  await page.keyboard.press("m");
  await expect(page.getByRole("contentinfo")).not.toContainText("no track file:");
}

async function loadDevice(page: Page) {
  await start(page);
  await page.getByRole("tablist", { name: "Library sources" }).getByRole("tab", { name: "Devices" }).click();
  await page.getByRole("button", { name: "Expand TEST" }).dispatchEvent("mousedown");
  await page.locator('[role="treeitem"][data-kind="deviceAllTracks"]').first().click();
  await expect(trackRows(page)).toHaveCount(6);
  await trackRows(page).first().locator('[role="gridcell"][data-col="title"]').dblclick();
}

test("device cue writes stay disabled while device grid editing remains available", async ({ page }) => {
  await loadDevice(page);
  await assertCueWritesBlocked(page);
  await player(page).getByRole("tab", { name: "GRID", exact: true }).click();
  await expect(player(page).getByRole("button", { name: "Double the tempo", exact: true })).toBeEnabled();
  await page.getByRole("button", { name: "Layout", exact: true }).click();
  await page.getByRole("menuitemradio", { name: "2 PLAYER", exact: true }).click();
  const dual = page.getByTestId("player-controls").first().getByRole("button", { name: "Set memory cue", exact: true });
  await expect(dual).toBeDisabled();
  await expect(dual).toHaveAttribute("title", "That track is not in the library.");
});

test("an unregistered Explorer file cannot write collection cues", async ({ page }) => {
  await start(page);
  await page.getByRole("tablist", { name: "Library sources" }).getByRole("tab", { name: "Explorer" }).click();
  const tree = page.getByRole("navigation", { name: "Library" });
  await tree.getByRole("button", { name: "Expand Music" }).click();
  await tree.getByRole("treeitem", { name: /Downloads$/ }).click();
  await expect(trackRows(page)).toHaveCount(12);
  const loose = trackRows(page).nth(6);
  await expect(loose).toContainText("Untitled Bounce 1");
  await loose.locator('[role="gridcell"][data-col="title"]').dblclick();
  await assertCueWritesBlocked(page);
});


test("saved file cues still play but never open collection-edit menus", async ({ page }) => {
  await loadDevice(page);
  // The default mock has no device cues. Supply a read-only cue fixture
  // after the player is loaded and force its normal focus refresh.
  await page.evaluate(async () => {
    const moduleUrl = performance.getEntriesByType("resource").map(entry => entry.name)
      .find(url => url.includes("/src/ipc/client.ts"));
    if (!moduleUrl) throw new Error("The active backend module was not loaded.");
    const { getBackend } = await import(/* @vite-ignore */ moduleUrl) as typeof import("../src/ipc/client");
    const backend = await getBackend();
    const reads: string[] = [];
    const writes: string[] = [];
    Object.assign(window, { fileCueFixture: { reads, writes } });
    backend.trackCues = id => {
      reads.push(id);
      return Promise.resolve([
        { id: "", letter: "A", memory: false, positionMs: 1000, outMs: 0, colour: null },
        { id: "", letter: "", memory: true, positionMs: 2000, outMs: 0, colour: null },
      ]);
    };
    for (const name of ["addCue", "addLoop", "deleteCue", "moveHotCue", "setCueColour", "setCueComment", "setActiveLoop"] as const) {
      const original = backend.edits[name].bind(backend.edits) as (...args: unknown[]) => Promise<unknown>;
      Object.assign(backend.edits, { [name]: (...args: unknown[]) => { writes.push(name); return original(...args); } });
    }
    window.dispatchEvent(new Event("focus"));
  });
  await expect(pad(page, "A")).toHaveAttribute("aria-pressed", "true");
  const evidence = () => page.evaluate(() => (window as unknown as { fileCueFixture: { reads: string[]; writes: string[] } }).fileCueFixture);
  expect((await evidence()).reads.some(id => id.startsWith("file:"))).toBe(true);
  await expect(pad(page, "A")).toBeEnabled();
  await pad(page, "A").click({ button: "right" });
  await expect(page.getByRole("menu", { name: "Hot cue color" })).toHaveCount(0);
  const side = page.getByRole("complementary", { name: "Cue list" });
  await side.getByRole("tab", { name: "HOT CUE", exact: true }).click();
  const hot = side.getByRole("button", { name: "Hot cue A", exact: true });
  await hot.click({ button: "right" });
  await expect(page.getByRole("menu", { name: "Hot cue color" })).toHaveCount(0);
  await hot.click();
  await expect(player(page).getByRole("button", { name: "Pause", exact: true })).toBeEnabled();
  await player(page).getByRole("button", { name: "Pause", exact: true }).click();
  await side.getByRole("tab", { name: "MEMORY", exact: true }).click();
  const memory = side.locator('[role="button"]').filter({ hasText: "00:02:000" });
  await memory.click({ button: "right" });
  await expect(page.getByRole("menu", { name: "Memory cue color" })).toHaveCount(0);
  await memory.click();
  await expect(player(page).locator('[class*="elapsed"]')).toContainText("00:02");
  await page.keyboard.press("3");
  await page.keyboard.press("m");
  expect((await evidence()).writes).toEqual([]);
});
