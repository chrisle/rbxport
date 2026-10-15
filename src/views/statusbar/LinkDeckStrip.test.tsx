/**
 * @vitest-environment jsdom
 *
 * The Pro DJ LINK strip: hidden until a player or mixer is heard, then the
 * LINK button alone in a full-height strip; once on, a deck per player with
 * the mixer between them, each deck a drop target while a track is dragged.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import type { LinkPeerSeen, LinkStatus } from "@/ipc/types";
import { LinkDeckStrip } from "./LinkDeckStrip";

declare global {
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  (window as unknown as Record<string, unknown>)["__TAURI_INTERNALS__"] = {};
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const off = (players: LinkStatus["players"] = []): LinkStatus => ({
  on: false,
  problem: null,
  interface: null,
  players,
  interfaces: [],
  master: false,
  masterBpm: 120,
  state: "off",
  number: null,
});

const on = (players: LinkStatus["players"] = []): LinkStatus => ({ ...off(players), on: true, state: "up", number: 17 });

const peer = (over: Partial<LinkPeerSeen> = {}): LinkPeerSeen => ({
  number: 1,
  name: "CDJ-3000",
  kind: "player",
  address: "192.168.1.152",
  ...over,
});

const cdj = (over: Partial<LinkStatus["players"][number]> = {}): LinkStatus["players"][number] => ({
  number: 1,
  name: "CDJ-3000",
  kind: "player",
  address: "192.168.1.152",
  loaded: null,
  playing: false,
  master: false,
  sync: false,
  cued: false,
  linkCue: false,
  mounted: true,
  ...over,
});

function render(props: Parameters<typeof LinkDeckStrip>[0]) {
  act(() => root.render(<LinkDeckStrip {...props} />));
}

describe("LinkDeckStrip", () => {
  it("draws nothing until a player or mixer is heard", () => {
    render({ peers: [], link: off(), onToggle: () => {} });
    expect(host.querySelector('[data-testid="link-deck-strip"]')).toBeNull();

    // A source of our own is not something to link to.
    render({ peers: [peer({ kind: "rekordbox" })], link: off(), onToggle: () => {} });
    expect(host.querySelector('[data-testid="link-deck-strip"]')).toBeNull();
  });

  it("is the LINK button alone once a device is heard, before LINK is on", () => {
    render({ peers: [peer()], link: off(), onToggle: () => {} });

    expect(host.querySelector('[data-testid="link-deck-strip"]')).not.toBeNull();
    expect(host.querySelector('[data-testid="link-button"]')?.getAttribute("aria-pressed")).toBe("false");
    // No decks until it is on: the strip is otherwise empty.
    expect(host.querySelector('[aria-label="Players on the link"]')).toBeNull();
    expect(host.querySelectorAll('[aria-label^="Player "]').length).toBe(0);
  });

  it("turns LINK on from the button", () => {
    let toggled = 0;
    render({ peers: [peer()], link: off(), onToggle: () => toggled++ });
    act(() => (host.querySelector('[data-testid="link-button"]') as HTMLButtonElement).click());
    expect(toggled).toBe(1);
  });

  it("draws a deck per player with its number, master and loaded track", () => {
    const link = on([
      cdj({ number: 1, loaded: { id: "42", title: "Bora Bora", artist: "MAGAN & RODRIGUEZ" }, playing: true, master: true }),
      cdj({ number: 2, address: "192.168.1.153" }),
    ]);
    render({ peers: [peer(), peer({ number: 2 })], link, onToggle: () => {} });

    expect(host.querySelector('[data-testid="link-deck-strip"]')).not.toBeNull();
    expect(host.querySelector('[data-testid="link-button"]')?.getAttribute("aria-pressed")).toBe("true");
    expect(host.querySelectorAll('[aria-label^="Player "]').length).toBe(2);
    expect(host.textContent).toContain("Bora Bora");
    expect(host.querySelector('[aria-label="Player 1"] [title="Bora Bora"]')).not.toBeNull();
    expect(host.textContent).toContain("MASTER");
    // A playing deck says PLAY.
    expect(host.querySelector('[aria-label="Player 1"]')?.textContent).toContain("PLAY");
    expect(host.querySelector('[aria-label="Player 1"]')?.hasAttribute("data-loaded")).toBe(true);
    expect(host.querySelector('[aria-label="Player 2"]')?.hasAttribute("data-loaded")).toBe(false);
  });

  it("lights each lamp from what the player reports", () => {
    const deck = (n: number) => host.querySelector(`[aria-label="Player ${n}"]`);
    const lit = (n: number, lamp: string) =>
      [...(deck(n)?.querySelectorAll("[data-on]") ?? [])].some((el) => el.textContent === lamp);

    render({
      peers: [peer()],
      link: on([
        // Playing: PLAY, and the deck is marked so the lamp goes green.
        cdj({ number: 1, loaded: { id: "1", title: "A", artist: "X" }, playing: true }),
        // Cued at its cue point: CUE.
        cdj({ number: 2, address: "192.168.1.153", loaded: { id: "2", title: "B", artist: "Y" }, cued: true }),
        // Loaded but neither playing nor cued: no lamp at all. This is the
        // case that used to read CUE regardless.
        cdj({ number: 3, address: "192.168.1.154", loaded: { id: "3", title: "C", artist: "Z" } }),
      ]),
      onToggle: () => {},
    });

    expect(deck(1)?.textContent).toContain("PLAY");
    expect(deck(1)?.hasAttribute("data-playing")).toBe(true);
    expect(deck(2)?.textContent).toContain("CUE");
    expect(deck(2)?.hasAttribute("data-playing")).toBe(false);
    expect(deck(3)?.textContent).not.toContain("CUE");
    expect(deck(3)?.textContent).not.toContain("PLAY");

    // MASTER and SYNC light independently of the track.
    render({
      peers: [peer()],
      link: on([
        cdj({ number: 1, master: true }),
        cdj({ number: 2, address: "192.168.1.153", sync: true }),
      ]),
      onToggle: () => {},
    });
    expect(lit(1, "MASTER")).toBe(true);
    expect(lit(1, "SYNC")).toBe(false);
    expect(lit(2, "SYNC")).toBe(true);
    expect(lit(2, "MASTER")).toBe(false);
  });

  it("shows the mixer's LINK CUE lamp, lit only when it is on", () => {
    const mixer = () => host.querySelector('[aria-label="Mixer 33"]');
    const djm = (over = {}) =>
      cdj({ number: 33, name: "DJM-V5", kind: "mixer", address: "192.168.1.155", ...over });

    render({ peers: [peer()], link: on([cdj({ number: 1 }), djm()]), onToggle: () => {} });
    expect(mixer()?.textContent).toContain("LINK CUE");
    expect([...(mixer()?.querySelectorAll("[data-on]") ?? [])].some((el) => el.textContent === "LINK CUE")).toBe(false);

    render({ peers: [peer()], link: on([cdj({ number: 1 }), djm({ linkCue: true })]), onToggle: () => {} });
    expect([...(mixer()?.querySelectorAll("[data-on]") ?? [])].some((el) => el.textContent === "LINK CUE")).toBe(true);
  });

  it("seats the mixer between the decks, as rekordbox does", () => {
    const link = on([
      cdj({ number: 1 }),
      cdj({ number: 2, address: "192.168.1.153" }),
      cdj({ number: 33, name: "DJM-V5", kind: "mixer", address: "192.168.1.155", master: true }),
    ]);
    render({ peers: [peer()], link, onToggle: () => {} });

    const cells = [...host.querySelectorAll("[aria-label^='Player '], [aria-label^='Mixer ']")].map((el) =>
      el.getAttribute("aria-label"),
    );
    expect(cells).toEqual(["Player 1", "Mixer 33", "Player 2"]);
    expect(host.textContent).toContain("MIXER");
  });

  it("makes a player deck a drop target and reports the drop while dragging", () => {
    const dropped: number[] = [];
    render({
      peers: [peer()],
      link: on([cdj({ number: 3 })]),
      onToggle: () => {},
      dragging: true,
      onDropToPlayer: (n) => dropped.push(n),
    });

    const deck = host.querySelector("[data-drop-target]");
    expect(deck).not.toBeNull();
    act(() => {
      deck?.dispatchEvent(new Event("drop", { bubbles: true }));
    });
    expect(dropped).toEqual([3]);
  });

  // #435: an XDJ-RX3 sits on the link as decks 9 and 10, both announced with
  // device type 0x07. The backend reads that type as "player" (src-tauri
  // link.rs `kind_name`); 1.6.0 read it as "device", which drew each deck as
  // a mixer cell that never took a drop.
  it("makes both decks of an XDJ-RX3 drop targets and reports which one took the drop", () => {
    const dropped: number[] = [];
    const rx3 = (number: number) => cdj({ number, name: "XDJ-RX3", address: "169.254.20.2" });
    render({
      peers: [peer({ number: 9, name: "XDJ-RX3", address: "169.254.20.2" })],
      link: on([rx3(9), rx3(10)]),
      onToggle: () => {},
      dragging: true,
      onDropToPlayer: (n) => dropped.push(n),
    });

    const decks = [...host.querySelectorAll("[data-drop-target]")];
    expect(decks.map((el) => el.getAttribute("aria-label"))).toEqual(["Player 9", "Player 10"]);
    for (const deck of decks) {
      const over = new Event("dragover", { bubbles: true, cancelable: true });
      Object.defineProperty(over, "dataTransfer", { value: { dropEffect: "none" } });
      act(() => {
        deck.dispatchEvent(over);
      });
      // A dragover left alone is the forbidden cursor the reporter saw.
      expect(over.defaultPrevented).toBe(true);
      act(() => {
        deck.dispatchEvent(new Event("drop", { bubbles: true }));
      });
    }
    expect(dropped).toEqual([9, 10]);
  });

  it("does not offer a device it cannot read as a player as a deck", () => {
    render({
      peers: [peer()],
      link: on([cdj({ number: 9, name: "XDJ-RX3", kind: "device" })]),
      onToggle: () => {},
      dragging: true,
      onDropToPlayer: () => {},
    });
    expect(host.querySelector("[data-drop-target]")).toBeNull();
    expect(host.querySelector('[aria-label="Mixer 9"]')).not.toBeNull();
  });

  it("does not make decks drop targets when nothing is being dragged", () => {
    render({ peers: [peer()], link: on([cdj({ number: 3 })]), onToggle: () => {}, onDropToPlayer: () => {} });
    expect(host.querySelector("[data-drop-target]")).toBeNull();
  });

  it("marks LINK unavailable and explains why on click when it is blocked", () => {
    const problem = "rekordbox is running and holds the link ports. Quit it to turn LINK on.";
    let toggled = 0;
    // Blocked with nothing on the network is no strip at all: the reason is
    // told in Preferences › DJ System instead.
    render({ peers: [], link: { ...off(), problem }, onToggle: () => toggled++ });
    expect(host.querySelector('[data-testid="link-deck-strip"]')).toBeNull();

    // With a player heard but LINK still blocked, there is no button to
    // press either: a LINK button that cannot do anything is not shown.
    render({ peers: [peer({ number: 2 })], link: { ...off(), problem }, onToggle: () => toggled++ });
    expect(host.querySelector('[data-testid="link-deck-strip"]')).toBeNull();
    expect(toggled).toBe(0);
  });

  it("shows the master clock and drives its controls", () => {
    const masterCalls: boolean[] = [];
    const nudges: number[] = [];
    let took = 0;
    const link = { ...on([cdj({ number: 1, master: true })]), master: false, masterBpm: 130 };
    render({
      peers: [peer()],
      link,
      onToggle: () => {},
      onSetMaster: (v) => masterCalls.push(v),
      onNudgeMaster: (d) => nudges.push(d),
      onTakeMasterTempo: () => took++,
    });

    // The BPM is shown to two places, and a MASTER toggle exists.
    expect(host.textContent).toContain("130.00");
    expect([...host.querySelectorAll("button")].some((b) => b.textContent === "MASTER")).toBe(true);

    // −/+ nudge by ∓1, MASTER toggles on, ⟳ takes the tempo.
    act(() => (host.querySelector('[aria-label="Master tempo down"]') as HTMLButtonElement).click());
    act(() => (host.querySelector('[aria-label="Master tempo up"]') as HTMLButtonElement).click());
    expect(nudges).toEqual([-1, 1]);
    act(() => (host.querySelector('button[aria-pressed="false"]') as HTMLButtonElement).click());
    expect(masterCalls).toEqual([true]);
    act(() => (host.querySelector('[aria-label="Take the master player\'s tempo"]') as HTMLButtonElement).click());
    expect(took).toBe(1);
  });

  it("disables ⟳ when no player is master", () => {
    const link = { ...on([cdj({ number: 1, master: false })]), master: false, masterBpm: 120 };
    render({ peers: [peer()], link, onToggle: () => {}, onTakeMasterTempo: () => {} });
    const recycle = host.querySelector('[aria-label="Take the master player\'s tempo"]') as HTMLButtonElement;
    expect(recycle.disabled).toBe(true);
  });
});
