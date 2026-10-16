import { describe, expect, it, vi } from "vitest";

import { hasCommand, registerCommand, registerCommands, runCommand } from "./commands";

describe("the shared commands", () => {
  it("run nothing until something on screen answers them", () => {
    expect(hasCommand("playPause", "a")).toBe(false);
    expect(runCommand("playPause", { scope: "a" })).toBe(false);
  });

  it("give every entry point the same handler, and say whether it applied", () => {
    const handler = vi.fn(() => true);
    const release = registerCommand("analyseSelection", handler);
    // A key, a menu item and a button all come through here.
    expect(runCommand("analyseSelection", { repeat: false })).toBe(true);
    expect(runCommand("analyseSelection")).toBe(true);
    expect(handler).toHaveBeenNthCalledWith(1, { repeat: false });
    expect(handler).toHaveBeenNthCalledWith(2, { repeat: false });
    release();
    expect(runCommand("analyseSelection")).toBe(false);
  });

  it("pass a held key's repeats on, for a once-per-press command to ignore", () => {
    const handler = vi.fn(() => true);
    const release = registerCommand("memoryCue", handler, "a");
    runCommand("memoryCue", { scope: "a", repeat: true });
    expect(handler).toHaveBeenCalledWith({ repeat: true });
    release();
  });

  it("keep Player A's command apart from Player B's", () => {
    const a = vi.fn(() => true);
    const b = vi.fn(() => false);
    const release = registerCommands({ playPause: a }, "a");
    const releaseB = registerCommands({ playPause: b }, "b");
    expect(runCommand("playPause", { scope: "a" })).toBe(true);
    // B's deck is empty, so its command does not apply.
    expect(runCommand("playPause", { scope: "b" })).toBe(false);
    // Unscoped is neither deck's.
    expect(runCommand("playPause")).toBe(false);
    expect(a).toHaveBeenCalledTimes(1);
    expect(b).toHaveBeenCalledTimes(1);
    release();
    releaseB();
    expect(hasCommand("playPause", "a")).toBe(false);
  });

  it("hand a command back to the earlier owner when a newer one goes", () => {
    const first = vi.fn(() => true);
    const second = vi.fn(() => true);
    const releaseFirst = registerCommand("selectAll", first, "main");
    const releaseSecond = registerCommand("selectAll", second, "main");
    runCommand("selectAll", { scope: "main" });
    expect(second).toHaveBeenCalledTimes(1);
    expect(first).not.toHaveBeenCalled();
    releaseSecond();
    releaseSecond();
    runCommand("selectAll", { scope: "main" });
    expect(first).toHaveBeenCalledTimes(1);
    releaseFirst();
    expect(hasCommand("selectAll", "main")).toBe(false);
  });
});
