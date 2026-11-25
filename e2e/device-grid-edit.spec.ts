import { expect, test, type Page } from "@playwright/test";

/**
 * The GRID panel over a track loaded from a stick's Devices tree (#660).
 *
 * rekordbox edits a device track's beat grid and saves it to the device
 * (`BeatGridAdjustment::saveBeatPosition` sends it to the device's library
 * and analysis files). The mock's TEST stick holds analysed tracks, so the
 * panel is live for them; the lock stays off, since rbxport keeps Analysis
 * Lock for the collection.
 */

const player = (page: Page) => page.getByRole("region", { name: "Preview player" });
const gridPanel = (page: Page) => player(page).getByRole("region", { name: "Beat grid" });
const button = (page: Page, label: string) => gridPanel(page).getByRole("button", { name: label, exact: true });
const bpmField = (page: Page) => page.getByTestId("grid-bpm");
const rows = (page: Page) => page.getByRole("row").filter({ has: page.getByRole("gridcell") });

async function loadFromStick(page: Page) {
  await page.addInitScript(() => localStorage.setItem("rbl.preferences", JSON.stringify({ view: { tooltips: true } })));
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.getByRole("tablist", { name: "Library sources" }).getByRole("tab", { name: "Devices" }).click();
  await page.getByRole("button", { name: "Expand TEST" }).dispatchEvent("mousedown");
  await page.locator('[role="treeitem"][data-kind="deviceAllTracks"]').first().click();
  await expect(rows(page)).toHaveCount(6);
  await rows(page).first().locator('[role="gridcell"][data-col="title"]').dblclick();
  await expect(player(page).getByRole("button", { name: "Play", exact: true })).toBeEnabled();
  await player(page).getByRole("tab", { name: "GRID" }).click();
  await expect(gridPanel(page)).toBeVisible();
}

test("a track from the Devices tree has its beat grid edited, undone and redone", async ({ page }) => {
  await loadFromStick(page);
  await expect(bpmField(page)).toHaveValue("128.00");
  await expect(button(page, "Double the tempo")).toBeEnabled();

  await button(page, "Double the tempo").click();
  await expect(bpmField(page)).toHaveValue("256.00");
  await button(page, "Undo the last grid edit").click();
  await expect(bpmField(page)).toHaveValue("128.00");
  await button(page, "Redo the last grid edit").click();
  await expect(bpmField(page)).toHaveValue("256.00");
});

test("a track from the Devices tree offers no Analysis Lock", async ({ page }) => {
  await loadFromStick(page);
  await expect(button(page, "Lock the grid")).toBeDisabled();
});

// The coalesced nudge uses a separate save path from tempo edits.
test("a device grid shift saves to its loaded source with undo and redo", async ({ page }) => {
  await loadFromStick(page);
  await expect(button(page, "Undo the last grid edit")).toBeDisabled();
  await button(page, "Shift the grid later").click();
  await expect(button(page, "Undo the last grid edit")).toBeEnabled();
  await button(page, "Undo the last grid edit").click();
  await expect(button(page, "Undo the last grid edit")).toBeDisabled();
  await button(page, "Redo the last grid edit").click();
  await expect(button(page, "Undo the last grid edit")).toBeEnabled();
});

test("the same audio does not show the other device library's grid while loading", async ({ page }) => {
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.evaluate(async () => {
    const moduleUrl = "/src/ipc/client.ts";
    const { getBackend } = await import(moduleUrl) as typeof import("../src/ipc/client");
    const backend = await getBackend();
    const original = backend.trackBeats.bind(backend);
    const originalLoad = backend.deckLoad.bind(backend);
    // The mock decoder knows Collection IDs. Give this loose device audio
    // the existing fixture's duration and normal deck load events.
    backend.deckLoad = (deck, id, loadId) => originalLoad(deck, id.startsWith("file:") ? "100000" : id, loadId);
    const sourceIds: string[] = [];
    let finishOneLibrary!: () => void;
    const oneLibraryReady = new Promise<void>(resolve => { finishOneLibrary = resolve; });
    Object.assign(window, { gridAliasFixture: { sourceIds, finishOneLibrary, deckState: backend.deckState.bind(backend) } });
    backend.trackBeats = async (id, source) => {
      if (!source) return original(id, source);
      sourceIds[source.format === "deviceLibrary" ? 0 : 1] = id;
      if (source.format === "oneLibrary") await oneLibraryReady;
      const data = new Uint8Array(7);
      const beat = new DataView(data.buffer);
      beat.setUint32(0, source.format === "deviceLibrary" ? 421 : 689, true);
      beat.setUint8(4, 1);
      beat.setUint16(5, 12800, true);
      return data;
    };
  });
  const fixture = () => page.evaluate(async () => {
    const current = (window as unknown as { gridAliasFixture: {
      sourceIds: string[]; deckState: import("../src/ipc/types").Backend["deckState"];
    } }).gridAliasFixture;
    const { a } = await current.deckState();
    return { sourceIds: current.sourceIds, loadId: a.loadId, loaded: a.loaded && a.totalFrames > 0 };
  });
  await page.getByRole("tablist", { name: "Library sources" }).getByRole("tab", { name: "Devices" }).click();
  await page.getByRole("button", { name: "Expand TEST" }).dispatchEvent("mousedown");
  const libraries = page.locator('[role="treeitem"][data-kind="deviceAllTracks"]');
  await expect(libraries).toHaveCount(2);
  const load = async (index: number) => {
    await libraries.nth(index).click();
    await expect(rows(page)).toHaveCount(6);
    await rows(page).first().locator('[data-col="title"]').dblclick();
  };
  await load(0);
  const markers = page.getByTestId("beat-grid-marker");
  await expect(markers).toHaveAttribute("data-beat-ms", "421");
  const first = await fixture();
  const firstAudio = first.sourceIds[0];
  expect(firstAudio).toBeTruthy();

  await load(1);
  await expect.poll(async () => {
    const next = await fixture();
    return { sourceIds: next.sourceIds, ready: next.loaded && next.loadId !== first.loadId };
  }).toEqual({ sourceIds: [firstAudio, firstAudio], ready: true });
  await expect(markers).toHaveCount(0);
  await page.evaluate(() => (window as unknown as { gridAliasFixture: { finishOneLibrary: () => void } }).gridAliasFixture.finishOneLibrary());
  await expect(markers).toHaveAttribute("data-beat-ms", "689");
});
