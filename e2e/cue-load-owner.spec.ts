import { expect, test } from "@playwright/test";
for (const pending of [true, false]) test(`CUE after a new playing load owns its cue (metadata pending=${pending})`, async ({ page }) => {
  await page.route("**/src/main.tsx*", async route => {
    const response = await route.fetch();
    await route.fulfill({ response, body: `
      import { getBackend as backend350 } from "/src/ipc/client.ts";
      const backend = await backend350();
      let first = null;
      const requests = [], seeks = [], loads = [];
      backend.trackCues = id => {
        first ??= id;
        requests.push(id);
        if (id === first) return Promise.resolve([{id:"first-memory",positionMs:10000,outMs:0,letter:"",memory:true,colour:null}]);
        return ${pending ? "new Promise(() => {})" : 'Promise.resolve([{id:"second-memory",positionMs:3000,outMs:0,letter:"",memory:true,colour:null}])'};
      };
      const seek = backend.deckSeek.bind(backend);
      backend.deckSeek = (...args) => { seeks.push(args); return seek(...args); };
      const load = backend.deckLoad.bind(backend);
      backend.deckLoad = (...args) => { loads.push(args); return load(...args); };
      window.probe350 = async () => ({ requests, seeks, loads, state: await backend.deckState() });
    ` + await response.text() });
  });
  await page.goto("/");
  const rows = page.locator('[role="gridcell"][data-col="title"]');
  await expect(rows.nth(4)).toBeVisible();
  const read = () => page.evaluate(async () => (window as unknown as { probe350: () => Promise<{
    requests: string[]; seeks: [string,number][]; loads: [string,string,number][]; state:{a:{playing:boolean;loaded:boolean;loadId:number}};
  }> }).probe350());
  await rows.nth(3).dblclick();
  const player = page.getByRole("region", { name:"Preview player",exact:true });
  await expect.poll(async () => (await read()).requests.length).toBeGreaterThan(0);
  await expect(player.locator('[aria-label="Cue"]')).toBeEnabled();
  await player.getByRole("button", {name:"Play",exact:true}).click();
  await expect.poll(async () => (await read()).state.a.playing).toBe(true);
  await rows.nth(4).dblclick();
  await expect.poll(async () => (await read()).loads.length).toBe(2);
  await expect.poll(async () => (await read()).state.a.playing).toBe(true);
  await expect.poll(async () => new Set((await read()).requests).size).toBe(2);
  const loaded = await read();
  expect(loaded.loads[0]![1]).not.toBe(loaded.loads[1]![1]);
  expect(loaded.state.a.loadId).toBe(loaded.loads[1]![2]);
  const count = loaded.seeks.length;
  await player.getByRole("button", {name:"Cue",exact:true}).click();
  await expect.poll(async () => (await read()).seeks.length).toBeGreaterThan(count);
  const last = (await read()).seeks.at(-1)!;
  // No metadata for the new track has arrived. Its temporary load cue must
  // not be borrowed from the previous, distinct audio's 10-second cue.
  if (pending) expect(last[1]).not.toBe(10000);
  else expect(last[1]).toBe(3000);
});
