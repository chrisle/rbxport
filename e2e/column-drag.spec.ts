/**
 * Dragging a column heading (#207). rekordbox moves the columns while the
 * heading is held and floats a copy of it under the pointer, so where it will
 * land is on show before it is let go; the order it ends in is remembered.
 */
import { expect, test, type Page } from "@playwright/test";

const headings = (page: Page) =>
  page.locator('[role="row"] [role="columnheader"]').allInnerTexts();

test("a dragged heading floats with the pointer and the columns make room as it goes", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  const before = await headings(page);
  const keyAt = before.indexOf("Key");
  expect(before[keyAt + 1]).toBe("BPM");

  const key = page.locator('[role="row"] [role="columnheader"]', { hasText: /^Key$/ });
  const box = await key.boundingBox();
  if (!box) throw new Error("the Key heading has no box");
  const y = box.y + box.height / 2;
  await page.mouse.move(box.x + box.width / 2, y);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2 + 60, y, { steps: 4 });
  await page.mouse.move(box.x + box.width / 2 + 140, y, { steps: 4 });

  // Held: a copy of the heading is under the pointer, and Key has already
  // moved past BPM.
  const ghost = page.getByTestId("column-drag-ghost");
  await expect(ghost).toHaveText("Key");
  const ghostBox = await ghost.boundingBox();
  expect(Math.abs((ghostBox?.x ?? 0) - (box.x + 140))).toBeLessThan(4);
  const held = await headings(page);
  expect(held.indexOf("Key")).toBeGreaterThan(held.indexOf("BPM"));

  await page.mouse.up();
  await expect(ghost).toHaveCount(0);
  const after = await headings(page);
  expect(after.indexOf("Key")).toBeGreaterThan(after.indexOf("BPM"));

  // And remembered.
  await page.reload();
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  const reloaded = await headings(page);
  expect(reloaded.indexOf("Key")).toBeGreaterThan(reloaded.indexOf("BPM"));
});
