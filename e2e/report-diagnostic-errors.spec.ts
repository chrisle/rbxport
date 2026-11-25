import { expect, test } from "@playwright/test";
import { deliverVerification, isolateReportRequests, openReport } from "./report-fixture";

const readFailure = "Fixture diagnostic log read failed (os error 267)";
const previewFailure = "Fixture attachment preview could not open";

test("verification success cannot hide a diagnostic-read failure or unblock an attached report", async ({ page }) => {
  const requests = await isolateReportRequests(page);
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.evaluate(async () => {
    const moduleUrl = "/src/ipc/client.ts";
    const { getBackend } = await import(moduleUrl) as typeof import("../src/ipc/client");
    (await getBackend()).reportAttachment = () => Promise.reject(new Error("Fixture diagnostic log read failed (os error 267)"));
  });
  const report = await openReport(page);
  await expect(report.getByRole("alert")).toHaveText(readFailure);
  await report.getByLabel("What happened?").fill("Generated local fixture only");
  await deliverVerification(page, report, { type: "token", token: "local-fixture-token" });
  await expect(report.getByRole("alert")).toHaveText(readFailure);
  await expect(report.getByRole("button", { name: "Send report" })).toBeDisabled();
  await expect(report.getByRole("button", { name: "Show log" })).toBeDisabled();
  await report.getByRole("checkbox").uncheck();
  await expect(report.getByRole("alert")).toHaveCount(0);
  await expect(report.getByRole("button", { name: "Send report" })).toBeEnabled();
  expect(requests).toEqual(["GET /verify"]);
});

test("successful diagnostic preparation cannot clear a verification failure", async ({ page }) => {
  const requests = await isolateReportRequests(page);
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.evaluate(async () => {
    const moduleUrl = "/src/ipc/client.ts";
    const { getBackend } = await import(moduleUrl) as typeof import("../src/ipc/client");
    (await getBackend()).reportAttachment = () => new Promise(resolve => {
      Object.assign(window, { resolveFixtureDiagnostic: () => resolve("System information\nGenerated fixture\n") });
    });
  });
  const report = await openReport(page);
  await deliverVerification(page, report, { type: "error", code: "fixture-verification-error" });
  await expect(report.getByRole("alert")).toContainText("fixture-verification-error");
  await page.evaluate(() => (window as unknown as { resolveFixtureDiagnostic: () => void }).resolveFixtureDiagnostic());
  await expect(report.getByRole("button", { name: "Show log" })).toBeEnabled();
  await expect(report.getByRole("alert")).toContainText("fixture-verification-error");
  await expect(report.getByRole("button", { name: "Send report" })).toBeDisabled();
  expect(requests).toEqual(["GET /verify"]);
});

test("verification success preserves a failed preview of the captured attachment", async ({ page }) => {
  const requests = await isolateReportRequests(page);
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.evaluate(async () => {
    const moduleUrl = "/src/ipc/client.ts";
    const { getBackend } = await import(moduleUrl) as typeof import("../src/ipc/client");
    const backend = await getBackend();
    backend.reportAttachment = () => Promise.resolve("System information\nGenerated fixture\nApplication log\nSynthetic log\n");
    backend.openReportAttachment = text => {
      Object.assign(window, { capturedFixturePreview: text });
      return Promise.reject(new Error("Fixture attachment preview could not open"));
    };
  });
  const report = await openReport(page);
  await report.getByLabel("Your email (optional)").fill("fixture@example.test");
  await report.getByRole("button", { name: "Show log" }).click();
  await expect(report.getByRole("alert")).toHaveText(previewFailure);
  await deliverVerification(page, report, { type: "token", token: "local-fixture-token" });
  await expect(report.getByRole("alert")).toHaveText(previewFailure);
  const captured = await page.evaluate(() => (window as unknown as { capturedFixturePreview: string }).capturedFixturePreview);
  expect(captured).toContain('"email": "fixture@example.test"');
  expect(captured).toContain("System information\nGenerated fixture\nApplication log\nSynthetic log\n");
  expect(requests).toEqual(["GET /verify"]);
});

