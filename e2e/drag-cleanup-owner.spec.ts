import { expect, test, type Locator, type Page } from "@playwright/test";

type NativeFixture = { starts: string[][]; finish(index: number): void; read(): Promise<string[]> };

async function fixture(page: Page) {
  await page.addInitScript(() => {
    const ends: (() => void)[] = [];
    (window as unknown as { nativeDragOwner: unknown }).nativeDragOwner = {
      starts: [],
      begin(this: { starts: string[][] }, ids: string[]) {
        this.starts.push([...ids]);
        return new Promise<void>(resolve => ends.push(resolve));
      },
      finish(index: number) { ends[index]?.(); },
    };
  });
  // Exercise the App's native-drag lifecycle without a native operation.
  await page.route("**/src/ipc/client.ts*", async route => {
    const response = await route.fetch();
    const original = await response.text();
    const body = original.replace("return isTauri && detectPlatform().mac;", "return true;");
    if (body === original) throw new Error("native drag fixture entry point changed");
    await route.fulfill({ response, body });
  });
  await page.route("**/src/ipc/invoke.ts*", async route => {
    const response = await route.fetch();
    const original = await response.text();
    const body = original.replace(/export async function tauriInvoke\(\)[\s\S]*$/, `
      export async function tauriInvoke() {
        return async (command, args) => {
          if (command !== "drag_tracks") throw new Error("unexpected native fixture operation");
          return window.nativeDragOwner.begin(args.ids);
        };
      }
    `);
    if (body === original) throw new Error("native invoke fixture entry point changed");
    await route.fulfill({ response, body });
  });
  await page.route("**/src/main.tsx*", async route => {
    const response = await route.fetch();
    await route.fulfill({ response, body: `
      import { getBackend as ownerFixtureBackend } from "/src/ipc/client.ts";
      const backend = await ownerFixtureBackend();
      await backend.edits.createPlaylist("Native owner target", "root");
      const target = (await backend.playlistTree()).find(node => node.name === "Native owner target");
      window.nativeDragOwner.read = async () => {
        const view = await backend.openView({ source:{kind:"playlist",id:target.id},sort:"trackNo",descending:false,query:"" });
        return view.len ? backend.viewIdsInRange(view.viewId,0,view.len-1) : [];
      };
    ` + await response.text() });
  });
  await page.goto("/?writable=1");
  await expect(page.getByTestId("browser-title")).toContainText("Tracks)");
  await page.getByRole("button", { name: "Sub-Browser Window", exact: true }).click();
  await expect(page.getByRole("region", { name: "Sub-Browser" })).toBeVisible();
}

async function begin(row: Locator) {
  await row.evaluate(element => {
    const box = element.getBoundingClientRect();
    element.dispatchEvent(new PointerEvent("pointerdown", { bubbles:true,button:0,clientX:box.x+8,clientY:box.y+8 }));
    window.dispatchEvent(new PointerEvent("pointermove", { bubbles:true,buttons:1,clientX:box.x+20,clientY:box.y+8 }));
  });
}
const starts = (page: Page) => page.evaluate(() => (window as unknown as {nativeDragOwner:NativeFixture}).nativeDragOwner.starts);
const target = (page: Page) => page.getByRole("treeitem").filter({hasText:/^\s*Native owner target/}).first();
async function drop(target: Locator) {
  return target.evaluate(element => {
    const transfer = new DataTransfer();
    const over = new DragEvent("dragover", {bubbles:true,cancelable:true,dataTransfer:transfer});
    element.dispatchEvent(over);
    element.dispatchEvent(new DragEvent("drop", {bubbles:true,cancelable:true,dataTransfer:transfer}));
    return over.defaultPrevented;
  });
}

for (const next of ["same", "sub", "main"] as const) {
  test(`late source cleanup preserves newer ${next} drag`, async ({page}) => {
    await fixture(page);
    const main = page.locator('[data-browser-side="main"]').locator('[role="gridcell"][data-col="title"]');
    const sub = page.getByRole("region", {name:"Sub-Browser"}).locator('[role="gridcell"][data-col="title"]');
    await begin((next === "main" ? sub : main).nth(3));
    await expect.poll(async () => (await starts(page)).length).toBe(1);
    await (next === "sub" ? sub : main).nth(4).evaluate(async element => {
      (window as unknown as {nativeDragOwner:NativeFixture}).nativeDragOwner.finish(0);
      for(let i=0;i<8;i++) await Promise.resolve();
      const box = element.getBoundingClientRect();
      element.dispatchEvent(new PointerEvent("pointerdown", {bubbles:true,button:0,clientX:box.x+8,clientY:box.y+8}));
      window.dispatchEvent(new PointerEvent("pointermove", {bubbles:true,buttons:1,clientX:box.x+20,clientY:box.y+8}));
    });
    await expect.poll(async () => (await starts(page)).length).toBe(2);
    await page.waitForTimeout(150);
    expect(await drop(target(page))).toBe(true);
    const expected = (await starts(page))[1];
    await expect.poll(() => page.evaluate(() => (window as unknown as {nativeDragOwner:NativeFixture}).nativeDragOwner.read())).toEqual(expected);
    await page.evaluate(() => (window as unknown as {nativeDragOwner:NativeFixture}).nativeDragOwner.finish(1));
  });
}

for (const dropped of [false,true]) {
  test(`own completed native drag cleans up (drop during handoff=${dropped})`, async ({page}) => {
    await fixture(page);
    const row = page.locator('[data-browser-side="main"]').locator('[role="gridcell"][data-col="title"]').nth(3);
    await begin(row);
    await expect.poll(async () => (await starts(page)).length).toBe(1);
    if(dropped) {
      const accepted = await target(page).evaluate(async element => {
        (window as unknown as {nativeDragOwner:NativeFixture}).nativeDragOwner.finish(0);
        for(let i=0;i<8;i++) await Promise.resolve();
        // The drop runs in the earlier timer's microtask, before the 100ms
        // cleanup, without a host round trip consuming the handoff window.
        await new Promise(resolve => setTimeout(resolve, 30));
        const transfer = new DataTransfer();
        const over = new DragEvent("dragover", {bubbles:true,cancelable:true,dataTransfer:transfer});
        element.dispatchEvent(over);
        element.dispatchEvent(new DragEvent("drop", {bubbles:true,cancelable:true,dataTransfer:transfer}));
        return over.defaultPrevented;
      });
      expect(accepted).toBe(true);
      await expect.poll(() => page.evaluate(() => (window as unknown as {nativeDragOwner:NativeFixture}).nativeDragOwner.read())).toEqual((await starts(page))[0]);
    } else {
      await page.evaluate(() => (window as unknown as {nativeDragOwner:NativeFixture}).nativeDragOwner.finish(0));
    }
    await page.waitForTimeout(150);
    expect(await drop(target(page))).toBe(false);
  });
}
