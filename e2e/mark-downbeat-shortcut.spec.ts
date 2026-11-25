import { expect, test, type Page } from "@playwright/test";

async function count(page: Page) {
  return page.evaluate(() => (window as unknown as { __marks: unknown[] }).__marks.length);
}
async function protectedPreference(page: Page, protectLibrary: boolean) {
  await page.evaluate(protect => {
    const current = JSON.parse(localStorage.getItem("rbl.preferences") ?? "{}");
    localStorage.setItem("rbl.preferences", JSON.stringify({ ...current, advanced: { ...current.advanced, protectLibrary: protect } }));
    window.dispatchEvent(new StorageEvent("storage", { key: "rbl.preferences" }));
  }, protectLibrary);
}

test("assigned downbeat keys reuse the existing button, deck scope and edit guards", async ({ page }) => {
  await page.addInitScript(() => {
    localStorage.setItem("rbl.preferences", JSON.stringify({ advanced: { protectLibrary: false }, keyboard: {
      preset: "export", overrides: { "a.markDownbeat": { key: "F8" }, "b.markDownbeat": { key: "F8", shiftKey: true } },
    } }));
  });
  await page.route("**/src/main.tsx*", async route => {
    const response = await route.fetch();
    const probe = `
      import { getBackend as markFixture } from "/src/ipc/client.ts";
      const backend = await markFixture();
      window.__marks = [];
      const edit = backend.edits.gridEdit.bind(backend.edits);
      backend.edits.gridEdit = (...args) => { window.__marks.push(args); return edit(...args); };
    `;
    await route.fulfill({ response, body: probe + await response.text() });
  });
  await page.goto("/?writable=1");
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();
  const a = page.getByRole("region", { name: "Preview player", exact: true });
  await expect(a.getByRole("button", { name: "Play", exact: true })).toBeEnabled();
  await a.getByRole("tab", { name: "GRID", exact: true }).click();
  const button = (name: string) => a.getByRole("button", { name, exact: true });
  const mark = button("Mark the downbeat here");
  await expect(mark).toBeEnabled();
  await mark.click();
  await expect.poll(() => count(page)).toBe(1);
  await page.keyboard.press("F8");
  await expect.poll(() => count(page)).toBe(2);
  const calls = await page.evaluate(() => (window as unknown as { __marks: unknown[][] }).__marks);
  expect(calls[1]).toEqual(calls[0]);
  expect(calls[1]?.[1]).toMatchObject({ kind: "downbeat" });
  expect(calls[1]?.[2]).toMatchObject({ deck: "a" });
  await page.keyboard.down("F8"); await page.keyboard.down("F8"); await page.keyboard.up("F8");
  await expect.poll(() => count(page)).toBe(3); // auto-repeat is not another mark.

  await button("Lock the grid").click();
  await expect(mark).toBeDisabled();
  await page.keyboard.press("F8");
  expect(await count(page)).toBe(3);
  await button("Lock the grid").click();
  await button("Adjust beats from here").click();
  await expect(mark).toBeDisabled();
  await page.keyboard.press("F8");
  expect(await count(page)).toBe(4);
  await button("Adjust all beats").click();
  await protectedPreference(page, true);
  await expect(mark).toBeDisabled();
  await page.keyboard.press("F8");
  expect(await count(page)).toBe(4);
  await protectedPreference(page, false);
  await expect(mark).toBeEnabled();

  await page.getByRole("button", { name: "Layout", exact: true }).click();
  await page.getByRole("menuitemradio", { name: "2 PLAYER", exact: true }).click();
  await page.keyboard.press("Shift+F8");
  expect(await count(page)).toBe(4); // The still-empty B cannot edit.
  await page.locator('[role="gridcell"][data-col="title"]').nth(4).click();
  await page.keyboard.press("Shift+Enter");
  const b = page.getByRole("region", { name: "Preview player B", exact: true });
  await expect(b.getByRole("button", { name: "Mark the downbeat here", exact: true })).toBeEnabled();
  await page.keyboard.press("Shift+F8");
  await expect.poll(() => count(page)).toBe(5);
  const last = await page.evaluate(() => (window as unknown as { __marks: unknown[][] }).__marks.at(-1));
  expect(last?.[0]).not.toBe(calls[0]?.[0]);
  expect(last?.[1]).toMatchObject({ kind: "downbeat" });
  expect(last?.[2]).toMatchObject({ deck: "b" });
  await page.getByRole("button", { name: "Layout", exact: true }).click();
  await page.getByRole("menuitemradio", { name: "1 PLAYER", exact: true }).click();
  await page.keyboard.press("Shift+F8");
  expect(await count(page)).toBe(5); // Hidden B has no registered handler.
});
