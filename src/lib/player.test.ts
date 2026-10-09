import { describe, expect, it } from "vitest";

import {
  type BeatGrid,
  tempoAnnotations,
  beatAtMs,
  beatLoopRange,
  loopBeatsLabel,
  resizedLoopRange,
  wrapIntoLoop,
  BEATS_PER_BAR,
  DETAIL_BARS,
  beatsIn,
  detailSpan,
  dragSeconds,
  headPercent,
  JUMP_SIZES,
  JUMP_SIZE_ID,
  jumpSizeById,
  jumpStepSeconds,
  FINE_JUMP_SECONDS,
  jumpSeconds,
  nextJumpSize,
  showsEveryBeat,
  waveSlice,
  tempoToFader,
  faderToTempo,
  tempoForTypedBpm,
  beatCountText,
  detailWaveWindow,
  detailWaveOriginMs,
  clickSeconds,
  isClick,
  ZOOM_STEPS,
  zoomBy,
  createWheelZoomGate,
  tempoChangeAtMs,
  phraseKind,
  phraseSpans,
  splitTime,
  memoryTime,
  cuesFor,
  nearestBeatMs,
  quantizedLaunchMs,
  foldIntoLoop,
  callLeavesFrom,
  needsRedraw,
  NO_BEATS,
  scrollOffset,
  pressCue,
  releaseCue,
  parseBeatGrid,
  subdivideGrid,
  tempoAtMs,
  windowAround,
} from "./player";

describe("detailSpan", () => {
  it("shows the same number of bars whatever the tempo", () => {
    // Twelve bars at 128 BPM is 22.5 s. A fixed fraction cannot do this: on a
    // three-minute track and a ninety-minute mix it shows wildly different
    // amounts of music, and neither matches a CDJ.
    const seconds = (DETAIL_BARS * BEATS_PER_BAR * 60) / 128;
    expect(seconds).toBeCloseTo(22.5, 5);
    expect(detailSpan(DETAIL_BARS, 12_800, 338)).toBeCloseTo(seconds / 338, 6);
    // Same bars, faster track: a smaller slice of the same length.
    expect(detailSpan(DETAIL_BARS, 17_400, 338)).toBeLessThan(
      detailSpan(DETAIL_BARS, 12_800, 338),
    );
  });

  it("covers twice as much at twice the bars", () => {
    expect(detailSpan(24, 12_800, 338)).toBeCloseTo(detailSpan(12, 12_800, 338) * 2, 6);
  });

  it("never exceeds the whole track", () => {
    // Twelve bars of a very slow, very short track is longer than the track.
    expect(detailSpan(DETAIL_BARS, 6_000, 5)).toBe(1);
  });

  it("falls back rather than dividing by zero", () => {
    for (const [bpm, dur] of [[0, 338], [12_800, 0], [0, 0]] as const) {
      const span = detailSpan(DETAIL_BARS, bpm, dur);
      expect(span).toBeGreaterThan(0);
      expect(span).toBeLessThanOrEqual(1);
    }
  });
});

describe("windowAround", () => {
  it("centres on the playhead", () => {
    expect(windowAround(0.5, 0.1)).toEqual({ from: 0.45, to: 0.55 });
  });

  it("runs off the ends rather than sliding the head across the strip", () => {
    // The head is the middle of the strip, always. At the start of a track the
    // window hangs off the front and the left half draws empty — pinning it
    // instead makes the head mean "somewhere in here" for the first bars.
    expect(windowAround(0, 0.1)).toEqual({ from: -0.05, to: 0.05 });
    expect(windowAround(1, 0.1).from).toBeCloseTo(0.95, 6);
    expect(windowAround(1, 0.1).to).toBeCloseTo(1.05, 6);
  });

  it("handles a span wider than the track", () => {
    expect(windowAround(0.3, 5)).toEqual({ from: -2.2, to: 2.8 });
  });
});

describe("headPercent", () => {
  it("is the middle of the strip wherever the track is", () => {
    expect(headPercent()).toBe(50);
  });
});

describe("dragSeconds", () => {
  it("pulls the track the way the pointer moves", () => {
    // Dragging right brings earlier music into view, so the playhead goes back.
    expect(dragSeconds(100, 1000, 0.1, 300)).toBeCloseTo(-3, 6);
    expect(dragSeconds(-100, 1000, 0.1, 300)).toBeCloseTo(3, 6);
  });

  it("moves less music per pixel the further it is zoomed in", () => {
    expect(Math.abs(dragSeconds(50, 1000, 0.02, 300)))
      .toBeLessThan(Math.abs(dragSeconds(50, 1000, 0.2, 300)));
  });

  it("returns nothing rather than a NaN when there is nothing to drag", () => {
    for (const [dx, w, span, dur] of [
      [10, 0, 0.1, 300], [10, 100, 0.1, 0], [Number.NaN, 100, 0.1, 300],
    ] as const) {
      expect(dragSeconds(dx, w, span, dur)).toBe(0);
    }
  });
});

describe("splitTime", () => {
  it("splits minutes from tenths, the way rekordbox prints them", () => {
    expect(splitTime(338.1)).toEqual({ main: "05:38", tenths: "1" });
    expect(splitTime(0)).toEqual({ main: "00:00", tenths: "0" });
    expect(splitTime(65.95)).toEqual({ main: "01:05", tenths: "9" });
  });

  it("pads the minutes, so the readout does not shift at ten minutes", () => {
    // Unpadded, the whole row of readouts beside it moves by a character.
    expect(splitTime(599).main).toHaveLength(5);
    expect(splitTime(601).main).toHaveLength(5);
  });

  it("shows negative positions and handles NaN", () => {
    expect(splitTime(-5).main).toBe("−00:05");
    expect(splitTime(Number.NaN).main).toBe("00:00");
  });
});

describe("memoryTime", () => {
  it("prints to the millisecond, minutes padded like everything else", () => {
    expect(memoryTime(46)).toBe("00:00:046");
    expect(memoryTime(165_046)).toBe("02:45:046");
  });

  it("agrees with splitTime about the minutes and seconds", () => {
    // Two lists disagreeing about padding is what stops columns lining up.
    expect(memoryTime(338_100).startsWith(splitTime(338.1).main)).toBe(true);
  });

  it("shows negative positions and handles NaN", () => {
    expect(memoryTime(-1)).toBe("00:00:000");
    expect(memoryTime(Number.NaN)).toBe("00:00:000");
  });
});

