import { expect, test } from "@playwright/test";
for (const delayed of [true, false]) test(`hot cue E set twice then cleared has one row owner (refresh delayed=${delayed})`, async ({ page }, testInfo) => {
  const pageErrors: string[] = [];
  const failedRequests: { path: string; error: string | null }[] = [];
  page.on("pageerror", error => pageErrors.push(error.message));
  page.on("requestfailed", request => failedRequests.push({
    path: new URL(request.url()).pathname, error: request.failure()?.errorText ?? null,
  }));
  await page.route("**/src/main.tsx*", async route => {
    const response = await route.fetch();
    await route.fulfill({ response, body: `
      import { getBackend as backend486 } from "/src/ipc/client.ts";
      const backend = await backend486();
      const raw = backend.trackCues.bind(backend);
      const add = backend.edits.addCue.bind(backend.edits);
      let track = null, hold = false;
      const replies = [], writes = [], failures = [];
      let attempts = 0;
      backend.trackCues = (...args) => {
        track = args[0];
        return hold ? new Promise(resolve => replies.push(() => raw(...args).then(resolve))) : raw(...args);
      };
      backend.edits.addCue = async (...args) => {
        hold = ${delayed};
        attempts++;
        try {
          const id = await add(...args);
          writes.push(id);
          return id;
        } catch (error) { failures.push({ kind:error.kind, message:error.message }); throw error; }
      };
      window.hot486 = async () => ({ attempts, writes, failures, cues: await raw(track) });
      window.release486 = () => { hold = false; replies.splice(0).forEach(reply => reply()); };
    ` + await response.text() });
  });
  const started = Date.now();
  await page.goto("/?writable=1");
  try {
    await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  } finally {
    await testInfo.attach("startup diagnostics", { contentType: "application/json", body: JSON.stringify({
      elapsedMs: Date.now() - started, pageErrors, failedRequests,
      state: await page.evaluate(() => ({ ready: document.readyState, fixtureReady: "hot486" in window,
        rootChildren: document.getElementById("root")?.childElementCount ?? 0 })).catch(() => null),
    }, null, 2) });
  }
  await page.locator('[role="gridcell"][data-col="title"]').nth(3).dblclick();
  const player = page.getByRole("region", { name:"Preview player",exact:true });
  const pad = player.locator('[aria-label="Hot cues"]').getByRole("button", { name:"Hot cue E",exact:true });
  const panel = page.getByRole("complementary", { name:"Cue list" });
  await panel.getByRole("tab", { name:"HOT CUE",exact:true }).click();
  const read = () => page.evaluate(async () => (window as unknown as { hot486: () => Promise<{
    attempts:number; writes: string[]; failures:{kind:string;message:string}[]; cues: { id:string;letter:string;memory:boolean }[];
  }> }).hot486());
  await expect(pad).toHaveAttribute("aria-pressed","false");
  await pad.click();
  await expect.poll(async () => (await read()).writes.length).toBe(1);
  if (!delayed) await expect(pad).toHaveAttribute("aria-pressed","true");
  await pad.click();
  if (delayed) {
    await expect.poll(async () => (await read()).attempts).toBe(2);
    await expect(page.getByRole("contentinfo")).toContainText("Hot Cue E has been set since");
    expect((await read()).failures).toEqual([{ kind:"conflict", message:"Hot Cue E has been set since" }]);
  }
  await page.evaluate(() => (window as unknown as { release486: () => void }).release486());
  await expect(pad).toHaveAttribute("aria-pressed","true");
  const before = await read();
  await panel.getByRole("button", { name:"Clear hot cue E",exact:true }).click();
  await expect.poll(async () => (await read()).cues.filter(cue => !cue.memory && cue.letter === "E").length)
    .toBe(before.cues.filter(cue => !cue.memory && cue.letter === "E").length - 1);
  const after = await read();
  expect(before.writes, "an occupied slot must not be assigned again while its acknowledgement is refreshing").toHaveLength(1);
  expect(after.cues.filter(cue => !cue.memory && cue.letter === "E"), "one ordinary Clear removes the slot").toHaveLength(0);
  await expect(pad).toHaveAttribute("aria-pressed", "false");
});
