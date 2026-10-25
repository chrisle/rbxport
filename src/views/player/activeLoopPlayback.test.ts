import { describe, expect, it } from "vitest";
import type { Cue } from "@/ipc/types";
import { activeMemoryLoop } from "./activeLoopPlayback";

const cue = (values: Partial<Cue>): Cue => ({
  id: "1", positionMs: 2_000, outMs: 4_000, letter: "", memory: true,
  colour: null, ...values,
});

describe("saved Active Loop candidate", () => {
  it("selects only a valid Kind-4 memory loop", () => {
    expect(activeMemoryLoop([
      cue({ id: "plain", activeLoop: false }),
      cue({ id: "hot", letter: "A", memory: false, activeLoop: true }),
      cue({ id: "point", outMs: 0, activeLoop: true }),
      cue({ id: "active", activeLoop: true }),
    ])?.id).toBe("active");
  });

  it("has no candidate after the mark is cleared", () => {
    expect(activeMemoryLoop([cue({ activeLoop: false })])).toBeNull();
  });
});