describe("phraseKind", () => {
  it("gives each UP its own colour, as rekordbox does", () => {
    expect(phraseKind("UP 1")).toBe("up");
    expect(phraseKind("UP 2")).toBe("up2");
    expect(phraseKind("UP 3")).toBe("up3");
  });

  it("keys everything else off its first word", () => {
    expect(phraseKind("INTRO 2")).toBe("intro");
    expect(phraseKind("CHORUS 1")).toBe("chorus");
    expect(phraseKind("DOWN")).toBe("down");
    expect(phraseKind("OUTRO")).toBe("outro");
    expect(phraseKind("OUT")).toBe("outro");
  });

  it("still colours a label it does not know", () => {
    // A hole in the phrase bar reads as missing analysis, which is a
    // different problem from an unfamiliar label.
    expect(phraseKind("SOMETHING ELSE")).toBe("verse");
  });
});

describe("phraseSpans", () => {
  const phrases = [
    { timeMs: 0, beat: 1, label: "INTRO 2" },
    { timeMs: 30_000, beat: 65, label: "CHORUS 1" },
    { timeMs: 60_000, beat: 129, label: "DOWN" },
  ];

  it("runs each phrase to the start of the next, and the last to the end", () => {
    const spans = phraseSpans(phrases, 90_000);
    expect(spans.map((s) => [s.from, s.to])).toEqual([
      [0, 1 / 3],
      [1 / 3, 2 / 3],
      [2 / 3, 1],
    ]);
    expect(spans.map((s) => s.kind)).toEqual(["intro", "chorus", "down"]);
  });

  it("sorts before laying out, so an out-of-order tag still reads left to right", () => {
    const spans = phraseSpans([...phrases].reverse(), 90_000);
    expect(spans.map((s) => s.label)).toEqual(["INTRO 2", "CHORUS 1", "DOWN"]);
  });

  it("drops a zero-width phrase rather than drawing an invisible block", () => {
    const spans = phraseSpans(
      [{ timeMs: 0, beat: 1, label: "INTRO" }, { timeMs: 0, beat: 1, label: "CHORUS" }],
      90_000,
    );
    expect(spans).toHaveLength(1);
  });

  it("returns nothing for a track with no length", () => {
    expect(phraseSpans(phrases, 0)).toEqual([]);
  });

  it("clamps a phrase that starts past the end", () => {
    const spans = phraseSpans([{ timeMs: 999_999, beat: 9_999, label: "OUTRO" }], 90_000);
    expect(spans).toEqual([]);
  });

  it("places a phrase the beat grid did not reach, when the tempo is known", () => {
    // `timeMs` is absent on a track whose analysis is older than its length.
    // A beat and a tempo still say where the phrase is.
    const beatMs = 60_000 / 128;
    const spans = phraseSpans(
      [
        { timeMs: null, beat: 1, label: "INTRO" },
        { timeMs: null, beat: 129, label: "CHORUS" },
      ],
      90_000,
      beatMs,
    );
    expect(spans).toHaveLength(2);
    expect(spans[1]?.from).toBeCloseTo((128 * beatMs) / 90_000, 6);
  });

  it("drops an unresolved phrase rather than stacking it on the left edge", () => {
    // With no tempo there is nothing to place it from, and a block at zero
    // would read as a mangled track rather than as missing data.
    const spans = phraseSpans(
      [
        { timeMs: null, beat: 1, label: "INTRO" },
        { timeMs: 30_000, beat: 65, label: "CHORUS" },
      ],
      90_000,
    );
    expect(spans.map((s) => s.label)).toEqual(["CHORUS"]);
  });
});

describe("cuesFor", () => {
  const cues = [
    { memory: false, letter: "B", positionMs: 30_000 },
    { memory: true, letter: "", positionMs: 60_000 },
    { memory: false, letter: "A", positionMs: 10_000 },
    { memory: true, letter: "", positionMs: 5_000 },
  ];

  it("splits the one list the two tabs share", () => {
    // Memory cues and hot cues are the same rows told apart by a flag.
    expect(cuesFor(cues, "memory").map((c) => c.positionMs)).toEqual([5_000, 60_000]);
    expect(cuesFor(cues, "hotCue").map((c) => c.letter)).toEqual(["A", "B"]);
  });

  it("orders by position, so the list can be read as the track", () => {
    expect(cuesFor(cues, "hotCue").map((c) => c.positionMs)).toEqual([10_000, 30_000]);
  });

  it("lists nothing for the info tab, which shows fields instead", () => {
    expect(cuesFor(cues, "info")).toEqual([]);
  });

  it("does not reorder the array it was given", () => {
    const before = [...cues];
    cuesFor(cues, "memory");
    expect(cues).toEqual(before);
  });
});

describe("parseBeatGrid", () => {
  /**
   * The backend's encoding: a little-endian `u32` of ms, the beat's number,
   * then a little-endian `u16` of the tempo there x100.
   */
  const encode = (beats: readonly [number, number, number?][]): Uint8Array => {
    const bytes = new Uint8Array(beats.length * 7);
    const view = new DataView(bytes.buffer);
    beats.forEach(([ms, number, tempo], i) => {
      view.setUint32(i * 7, ms, true);
      view.setUint8(i * 7 + 4, number);
      view.setUint16(i * 7 + 5, tempo ?? 12_800, true);
    });
    return bytes;
  };

  it("reads the seven-byte records the backend writes", () => {
    const grid = parseBeatGrid(encode([[0, 1], [469, 2], [938, 3], [1407, 4]]));
    expect(Array.from(grid.times)).toEqual([0, 469, 938, 1407]);
    expect(Array.from(grid.numbers)).toEqual([1, 2, 3, 4]);
    expect(Array.from(grid.tempos)).toEqual([12_800, 12_800, 12_800, 12_800]);
  });

  it("keeps each beat's own tempo, so a grid can change tempo partway", () => {
    const grid = parseBeatGrid(encode([[0, 1, 13_800], [435, 2, 13_800], [870, 3, 16_000]]));
    expect(Array.from(grid.tempos)).toEqual([13_800, 13_800, 16_000]);
  });

  it("holds a whole track without turning it into objects", () => {
    // A three-hour mix is about 23,000 beats — a list of objects here is what
    // the windowed fetch existed to avoid.
    const beats: [number, number][] = Array.from({ length: 23_000 }, (_, i) => [i * 469, (i % 4) + 1]);
    const grid = parseBeatGrid(encode(beats));
    expect(grid.times.length).toBe(23_000);
    expect(grid.times).toBeInstanceOf(Uint32Array);
  });

  it("drops a truncated last record rather than reading past it", () => {
    const grid = parseBeatGrid(encode([[0, 1], [469, 2]]).subarray(0, 10));
    expect(Array.from(grid.times)).toEqual([0]);
  });

  it("reads a view into a larger buffer, which is what the IPC hands back", () => {
    const whole = new Uint8Array(20);
    whole.set(encode([[1_000, 1]]), 5);
    const grid = parseBeatGrid(whole.subarray(5, 12));
    expect(Array.from(grid.times)).toEqual([1_000]);
  });

  it("gives an empty grid for a track with no analysis", () => {
    expect(parseBeatGrid(new Uint8Array()).times.length).toBe(0);
  });
});

