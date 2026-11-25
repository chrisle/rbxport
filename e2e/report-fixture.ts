import { expect, type Locator, type Page } from "@playwright/test";

/** Local HTML only: no Turnstile service or report submission is contacted. */
export async function isolateReportRequests(page: Page): Promise<string[]> {
  const requests: string[] = [];
  await page.context().route("https://report.rbxport.com/**", async route => {
    requests.push(`${route.request().method()} ${new URL(route.request().url()).pathname}`);
    if (new URL(route.request().url()).pathname === "/verify") {
      await route.fulfill({ contentType: "text/html", body: "<!doctype html><body>Local verification fixture</body>" });
    } else await route.abort();
  });
  return requests;
}

export async function openReport(page: Page): Promise<Locator> {
  await page.getByRole("contentinfo").getByRole("button", { name: "Report bug", exact: true }).click();
  const report = page.getByRole("dialog", { name: "Report bug", exact: true });
  await expect(report).toBeVisible();
  return report;
}

export async function deliverVerification(page: Page, report: Locator, message:
  { type: "token"; token: string } | { type: "error"; code: string }): Promise<void> {
  const marker = crypto.randomUUID();
  await page.evaluate(marker => {
    Object.assign(window, { fixtureVerificationDelivered: false });
    const hear = (event: MessageEvent) => {
      if (event.origin !== "https://report.rbxport.com" || event.data?.fixtureMarker !== marker) return;
      Object.assign(window, { fixtureVerificationDelivered: true });
      window.removeEventListener("message", hear);
    };
    window.addEventListener("message", hear);
  }, marker);
  await report.frameLocator('iframe[title="Human verification"]').locator("body").evaluate((_, data) => {
    parent.postMessage(data.message, data.origin);
  }, { origin: new URL(page.url()).origin, message: { source: "rbxport-verify", ...message, fixtureMarker: marker } });
  await expect.poll(() => page.evaluate(() => (window as unknown as {
    fixtureVerificationDelivered: boolean;
  }).fixtureVerificationDelivered)).toBe(true);
}
