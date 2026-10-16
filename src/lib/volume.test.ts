import { describe, expect, it } from "vitest";

import { gainToKnob, knobLabel, knobToDb, knobToGain, KNOB_FULL, stepMasterGain, toggleMute } from "./volume";

describe("the master knob's scale", () => {
  it("puts 10 at -1 dB and 11 at +2 dB", () => {
    expect(knobToDb(10)).toBeCloseTo(-1, 6);
    expect(knobToDb(11)).toBe(2);
    expect(knobToGain(11)).toBeCloseTo(1.259, 3);
    expect(knobToGain(10)).toBeCloseTo(0.891, 3);
    expect(knobToGain(0)).toBe(0);
    expect(knobToDb(0)).toBe(Number.NEGATIVE_INFINITY);
  });

  it("tapers forty decibels across the travel", () => {
    expect(knobToDb(1)).toBeCloseTo(-41, 6);
    expect(knobToDb(5)).toBeCloseTo(-13.04, 2);
  });

  it("reads the engine's gain back as the reading that set it", () => {
    for (const reading of [0, 1, 2.5, 5, 7.5, 10, 11]) {
      expect(gainToKnob(knobToGain(reading))).toBeCloseTo(reading, 6);
    }
    expect(gainToKnob(0.95)).toBe(11);
    expect(knobLabel(9.6)).toBe("10");
    expect(knobLabel(11)).toBe("11");
  });
});

describe("the Volume, Volume Down and Mute commands", () => {
  it("step the knob one reading, within 0 and 11", () => {
    expect(gainToKnob(stepMasterGain(knobToGain(5), 1))).toBeCloseTo(6, 6);
    expect(gainToKnob(stepMasterGain(knobToGain(5), -1))).toBeCloseTo(4, 6);
    expect(stepMasterGain(knobToGain(KNOB_FULL), 1)).toBe(knobToGain(KNOB_FULL));
    expect(stepMasterGain(0, -1)).toBe(0);
    expect(gainToKnob(stepMasterGain(0, 1))).toBeCloseTo(1, 6);
  });

  it("mute to silence and put the level back on the second press", () => {
    const muted = toggleMute(0.5, null);
    expect(muted).toEqual({ gain: 0, mutedFrom: 0.5 });
    expect(toggleMute(muted.gain, muted.mutedFrom)).toEqual({ gain: 0.5, mutedFrom: null });
  });

  it("leave a master turned all the way down alone", () => {
    expect(toggleMute(0, null)).toEqual({ gain: 0, mutedFrom: null });
  });
});