describe("beatsIn", () => {
  const grid = parseBeatGrid(
    (() => {
      const bytes = new Uint8Array(1_000 * 7);
      const view = new DataView(bytes.buffer);
      for (let i = 0; i < 1_000; i++) {
        view.setUint32(i * 7, i * 500, true);
        view.setUint8(i * 7 + 4, (i % 4) + 1);
        view.setUint16(i * 7 + 5, 12_000, true);
      }
      return bytes;
    })(),
  );

  it("returns the beats inside the window, ends included", () => {
    const found = beatsIn(grid, 1_000, 2_000);
    expect(found.map((b) => b.timeMs)).toEqual([1_000, 1_500, 2_000]);
  });

  it("marks the first beat of a bar as the downbeat", () => {
    expect(beatsIn(grid, 0, 1_500).map((b) => b.downbeat)).toEqual([true, false, false, false]);
  });

  it("reads only the window, however long the track", () => {
    // The point of the binary search: a window near the end of a long mix must
    // not cost a scan of everything before it.
    expect(beatsIn(grid, 499_000, 499_500)).toHaveLength(2);
  });

  it("is empty for a window past the end, and for one the wrong way round", () => {
    expect(beatsIn(grid, 1_000_000, 1_100_000)).toEqual([]);
    expect(beatsIn(grid, 2_000, 1_000)).toEqual([]);
  });
});

describe("pressCue", () => {
  it("stops and rewinds to the cue point while playing", () => {
    expect(pressCue(90, 30, true)).toEqual({ seekTo: 30, playing: false, cuePoint: 30 });
  });

  it("previews from the cue point when paused on it", () => {
    // Held, not toggled: the deck plays while the button is down.
    expect(pressCue(30, 30, false)).toEqual({ seekTo: null, playing: true, cuePoint: 30 });
    expect(pressCue(30.01, 30, false).playing).toBe(true);
  });

  it("sets the cue point when paused anywhere else", () => {
    // A CDJ has no separate "set cue" button because this is it.
    expect(pressCue(75.5, 30, false)).toEqual({ seekTo: null, playing: false, cuePoint: 75.5 });
  });

  it("never sets a cue point before the start of the track", () => {
    expect(pressCue(-4, 30, false).cuePoint).toBe(0);
  });
});

describe("releaseCue", () => {
  it("snaps back and stops after a held preview", () => {
    expect(releaseCue(true, 30)).toEqual({ seekTo: 30, playing: false, cuePoint: 30 });
  });

  it("does nothing after any other press", () => {
    // Rewinding here would undo the jump the press itself just made.
    expect(releaseCue(false, 30)).toBeNull();
  });
});

describe("tempoAtMs", () => {
  // Castles In The Sky (EDCLV23 Closer): 138 to bar 135, then 160.
  const grid = {
    times: new Uint32Array([216, 651, 1086, 233_260, 233_635]),
    numbers: new Uint8Array([1, 2, 3, 1, 2]),
    tempos: new Uint16Array([13_800, 13_800, 13_800, 16_000, 16_000]),
  };

  it("is the tempo of the beat the head is standing on or has passed", () => {
    expect(tempoAtMs(grid, 216)).toBe(13_800);
    expect(tempoAtMs(grid, 900)).toBe(13_800);
    expect(tempoAtMs(grid, 233_260)).toBe(16_000);
    expect(tempoAtMs(grid, 233_500)).toBe(16_000);
    expect(tempoAtMs(grid, 900_000)).toBe(16_000);
  });

  it("is the first beat's before the grid starts, and 0 without a grid", () => {
    expect(tempoAtMs(grid, 0)).toBe(13_800);
    expect(tempoAtMs(NO_BEATS, 1_000)).toBe(0);
  });
});

describe("nearestBeatMs", () => {
  const grid = {
    times: new Uint32Array([0, 500, 1000, 1500]),
    numbers: new Uint8Array([1, 2, 3, 4]),
    tempos: new Uint16Array([12_000, 12_000, 12_000, 12_000]),
  };

  it("snaps to whichever beat is closer", () => {
    expect(nearestBeatMs(grid, 460)).toBe(500);
    expect(nearestBeatMs(grid, 240)).toBe(0);
    expect(nearestBeatMs(grid, 260)).toBe(500);
  });

  it("takes the earlier beat on a tie, so a snap is repeatable", () => {
    expect(nearestBeatMs(grid, 250)).toBe(0);
  });

  it("clamps to the ends rather than running off the grid", () => {
    expect(nearestBeatMs(grid, -900)).toBe(0);
    expect(nearestBeatMs(grid, 99_000)).toBe(1500);
  });

  it("leaves the position alone when the track has no grid", () => {
    expect(nearestBeatMs(NO_BEATS, 1234)).toBe(1234);
  });
});

describe("pressCue with quantize on", () => {
  const grid = {
    times: new Uint32Array([0, 500, 1000, 1500]),
    numbers: new Uint8Array([1, 2, 3, 4]),
    tempos: new Uint16Array([12_000, 12_000, 12_000, 12_000]),
  };

  it("puts a new cue point on the nearest beat, and the playhead with it", () => {
    // The head has to move too: a cue point it is not standing on reads as
    // "paused somewhere else", and the next press would move the cue again.
    expect(pressCue(0.46, 9, false, grid)).toEqual({ seekTo: 0.5, playing: false, cuePoint: 0.5 });
  });

  it("leaves the cue where the finger put it when Q is off", () => {
    expect(pressCue(0.46, 9, false).cuePoint).toBeCloseTo(0.46, 6);
  });

  it("still previews and still rewinds, quantized or not", () => {
    expect(pressCue(0.5, 0.5, false, grid).playing).toBe(true);
    expect(pressCue(90, 30, true, grid)).toEqual({ seekTo: 30, playing: false, cuePoint: 30 });
  });
});

