// @vitest-environment jsdom
import { afterEach, describe, expect, it } from "vitest";

import { browserFocus, followBrowserFocus, onBrowserFocus, paneAt, setBrowserFocus } from "./browserFocus";

function pane(kind: string, side: string): HTMLElement {
  const el = document.createElement("div");
  el.setAttribute("data-browser-pane", kind);
  el.setAttribute("data-browser-side", side);
  document.body.append(el);
  return el;
}

afterEach(() => {
  document.body.replaceChildren();
  setBrowserFocus(null);
});

describe("browserFocus", () => {
  it("starts with nothing focused, as a fresh rekordbox does", () => {
    expect(browserFocus()).toBeNull();
  });

  it("names the pane a press landed in", () => {
    const row = document.createElement("span");
    pane("list", "sub").append(row);
    expect(paneAt(row)).toEqual({ pane: "list", side: "sub" });
    expect(paneAt(pane("tree", "main"))).toEqual({ pane: "tree", side: "main" });
  });

  it("leaves the focus alone for a press outside the browser, and drops it for a text field", () => {
    const deck = document.createElement("button");
    document.body.append(deck);
    expect(paneAt(deck)).toBeUndefined();
    const search = document.createElement("input");
    pane("list", "main").append(search);
    expect(paneAt(search)).toBeNull();
  });

  it("follows presses and tells the listeners", () => {
    const stop = followBrowserFocus(window);
    let calls = 0;
    const off = onBrowserFocus(() => { calls += 1; });
    const tree = pane("tree", "main");
    const deck = document.createElement("button");
    document.body.append(deck);
    tree.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    expect(browserFocus()).toEqual({ pane: "tree", side: "main" });
    deck.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    expect(browserFocus()).toEqual({ pane: "tree", side: "main" });
    expect(calls).toBe(1);
    off();
    stop();
  });
});
