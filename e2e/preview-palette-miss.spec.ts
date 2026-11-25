import { expect, test } from "@playwright/test";
test("a missing newly selected palette cannot keep the previous palette's preview", async ({ page }) => {
  await page.addInitScript(() => {
    localStorage.setItem("rbl.columns.v2.collection", JSON.stringify({order:["title","preview"],widths:{}}));
  });
  await page.route("**/src/main.tsx*", async route => {
    const response=await route.fetch();
    await route.fulfill({response,body:`
      import { getBackend as fixtureBackend } from "/src/ipc/client.ts";
      const backend = await fixtureBackend();
      window.__bluePreviewReads = 0;
      backend.trackWaveform = (id, kind) => {
        if (kind === "bands") return Promise.resolve(new Uint8Array(3600).fill(70));
        if (kind === "mono") window.__bluePreviewReads++;
        return Promise.resolve(new Uint8Array());
      };
    `+await response.text()});
  });
  await page.goto("/");
  await expect(page.getByTestId("browser-title").first()).toContainText("Tracks)");
  const canvas=page.locator('canvas[data-preview-wave]').first();
  const pixels=()=>canvas.evaluate((c:HTMLCanvasElement)=>{
    const ctx=c.getContext("2d")!;
    const data=ctx.getImageData(0,Math.floor(c.height/2),c.width,1).data;
    return Array.from(data).reduce((hash,value)=>Math.imul(hash^value,16777619)>>>0,2166136261);
  });
  await expect.poll(()=>canvas.evaluate((c:HTMLCanvasElement)=>{
    const bytes=c.getContext("2d")!.getImageData(0,0,c.width,c.height).data;
    return bytes.some((v,i)=>i%4===3&&v>0);
  })).toBe(true);
  const before=await pixels();
  await page.getByRole("banner").getByRole("button",{name:"Settings",exact:true}).click();
  const dialog=page.getByRole("dialog",{name:"Preferences"});
  await dialog.getByRole("tab",{name:"Color",exact:true}).click();
  await dialog.getByRole("radiogroup",{name:"Waveform color"}).getByRole("radio",{name:"BLUE",exact:true}).click();
  await page.keyboard.press("Escape");
  await expect.poll(()=>page.evaluate(()=>(window as unknown as {__bluePreviewReads:number}).__bluePreviewReads)).toBeGreaterThan(0);
  await expect.poll(pixels).not.toBe(before);
});
