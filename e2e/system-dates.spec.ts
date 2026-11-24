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
        return (await fetch(...args)).map(row => ({ ...row, dateAdded: "2025-03-04", dateLabels: { ...row.dateLabels, dateAdded: "04 OS 03 25" } }));
      };
      const details = dates.trackDetails.bind(dates);
      dates.trackDetails = async (id) => ({ ...await details(id), dateCreated: "2025-03-04", dateCreatedLabel: "04 INFO 03 25" });
    `;
    await route.fulfill({ response, body: fixture + await response.text() });
  });
  await page.goto("/?writable");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  const date = page.locator('[role="gridcell"][data-col="dateAdded"]').nth(2);
  await expect(date).toHaveText("04 OS 03 25");
  await page.locator('[role="gridcell"][data-col="title"]').nth(2).click();
  await page.evaluate(() => (window as unknown as { __menu: (id: string) => void }).__menu("info"));
  const panel = page.getByRole("complementary", { name: "Information" });
  await expect(panel).toBeVisible();
  await expect(panel.getByText("04 INFO 03 25", { exact: true })).toBeVisible();
  const requests = await page.evaluate(() => (window as unknown as { __datePages: { len: number; columns: string[] }[] }).__datePages);
  expect(requests.length).toBeGreaterThan(0);
  expect(requests.every(request => request.len <= 128)).toBe(true);
  expect(requests.some(request => request.columns.includes("dateAdded"))).toBe(true);
});
