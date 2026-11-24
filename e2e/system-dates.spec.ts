import { expect, test } from "@playwright/test";

test("table and Info retain batched native short-date labels verbatim", async ({ page }) => {
  // Represents native response labels, deliberately different from the
  // browser's locale. This is presentation wiring, not a native OS probe.
  await page.route("**/src/main.tsx", async (route) => {
    const response = await route.fetch();
    const fixture = `
      import { getBackend as dateBackend } from "/src/ipc/client.ts";
      const dates = await dateBackend();
      const fetch = dates.fetchRows.bind(dates);
      dates.fetchRows = async (...args) => {
        window.__datePages = [...(window.__datePages ?? []), { len: args[2], columns: args[3] ?? [] }];
        return (await fetch(...args)).map(row => ({ ...row, dateAdded: "2025-03-04", dateLabels: { ...row.dateLabels, dateAdded: "Play" } }));
      };
      const details = dates.trackDetails.bind(dates);
      dates.trackDetails = async (id) => ({ ...await details(id), dateCreated: "2025-03-04", dateCreatedLabel: "Play" });
    `;
    await route.fulfill({ response, body: fixture + await response.text() });
  });
  await page.goto("/?writable");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  const date = page.locator('[role="gridcell"][data-col="dateAdded"]').nth(2);
  await expect(date).toHaveText("Play");
  await page.locator('[role="gridcell"][data-col="title"]').nth(2).click();
  await page.evaluate(() => (window as unknown as { __menu: (id: string) => void }).__menu("info"));
  const panel = page.getByRole("complementary", { name: "Information" });
  await expect(panel).toBeVisible();
  await expect(panel.getByText("Play", { exact: true })).toBeVisible();
  await page.evaluate(() => {
    const preferences = JSON.parse(localStorage.getItem("rbl.preferences") ?? "{}");
    localStorage.setItem("rbl.preferences", JSON.stringify({ ...preferences, view: { ...preferences.view, locale: "de", tooltips: true } }));
    window.dispatchEvent(new StorageEvent("storage", { key: "rbl.preferences" }));
  });
  await expect(page.locator("html")).toHaveAttribute("lang", "de");
  await expect(page.getByRole("columnheader", { name: /^Trackname/ })).toBeVisible();
  await expect(date).toHaveText("Play");
  await expect(panel.getByText("Play", { exact: true })).toBeVisible();
  await expect(panel.getByText("Play", { exact: true })).toHaveAttribute("title", "Play");
  await page.evaluate(() => {
    const preferences = JSON.parse(localStorage.getItem("rbl.preferences") ?? "{}");
    localStorage.setItem("rbl.preferences", JSON.stringify({ ...preferences, view: { ...preferences.view, tooltips: false } }));
    window.dispatchEvent(new StorageEvent("storage", { key: "rbl.preferences" }));
  });
  await expect(panel.getByText("Play", { exact: true })).not.toHaveAttribute("title");
  const requests = await page.evaluate(() => (window as unknown as { __datePages: { len: number; columns: string[] }[] }).__datePages);
  expect(requests.length).toBeGreaterThan(0);
  expect(requests.every(request => request.len <= 128)).toBe(true);
  expect(requests.some(request => request.columns.includes("dateAdded"))).toBe(true);
});
