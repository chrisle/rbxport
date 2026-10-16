/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { __setBackend } from "@/ipc/client";
import type { Backend, LogLine } from "@/ipc/types";
import { LogView } from "./LogView";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

let host: HTMLDivElement;
let root: Root;

const at = Date.UTC(2026, 9, 16, 10, 55, 7, 0);
const lines: LogLine[] = [
  { seq: 1, timeMs: at, level: "debug", target: "rbxport_lib::logging", message: "logging to stdout and a daily file" },
  { seq: 2, timeMs: at, level: "warn", target: "rbxport::status", message: "The playlist could not be renamed." },
  { seq: 3, timeMs: at, level: "error", target: "rbxport_lib::error", message: "internal error error.detail=disk full" },
];

const logLines = vi.fn((after: number) => Promise.resolve({ lines: lines.filter(l => l.seq > after), oldest: 1 }));
const saveLogText = vi.fn((_text: string) => Promise.resolve("/tmp/rbxport-484.txt"));
const openLogFolder = vi.fn(() => Promise.resolve());

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  vi.useFakeTimers();
  __setBackend({ logLines, saveLogText, openLogFolder } as unknown as Backend);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  __setBackend(null);
  vi.useRealTimers();
  vi.clearAllMocks();
});

async function flush() {
  await act(async () => { await vi.advanceTimersByTimeAsync(0); });
}

const shown = () => [...host.querySelectorAll("[role=log] [data-level]")].map(line => line.textContent);

function change(element: HTMLInputElement | HTMLSelectElement, value: string) {
  // React tracks the value it last saw; the prototype's setter goes around it.
  const prototype = element instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
  act(() => {
    Object.getOwnPropertyDescriptor(prototype, "value")!.set!.call(element, value);
    element.dispatchEvent(new Event(element instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  });
}

it("shows what was logged, filtered by level and text, and saves what it shows", async () => {
  act(() => root.render(<LogView onClose={() => {}} />));
  await flush();
  expect(shown()).toEqual([
    "2026-10-16T10:55:07.000Z DEBUG rbxport_lib::logging: logging to stdout and a daily file",
    "2026-10-16T10:55:07.000Z  WARN rbxport::status: The playlist could not be renamed.",
    "2026-10-16T10:55:07.000Z ERROR rbxport_lib::error: internal error error.detail=disk full",
  ]);

  change(host.querySelector("select")!, "warn");
  expect(shown()).toHaveLength(2);
  change(host.querySelector<HTMLInputElement>("input[type=search]")!, "PLAYLIST");
  expect(shown()).toEqual(["2026-10-16T10:55:07.000Z  WARN rbxport::status: The playlist could not be renamed."]);
  expect(host.textContent).toContain("1 of 3 lines");

  const button = (label: string) => [...host.querySelectorAll("button")].find(b => b.textContent === label);
  await act(async () => { button("Save…")?.click(); await vi.advanceTimersByTimeAsync(0); });
  expect(saveLogText).toHaveBeenCalledWith("2026-10-16T10:55:07.000Z  WARN rbxport::status: The playlist could not be renamed.\n");
  expect(host.textContent).toContain("Saved 1 line.");

  await act(async () => { button("Open log folder")?.click(); await vi.advanceTimersByTimeAsync(0); });
  expect(openLogFolder).toHaveBeenCalledOnce();
});

it("asks only for lines after the newest it holds", async () => {
  act(() => root.render(<LogView onClose={() => {}} />));
  await flush();
  expect(logLines).toHaveBeenLastCalledWith(0);
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(logLines).toHaveBeenLastCalledWith(3);
  expect(shown()).toHaveLength(3);
});
