import { expect, test } from "@playwright/test";
test("whole-artwork preview raster matches the same native-height rendering", async ({ page }) => {
  await page.addInitScript(() => {
    const layout=JSON.stringify({order:["title","preview"],widths:{preview:160}});
    localStorage.setItem("rbl.columns.v2.collection",layout);
    localStorage.setItem("rbl.columns.v2.collection.full",layout);
    localStorage.setItem("rbl.preferences",JSON.stringify({view:{previewCueMarkers:false}}));
  });
  await page.route("**/src/main.tsx*",async route=>{
    const response=await route.fetch();
    await route.fulfill({response,body:`
      import { getBackend as fixtureBackend } from "/src/ipc/client.ts";
      import { renderPreview as fixtureRender } from "/src/canvas/waveform.ts";
      const backend=await fixtureBackend();
      const data=new Uint8Array(3600);
      for(let i=0;i<1200;i++){data[i*3]=31+i%40;data[i*3+1]=5+i%20;data[i*3+2]=3+i%10;}
      let target=null; window.__heightReads=0;
      backend.trackWaveform=(id,kind)=>{target??=id;if(id===target&&kind==="bands")window.__heightReads++;return Promise.resolve(data);};
      window.__heightComparison=async canvas=>{
        const box=canvas.getBoundingClientRect();
        const rendered=await fixtureRender(data,box.width,box.height,devicePixelRatio,"3band",true);
        const fresh=document.createElement("canvas");fresh.width=canvas.width;fresh.height=canvas.height;
        fresh.getContext("2d").drawImage(rendered.bitmap,0,0,fresh.width,fresh.height);
        const a=canvas.getContext("2d").getImageData(0,0,canvas.width,canvas.height).data;
        const b=fresh.getContext("2d").getImageData(0,0,fresh.width,fresh.height).data;
        let differingPixels=0,totalDifference=0,edgeDelta=0;
        for(let i=0;i<a.length;i+=4){if(a.slice(i,i+4).some((v,n)=>v!==b[i+n]))differingPixels++;for(let c=0;c<4;c++)totalDifference+=Math.abs(a[i+c]-b[i+c]);}
        for(let x=0;x<canvas.width;x++){
          let aa=-1,bb=-1;for(let y=0;y<canvas.height;y++){if(aa<0&&a[(y*canvas.width+x)*4+3]>0)aa=y;if(bb<0&&b[(y*canvas.width+x)*4+3]>0)bb=y;}
          if(aa>=0&&bb>=0)edgeDelta=Math.max(edgeDelta,Math.abs(aa-bb));
        }
        if("close" in rendered.bitmap)rendered.bitmap.close();
        return {width:canvas.width,height:canvas.height,cssWidth:box.width,cssHeight:box.height,differingPixels,totalDifference,edgeDelta};
      };
    `+await response.text()});
  });
  await page.goto("/");
  await expect(page.getByTestId("browser-title").first()).toContainText("Tracks)");
  const canvas=page.locator("canvas[data-preview-wave]").first();
  const compare=()=>canvas.evaluate(async c=>(window as unknown as {__heightComparison:(c:Element)=>Promise<{width:number;height:number;cssWidth:number;cssHeight:number;differingPixels:number;totalDifference:number;edgeDelta:number}>}).__heightComparison(c));
  await expect.poll(async()=> (await compare()).differingPixels).toBe(0);
  const partial=await compare();
  await page.getByTestId("artwork-mode-full").click();
  await expect.poll(async()=> (await compare()).height).toBeGreaterThan(partial.height);
  await expect.poll(async()=> (await compare()).differingPixels).toBe(0);
  const full=await compare();
  console.log(JSON.stringify({partial,full}));
  expect(full.cssWidth).toBe(partial.cssWidth);
  const reads=await page.evaluate(()=>(window as unknown as {__heightReads:number}).__heightReads);
  await page.getByTestId("artwork-mode-partial").click();
  await expect.poll(async()=> (await compare()).height).toBe(partial.height);
  await expect.poll(async()=> (await compare()).differingPixels).toBe(0);
  await page.getByTestId("artwork-mode-full").click();
  await expect.poll(async()=> (await compare()).height).toBe(full.height);
  await expect.poll(async()=> (await compare()).differingPixels).toBe(0);
  expect(await page.evaluate(()=>(window as unknown as {__heightReads:number}).__heightReads)).toBe(reads);
});
