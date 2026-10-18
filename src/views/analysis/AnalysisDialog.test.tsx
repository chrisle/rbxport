/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { AnalysisDialog, type AnalysisChoice } from "./AnalysisDialog";

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  // jsdom has no modal dialogs.
  HTMLDialogElement.prototype.showModal = vi.fn();
  HTMLDialogElement.prototype.close = vi.fn();
  localStorage.clear();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});

async function pressOk(props: { auto?: boolean; initialFirstBeatCue: boolean; initialKey?: boolean }): Promise<AnalysisChoice | undefined> {
  let chosen: AnalysisChoice | undefined;
  await act(async () => {
    root.render(<AnalysisDialog count={1} initialMode="rbxport" initialBpmRange="70-180" {...props}
      onConfirm={settings => { chosen = settings; }} onCancel={() => {}} />);
    await Promise.resolve();
  });
  const ok = [...host.querySelectorAll("button")].find(button => button.textContent === "OK");
  await act(async () => { ok?.click(); await Promise.resolve(); });
  return chosen;
}

it("takes the first-beat cue from Preferences over an earlier confirmed choice (#343)", async () => {
  // Confirmed once while the switch was off, by a version that remembered it.
  localStorage.setItem("rbxport.analysisChoice", JSON.stringify({ mode: "rbxport", bpmGrid: true, key: false, highPrecision: true, minBpm: 70, maxBpm: 180, firstBeatCue: false }));
  const chosen = await pressOk({ initialFirstBeatCue: true });
  expect(chosen).toMatchObject({ firstBeatCue: true, key: false });
});

it("follows the switch when it is turned off again", async () => {
  expect(await pressOk({ initialFirstBeatCue: true })).toMatchObject({ firstBeatCue: true });
  await act(async () => { root.render(<></>); await Promise.resolve(); });
  expect(await pressOk({ initialFirstBeatCue: false })).toMatchObject({ firstBeatCue: false });
});

it("follows the switch at the launch Auto Analysis prompt", async () => {
  expect(await pressOk({ auto: true, initialFirstBeatCue: true })).toMatchObject({ firstBeatCue: true, bpmGrid: true });
});

it("explains the chosen analysis mode (#125)", async () => {
  await act(async () => {
    root.render(<AnalysisDialog count={1} initialMode="rekordbox" initialBpmRange="70-180" initialFirstBeatCue={false}
      onConfirm={() => {}} onCancel={() => {}} />);
    await Promise.resolve();
  });
  expect(host.textContent).toContain("one steady BPM with a single, constant beat grid");
  const select = host.querySelector("select") as HTMLSelectElement;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value")?.set?.call(select, "rbxport");
    select.dispatchEvent(new Event("change", { bubbles: true }));
    await Promise.resolve();
  });
  expect(host.textContent).toContain("variable beat grid");
  expect(host.textContent).not.toContain("one steady BPM");
});

it("starts KEY unticked when Analyze key is off in Preferences, even after KEY was confirmed", async () => {
  localStorage.setItem("rbxport.analysisChoice", JSON.stringify({ key: true, bpmGrid: true }));
  expect(await pressOk({ initialFirstBeatCue: false, initialKey: false })).toMatchObject({ key: false, bpmGrid: true });
  expect(await pressOk({ auto: true, initialFirstBeatCue: false, initialKey: false })).toMatchObject({ key: false, bpmGrid: true });
});

it("keeps KEY ticked by default and lets it be ticked for one batch when Analyze key is off", async () => {
  expect(await pressOk({ initialFirstBeatCue: false })).toMatchObject({ key: true });
  await act(async () => { root.render(<></>); await Promise.resolve(); });
  let chosen: AnalysisChoice | undefined;
  await act(async () => {
    root.render(<AnalysisDialog count={1} initialMode="rbxport" initialBpmRange="70-180" initialFirstBeatCue={false} initialKey={false}
      onConfirm={settings => { chosen = settings; }} onCancel={() => {}} />);
    await Promise.resolve();
  });
  const box = [...host.querySelectorAll("label")].find(label => label.textContent === "KEY")?.querySelector("input");
  await act(async () => { box?.click(); await Promise.resolve(); });
  await act(async () => { [...host.querySelectorAll("button")].find(b => b.textContent === "OK")?.click(); await Promise.resolve(); });
  expect(chosen).toMatchObject({ key: true });
});
