import { expect, test } from "@playwright/test";

test("bug reports include diagnostics by default and open the attachment externally", async ({ page }) => {
  // The report Worker's Turnstile page, standing in: it answers the origin it
  // was framed for with a token, as the real one does once Turnstile passes.
  let framedFor = "";
  await page.route("https://report.rbxport.com/verify?**", async (route) => {
    framedFor = new URL(route.request().url()).searchParams.get("origin") ?? "";
    await route.fulfill({
      contentType: "text/html",
      body: `<script>parent.postMessage({ source: "rbxport-verify", type: "token", token: "test-turnstile-token" }, ${JSON.stringify(framedFor)});</script>`,
    });
  });
  await page.goto("/");
  await page.getByRole("contentinfo").getByRole("button", { name: "Report bug", exact: true }).click();
  const report = page.getByRole("dialog", { name: "Report bug", exact: true });
  await expect(report.locator('iframe[title="Human verification"]')).toHaveCount(1);
  await expect.poll(() => framedFor).toBe(new URL(page.url()).origin);
  await expect(report.getByRole("checkbox")).toBeChecked();
  await expect(report).toContainText("Reports are sent to TRIODE. I read every report, but please don’t expect a personal reply.");
  await expect(report.getByRole("button", { name: "Send report" })).toBeDisabled();
  await report.getByLabel("What happened?").fill("Playback stopped after loading a track.");
  await expect(report.getByRole("button", { name: "Send report" })).toBeEnabled();
  await report.getByRole("button", { name: "Show log" }).click();
  await expect(report.getByRole("alert")).toHaveText("Opening the text editor requires the desktop app.");
  await report.getByRole("checkbox").uncheck();
  await expect(report.getByRole("button", { name: "Show log" })).toBeDisabled();
  await report.getByRole("button", { name: "Close", exact: true }).click();
  await expect(report).toHaveCount(0);
});

test("Preferences offers Report bug at the bottom of its sidebar", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("banner").getByRole("button", { name: "Settings" }).click();
  const preferences = page.getByRole("dialog", { name: "Preferences", exact: true });
  await preferences.getByRole("navigation", { name: "Preference panes" }).getByRole("button", { name: "Report bug" }).click();
  const report = page.getByRole("dialog", { name: "Report bug", exact: true });
  await expect(report).toBeVisible();
  await report.getByRole("button", { name: "Close report" }).click();
  await expect(report).toHaveCount(0);
  await expect(preferences).toBeVisible();
});

test("a sent report shows the reporter where to follow it", async ({ page }) => {
  await page.route("https://report.rbxport.com/verify?**", async (route) => {
    const origin = new URL(route.request().url()).searchParams.get("origin") ?? "";
    await route.fulfill({
      contentType: "text/html",
      body: `<script>parent.postMessage({ source: "rbxport-verify", type: "token", token: "test-turnstile-token" }, ${JSON.stringify(origin)});</script>`,
    });
  });
  await page.route("https://report.rbxport.com/", async (route) => {
    await route.fulfill({
      contentType: "application/json",
      headers: { "access-control-allow-origin": new URL(page.url()).origin },
      body: JSON.stringify({ key: "#42", url: "https://github.com/chrisle/rbxport/issues/42", attachmentAdded: true }),
    });
  });
  await page.goto("/");
  await page.getByRole("contentinfo").getByRole("button", { name: "Report bug", exact: true }).click();
  const report = page.getByRole("dialog", { name: "Report bug", exact: true });
  await report.getByRole("checkbox").uncheck();
  await report.getByLabel("What happened?").fill("Playback stopped after loading a track.");
  await report.getByRole("button", { name: "Send report" }).click();
  await expect(report).toContainText("Thank you for your report.");
  const link = report.getByLabel("Bookmark this URL and check back to get updates of your report.");
  await expect(link).toHaveValue("https://github.com/chrisle/rbxport/issues/42");
  await expect(link).toBeFocused();
  await expect(report.getByRole("button", { name: "Send report" })).toHaveCount(0);
  await page.context().route("https://github.com/**", route => route.fulfill({ contentType: "text/html", body: "" }));
  const popup = page.waitForEvent("popup");
  await report.getByRole("button", { name: "Open in browser" }).click();
  expect((await popup).url()).toBe("https://github.com/chrisle/rbxport/issues/42");
  await report.getByRole("button", { name: "Close", exact: true }).click();
  await expect(report).toHaveCount(0);
});
