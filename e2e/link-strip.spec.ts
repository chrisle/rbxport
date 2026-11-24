/**
 * The Pro DJ LINK strip along the bottom, against the mock's `?link=` network
 * (a browser has none of its own).
 *
 * The shape it must keep, from docs/screenshots 2026-09-13: nothing at all
 * until a device is heard; then a full-height strip with the LINK button alone
 * at its left; then, once on, a deck per player with the mixer between them,
 * each deck taking a dropped track.
 */
import { expect, test } from "@playwright/test";

test("draws no strip until a device is heard", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("grid")).toBeVisible();
  await expect(page.getByTestId("link-deck-strip")).toBeHidden();
});

test("is the LINK button alone in a full-height strip before LINK is on", async ({ page }) => {
  await page.goto("/?link=detected");
  await expect(page.getByRole("grid")).toBeVisible();

  const strip = page.getByTestId("link-deck-strip");
  await expect(strip).toBeVisible();
  await expect(page.getByTestId("link-button")).toHaveAttribute("aria-pressed", "false");
  // The height is carried whether or not LINK is on.
  expect((await strip.boundingBox())?.height).toBeGreaterThan(60);
  // No decks yet.
  await expect(page.getByLabel("Players on the link")).toBeHidden();
});

test("clicking LINK joins the network and shows the players either side of the mixer", async ({ page }) => {
  await page.goto("/?link=detected");
  await expect(page.getByRole("grid")).toBeVisible();
  await page.getByTestId("link-button").click();

  await expect(page.getByTestId("link-button")).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByLabel("Player 1")).toBeVisible();
  await expect(page.getByLabel("Player 2")).toBeVisible();
  await expect(page.getByLabel("Mixer 33")).toBeVisible();
  // rekordbox seats the mixer between the decks. Scoped to the strip: the
  // preview deck's own menu is labelled "Player menu" and would match too.
  const order = await page
    .getByTestId("link-deck-strip")
    .locator("[aria-label^='Player '], [aria-label^='Mixer ']")
    .evaluateAll((els) => els.map((el) => el.getAttribute("aria-label")));
  expect(order).toEqual(["Player 1", "Mixer 33", "Player 2"]);
});

test("the mixer's MASTER and LINK CUE lamps share one compact row", async ({ page }) => {
  await page.goto("/?link=on");
  await expect(page.getByRole("grid")).toBeVisible();

  const mixer = page.getByLabel("Mixer 33");
  const master = mixer.getByText("MASTER", { exact: true });
  const linkCue = mixer.getByText("LINK CUE", { exact: true });
  const [masterBox, cueBox, sameParent] = await Promise.all([
    master.boundingBox(),
    linkCue.boundingBox(),
    mixer.evaluate((element) => {
      const spans = [...element.querySelectorAll("span")];
      const masterLamp = spans.find((span) => span.textContent === "MASTER");
      const cueLamp = spans.find((span) => span.textContent === "LINK CUE");
      return masterLamp?.parentElement === cueLamp?.parentElement;
    }),
  ]);

  expect(masterBox).not.toBeNull();
  expect(cueBox).not.toBeNull();
  expect(sameParent).toBe(true);
  expect(masterBox?.y).toBeCloseTo(cueBox?.y ?? 0, 0);
  expect(masterBox?.x ?? 0).toBeLessThan(cueBox?.x ?? 0);
});

test("a deck takes a track dragged from the library; the mixer does not", async ({ page }) => {
  await page.goto("/?link=on");
  await expect(page.getByRole("grid")).toBeVisible();

  const row = page.getByRole("row").filter({ has: page.getByRole("gridcell") }).nth(1);
  const deck = page.getByLabel("Player 1");
  await expect(deck).not.toHaveAttribute("data-drop-target", "true");

  await row.hover();
  await page.mouse.down();
  await deck.hover();
  await expect(deck).toHaveAttribute("data-drop-target", "true");
  // A mixer cannot be loaded, so it never becomes a target.
  await expect(page.getByLabel("Mixer 33")).not.toHaveAttribute("data-drop-target", "true");
  await page.mouse.up();
  // The drop is over before the test is: WebKit's browser went down when the
  // context closed with the drag still ending, and took the next test with it.
  await expect(deck).not.toHaveAttribute("data-drop-target", "true");
});

test("a split-view playlist drag loads only the chosen LINK player", async ({ page }) => {
  await page.goto("/?link=on");
  await page.getByRole("button", { name: "Sub-Browser Window" }).click();
  const panel = page.getByRole("region", { name: "Sub-Browser" });
  const row = panel.getByRole("grid").locator('[role="row"]:has([role="gridcell"])').first();
  await expect(row).toBeVisible();
  await page.evaluate(async () => {
    const moduleUrl = "/src/ipc/client.ts";
    const { getBackend } = await import(moduleUrl) as typeof import("../src/ipc/client");
    const backend = await getBackend();
    const loads: { player: number; id: string }[] = [];
    Object.assign(window, { linkDropEvidence: { loads, accepted: false, payload: "" } });
    const original = backend.loadTrackOnLink.bind(backend);
    backend.loadTrackOnLink = (player, id) => { loads.push({ player, id }); return original(player, id); };
    document.addEventListener("dragover", (event) => {
      if ((event.target as Element).closest('[aria-label="Player 2"]') && event.defaultPrevented) {
        (window as unknown as { linkDropEvidence: { accepted: boolean } }).linkDropEvidence.accepted = true;
      }
    });
    document.addEventListener("drop", (event) => {
      if ((event.target as Element).closest('[aria-label="Player 2"]')) {
        (window as unknown as { linkDropEvidence: { payload: string } }).linkDropEvidence.payload = event.dataTransfer?.getData("text/plain") ?? "";
      }
    }, true);
  });
  await row.locator('[data-col="title"]').hover({ position: { x: 8, y: 8 } });
  await page.mouse.down();
  const player = page.getByLabel("Player 2", { exact: true });
  try {
    await player.hover();
    await player.hover({ position: { x: 10, y: 10 } });
    await expect(player).toHaveAttribute("data-drop-target", "true");
    await expect(page.getByLabel("Mixer 33")).not.toHaveAttribute("data-drop-target", "true");
  } finally { await page.mouse.up(); }
  const evidence = await page.evaluate(() => (window as unknown as {
    linkDropEvidence: { loads: { player: number; id: string }[]; accepted: boolean; payload: string };
  }).linkDropEvidence);
  expect(evidence.accepted).toBe(true);
  expect(evidence.payload).toMatch(/^\d+$/);
  expect(evidence.loads).toEqual([{ player: 2, id: evidence.payload }]);
});
