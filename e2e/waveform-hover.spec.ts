import { expect, test, type Locator, type Page } from "@playwright/test";
async function open(page: Page) {
  await page.addInitScript(() => {
    localStorage.setItem("rbl.preferences", JSON.stringify({ view: { waveformClick: false } }));
    for (const context of ["collection", "playlist", "subBrowser"]) localStorage.setItem(`rbl.columns.v2.${context}`, JSON.stringify({ order: ["title", "preview"], widths: {} }));
  });
  await page.goto(`http://localhost:${(Number(process.env.E2E_PORT) || 1420) + 1}/`);
  await expect(page.getByTestId("browser-title").first()).toContainText("Tracks)");
}
async function layout(page: Page, name: string) {
  await page.getByRole("button", { name: "Layout", exact: true }).click();
  await page.getByRole("menuitemradio", { name, exact: true }).click();
}
async function hover(page: Page, surface: Locator, fraction: number) {
  const box = await surface.boundingBox();
  if (!box) throw new Error("Waveform surface is absent");
  await page.mouse.move(box.x + box.width * fraction, box.y + box.height * .8);
  return box;
}
async function elapsed(player: Locator) {
  const text = await player.locator('[class*="elapsed"]').textContent();
  const [, minutes, seconds] = /(\d+):(\d+(?:\.\d)?)/.exec(text ?? "") ?? [];
  return Number(minutes) * 60 + Number(seconds);
}

test("browser and Sub-Browser show a destination without selecting or starting audio", async ({ page }) => {
  await open(page);
  const main = page.getByTestId("track-scroll").first();
  const canvas = main.locator("canvas[data-preview-wave]").first();
  const wave = canvas.locator("..");
  const ghost = wave.locator("[data-hover-head]");
  const box = await hover(page, canvas, .6);
  await expect(ghost).toBeVisible();
  const painted = await ghost.evaluate(node => ({ x: parseFloat((node as HTMLElement).style.left), width: node.getBoundingClientRect().width, opacity: getComputedStyle(node).opacity }));
  expect(Math.abs(painted.x - box.width * .6)).toBeLessThanOrEqual(1);
  expect(painted.width).toBe(1); expect(painted.opacity).toBe("0.5");
  await expect(main.locator('[role="row"][aria-selected="true"]')).toHaveCount(0);
  await expect(wave.getByRole("button", { name: "Stop", exact: true })).toHaveCount(0);
  await page.keyboard.press("ArrowDown");
  await expect(ghost).toBeHidden();
  await hover(page, canvas, .6); await canvas.click({ position: { x: box.width * .6, y: box.height * .8 } });
  await expect(wave.getByRole("button", { name: "Stop", exact: true })).toBeVisible();
  await hover(page, canvas, .2); await expect(ghost).toBeVisible();
  await expect(wave.locator('[class*="previewHead"]')).toHaveCount(1);
  await wave.getByRole("button", { name: "Stop", exact: true }).click();
  await expect(ghost).toBeHidden();
  await page.getByRole("button", { name: "Sub-Browser Window", exact: true }).click();
  const sub = page.getByRole("region", { name: "Sub-Browser", exact: true });
  const other = sub.locator("canvas[data-preview-wave]").first();
  await hover(page, other, .4);
  await expect(other.locator("..").locator("[data-hover-head]")).toBeVisible();
  await page.mouse.move(0, 0);
  await expect(other.locator("..").locator("[data-hover-head]")).toBeHidden();
});

for (const mode of ["1 PLAYER", "2 PLAYER", "SIMPLE PLAYER"]) {
  test(`${mode} overview hover keeps playback separate and preserves scrub gestures`, async ({ page }) => {
    await open(page);
    if (mode !== "1 PLAYER") await layout(page, mode);
    const cell = page.locator('[role="gridcell"][data-col="title"]').first();
    await cell.click(); await page.keyboard.press(mode === "2 PLAYER" ? "Shift+Enter" : "Enter");
    const player = page.getByRole("region", { name: mode === "2 PLAYER" ? "Preview player B" : "Preview player", exact: true });
    const overview = player.getByTestId(mode === "SIMPLE PLAYER" ? "simple-player-overview" : "player-overview");
    const ghost = mode === "SIMPLE PLAYER" ? overview.locator("..").locator("[data-hover-head]") : overview.locator("[data-hover-head]");
    await expect.poll(() => elapsed(player)).toBe(0);
    const box = await hover(page, overview, .7);
    await expect(ghost).toBeVisible(); await expect.poll(() => elapsed(player)).toBe(0);
    await page.mouse.down(); await expect(ghost).toBeHidden();
    await page.mouse.move(box.x + box.width * .4, box.y + box.height / 2);
    await page.mouse.up();
    await expect.poll(() => elapsed(player)).toBeGreaterThan(10);
    const kept = await elapsed(player);
    await hover(page, overview, .2); await expect(ghost).toBeVisible();
    await expect.poll(() => elapsed(player)).toBe(kept);
    if (mode !== "SIMPLE PLAYER") {
      const detail = player.getByTestId("player-detail");
      await hover(page, detail, .8);
      await expect(detail.locator("[data-hover-head]")).toHaveCount(0);
      await expect(ghost).toBeHidden(); await expect.poll(() => elapsed(player)).toBe(kept);
    }
    await page.mouse.move(0, 0); await expect(ghost).toBeHidden();
  });
}