describe("scrollOffset", () => {
  it("does not move the layer when the head is on the anchor", () => {
    expect(scrollOffset(0.5, 0.5, 0.1, 800)).toBeCloseTo(0, 6);
  });

  it("slides the layer against the playhead, a strip width per span", () => {
    // Half a span past the anchor is half a strip of travel.
    expect(scrollOffset(0.55, 0.5, 0.1, 800)).toBeCloseTo(-400, 6);
    expect(scrollOffset(0.45, 0.5, 0.1, 800)).toBeCloseTo(400, 6);
  });

  it("stays at rest rather than dividing by zero", () => {
    expect(scrollOffset(0.5, 0.2, 0, 800)).toBe(0);
    expect(scrollOffset(0.5, 0.2, 0.1, 0)).toBe(0);
  });
});

describe("needsRedraw", () => {
  it("holds while the drawn layer still covers the strip", () => {
    expect(needsRedraw(0.5, 0.5, 0.1)).toBe(false);
    expect(needsRedraw(0.52, 0.5, 0.1)).toBe(false);
  });

  it("asks for one before the edge of what was drawn arrives", () => {
    // The layer reaches half a span past the anchor; this fires at a quarter,
    // so the redraw has time to land.
    expect(needsRedraw(0.53, 0.5, 0.1)).toBe(true);
    expect(needsRedraw(0.47, 0.5, 0.1)).toBe(true);
  });

  it("never asks when there is no span to cover", () => {
    expect(needsRedraw(0.9, 0.1, 0)).toBe(false);
  });
});

describe("the beat jump sizes", () => {
  it("lists what rekordbox's menu lists, in its order", () => {
    // Transcribed from the menu itself: it skips 1 and 2 beats and changes
    // unit at 8 bars, so this is not a generated run of powers of two.
    expect(JUMP_SIZES.map((size) => size.label)).toEqual([
      "Fine", "4Beats", "8Beats", "16Beats", "8Bars", "16Bars", "32Bars",
    ]);
  });

  it("counts a bar as four beats", () => {
    expect(jumpSizeById("8bars").beats).toBe(32);
    expect(jumpSizeById("32bars").beats).toBe(128);
  });

  it("falls back to the default rather than to nothing", () => {
    expect(jumpSizeById("nonsense").id).toBe(JUMP_SIZE_ID);
  });

  it("cycles the sizes, and wraps", () => {
    expect(nextJumpSize("fine")).toBe("4beats");
    expect(nextJumpSize("32bars")).toBe("fine");
  });

  it("snaps a size it does not know back to the default", () => {
    expect(nextJumpSize("nonsense")).toBe(JUMP_SIZES[0]?.id);
  });
});

describe("jumpStepSeconds", () => {
  it("turns beats into seconds at the tempo", () => {
    // 8 beats at 120 BPM is four seconds.
    expect(jumpStepSeconds(jumpSizeById("8beats"), 12_000)).toBeCloseTo(4, 6);
  });

  it("makes Fine a fixed nudge, not a musical length", () => {
    // The one size that is a time: it must not depend on the tempo, and must
    // still move a track the analysis never gave a tempo to.
    expect(jumpStepSeconds(jumpSizeById("fine"), 12_000)).toBe(FINE_JUMP_SECONDS);
    expect(jumpStepSeconds(jumpSizeById("fine"), 0)).toBe(FINE_JUMP_SECONDS);
  });

  it("cannot move a beat length with no tempo behind it", () => {
    expect(jumpStepSeconds(jumpSizeById("8beats"), 0)).toBe(0);
  });
});

describe("jumpSeconds", () => {
  it("is the beat length times the count", () => {
    expect(jumpSeconds(4, 12_000)).toBeCloseTo(2, 6);
    expect(jumpSeconds(16, 12_000)).toBeCloseTo(8, 6);
  });

  it("is nothing without a tempo, rather than an infinity", () => {
    expect(jumpSeconds(4, 0)).toBe(0);
    expect(jumpSeconds(0, 12_000)).toBe(0);
  });
});

describe("zoomBy", () => {
  it("steps one level at a time, in both directions", () => {
    expect(zoomBy(12, -1)).toBe(8);
    expect(zoomBy(12, 1)).toBe(16);
  });

  it("stops at the ends rather than wrapping round to the other extreme", () => {
    expect(zoomBy(0.25, -1)).toBe(0.25);
    expect(zoomBy(64, 1)).toBe(64);
  });

  it("reaches rekordbox's closest step and one PCM inspection step beyond it", () => {
    expect(zoomBy(2, -1)).toBe(1);
    expect(zoomBy(1, -1)).toBe(0.5);
    expect(zoomBy(0.5, -1)).toBe(0.25);
  });

  it("snaps an unrecognised zoom back to the default", () => {
    expect(zoomBy(9, 0)).toBe(DETAIL_BARS);
  });
});

describe("showsEveryBeat", () => {
  it("drops to bar lines only at the widest zoom", () => {
    // 64 bars across puts the beats a few pixels apart, and the downbeats that
    // make the grid readable are lost among them.
    expect(showsEveryBeat(64)).toBe(false);
    expect(showsEveryBeat(32)).toBe(true);
  });

  it("keeps every beat at every step the buttons can reach below it", () => {
    for (const bars of ZOOM_STEPS.slice(0, -1)) {
      expect(showsEveryBeat(bars)).toBe(true);
    }
  });
});

