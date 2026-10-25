import { expect, test, type Page } from "@playwright/test";

const quantize = (page: Page) => page.getByRole("region", { name: "Preview player", exact: true })
  .getByRole("button", { name: "Quantize" });

async function quantizeState(page: Page) {
  await expect(quantize(page)).toHaveAttribute("aria-pressed", "false");
  await expect(quantize(page)).not.toHaveAttribute("data-on");
  await expect.poll(() => page.evaluate(() =>
    JSON.parse(localStorage.getItem("rbl.deckQuantize.v1") ?? "{}") as { a?: boolean },
  )).toMatchObject({ a: false });
  return {
    pressed: await quantize(page).getAttribute("aria-pressed"),
    on: await quantize(page).getAttribute("data-on"),
    saved: await page.evaluate(() => JSON.parse(localStorage.getItem("rbl.deckQuantize.v1") ?? "{}") as { a?: boolean }),
  };
}

test("Q and the Quantize button leave the same visible and saved deck state", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();
  await expect(quantize(page)).toHaveAttribute("aria-pressed", "true");

  await page.keyboard.press("q");
  const fromShortcut = await quantizeState(page);

  await page.evaluate(() => localStorage.removeItem("rbl.deckQuantize.v1"));
  await page.reload();
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();
  await expect(quantize(page)).toHaveAttribute("aria-pressed", "true");

  await quantize(page).click();
  expect(await quantizeState(page)).toEqual(fromShortcut);
});

test("Q and the Quantize button also agree before a track loads", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await expect(quantize(page)).toHaveAttribute("aria-pressed", "true");

  await page.keyboard.press("q");
  const fromShortcut = await quantizeState(page);

  await page.evaluate(() => localStorage.removeItem("rbl.deckQuantize.v1"));
  await page.reload();
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await expect(quantize(page)).toHaveAttribute("aria-pressed", "true");

  await quantize(page).click();
  expect(await quantizeState(page)).toEqual(fromShortcut);
});

const deckAQuantize = (page: Page) => page.getByRole("region", { name: "Preview player", exact: true })
  .getByRole("button", { name: "Quantize" });
const deckB = (page: Page) => page.getByRole("region", { name: "Preview player B" });
const deckBQuantize = (page: Page) => deckB(page).getByRole("button", { name: "Quantize" });

async function openTwoPlayer(page: Page, loadB: boolean) {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.getByRole("button", { name: "Layout" }).click();
  await page.getByRole("menuitemradio", { name: "2 PLAYER" }).click();
  if (loadB) {
    await page.locator('[role="gridcell"][data-col="title"]').nth(1).click();
    await page.keyboard.press("Shift+Enter");
    await expect(deckB(page).getByTestId("player-title")).not.toHaveText("");
  } else {
    await expect(deckB(page).getByTestId("player-title")).toHaveText("");
  }
  await expect(deckAQuantize(page)).toHaveAttribute("aria-pressed", "true");
  await expect(deckAQuantize(page)).toHaveAttribute("data-on", "");
  await expect(deckBQuantize(page)).toHaveAttribute("aria-pressed", "true");
  await expect(deckBQuantize(page)).toHaveAttribute("data-on", "");
}

async function deckBQuantizeState(page: Page) {
  await expect(deckBQuantize(page)).toHaveAttribute("aria-pressed", "false");
  await expect(deckBQuantize(page)).not.toHaveAttribute("data-on");
  await expect(deckAQuantize(page)).toHaveAttribute("aria-pressed", "true");
  await expect(deckAQuantize(page)).toHaveAttribute("data-on", "");
  await expect.poll(() => page.evaluate(() =>
    JSON.parse(localStorage.getItem("rbl.deckQuantize.v1") ?? "{}") as { a?: boolean; b?: boolean },
  )).toEqual({ a: true, b: false });
  return {
    aPressed: await deckAQuantize(page).getAttribute("aria-pressed"),
    aOn: await deckAQuantize(page).getAttribute("data-on"),
    bPressed: await deckBQuantize(page).getAttribute("aria-pressed"),
    bOn: await deckBQuantize(page).getAttribute("data-on"),
    saved: await page.evaluate(() =>
      JSON.parse(localStorage.getItem("rbl.deckQuantize.v1") ?? "{}") as { a?: boolean; b?: boolean },
    ),
  };
}

for (const loadB of [false, true]) {
  test(`in 2 PLAYER, deck B's Shift+Q and button agree with ${loadB ? "loaded" : "empty"} B`, async ({ page }) => {
    await openTwoPlayer(page, loadB);
    await page.keyboard.press("Shift+q");
    const fromShortcut = await deckBQuantizeState(page);

    await page.evaluate(() => localStorage.removeItem("rbl.deckQuantize.v1"));
    await openTwoPlayer(page, loadB);
    await deckBQuantize(page).click();
    expect(await deckBQuantizeState(page)).toEqual(fromShortcut);
  });
}
