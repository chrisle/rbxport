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
  await expect(report).toContainText("What you write here is posted publicly on GitHub. Your email and the log are kept private.");
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

test("an email-only archive failure stays visible and retries the same submission", async ({ page, baseURL }) => {
  if (!baseURL) throw new Error("The report test requires the local app URL.");
  const origin = new URL(baseURL).origin;
  const submits: Record<string, unknown>[] = [];
  const message = "The report could not be submitted. Please try again later.";
  const email = `rbxport-476-${crypto.randomUUID()}@example.test`;
  const description = "The export stopped before it finished.";
  // This fixture checks client transport of an opaque receipt. Only the
  // Worker can authenticate it; no real signing key or service is used here.
  const payload = Buffer.from(JSON.stringify([42, crypto.randomUUID(), new Date().toISOString()])).toString("base64url");
  const retryReceipt = `${payload}.${Buffer.alloc(32).toString("base64url")}`;
  await page.route("**/*", async route => {
    if (new URL(route.request().url()).origin === origin) await route.continue();
    else await route.abort();
  });
  await page.route("https://report.rbxport.com/verify?**", async route => {
    expect(new URL(route.request().url()).searchParams.get("origin")).toBe(origin);
    await route.fulfill({
      contentType: "text/html",
      body: `<script>
        const origin = ${JSON.stringify(origin)};
        const send = token => parent.postMessage({source:"rbxport-verify",type:"token",token}, origin);
        addEventListener("message", event => {
          if (event.origin === origin && event.source === parent &&
              event.data?.source === "rbxport-verify" && event.data.type === "reset") {
            document.body.textContent = "Reset acknowledged";
            addEventListener("release-retry-token", () => send("retry-token"), {once: true});
          }
        });
        send("initial-token");
      </script><body></body>`,
    });
  });
  await page.route("https://report.rbxport.com/", async route => {
    const headers = {
      "access-control-allow-origin": origin,
      "access-control-allow-methods": "POST, PATCH",
      "access-control-allow-headers": "content-type",
    };
    if (route.request().method() === "OPTIONS") return route.fulfill({ status: 204, headers });
    expect(route.request().method()).toBe(submits.length === 0 ? "POST" : "PATCH");
    submits.push(route.request().postDataJSON() as Record<string, unknown>);
    const first = submits.length === 1;
    await route.fulfill({
      status: first ? 502 : 200,
      headers,
      json: first ? { error: message, retryReceipt } : {
        key: "#42", url: "https://github.com/chrisle/rbxport/issues/42", attachmentAdded: true,
      },
    });
  });

  await page.goto("/");
  await page.getByRole("contentinfo").getByRole("button", { name: "Report bug", exact: true }).click();
  const report = page.getByRole("dialog", { name: "Report bug", exact: true });
  const send = report.getByRole("button", { name: "Send report" });
  await report.getByLabel("Your email (optional)").fill(email);
  await report.getByLabel("What happened?").fill(description);
  await report.getByRole("checkbox").uncheck();
  await send.click();
  await expect(report.getByRole("alert")).toHaveText(message);
  const verifier = report.frameLocator('iframe[title="Human verification"]').locator("body");
  await expect(verifier).toContainText("Reset acknowledged");
  await expect(send).toBeDisabled();
  await expect(report.getByLabel("Your email (optional)")).toHaveValue(email);
  await expect(report.getByLabel("What happened?")).toHaveValue(description);
  await expect(report.getByText("Thank you for your report.")).toHaveCount(0);
  await verifier.evaluate(body => body.ownerDocument.defaultView?.dispatchEvent(new Event("release-retry-token")));
  await expect(send).toBeEnabled();
  await expect(report.getByRole("alert")).toHaveText(message);

  await send.click();
  await expect(report).toContainText("Thank you for your report.");
  await expect(report.getByLabel("Bookmark this URL and check back to get updates of your report."))
    .toHaveValue("https://github.com/chrisle/rbxport/issues/42");
  expect(submits).toEqual([
    { email, description, attachment: "", turnstileToken: "initial-token" },
    { email, description, attachment: "", turnstileToken: "retry-token", retryReceipt },
  ]);
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

/**
 * What the form hands the report service, through the real Report bug
 * button. The email goes in its own field, which the service archives
 * privately; the description, the only text that reaches the public issue,
 * never contains it. The service's side (archive, retrieval, issue payload)
 * is tested in the report Worker. A unique `.test` address is made per run.
 */
test.describe("the reporter's email", () => {
  async function openReport(page: import("@playwright/test").Page) {
    const sent: Record<string, unknown>[] = [];
    await page.route("https://report.rbxport.com/verify?**", async (route) => {
      const origin = new URL(route.request().url()).searchParams.get("origin") ?? "";
      await route.fulfill({
        contentType: "text/html",
        body: `<script>parent.postMessage({ source: "rbxport-verify", type: "token", token: "test-turnstile-token" }, ${JSON.stringify(origin)});</script>`,
      });
    });
    await page.route("https://report.rbxport.com/", async (route) => {
      if (route.request().method() === "POST") sent.push(route.request().postDataJSON() as Record<string, unknown>);
      await route.fulfill({
        contentType: "application/json",
        headers: { "access-control-allow-origin": new URL(page.url()).origin },
        body: JSON.stringify({ key: "#42", url: "https://github.com/chrisle/rbxport/issues/42", attachmentAdded: true }),
      });
    });
    await page.goto("/");
    await page.getByRole("contentinfo").getByRole("button", { name: "Report bug", exact: true }).click();
    const report = page.getByRole("dialog", { name: "Report bug", exact: true });
    return { report, sent };
  }

  /** The "Report details" block the app puts at the top of the attachment. */
  function reportDetails(attachment: unknown): { email: string | null } {
    const details = /^Report details\n([\s\S]+?)\n\nSystem information/.exec(String(attachment))?.[1];
    expect(details).toBeDefined();
    return JSON.parse(details ?? "{}") as { email: string | null };
  }

  const description = "Analysis stopped at 40% on one track.";

  test("is sent apart from the public description, with the log", async ({ page }) => {
    const email = `rbxport-e2e-${crypto.randomUUID()}@example.test`;
    const { report, sent } = await openReport(page);
    await report.getByLabel("Your email (optional)").fill(`  ${email} `);
    await report.getByLabel("What happened?").fill(description);
    await expect(report.getByRole("checkbox")).toBeChecked();
    await report.getByRole("button", { name: "Send report" }).click();
    await expect(report).toContainText("Thank you for your report.");
    expect(sent).toHaveLength(1);
    expect(sent[0]?.email).toBe(email);
    expect(sent[0]?.description).toBe(description);
    expect(reportDetails(sent[0]?.attachment).email).toBe(email);
    expect(String(sent[0]?.description)).not.toContain("rbxport-e2e-");
  });

  test("is sent when the log is not attached", async ({ page }) => {
    const email = `rbxport-e2e-${crypto.randomUUID()}@example.test`;
    const { report, sent } = await openReport(page);
    await report.getByLabel("Your email (optional)").fill(email);
    await report.getByLabel("What happened?").fill(description);
    await report.getByRole("checkbox").uncheck();
    await report.getByRole("button", { name: "Send report" }).click();
    await expect(report).toContainText("Thank you for your report.");
    expect(sent).toEqual([{ email, description, attachment: "", turnstileToken: "test-turnstile-token" }]);
  });

  test("left blank is sent as blank", async ({ page }) => {
    const { report, sent } = await openReport(page);
    await report.getByLabel("What happened?").fill(description);
    await report.getByRole("button", { name: "Send report" }).click();
    await expect(report).toContainText("Thank you for your report.");
    expect(sent).toHaveLength(1);
    expect(sent[0]?.email).toBe("");
    expect(reportDetails(sent[0]?.attachment).email).toBeNull();
  });
});
