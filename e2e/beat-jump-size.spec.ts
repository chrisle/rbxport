/**
 * The beat-jump size each deck's menu picks comes back after a restart, per
 * deck, the way the waveform zoom does: it is written to `rbl.session` as it
 * changes and read back when the window opens. With DUAL CONTROL on the two
 * decks share one size, and both come back with it.
 *
 * A reload stands in for the restart: the shell's window is a WebView, and
 * what it keeps between runs is this origin's localStorage.
 */
import { expect, test, type Page } from "@playwright/test";

/** The stored sizes, as the session on disk has them. */
const storedSizes = (page: Page) =>
  page.evaluate(() => {
    const raw = localStorage.getItem("rbl.session") ?? "{}";
    return (JSON.parse(raw) as { jumpSize?: { a?: string; b?: string } }).jumpSize;
  });

/** Opens the deck's size menu and picks a size by its label. */
async function pick(page: Page, deck: ReturnType<Page["getByRole"]>, label: string) {
  await deck.getByRole("button", { name: "Beat jump size" }).click();
  await page.getByRole("menu", { name: "Beat jump size" })
    .getByRole("menuitemradio", { name: label, exact: true }).click();
}

/**
 * Both decks on screen, nothing loaded: the size menu needs no track. In
 * this layout each deck's transport, size button included, sits in the
 * shared strip between the decks rather than inside the deck's own region.
 */
async function twoPlayer(page: Page) {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.getByRole("button", { name: "Layout" }).click();
  await page.getByRole("menuitemradio", { name: "2 PLAYER" }).click();
  const a = page.getByRole("group", { name: "Deck A transport" });
  const b = page.getByRole("group", { name: "Deck B transport" });
  await expect(b.getByRole("button", { name: "Beat jump size" })).toBeVisible();
  return { a, b };
}

test("each deck's jump size comes back after a restart, the other deck's untouched", async ({ page }) => {
  const { a, b } = await twoPlayer(page);
  await expect(a.getByRole("button", { name: "Beat jump size" })).toHaveText("4Beats");

  await pick(page, a, "8Bars");
  await expect(a.getByRole("button", { name: "Beat jump size" })).toHaveText("8Bars");
  await expect(b.getByRole("button", { name: "Beat jump size" })).toHaveText("4Beats");
  await expect.poll(() => storedSizes(page)).toEqual({ a: "8bars", b: "4beats" });

  // The layout is remembered too, so the window reopens with both decks.
  await page.reload();
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await expect(a.getByRole("button", { name: "Beat jump size" })).toHaveText("8Bars");
  await expect(b.getByRole("button", { name: "Beat jump size" })).toHaveText("4Beats");
});

test("DUAL CONTROL's shared jump size comes back on both decks", async ({ page }) => {
  const { a, b } = await twoPlayer(page);
  await pick(page, a, "8Bars");
  await expect.poll(() => storedSizes(page)).toEqual({ a: "8bars", b: "4beats" });

  // Linking takes deck A's size to both, as it does the zoom.
  await page.getByRole("button", { name: "Dual control" }).click();
  await expect(b.getByRole("button", { name: "Beat jump size" })).toHaveText("8Bars");

  // A pick on either deck is written to both.
  await pick(page, b, "16Bars");
  await expect(a.getByRole("button", { name: "Beat jump size" })).toHaveText("16Bars");
  await expect(b.getByRole("button", { name: "Beat jump size" })).toHaveText("16Bars");
  await expect.poll(() => storedSizes(page)).toEqual({ a: "16bars", b: "16bars" });

  await page.reload();
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await expect(page.getByRole("button", { name: "Dual control" })).toHaveAttribute("aria-pressed", "true");
  await expect(a.getByRole("button", { name: "Beat jump size" })).toHaveText("16Bars");
  await expect(b.getByRole("button", { name: "Beat jump size" })).toHaveText("16Bars");
});