describe("waveSlice", () => {
  const BYTES = 4000 * 3;

  it("keeps a transient on its timestamp when the crop falls between analysis columns", () => {
    // One second at 150 columns/second, magnified enough to expose a column
    // being stretched into the wrong place relative to an exact beat line.
    const columns = 150;
    for (const progress of [0.5001, 0.503, 0.509, 0.0123, 0.9887]) {
      const span = 0.1;
      const width = 1200;
      const slice = waveSlice(progress, span, columns * 3, width);
      const first = slice.first / 3;
      const last = slice.last / 3;
      for (let column = first; column <= last; column++) {
        const drawn = slice.x0 + (column - first) / (last - first) * slice.width;
        const timestamp = (column / columns - (progress - span / 2)) / span * width;
        // Only the final device-pixel placement may round, not the time range.
        expect(Math.abs(drawn - timestamp)).toBeLessThanOrEqual(0.500001);
      }
    }
  });

  it("puts a window in the middle of the track across the whole canvas", () => {
    const slice = waveSlice(0.5, 0.05, BYTES, 1200);
    expect(slice.x0).toBe(0);
    expect(slice.width).toBe(1200);
  });

  it("keeps the offset and width on whole pixels when the window overhangs", () => {
    // The bug: a fractional translate splits every one-pixel bar across two
    // columns at partial alpha, and the strip goes pale. 0.0131 is one of the
    // nine in ten overhanging positions that used to land off the grid.
    for (const progress of [0.0131, 0.00733, 0.9917, 0.001, 0.9999]) {
      const slice = waveSlice(progress, 0.05, BYTES, 1200);
      expect(Number.isInteger(slice.x0)).toBe(true);
      expect(Number.isInteger(slice.width)).toBe(true);
    }
  });

  it("draws the overhang as nothing rather than as a stretched first bar", () => {
    // Half the window is before the track starts, so half the canvas is empty
    // and the music occupies the other half.
    const slice = waveSlice(0, 0.05, BYTES, 1200);
    expect(slice.x0).toBe(600);
    expect(slice.width).toBe(600);
    expect(slice.first).toBe(0);
  });

  it("never reads past the end of the tag", () => {
    const slice = waveSlice(1, 0.05, BYTES, 1200);
    expect(slice.last).toBeLessThanOrEqual(BYTES);
    expect(slice.first).toBeLessThan(slice.last);
  });

  it("asks for nothing when the track has no waveform at all", () => {
    const slice = waveSlice(0.5, 0.05, 0, 1200);
    expect(slice.first).toBe(0);
    expect(slice.last).toBe(0);
  });
});

describe("detailWaveWindow", () => {
  it("keeps fixed-rate waveform columns on their own millisecond clock", () => {
    // DFBL's decoded audio is 211.243764 s while its 31,696 PWV7 columns
    // cover 211.306667 s at the format's fixed 150 columns per second.
    const window = detailWaveWindow(100 / 211.243764, 4 / 211.243764, 211_243.764, 31_696);
    expect(window.progress * (31_696 / 150 * 1000)).toBeCloseTo(100_000, 8);
    expect(window.span * (31_696 / 150 * 1000)).toBeCloseTo(4_000, 8);
  });

  it("falls back to the supplied fractions without timing metadata", () => {
    expect(detailWaveWindow(0.4, 0.1, 0, 100)).toEqual({ progress: 0.4, span: 0.1 });
  });
});

describe("detailWaveOriginMs", () => {
  it("removes DFBL's fractional MP3 encoder delay without dropping whole PWV7 columns", () => {
    const bytes = new Uint8Array(12 * 3);
    bytes.set([7, 40, 103], 7 * 3);
    expect(detailWaveOriginMs(25, bytes, 3)).toBeCloseTo(25, 6);
  });

  it("does not align a waveform that already has audio at its origin", () => {
    expect(detailWaveOriginMs(25, new Uint8Array([1, 2, 3, 0, 0, 0]), 3)).toBe(0);
  });
});

describe("subdivideGrid", () => {
  const grid = {
    times: Uint32Array.from([0, 500, 1000, 1500]),
    numbers: Uint8Array.from([1, 2, 3, 4]),
    tempos: Uint16Array.from([12_000, 12_000, 12_000, 12_000]),
  };

  it("splits every beat into equal steps that keep their beat's number", () => {
    const halves = subdivideGrid(grid, 2);
    expect(Array.from(halves.times)).toEqual([0, 250, 500, 750, 1000, 1250, 1500]);
    expect(Array.from(halves.numbers)).toEqual([1, 1, 2, 2, 3, 3, 4]);
    const eighths = subdivideGrid(grid, 8);
    expect(eighths.times.length).toBe(3 * 8 + 1);
    expect(eighths.times[1]).toBe(63);
    // The quantized cue snaps to a step, not to a beat.
    expect(nearestBeatMs(halves, 260)).toBe(250);
    expect(nearestBeatMs(grid, 260)).toBe(500);
  });

  it("leaves a whole-beat value, an empty grid and one beat alone", () => {
    expect(subdivideGrid(grid, 1)).toBe(grid);
    expect(subdivideGrid(NO_BEATS, 4)).toBe(NO_BEATS);
    const one = {
      times: Uint32Array.from([100]),
      numbers: Uint8Array.from([1]),
      tempos: Uint16Array.from([12_000]),
    };
    expect(subdivideGrid(one, 4)).toBe(one);
  });
});

