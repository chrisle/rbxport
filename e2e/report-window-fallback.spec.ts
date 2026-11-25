import { expect, test, type Page } from "@playwright/test";
import { isolateReportRequests } from "./report-fixture";

async function reportButton(page: Page, origin: "status" | "preferences") {
  if (origin === "status") return page.getByRole("contentinfo").getByRole("button", { name: "Report bug", exact: true });
  await page.getByRole("banner").getByRole("button", { name: "Settings" }).click();
  const preferences = page.getByRole("dialog", { name: "Preferences", exact: true });
  await expect(preferences).toBeVisible();
  return preferences.getByRole("navigation", { name: "Preference panes" }).getByRole("button", { name: "Report bug" });
}

for (const origin of ["status", "preferences"] as const) {
  test(`a rejected native report launch falls back to a usable form from ${origin}`, async ({ page }) => {
    const requests = await isolateReportRequests(page);
    const errors: string[] = [];
    page.on("pageerror", error => errors.push(error.message));
    await page.goto("/");
    await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
    await page.evaluate(async () => {
      const moduleUrl = "/src/ipc/client.ts";
      const { getBackend } = await import(moduleUrl) as typeof import("../src/ipc/client");
      (await getBackend()).openReportWindow = () => Promise.reject(new Error("Fixture native report window failed"));
    });
    const button = await reportButton(page, origin);
    await button.click();
    const report = page.getByRole("dialog", { name: "Report bug", exact: true });
    await expect(report).toBeVisible();
    await expect(report.getByRole("button", { name: "Show log" })).toBeEnabled();
    await report.getByLabel("What happened?").fill("Generated local fixture only");
    await expect(report.getByRole("button", { name: "Send report" })).toBeDisabled();
    await report.getByRole("button", { name: "Close report" }).click();
    await expect(report).toHaveCount(0);
    await button.click();
    await expect(report).toHaveCount(1);
    await expect(report.getByRole("button", { name: "Show log" })).toBeEnabled();
    expect(errors).toEqual([]);
    expect(requests).toEqual(["GET /verify", "GET /verify"]);
  });

  test(`a successful native report launch does not duplicate an inline form from ${origin}`, async ({ page }) => {
    const requests = await isolateReportRequests(page);
    await page.goto("/");
    await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
    await page.evaluate(async () => {
      const moduleUrl = "/src/ipc/client.ts";
      const { getBackend } = await import(moduleUrl) as typeof import("../src/ipc/client");
      (await getBackend()).openReportWindow = () => {
        Object.assign(window, { fixtureNativeReportAcknowledged: true });
        return Promise.resolve(true);
      };
    });
    await (await reportButton(page, origin)).click();
    await expect.poll(() => page.evaluate(() => (window as unknown as {
      fixtureNativeReportAcknowledged?: boolean;
    }).fixtureNativeReportAcknowledged)).toBe(true);
    await expect(page.getByRole("dialog", { name: "Report bug", exact: true })).toHaveCount(0);
    expect(requests).toEqual([]);
  });
}

test("a closed Preferences window does not reopen for a late rejected report launch", async ({ page }) => {
  const requests = await isolateReportRequests(page);
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.evaluate(async () => {
    const moduleUrl = "/src/ipc/client.ts";
    const { getBackend } = await import(moduleUrl) as typeof import("../src/ipc/client");
    (await getBackend()).openReportWindow = () => new Promise((_, reject) => {
      Object.assign(window, { rejectClosedReportLaunch: () => reject(new Error("Retired fixture native window failure")) });
    });
  });
  await (await reportButton(page, "preferences")).click();
  await expect.poll(() => page.evaluate(() => typeof (window as unknown as {
    rejectClosedReportLaunch?: unknown;
  }).rejectClosedReportLaunch)).toBe("function");
  const preferences = page.getByRole("dialog", { name: "Preferences", exact: true });
  await preferences.getByRole("button", { name: "Close", exact: true }).click();
  await expect(preferences).toHaveCount(0);
  await page.evaluate(() => (window as unknown as { rejectClosedReportLaunch: () => void }).rejectClosedReportLaunch());
  const retry = await reportButton(page, "preferences");
  await expect(page.getByRole("dialog", { name: "Report bug", exact: true })).toHaveCount(0);
  await page.evaluate(async () => {
    const moduleUrl = "/src/ipc/client.ts";
    const { getBackend } = await import(moduleUrl) as typeof import("../src/ipc/client");
    (await getBackend()).openReportWindow = () => Promise.resolve(false);
  });
  await retry.click();
  await expect(page.getByRole("dialog", { name: "Report bug", exact: true })).toBeVisible();
  expect(errors).toEqual([]);
  expect(requests).toEqual(["GET /verify"]);
});

for (const origin of ["status", "preferences"] as const) {
  for (const staleOutcome of ["reject", "inline"] as const) {
    for (const latestOutcome of ["inline", "native"] as const) {
      test(`a stale ${staleOutcome} cannot replace the latest ${latestOutcome} report request from ${origin}`, async ({ page }) => {
        const requests = await isolateReportRequests(page);
        const errors: string[] = [];
        page.on("pageerror", error => errors.push(error.message));
        await page.goto("/");
        await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
        await page.evaluate(async ({ staleOutcome, latestOutcome }) => {
          const moduleUrl = "/src/ipc/client.ts";
          const { getBackend } = await import(moduleUrl) as typeof import("../src/ipc/client");
          let calls = 0;
          (await getBackend()).openReportWindow = () => ++calls === 1 ? new Promise((resolve, reject) => {
            Object.assign(window, { completeStaleNativeReport: () => {
              if (staleOutcome === "reject") reject(new Error("Retired fixture window request"));
              else resolve(false);
            } });
          }) : Promise.resolve(latestOutcome === "native");
        }, { staleOutcome, latestOutcome });
        const button = await reportButton(page, origin);
        await button.click();
        await expect.poll(() => page.evaluate(() => typeof (window as unknown as {
          completeStaleNativeReport?: unknown;
        }).completeStaleNativeReport)).toBe("function");
        await button.click();
        const report = page.getByRole("dialog", { name: "Report bug", exact: true });
        if (latestOutcome === "inline") {
          await expect(report).toBeVisible();
          await report.getByRole("button", { name: "Close report" }).click();
        }
        await expect(report).toHaveCount(0);
        await page.evaluate(async () => {
          (window as unknown as { completeStaleNativeReport: () => void }).completeStaleNativeReport();
          await new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
        });
        await expect(report).toHaveCount(0);
        expect(errors).toEqual([]);
        expect(requests).toEqual(latestOutcome === "inline" ? ["GET /verify"] : []);
      });
    }
  }
}
