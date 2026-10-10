/** @vitest-environment jsdom */
/**
 * The bands a stored loop draws over the waveform. A hot cue saved as a loop
 * (`djmdCue` with an `OutMsec`) is a loop as much as a memory loop is, and
 * the waveform shows it from its in point to its out point; before #273 only
 * memory loops drew one.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import type { Cue } from "@/ipc/types";
import { CueMarkers } from "./Player";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const cue = (over: Partial<Cue>): Cue => ({
  id: "1", positionMs: 0, outMs: 0, letter: "", memory: false, colour: null, ...over,
});

const bands = () => [...host.querySelectorAll<HTMLElement>("[data-cue-loop]")];
/** A percentage from a style, to a tenth of a percent. */
const pct = (value: string) => Math.round(Number.parseFloat(value) * 10) / 10;

describe("stored loop bands", () => {
  it("draws a hot cue loop from its in point to its out point", () => {
    act(() => root.render(
      <CueMarkers totalMs={100_000} cues={[cue({ id: "b", letter: "B", positionMs: 20_000, outMs: 25_000 })]} />,
    ));
    expect(bands().map((b) => [b.dataset.cueLoop, pct(b.style.left), pct(b.style.width)])).toEqual([["B", 20, 5]]);
  });

  it("draws memory and hot loops alike, and nothing for a plain cue", () => {
    act(() => root.render(
      <CueMarkers totalMs={100_000} cues={[
        cue({ id: "m", memory: true, positionMs: 10_000, outMs: 12_000 }),
        cue({ id: "a", letter: "A", positionMs: 30_000 }),
        cue({ id: "b", letter: "B", positionMs: 40_000, outMs: 41_000 }),
      ]} />,
    ));
    expect(bands().map((b) => b.dataset.cueLoop)).toEqual(["memory", "B"]);
  });

  it("clips a hot loop to the detail window", () => {
    act(() => root.render(
      <CueMarkers band="detail" totalMs={100_000} window={{ from: 0.5, to: 0.6 }}
        cues={[cue({ id: "b", letter: "B", positionMs: 55_000, outMs: 70_000 })]} />,
    ));
    expect(bands().map((b) => [pct(b.style.left), pct(b.style.width)])).toEqual([[50, 50]]);
  });
});