describe("beatCountText", () => {
  const shiftedGrid: BeatGrid = {
    times: Uint32Array.from({ length: 40 }, (_, i) => 18 + Math.round(i * 60_000 / 136)),
    numbers: Uint8Array.from({ length: 40 }, (_, i) => i % 4 + 1),
    tempos: new Uint16Array(40).fill(13_600),
  };

  it("changes from 8.4 to 9.1 only when the stored downbeat crosses the head", () => {
    const downbeat = shiftedGrid.times[32]! / 1000;
    expect(beatCountText(downbeat - 0.001, 136, "position", [], shiftedGrid)).toBe("8.4 Bars");
    expect(beatCountText(downbeat, 136, "position", [], shiftedGrid)).toBe("9.1 Bars");
    expect(beatCountText(downbeat + 0.001, 136, "position", [], shiftedGrid)).toBe("9.1 Bars");
    // A backwards scrub must cross the same boundary in reverse.
    expect(beatCountText(downbeat - 0.001, 136, "position", [], shiftedGrid)).toBe("8.4 Bars");
  });

  it("follows an edited grid and tempo changes instead of the displayed BPM", () => {
    const grid = {
      times: Uint32Array.from([18, 518, 1018, 1518, 2018, 2418, 2818, 3218, 3618]),
      numbers: Uint8Array.from([1, 2, 3, 4, 1, 2, 3, 4, 1]),
      tempos: Uint16Array.from([12000, 12000, 12000, 12000, 15000, 15000, 15000, 15000, 15000]),
    };
    expect(beatCountText(3.617, 150, "position", [], grid)).toBe("2.4 Bars");
    expect(beatCountText(3.618, 120, "position", [], grid)).toBe("3.1 Bars");
    expect(beatCountText(3.618, 0, "position", [], grid)).toBe("3.1 Bars");
    const shifted = { ...grid, times: grid.times.map((ms) => ms + 50) };
    expect(beatCountText(3.618, 120, "position", [], shifted)).toBe("2.4 Bars");
  });

  it("keeps the run-in before the first downbeat and respects a partial first bar", () => {
    expect(beatCountText(0, 136, "position", [], shiftedGrid)).toBe("-1.4 Bars");
    expect(beatCountText(0.018, 136, "position", [], shiftedGrid)).toBe("1.1 Bars");
    const grid = {
      times: Uint32Array.from([18, 518, 1018]),
      numbers: Uint8Array.from([3, 4, 1]),
      tempos: new Uint16Array(3).fill(12000),
    };
    expect(beatCountText(0.018, 120, "position", [], grid)).toBe("1.3 Bars");
    expect(beatCountText(1.018, 120, "position", [], grid)).toBe("2.1 Bars");
  });

  // 120 BPM: two beats a second, a bar every two seconds.
  it("counts bars and beats from the start by default, the beat only ever 1 to 4", () => {
    // Ten seconds at 120 is twenty beats: the first beat of the sixth bar.
    expect(beatCountText(10, 120, "position", [30])).toBe("6.1 Bars");
    expect(beatCountText(0, 120, "position", [])).toBe("1.1 Bars");
    expect(beatCountText(1.5, 120, "position", [])).toBe("1.4 Bars");
    expect(beatCountText(2, 120, "position", [])).toBe("2.1 Bars");
  });

  it("counts down to the next memory cue in bars and beats, or beats", () => {
    expect(beatCountText(10, 120, "toMemoryBars", [30, 14, 5])).toBe("-2.0 Bars");
    expect(beatCountText(10, 120, "toMemoryBeats", [30, 14, 5])).toBe("-8Beats");
    expect(beatCountText(10.5, 120, "toMemoryBars", [14])).toBe("-1.3 Bars");
    // Part way through a beat rounds up: seven and a bit beats left is eight.
    expect(beatCountText(10.1, 120, "toMemoryBeats", [14])).toBe("-8Beats");
    // At the cue itself the count is zero, not the cue before it.
    expect(beatCountText(14, 120, "toMemoryBars", [14, 5])).toBe("-0.0 Bars");
  });

  it("shows nothing with no cue ahead, or no grid", () => {
    expect(beatCountText(20, 120, "toMemoryBars", [5, 14])).toBe("");
    expect(beatCountText(20, 120, "toMemoryBeats", [])).toBe("");
    expect(beatCountText(20, 0, "position", [])).toBe("");
  });
});

describe("clickSeconds", () => {
  // A 1000px detail showing a tenth of a 200 s track: 20 s across, 0.02 s a pixel.
  it("puts the head under the pointer, at the window's own rate", () => {
    expect(clickSeconds(500, 1000, 100, 0.1, 200)).toBeCloseTo(100, 6);
    expect(clickSeconds(750, 1000, 100, 0.1, 200)).toBeCloseTo(105, 6);
    expect(clickSeconds(0, 1000, 100, 0.1, 200)).toBeCloseTo(90, 6);
  });

  it("stays inside the track", () => {
    expect(clickSeconds(0, 1000, 2, 0.1, 200)).toBe(0);
    expect(clickSeconds(1000, 1000, 199, 0.1, 200)).toBe(200);
  });
});

describe("isClick", () => {
  it("is a release within a few pixels of the press", () => {
    expect(isClick(0, 0)).toBe(true);
    expect(isClick(3, -3)).toBe(true);
    expect(isClick(4, 0)).toBe(false);
    expect(isClick(0, 12)).toBe(false);
  });
});

describe("the tempo fader", () => {
  it("maps a range's ends to ±1 and the file's speed to 0", () => {
    expect(tempoToFader(1, 6)).toBe(0);
    expect(tempoToFader(1.06, 6)).toBeCloseTo(1, 6);
    expect(tempoToFader(0.9, 10)).toBeCloseTo(-1, 6);
    expect(tempoToFader(1.08, 16)).toBeCloseTo(0.5, 6);
    // Past the end is the end.
    expect(tempoToFader(1.3, 6)).toBe(1);
  });

  it("WIDE is half speed to double, with the middle still the file's own", () => {
    expect(faderToTempo(-1, "wide")).toBe(0.5);
    expect(faderToTempo(1, "wide")).toBe(2);
    expect(faderToTempo(0, "wide")).toBe(1);
    expect(tempoToFader(0.75, "wide")).toBeCloseTo(-0.5, 6);
    expect(tempoToFader(1.5, "wide")).toBeCloseTo(0.5, 6);
  });

  it("goes back and forth without drift", () => {
    for (const range of [6, 10, 16, "wide"] as const) {
      for (const at of [-1, -0.3, 0, 0.42, 1]) {
        expect(tempoToFader(faderToTempo(at, range), range)).toBeCloseTo(at, 6);
      }
    }
  });

  it("a typed BPM is a tempo against the track's own", () => {
    expect(tempoForTypedBpm("130", 12_800)).toBeCloseTo(130 / 128, 6);
    expect(tempoForTypedBpm(" 128,5 ", 12_800)).toBeCloseTo(1.00390625, 6);
    // Clamped to what the engine plays.
    expect(tempoForTypedBpm("400", 12_800)).toBe(2);
    expect(tempoForTypedBpm("10", 12_800)).toBe(0.5);
    // Nothing to work with.
    expect(tempoForTypedBpm("fast", 12_800)).toBeNull();
    expect(tempoForTypedBpm("130", 0)).toBeNull();
  });
});

