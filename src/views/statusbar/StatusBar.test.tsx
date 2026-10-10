/** @vitest-environment jsdom */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { StatusBar } from "./StatusBar";

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

it("opens the application log when the name and version are right-clicked", () => {
  const onOpenLog = vi.fn();
  act(() => root.render(<StatusBar version="1.2.3" onOpenLog={onOpenLog} />));

  const logo = host.querySelector<HTMLElement>("footer > span");
  expect(logo?.textContent).toBe("rbxport 1.2.3");
  const event = new MouseEvent("contextmenu", { bubbles: true, cancelable: true });
  act(() => {
    logo?.dispatchEvent(event);
  });

  expect(event.defaultPrevented).toBe(true);
  expect(onOpenLog).toHaveBeenCalledOnce();
});

it("names read-only as a library state", () => {
  act(() => root.render(<StatusBar readOnly />));

  const badge = host.querySelector<HTMLButtonElement>("footer > button");
  expect(badge?.textContent).toBe("Library read-only");
  expect(badge?.title).toContain("Editing is locked while rekordbox is running");
});

it("offers the support action when it is available", () => {
  const onSupport = vi.fn();
  act(() => root.render(<StatusBar onSupport={onSupport} />));

  const button = Array.from(host.querySelectorAll("button")).find(candidate => candidate.textContent?.includes("Support rbxport"));
  act(() => button?.click());

  expect(button).toBeDefined();
  expect(onSupport).toHaveBeenCalledOnce();
});

it("lists the running job and then those waiting, each with its own Stop", () => {
  const onStopJob = vi.fn();
  act(() => root.render(<StatusBar onStopJob={onStopJob} jobs={[
    { id: 1, label: "Adding 495 to Set", done: 99, total: 495, state: "running", target: "p", pendingRows: 396, stoppable: true, holdsAnalysis: false },
    { id: 2, label: "Removing 10 from the collection", done: 0, total: 10, state: "queued", target: null, pendingRows: 0, stoppable: true, holdsAnalysis: false },
  ]} />));
  const meters = [...host.querySelectorAll("[data-state]")];
  expect(meters.map((m) => m.getAttribute("data-state"))).toEqual(["running", "queued"]);
  expect(meters[0]?.textContent).toContain("(20%)");
  expect(meters[1]?.textContent).toContain("Queued");
  const stops = host.querySelectorAll<HTMLButtonElement>("button[aria-label^='Stop']");
  expect(stops).toHaveLength(2);
  act(() => stops[1]?.click());
  expect(onStopJob).toHaveBeenCalledWith(2);
});

it("shows a search that cannot be stopped without Stop, and analysis waiting behind it as Queued (#286)", () => {
  const onStopJob = vi.fn();
  act(() => root.render(<StatusBar onStopJob={onStopJob} jobs={[
    { id: 3, label: "Searching...", done: 0, total: 0, state: "running", target: null, pendingRows: 0, stoppable: false, holdsAnalysis: true },
  ]} analysisProgress={{ completed: 0, total: 1, waiting: true }} onCancelAnalysis={() => undefined} />));
  const search = host.querySelector("[data-state='running']");
  expect(search?.textContent).toBe("Searching...");
  expect(host.querySelector("button[aria-label^='Stop']")).toBeNull();
  expect(host.textContent).toContain("Analyzing 1 trackQueued");
  expect(host.querySelector("progress[aria-label='Analysis progress']")).toBeNull();

  act(() => root.render(<StatusBar analysisProgress={{ completed: 0, total: 1 }} />));
  expect(host.querySelector("progress[aria-label='Analysis progress']")).not.toBeNull();
  expect(host.textContent).not.toContain("Queued");
});
