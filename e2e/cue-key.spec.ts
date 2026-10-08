/**
 * The C key is the CUE button, and CUE is a held control.
 *
 * rekordbox's own Export preset maps it — `<MAPPING commandId="3007"
 * description="Cue" key="C"/>` — and on the hardware holding CUE at the cue
 * point plays for as long as it is down and snaps back when it comes up. The
 * mouse button had that from the start, through `onPointerDown`/`onPointerUp`.
 * The key did both on the way down, so it was the one control that could not
 * preview, and auto-repeat then ran the pair for as long as the key was held.
 */
import { expect, test } from "@playwright/test";

test("holding C previews from the cue point and lets go back to it", async ({ page }) => {
  await page.goto("/");
  await page.locator('[role="gridcell"][data-col="title"]').first().dblclick();

  const clock = page.getByTestId("player-time");
  const play = page.getByRole("button", { name: "Play", exact: true });
  await expect(play).toBeEnabled();

  // Somewhere past the cue point, then back to it: the deck is now paused on
  // the cue, which is the state CUE previews from.
  await play.click();
  await page.waitForTimeout(600);
  await page.getByRole("button", { name: "Pause", exact: true }).click();
  await page.keyboard.press("c");
  const atCue = await clock.textContent();

  // Held: the deck runs.
  await page.keyboard.down("c");
  await expect(clock).not.toHaveText(atCue ?? "");

  // Released: back where it started, and stopped.
  await page.keyboard.up("c");
  await expect(clock).toHaveText(atCue ?? "");
  await expect(page.getByRole("button", { name: "Play", exact: true })).toBeVisible();
});

test("the repeats a held key sends are ignored", async ({ page }) => {
  // Auto-repeat sends a keydown about thirty times a second while a key is
  // held. Each one used to run a press *and* a release, so the deck seeked and
  // toggled on every repeat — a stutter, and a seek per repeat sent to the
  // backend. Playwright's `keyboard.down` emits no repeats of its own, so they
  // are dispatched here to exercise the guard rather than hoped for.
  await page.goto("/");
  await page.locator('[role="gridcell"][data-col="title"]').first().dblclick();
  const play = page.getByRole("button", { name: "Play", exact: true });
  await expect(play).toBeEnabled();

  const clock = page.getByTestId("player-time");
  await play.click();
  await page.waitForTimeout(500);
  await page.getByRole("button", { name: "Pause", exact: true }).click();
  await page.keyboard.press("c");
  const atCue = await clock.textContent();

  await page.keyboard.down("c");
  await expect(clock).not.toHaveText(atCue ?? "");

  // What the keyboard would be sending all this time.
  await page.evaluate(() => {
    for (let i = 0; i < 20; i++) {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "c", repeat: true, bubbles: true }));
    }
  });
  const during = await clock.textContent();
  await page.waitForTimeout(400);
  // Still running forward, not yanked back to the cue by every repeat.
  await expect(clock).not.toHaveText(during ?? "");

  await page.keyboard.up("c");
  await expect(clock).toHaveText(atCue ?? "");
});

test("a preview ends even if the key is let go somewhere else", async ({ page }) => {
  // The press is guarded against firing while something is being typed into,
  // and the release deliberately is not: hold C, click into the search box,
  // let go there, and the deck still has to stop. Otherwise the preview runs
  // on with the key that started it already up.
  await page.goto("/");
  await page.locator('[role="gridcell"][data-col="title"]').first().dblclick();
  const play = page.getByRole("button", { name: "Play", exact: true });
  await expect(play).toBeEnabled();

  const clock = page.getByTestId("player-time");
  await play.click();
  await page.waitForTimeout(500);
  await page.getByRole("button", { name: "Pause", exact: true }).click();
  await page.keyboard.press("c");
  const atCue = await clock.textContent();

  await page.keyboard.down("c");
  await expect(clock).not.toHaveText(atCue ?? "");

  // The focus moves while the key is still down.
  await page.getByRole("searchbox", { name: /search within/i }).focus();
  await page.keyboard.up("c");

  await expect(clock).toHaveText(atCue ?? "");
  await expect(page.getByRole("button", { name: "Play", exact: true })).toBeVisible();
});

test("pressing Space while C is held keeps playing after C is released", async ({ page }) => {
  // Hold CUE, press PLAY, let go of CUE: the preview latches into playback.
  // rekordbox 7 does this with the C key and with the on-screen CUE held by
  // the mouse [OBS chris-win11 2026-10-08: 00:01.7 held, Space, released,
  // then 00:04.9 -> 00:07.0 -> 00:09.1 still playing]. Releasing without PLAY
  // still snaps back (first test above).
  await page.goto("/");
  await page.locator('[role="gridcell"][data-col="title"]').first().dblclick();
  const play = page.getByRole("button", { name: "Play", exact: true });
  await expect(play).toBeEnabled();

  const clock = page.getByTestId("player-time");
  await play.click();
  await page.waitForTimeout(500);
  await page.getByRole("button", { name: "Pause", exact: true }).click();
  await page.keyboard.press("c");
  const atCue = await clock.textContent();

  await page.keyboard.down("c");
  await expect(clock).not.toHaveText(atCue ?? "");
  await page.keyboard.press("Space");
  await page.keyboard.up("c");

  // Still playing, past the cue point.
  await expect(page.getByRole("button", { name: "Pause", exact: true })).toBeVisible();
  const after = await clock.textContent();
  await page.waitForTimeout(400);
  await expect(clock).not.toHaveText(after ?? "");
  await expect(clock).not.toHaveText(atCue ?? "");
});

test("Space after a held preview ran out at the end stays there when C is released", async ({ page }) => {
  // rekordbox 7 [OBS chris-win11 2026-10-08]: cue set at 02:51.1 of 02:52.4,
  // C held past the end, Space, C released: the deck stays at 02:52.4. With
  // no Space, the same release goes back to 02:51.1.
  await page.goto("/");
  await page.locator('[role="gridcell"][data-col="title"]').first().dblclick();
  const play = page.getByRole("button", { name: "Play", exact: true });
  await expect(play).toBeEnabled();

  // A cue point a moment before the end: seek there, then C sets it.
  const overview = await page.getByTestId("player-overview").boundingBox();
  await page.mouse.click(
    (overview?.x ?? 0) + (overview?.width ?? 0) * 0.997,
    (overview?.y ?? 0) + (overview?.height ?? 0) / 2,
  );
  const clock = page.getByTestId("player-time");
  await page.keyboard.press("c");
  const atCue = await clock.textContent();

  // Held until the track runs out and the deck stops by itself.
  await page.keyboard.down("c");
  await expect(page.getByRole("button", { name: "Pause", exact: true })).toBeVisible();
  await expect(play).toBeVisible({ timeout: 10_000 });
  const atEnd = await clock.textContent();
  expect(atEnd).not.toBe(atCue);

  await page.keyboard.press("Space");
  await page.keyboard.up("c");
  await page.waitForTimeout(300);
  await expect(clock).toHaveText(atEnd ?? "");
});