test("a retired diagnostic read cannot replace the current retry failure", async ({ page }) => {
  const requests = await isolateReportRequests(page);
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.evaluate(async () => {
    const moduleUrl = "/src/ipc/client.ts";
    const { getBackend } = await import(moduleUrl) as typeof import("../src/ipc/client");
    let reads = 0;
    (await getBackend()).reportAttachment = () => ++reads === 1 ? new Promise((_, reject) => {
      Object.assign(window, { rejectRetiredDiagnostic: () => reject(new Error("Retired fixture diagnostic failure")) });
    }) : Promise.reject(new Error("Current fixture diagnostic failure"));
  });
  const report = await openReport(page);
  await expect.poll(() => page.evaluate(() => typeof (window as unknown as { rejectRetiredDiagnostic?: unknown }).rejectRetiredDiagnostic)).toBe("function");
  await report.getByRole("checkbox").uncheck();
  await report.getByRole("checkbox").check();
  await expect(report.getByRole("alert")).toHaveText("Current fixture diagnostic failure");
  await page.evaluate(() => (window as unknown as { rejectRetiredDiagnostic: () => void }).rejectRetiredDiagnostic());
  await deliverVerification(page, report, { type: "token", token: "local-fixture-token" });
  await expect(report.getByRole("alert")).toHaveText("Current fixture diagnostic failure");
  expect(requests).toEqual(["GET /verify"]);
});

test("a retired preview cannot overwrite the next attachment failure", async ({ page }) => {
  const requests = await isolateReportRequests(page);
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.evaluate(async () => {
    const moduleUrl = "/src/ipc/client.ts";
    const { getBackend } = await import(moduleUrl) as typeof import("../src/ipc/client");
    const backend = await getBackend();
    backend.reportAttachment = () => (window as unknown as { failNextFixtureDiagnostic?: boolean }).failNextFixtureDiagnostic
      ? Promise.reject(new Error("Current fixture diagnostic failure"))
      : Promise.resolve("System information\nGenerated fixture\n");
    backend.openReportAttachment = () => new Promise((_, reject) => {
      Object.assign(window, { rejectRetiredPreview: () => reject(new Error("Retired fixture preview failure")) });
    });
  });
  const report = await openReport(page);
  await report.getByRole("button", { name: "Show log" }).click();
  await expect.poll(() => page.evaluate(() => typeof (window as unknown as { rejectRetiredPreview?: unknown }).rejectRetiredPreview)).toBe("function");
  await page.evaluate(() => Object.assign(window, { failNextFixtureDiagnostic: true }));
  await report.getByRole("checkbox").uncheck();
  await report.getByRole("checkbox").check();
  await expect(report.getByRole("alert")).toHaveText("Current fixture diagnostic failure");
  await page.evaluate(() => (window as unknown as { rejectRetiredPreview: () => void }).rejectRetiredPreview());
  await deliverVerification(page, report, { type: "token", token: "local-fixture-token" });
  await expect(report.getByRole("alert")).toHaveText("Current fixture diagnostic failure");
  await expect(report.getByRole("button", { name: "Show log" })).toBeDisabled();
  expect(requests).toEqual(["GET /verify"]);
});

test("a closed report ignores its pending diagnostic preparation", async ({ page }) => {
  const requests = await isolateReportRequests(page);
  await page.goto("/");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.evaluate(async () => {
    const moduleUrl = "/src/ipc/client.ts";
    const { getBackend } = await import(moduleUrl) as typeof import("../src/ipc/client");
    let reads = 0;
    (await getBackend()).reportAttachment = () => ++reads === 1 ? new Promise(resolve => {
      Object.assign(window, { resolveClosedDiagnostic: () => resolve("Retired generated attachment") });
    }) : Promise.reject(new Error("Current fixture diagnostic failure"));
  });
  let report = await openReport(page);
  await expect.poll(() => page.evaluate(() => typeof (window as unknown as { resolveClosedDiagnostic?: unknown }).resolveClosedDiagnostic)).toBe("function");
  await report.getByRole("button", { name: "Close report" }).click();
  await expect(report).toHaveCount(0);
  report = await openReport(page);
  await expect(report.getByRole("alert")).toHaveText("Current fixture diagnostic failure");
  await page.evaluate(() => (window as unknown as { resolveClosedDiagnostic: () => void }).resolveClosedDiagnostic());
  await deliverVerification(page, report, { type: "token", token: "local-fixture-token" });
  await expect(report.getByRole("alert")).toHaveText("Current fixture diagnostic failure");
  await expect(report.getByRole("button", { name: "Send report" })).toBeDisabled();
  expect(requests).toEqual(["GET /verify", "GET /verify"]);
});