describe("beatLoopRange", () => {
  const grid = {
    times: new Uint32Array([1000, 1500, 2000, 2500, 3000]),
    numbers: new Uint8Array([1, 2, 3, 4, 1]),
    tempos: new Uint16Array([12_000, 12_000, 12_000, 12_000, 12_000]),
  };
  it("runs from the snapped beat for the asked number of beats on the grid", () => {
    expect(beatLoopRange(grid, grid, 1620, 2)).toEqual([1500, 2500]);
    // Off the grid's end, the average beat carries the loop on.
    expect(beatLoopRange(grid, grid, 2400, 4)).toEqual([2500, 4500]);
    // Unquantized, it starts where the head is.
    expect(beatLoopRange(grid, null, 1620, 1)).toEqual([1620, 2120]);
    // A fraction of a beat.
    expect(beatLoopRange(grid, grid, 1000, 0.5)).toEqual([1000, 1250]);
  });
  it("has nothing to count on without a grid", () => {
    expect(beatLoopRange(NO_BEATS, null, 100, 4)).toBeNull();
    expect(beatLoopRange(grid, grid, 1000, 0)).toBeNull();
  });
});

describe("resizedLoopRange", () => {
  const grid = {
    times: new Uint32Array([1000, 1500, 2000, 2500, 3000]),
    numbers: new Uint8Array([1, 2, 3, 4, 1]),
    tempos: new Uint16Array([12_000, 12_000, 12_000, 12_000, 12_000]),
  };
  it("halves and doubles a beat loop on the grid, from its own in point", () => {
    expect(resizedLoopRange(grid, 1000, 3000, 4, 0.5)).toEqual({ range: [1000, 2000], beats: 2 });
    expect(resizedLoopRange(grid, 1000, 2000, 2, 2)).toEqual({ range: [1000, 3000], beats: 4 });
  });
  it("scales a manual loop by its own length and keeps the beat loop length", () => {
    // Three beats by hand, with the beat loop length at four.
    expect(resizedLoopRange(grid, 1000, 2500, 4, 0.5)).toEqual({ range: [1000, 1750], beats: 4 });
  });
  it("keeps the length within a quarter beat and 32 beats", () => {
    expect(resizedLoopRange(grid, 1000, 1125, 0.25, 0.5)).toEqual({ range: [1000, 1125], beats: 0.25 });
    expect(resizedLoopRange(grid, 1000, 1100, 4, 0.5)).toEqual({ range: [1000, 1125], beats: 4 });
    expect(resizedLoopRange(grid, 1000, 13_000, 4, 2)).toEqual({ range: [1000, 17_000], beats: 4 });
  });
  it("has nothing to count on without a grid", () => {
    expect(resizedLoopRange(NO_BEATS, 1000, 3000, 4, 0.5)).toBeNull();
  });
});

describe("wrapIntoLoop", () => {
  it("leaves a head inside the loop where it is", () => {
    expect(wrapIntoLoop(1.5, 1, 2)).toBe(1.5);
    expect(wrapIntoLoop(0.5, 1, 2)).toBe(0.5);
  });
  it("takes a head past the end back by whole loops, in time", () => {
    expect(wrapIntoLoop(2, 1, 2)).toBe(1);
    expect(wrapIntoLoop(2.25, 1, 2)).toBe(1.25);
    expect(wrapIntoLoop(4.5, 1, 2)).toBe(1.5);
  });
});

describe("loopBeatsLabel", () => {
  it("writes a fraction of a beat as rekordbox does", () => {
    expect(loopBeatsLabel(0.25)).toBe("1/4");
    expect(loopBeatsLabel(0.5)).toBe("1/2");
    expect(loopBeatsLabel(1)).toBe("1");
    expect(loopBeatsLabel(32)).toBe("32");
  });
});

describe("beatAtMs", () => {
  const grid = {
    times: new Uint32Array([1000, 1500, 2000]),
    numbers: new Uint8Array([1, 2, 3]),
    tempos: new Uint16Array([12_000, 12_000, 12_000]),
  };
  it("is the last beat at or before the head, 1-based", () => {
    expect(beatAtMs(grid, 1000)).toBe(1);
    expect(beatAtMs(grid, 1700)).toBe(2);
    expect(beatAtMs(grid, 2000)).toBe(3);
    expect(beatAtMs(grid, 9000)).toBe(3);
    expect(beatAtMs(grid, 10)).toBe(1);
    expect(beatAtMs(NO_BEATS, 500)).toBe(1);
  });
});

describe("tempoChangeAtMs", () => {
  it("finds Cannonball's 48.4 change even when its memory cue is four milliseconds later", () => {
    const grid = {
      times: new Uint32Array([85563, 86032, 86446]),
      numbers: new Uint8Array([3, 4, 1]),
      tempos: new Uint16Array([12800, 14500, 14500]),
    };
    expect(tempoChangeAtMs(grid, 86032)).toBe(14500);
    expect(tempoChangeAtMs(grid, 86036)).toBeNull();
    expect(tempoChangeAtMs(grid, 86446)).toBeNull();
  });
  const grid = { times: new Uint32Array([0, 500, 1000, 1400]), numbers: new Uint8Array([1, 2, 3, 4]), tempos: new Uint16Array([12000, 12000, 15000, 15000]) };
  it("labels exact tempo-change beats independently of nearby cues", () => {
    expect(tempoChangeAtMs(grid, 0)).toBe(12000);
    expect(tempoChangeAtMs(NO_BEATS, 0)).toBeNull();
    expect(tempoChangeAtMs(grid, 1000)).toBe(15000);
    expect(tempoChangeAtMs(grid, 1001)).toBeNull();
    for (const ms of [500, 1010, 1400, 99999]) expect(tempoChangeAtMs(grid, ms)).toBeNull();
  });
});

describe("tempoAnnotations", () => {
  const gridOf = (tempos: number[]) => ({ times: Uint32Array.from(tempos, (_, i) => i * 500),
    numbers: Uint8Array.from(tempos, (_, i) => i % 4 + 1), tempos: Uint16Array.from(tempos) });
  it("groups a curved ramp between steady tempos without changing the grid", () => {
    const grid = gridOf([12800, 12800, 12800, 12800, 12820, 12900, 13500, 15000, 16600, 17300, 17400, 17400, 17400, 17400]);
    const before = grid.tempos.slice();
    expect(tempoAnnotations(grid)).toEqual([
      { fromMs: 0, toMs: 0, fromBpmX100: 12800, toBpmX100: 12800 },
      { fromMs: 2000, toMs: 5000, fromBpmX100: 12800, toBpmX100: 17400 },
    ]);
    expect(grid.tempos).toEqual(before);
  });
  it("keeps an abrupt change at its exact timestamp", () => {
    expect(tempoAnnotations(gridOf([12800, 12800, 17400, 17400]))).toEqual([
      { fromMs: 0, toMs: 0, fromBpmX100: 12800, toBpmX100: 12800 },
      { fromMs: 1000, toMs: 1000, fromBpmX100: 17400, toBpmX100: 17400 },
    ]);
  });
  it("handles descending ramps and a ramp that reaches the end of the file", () => {
    expect(tempoAnnotations(gridOf([17400, 17400, 17400, 17400, 16500, 15000, 12800]))[1])
      .toEqual({ fromMs: 2000, toMs: 3000, fromBpmX100: 17400, toBpmX100: 12800 });
    expect(tempoAnnotations(NO_BEATS)).toEqual([]);
  });
});

