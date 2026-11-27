/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { __setBackend } from "@/ipc/client";
import type { Backend, Device, JogImageState } from "@/ipc/types";
import { JogImageControls } from "./JogImageControls";

const device = (volumeId = "volume-a"): Device => ({ path: "/generated-usb", name: "Generated", volumeId, totalBytes: 0, freeBytes: 0, removable: true, export: null });
const initial: JogImageState = { mode: "artwork", image: null };
const image = "data:image/jpeg;base64,generated-preview";
let host: HTMLDivElement; let root: Root;
const error = vi.fn();
const settle = () => act(async () => { for (let i=0; i<12; i++) await Promise.resolve(); });
function pending<T>() { let resolve!: (value:T)=>void; let reject!: (error:Error)=>void; const promise = new Promise<T>((yes,no)=>{resolve=yes;reject=no;}); return {promise,resolve,reject}; }
function render(volumeId = "volume-a", onError = error, busy = false) { act(() => root.render(<JogImageControls device={device(volumeId)} onError={onError} busy={busy} />)); }
function click(name: string) { act(() => Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find(b=>b.textContent===name)!.click()); }
function custom() { act(() => host.querySelectorAll<HTMLInputElement>('input[type="radio"]')[1]!.click()); }
beforeEach(()=>{ globalThis.IS_REACT_ACT_ENVIRONMENT=true;host=document.createElement("div");document.body.append(host);root=createRoot(host);error.mockReset(); });
afterEach(()=>{act(()=>root.unmount());host.remove();__setBackend(null);});

it("reads without writing, then uses explicit mode/import/delete commands on the selected owner", async()=>{
 const call=vi.fn((_path:string,_id:string,action:string)=>Promise.resolve(action==="read"?initial:{mode:"custom",image:action==="import"?image:null}));
 __setBackend({jogImage:call,pickImage:()=>Promise.resolve("/generated/source.png")} as unknown as Backend);
 render();await settle();expect(call).toHaveBeenCalledExactlyOnceWith("/generated-usb","volume-a","read");
 custom();await settle();expect(call).toHaveBeenLastCalledWith("/generated-usb","volume-a","custom",undefined);
 click("Import image file");await settle();expect(call).toHaveBeenLastCalledWith("/generated-usb","volume-a","import","/generated/source.png");
 expect(host.querySelector("img")?.src).toBe(image);click("Delete image");await settle();expect(call).toHaveBeenLastCalledWith("/generated-usb","volume-a","delete",undefined);
 expect(host.querySelectorAll<HTMLInputElement>('input[type="radio"]')[1]!.checked).toBe(true);expect(error).not.toHaveBeenCalled();
});
it("cancels a chooser without writing or changing mode", async()=>{
 const call=vi.fn((_path: string, _id: string, _action: string)=>Promise.resolve(initial));__setBackend({jogImage:call,pickImage:()=>Promise.resolve(null)} as unknown as Backend);
 render();await settle();click("Import image file");await settle();expect(call).toHaveBeenCalledTimes(1);expect(host.querySelector('fieldset')?.disabled).toBe(false);expect(error).not.toHaveBeenCalled();
});
it("retires an undispatched chooser on same-path replacement", async()=>{
 const choice=pending<string|null>();const call=vi.fn((_path: string, _id: string, _action: string)=>Promise.resolve(initial));__setBackend({jogImage:call,pickImage:()=>choice.promise} as unknown as Backend);
 render();await settle();click("Import image file");await settle();render("volume-b");await settle();choice.resolve("/generated/old.png");await settle();
 expect(call.mock.calls.map(c=>c[2])).toEqual(["read","read"]);expect(error).not.toHaveBeenCalled();
});
it("retires an already dispatched old result without changing the replacement", async()=>{
 const old=pending<JogImageState>();const call=vi.fn((_path:string,id:string,action:string)=>action==="custom"?old.promise:Promise.resolve(id==="volume-b"?{mode:"artwork",image}:initial));
 __setBackend({jogImage:call} as unknown as Backend);render();await settle();custom();await settle();render("volume-b");await settle();old.resolve({mode:"custom",image:null});await settle();
 expect(host.querySelector("img")?.src).toBe(image);expect(host.querySelectorAll<HTMLInputElement>('input[type="radio"]')[0]!.checked).toBe(true);
});
it("keeps a failed action visible and permits a later action; callback changes do not retire work", async()=>{
 const held=pending<JogImageState>();let first=true;const call=vi.fn((_path:string,_id:string,action:string)=>action==="read"?Promise.resolve(initial):first?(first=false,Promise.reject(new Error("generated refusal"))):held.promise);
 __setBackend({jogImage:call} as unknown as Backend);render();await settle();custom();await settle();expect(host.querySelector('[role="alert"]')?.textContent).toBe("generated refusal");
 custom();await settle();render("volume-a",vi.fn());await settle();held.resolve({mode:"custom",image:null});await settle();expect(host.querySelector('fieldset')?.disabled).toBe(false);expect(host.querySelectorAll<HTMLInputElement>('input[type="radio"]')[1]!.checked).toBe(true);
});
it("offers deletion for invalid images while sync disables device writes", async()=>{
 const call=vi.fn(()=>Promise.resolve({mode:null,image:null,imagePresent:true,imageError:"invalid image"}));__setBackend({jogImage:call} as unknown as Backend);
 render();await settle();expect(Array.from(host.querySelectorAll<HTMLButtonElement>('button')).find(b=>b.textContent==="Delete image")?.disabled).toBe(false);
 render("volume-a",error,true);click("Delete image");await settle();expect(call).toHaveBeenCalledTimes(1);
});
it("Preferences is an app-owned image source with no device mode or automatic USB actions", async()=>{
 const call=vi.fn((_path:null,_id:null,action:string)=>Promise.resolve({mode:null,image:action==="import"?image:null}));__setBackend({jogImage:call,pickImage:()=>Promise.resolve("/generated/preference.png")} as unknown as Backend);
 act(()=>root.render(<JogImageControls />));await settle();expect(host.querySelector('input[type="radio"]')).toBeNull();expect(host.textContent).not.toContain("Copy from");
 click("Import image file");await settle();expect(call).toHaveBeenLastCalledWith(null,null,"import","/generated/preference.png");
});
