import { expect, test, type Page } from "@playwright/test";

async function installImageBackend(page: Page) {
  await page.evaluate(async () => {
    const moduleUrl = "/src/ipc/client.ts";
    const { getBackend } = await import(moduleUrl) as typeof import("../src/ipc/client");
    const backend = await getBackend();
    const canvas = document.createElement("canvas"); canvas.width = canvas.height = 32;
    const context = canvas.getContext("2d")!; context.fillStyle = "orange"; context.fillRect(0,0,32,32);
    const preview = canvas.toDataURL("image/jpeg");
    let mode: "artwork" | "custom" = "artwork"; let image: string | null = null;
    const calls: {path:string|null;volumeId:string|null;action:string;source:string|undefined}[]=[];
    backend.pickImage = () => Promise.resolve("/generated/jog.png");
    backend.jogImage = (path, volumeId, action, source) => {
      calls.push({path,volumeId,action,source});
      if (action === "artwork" || action === "custom") mode=action;
      if (action === "import" || action === "copy") image=preview;
      if (action === "delete") image=null;
      return Promise.resolve({mode:path ? mode:null,image});
    };
    Object.assign(window,{__jogCalls:calls});
  });
}

test("the normal USB General tab explicitly imports, selects mode, copies and deletes its jog image", async({page})=>{
  await page.goto("/"); await installImageBackend(page);
  await page.getByRole("tablist",{name:"Library sources"}).getByRole("tab",{name:"Devices"}).click();
  await page.getByRole("treeitem",{name:/TEST/}).click();
  const panel=page.getByRole("region",{name:"Device TEST"});
  const group=panel.getByRole("group",{name:"Image On-Jog Display"});
  await expect(group.getByRole("button",{name:"Import image file"})).toBeEnabled();
  await group.getByRole("button",{name:"Import image file"}).click();
  await expect(group.getByRole("img",{name:"CUSTOMIMAGE"})).toBeVisible();
  await expect(group.getByRole("radio",{name:"ARTWORK",exact:true})).toBeChecked();
  await group.getByRole("radio",{name:"CUSTOMIMAGE",exact:true}).click();
  await expect(group.getByRole("radio",{name:"CUSTOMIMAGE",exact:true})).toBeChecked();
  await group.getByRole("button",{name:"Copy from [Preferences]"}).click();
  await group.getByRole("button",{name:"Delete image"}).click();
  await expect(group.getByText("No image",{exact:true})).toBeVisible();
  await expect(group.getByRole("radio",{name:"CUSTOMIMAGE",exact:true})).toBeChecked();
  const calls=await page.evaluate(()=> (window as unknown as {__jogCalls:{path:string|null;volumeId:string|null;action:string;source?:string}[]}).__jogCalls);
  expect(calls.filter(c=>c.action!=="read").map(c=>c.action)).toEqual(["import","custom","copy","delete"]);
  expect(calls.every(c=>c.path===calls[0]!.path && c.volumeId===calls[0]!.volumeId)).toBe(true);
  expect(calls.find(c=>c.action==="import")!.source).toBe("/generated/jog.png");
});

test("Preferences stores a source image without writing a USB or offering a device mode",async({page})=>{
  await page.goto("/#preferences/djSystem"); await installImageBackend(page);
  const group=page.getByRole("group",{name:"Image On-Jog Display"});
  await expect(group.getByRole("button",{name:"Import image file"})).toBeEnabled();
  await expect(group.getByRole("radio")).toHaveCount(0);
  await expect(group.getByRole("button",{name:"Copy from [Preferences]"})).toHaveCount(0);
  await group.getByRole("button",{name:"Import image file"}).click();
  await expect(group.getByRole("img",{name:"CUSTOMIMAGE"})).toBeVisible();
  await group.getByRole("button",{name:"Delete image"}).click();
  await expect(group.getByText("No image",{exact:true})).toBeVisible();
  const calls=await page.evaluate(()=> (window as unknown as {__jogCalls:{path:string|null;action:string}[]}).__jogCalls);
  expect(calls.filter(c=>c.action!=="read").map(c=>[c.path,c.action])).toEqual([[null,"import"],[null,"delete"]]);
});