describe("createWheelZoomGate", () => {
  it("steps once for a mouse notch", () => {
    const gate = createWheelZoomGate();
    expect(gate(100, 0)).toBe(1);
    expect(gate(-120, 1000)).toBe(-1);
  });

  it("turns a trackpad swipe and its inertia tail into one or two steps", () => {
    const gate = createWheelZoomGate();
    let steps = 0;
    // 80 events of 8px over 800ms, 16ms apart.
    for (let i = 0; i < 80; i++) steps += Math.abs(gate(8, i * 10));
    expect(steps).toBeGreaterThan(0);
    expect(steps).toBeLessThanOrEqual(6);
  });

  it("drops partial travel after a pause or a reversal", () => {
    const gate = createWheelZoomGate();
    expect(gate(60, 0)).toBe(0);
    expect(gate(60, 1000)).toBe(0);
    expect(gate(-60, 1010)).toBe(0);
    // The old direction's travel was discarded, not netted against.
    expect(gate(-60, 1020)).toBe(-1);
  });
});

describe("quantizedLaunchMs", () => {
  // 120 BPM: a beat every 500 ms from 1000 ms.
  const grid = {
    times: new Uint32Array([1000, 1500, 2000, 2500, 3000]),
    numbers: new Uint8Array([1, 2, 3, 4, 1]),
    tempos: new Uint16Array([12_000, 12_000, 12_000, 12_000, 12_000]),
  };

  it("fires a cue on the grid at the next beat", () => {
    expect(quantizedLaunchMs(grid, 1700, 1000)).toBe(2000);
    expect(quantizedLaunchMs(grid, 1501, 2500)).toBe(2000);
  });

  it("fires at once when the playhead is on a beat", () => {
    expect(quantizedLaunchMs(grid, 2000, 1000)).toBe(2000);
  });

  it("keeps an off-grid cue's place within the beat", () => {
    // The cue sits 100 ms past a beat, so the jump waits for the next point
    // 100 ms past a beat: the bar runs on unbroken.
    expect(quantizedLaunchMs(grid, 1700, 2600)).toBe(2100);
    expect(quantizedLaunchMs(grid, 1550, 2600)).toBe(1600);
  });

  it("follows the quantize beat value through a finer grid", () => {
    const halves = subdivideGrid(grid, 2);
    expect(quantizedLaunchMs(halves, 1600, 1000)).toBe(1750);
  });

  it("carries the edge spacing past the ends of the grid", () => {
    expect(quantizedLaunchMs(grid, 3100, 1000)).toBe(3500);
    expect(quantizedLaunchMs(grid, 200, 1000)).toBe(500);
  });

  it("has nothing to time against without two beats", () => {
    const one = { times: new Uint32Array([1000]), numbers: new Uint8Array([1]), tempos: new Uint16Array([12_000]) };
    expect(quantizedLaunchMs(one, 1200, 1000)).toBeNull();
  });
});

describe("quantizedLaunchMs inside a playing loop", () => {
  // rekordbox 7.2.19: moveToCueAndPlayWithWait fires at min(out, max(step,
  // head)) when doHotCueLaunch left a loop [OBS static, parity/issue-126].
  const grid = {
    times: new Uint32Array([1000, 1500, 2000, 2500, 3000]),
    numbers: new Uint8Array([1, 2, 3, 4, 1]),
    tempos: new Uint16Array([12_000, 12_000, 12_000, 12_000, 12_000]),
  };

  it("fires at the out point of a one-beat loop, where the head would wrap", () => {
    expect(quantizedLaunchMs(grid, 1200, 500, 1500)).toBe(1500);
  });

  it("never fires past the out point for an off-grid cue", () => {
    // The step at the cue's phase is 1600, past the loop's end at 1500.
    expect(quantizedLaunchMs(grid, 1200, 2600, 1500)).toBe(1500);
  });

  it("fires at the next step when that comes before the out point", () => {
    expect(quantizedLaunchMs(grid, 1200, 500, 3000)).toBe(1500);
  });
});

describe("foldIntoLoop", () => {
  const loop = { inSeconds: 1, outSeconds: 1.5 };

  it("leaves a head inside the loop where it is", () => {
    expect(foldIntoLoop(1.2, loop)).toBe(1.2);
  });

  it("brings a head read past the out point back by whole loops", () => {
    expect(foldIntoLoop(1.6, loop)).toBeCloseTo(1.1);
    expect(foldIntoLoop(2.1, loop)).toBeCloseTo(1.1);
  });
});

describe("callLeavesFrom", () => {
  it("jumps from the launch point when the timer is a little late", () => {
    expect(callLeavesFrom(1.52, 1.5, 0, 0.15)).toBe(1.5);
  });

  it("drops the call when something else moved the head", () => {
    expect(callLeavesFrom(3, 1.5, 0, 0.15)).toBeNull();
  });

  it("takes a reading run on past a one-beat loop's out point as the launch point", () => {
    // Read from a tick before the engine wrapped: a loop or two ahead.
    expect(callLeavesFrom(1.99, 1.5, 0.5, 0.15)).toBe(1.5);
    expect(callLeavesFrom(2.51, 1.5, 0.5, 0.15)).toBe(1.5);
  });

  it("jumps from a loop back when the engine wrapped before the exit reached it", () => {
    expect(callLeavesFrom(1.01, 1.5, 0.5, 0.15)).toBe(1);
  });

  it("still drops a call the head moved away from inside a loop", () => {
    expect(callLeavesFrom(1.75, 1.5, 0.5, 0.15)).toBeNull();
  });
});
