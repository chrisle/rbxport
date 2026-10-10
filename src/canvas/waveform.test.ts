import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import {
  BAND_HIGH, BAND_LOW, BAND_MID, bandColour, bandStops, drawBands, drawColumns, drawPreviewCues, drawPreviewMemoryCues, PREVIEW_BADGE, previewClickMs, ramp, segments,
  strideOf, waveformKindOf,
} from "./waveform";

describe("the three-band colours", () => {
  it("colours a stretch by the bands that reach it: rekordbox's seven", () => {
    // Sampled from rekordbox itself (mixxxdj/mixxx#12326): a band alone,
    // two overlapping, all three.
    expect(bandColour(BAND_LOW)).toBe("rgb(0,85,225)");
    expect(bandColour(BAND_MID)).toBe("rgb(255,166,0)");
    expect(bandColour(BAND_HIGH)).toBe("rgb(255,255,255)");
    expect(bandColour(BAND_LOW | BAND_MID)).toBe("rgb(180,105,10)");
    expect(bandColour(BAND_LOW | BAND_HIGH)).toBe("rgb(210,220,250)");
    expect(bandColour(BAND_MID | BAND_HIGH)).toBe("rgb(255,240,215)");
    expect(bandColour(BAND_LOW | BAND_MID | BAND_HIGH)).toBe("rgb(245,235,215)");
  });

  it("cuts a column into stretches from the outside in", () => {
    // Low 127 reaches the whole way, mid half, high a quarter: blue outside,
    // brown where the mid joins, cream where all three do.
    expect(segments(127, 64, 32, 100)).toEqual([
      [100, BAND_LOW],
      [100 * (64 / 127), BAND_LOW | BAND_MID],
      [100 * (32 / 127), BAND_LOW | BAND_MID | BAND_HIGH],
    ]);
    // A mid louder than the low is amber outside and brown within.
    expect(segments(32, 127, 0, 100)).toEqual([[100, BAND_MID], [100 * (32 / 127), BAND_LOW | BAND_MID]]);
    // Bands that reach the same distance share a stretch; silence has none.
    expect(segments(64, 64, 0, 100)).toEqual([[100 * (64 / 127), BAND_LOW | BAND_MID]]);
    expect(segments(0, 0, 0, 100)).toEqual([]);
    // The quietest sound is still half a pixel.
    expect(segments(1, 0, 0, 10)).toEqual([[0.5, BAND_LOW]]);
  });

  it("ramps blue, amber, cream for a drawing with one value a column", () => {
    const stops = bandStops("detail");
    expect(ramp(stops, 0)).toBe("rgb(0,85,225)");
    expect(ramp(stops, 0.5)).toBe("rgb(255,166,0)");
    expect(ramp(stops, 1)).toBe("rgb(245,235,215)");
    expect(bandStops("overview")).toEqual(bandStops("detail"));
    const quarter = ramp(stops, 0.25);
    expect(quarter).not.toBe(ramp(stops, 0));
    expect(quarter).not.toBe(ramp(stops, 0.5));
    expect(ramp(stops, -1)).toBe(ramp(stops, 0));
    expect(ramp(stops, 99)).toBe(ramp(stops, 1));
    expect(ramp(stops, Number.NaN)).toBe(ramp(stops, 0));
  });

  it("matches the tokens the stylesheet ships", () => {
    // The canvas cannot read a CSS variable per column, so the palette is
    // duplicated here. This is the guard against the two drifting apart.
    const css = readFileSync("src/styles/tokens.css", "utf8");
    const token = (name: string) =>
      new RegExp(`--c-wave-${name}:\\s*(#[0-9A-Fa-f]{6})`).exec(css)?.[1]?.toUpperCase();
    const hex = (rgb: string) => {
      const [r, g, b] = rgb.slice(4, -1).split(",").map(Number);
      return `#${[r, g, b].map((n) => (n ?? 0).toString(16).padStart(2, "0")).join("")}`.toUpperCase();
    };
    expect(hex(bandColour(BAND_LOW))).toBe(token("low"));
    expect(hex(bandColour(BAND_MID))).toBe(token("mid"));
    expect(hex(bandColour(BAND_HIGH))).toBe(token("high"));
    expect(hex(bandColour(BAND_LOW | BAND_MID))).toBe(token("low-mid"));
    expect(hex(bandColour(BAND_LOW | BAND_HIGH))).toBe(token("low-high"));
    expect(hex(bandColour(BAND_MID | BAND_HIGH))).toBe(token("mid-high"));
    expect(hex(bandColour(BAND_LOW | BAND_MID | BAND_HIGH))).toBe(token("all"));
  });
});

describe("the three-band waveform", () => {
  /** A tiny 2D context that records what was filled. */
  function recorder() {
    const fills: { style: string; x: number; y: number; w: number; h: number }[] = [];
    const ctx = {
      fillStyle: "",
      clearRect: () => undefined,
      fillRect(x: number, y: number, w: number, h: number) {
        fills.push({ style: String(this.fillStyle), x, y, w, h });
      },
    };
    return { ctx: ctx as unknown as CanvasRenderingContext2D, fills };
  }

  it("draws each stretch in the colour of the bands that reach it, the core last", () => {
    // One column: low 60, mid 30, high 10. Blue where only the low reaches,
    // brown where the mid joins it, the cream core where all three do.
    const { ctx, fills } = recorder();
    drawBands(ctx, new Uint8Array([60, 30, 10]), 1, 100, "overview");
    expect(fills).toHaveLength(3);
    expect(fills.map((f) => f.style)).toEqual([
      bandColour(BAND_LOW),
      bandColour(BAND_LOW | BAND_MID),
      bandColour(BAND_LOW | BAND_MID | BAND_HIGH),
    ]);
    // Low is the outer envelope; the bright core is smallest and on top.
    expect(fills[0]!.h).toBeGreaterThan(fills[1]!.h);
    expect(fills[1]!.h).toBeGreaterThan(fills[2]!.h);
  });

  it("centres every band on the middle line", () => {
    const { ctx, fills } = recorder();
    drawBands(ctx, new Uint8Array([60, 30, 10]), 1, 100, "overview");
    for (const fill of fills) {
      expect(fill.y + fill.h / 2).toBeCloseTo(50, 5);
    }
  });

  it("puts both tags on the same seven-bit scale", () => {
    // Measured across 80 tracks: PWV7 reaches 127 and PWV6 reaches 98. An
    // earlier reading of 63 came from one track and drew every overview at
    // double height.
    for (const band of ["overview", "detail"] as const) {
      const r = recorder();
      drawBands(r.ctx, new Uint8Array([127, 0, 0]), 1, 100, band);
      expect(r.fills[0]!.h).toBeCloseTo(100, 5);
    }
  });

  it("stacks the bands from the bottom for a row preview", () => {
    // The track list draws a half waveform: blue from the baseline, amber on
    // top of it, near-white above that. Overlaying from a baseline instead
    // would hide the amber whenever the lows are louder, which is most of the
    // time.
    const { ctx, fills } = recorder();
    drawBands(ctx, new Uint8Array([75, 45, 30]), 1, 150, "overview", true);
    expect(fills).toHaveLength(3);

    // Each sits directly on the one below, and the lowest sits on the floor.
    expect(fills[0]!.y + fills[0]!.h).toBeCloseTo(150, 5);
    expect(fills[1]!.y + fills[1]!.h).toBeCloseTo(fills[0]!.y, 5);
    expect(fills[2]!.y + fills[2]!.h).toBeCloseTo(fills[1]!.y, 5);

    // Blue at the bottom, then amber, then near-white.
    expect(fills.map((f) => f.style)).toEqual([
      ramp(bandStops("overview"), 0),
      ramp(bandStops("overview"), 0.5),
      ramp(bandStops("overview"), 1),
    ]);
  });

  it("overlays the bands from the baseline for the 2 PLAYER detail", () => {
    // The two decks' details meet at the line between them, each a half
    // drawn from that line: blue behind as the envelope, amber over it, the
    // near-white core last — the centred waveform's layering, one-sided.
    const { ctx, fills } = recorder();
    drawBands(ctx, new Uint8Array([127, 64, 32]), 1, 100, "detail", "overlaid", { top: 8, bottom: 2 });
    expect(fills).toHaveLength(3);
    // Every stretch stands on the floor, and the loudest reaches the top inset.
    for (const fill of fills) expect(fill.y + fill.h).toBeCloseTo(98, 5);
    expect(fills[0]!.y).toBeCloseTo(8, 5);
    expect(fills[1]!.h).toBeCloseTo(90 * (64 / 127), 5);
    expect(fills[2]!.h).toBeCloseTo(90 * (32 / 127), 5);
    expect(fills.map((f) => f.style)).toEqual([
      bandColour(BAND_LOW),
      bandColour(BAND_LOW | BAND_MID),
      bandColour(BAND_LOW | BAND_MID | BAND_HIGH),
    ]);
  });

  it("weighs the three bands the way rekordbox paints them", () => {
    // Not one scale for all three. Matched column for column against a 2x
    // capture, the blue rises v/128 of the band, the amber v/256 and the
    // near-white v/128. Dividing all three by a flat 150 drew the amber thin
    // and capped every loud passage in white.
    const { ctx, fills } = recorder();
    drawBands(ctx, new Uint8Array([32, 32, 32]), 1, 256, "overview", true);
    expect(fills.map((f) => f.h)).toEqual([64, 32, 64]);
  });

  it("never draws a stacked column past the top of the cell", () => {
    // The bands do not peak together, so the stack scale is well under three
    // times the band scale — a loud column clips rather than overflowing.
    const { ctx, fills } = recorder();
    drawBands(ctx, new Uint8Array([127, 127, 127]), 1, 40, "overview", true);
    for (const fill of fills) {
      expect(fill.y).toBeGreaterThanOrEqual(0);
      expect(fill.y + fill.h).toBeLessThanOrEqual(40.001);
    }
  });

  it("draws a zero-amplitude line for a silent column, and survives an empty tag", () => {
    const silent = recorder();
    drawBands(silent.ctx, new Uint8Array([0, 0, 0]), 1, 100);
    expect(silent.fills).toEqual([{ style: bandColour(BAND_LOW | BAND_HIGH), x: 0, y: 50, w: 1, h: 1 }]);

    const empty = recorder();
    expect(() => drawBands(empty.ctx, new Uint8Array(), 10, 10)).not.toThrow();
    // A trailing partial entry must not be read as a column.
    expect(() => drawBands(empty.ctx, new Uint8Array([1, 2]), 10, 10)).not.toThrow();
    expect(empty.fills).toHaveLength(0);
  });

  it("leaves the measured margin clear at the top and bottom", () => {
    // rekordbox's detail waveform paints y 374..647 inside a band running
    // 358..650: the strip above carries the bar count and the cue heads.
    const { ctx, fills } = recorder();
    drawBands(ctx, new Uint8Array([127, 0, 0]), 1, 100, "detail", false, { top: 8, bottom: 2 });
    const fill = fills[0]!;
    expect(fill.y).toBeCloseTo(8, 5);
    expect(fill.y + fill.h).toBeCloseTo(98, 5);
  });

  it("insets a stacked half waveform from the same edges", () => {
    const { ctx, fills } = recorder();
    drawBands(ctx, new Uint8Array([150, 0, 0]), 1, 100, "overview", true, { top: 8, bottom: 2 });
    const fill = fills[0]!;
    expect(fill.y + fill.h).toBeCloseTo(98, 5);
    expect(fill.y).toBeGreaterThanOrEqual(8);
  });

  it("still fills the band when no inset is asked for", () => {
    // The row preview passes none, and must keep every pixel of a 25px row.
    const { ctx, fills } = recorder();
    drawBands(ctx, new Uint8Array([150, 0, 0]), 1, 25, "overview", true);
    expect(fills[0]!.y + fills[0]!.h).toBeCloseTo(25, 5);
  });

  it("survives an inset taller than the strip", () => {
    const { ctx, fills } = recorder();
    expect(() =>
      drawBands(ctx, new Uint8Array([127, 0, 0]), 1, 6, "detail", false, { top: 40, bottom: 40 }),
    ).not.toThrow();
    for (const fill of fills) {
      expect(fill.y).toBeGreaterThanOrEqual(0);
      expect(fill.y + fill.h).toBeLessThanOrEqual(6.001);
    }
  });

  it("takes the loudest column when many share a pixel", () => {
    // Four columns into one pixel: the peak must survive, or a transient
    // vanishes at overview width.
    const { ctx, fills } = recorder();
    const data = new Uint8Array([1, 0, 0, 127, 0, 0, 1, 0, 0, 1, 0, 0]);
    drawBands(ctx, data, 1, 100, "overview");
    expect(fills[0]!.h).toBeCloseTo(100, 5);
  });

  it("keeps magnified attacks sharp and on time while smoothing their decay", () => {
    const { ctx, fills } = recorder();
    drawBands(ctx, new Uint8Array([0, 0, 0, 127, 0, 0, 0, 0, 0]), 12, 100, "detail");
    const heightAt = (x: number) => Math.max(0, ...fills.filter((fill) => fill.x === x && fill.h > 1).map((fill) => fill.h));
    expect(heightAt(0)).toBe(0);
    expect(heightAt(2)).toBe(0);
    expect(heightAt(3)).toBe(0);
    expect(heightAt(4)).toBeCloseTo(100);
    expect(heightAt(6)).toBeCloseTo(50);
    expect(heightAt(8)).toBe(0);
    expect(heightAt(11)).toBe(0);
    expect(fills.every((fill) => fill.h <= 100)).toBe(true);
  });

  it("preserves attacks above a nonzero floor in each frequency band", () => {
    for (const channel of [0, 1, 2]) {
      const { ctx, fills } = recorder();
      const data = new Uint8Array(9);
      data[channel] = 32;
      data[3 + channel] = 96;
      data[6 + channel] = 32;
      drawBands(ctx, data, 12, 127, "detail");
      const heightAt = (x: number) => fills.find((fill) => fill.x === x)?.h ?? 0;
      expect(heightAt(3)).toBeCloseTo(32);
      expect(heightAt(4)).toBeCloseTo(96);
      expect(heightAt(6)).toBeCloseTo(64);
      expect(heightAt(8)).toBeCloseTo(32);
    }
  });

  it("never draws even a quiet band before its first nonzero bin", () => {
    const { ctx, fills } = recorder();
    // The first bins of Love is Gonna Save Us: a quiet low-band onset must
    // not be interpolated backwards just because it is below the jump threshold.
    drawBands(ctx, new Uint8Array([0, 0, 0, 11, 79, 103]), 8, 254, "detail");
    expect(fills.some((fill) => fill.x < 4 && fill.h > 1)).toBe(false);
    expect(fills.some((fill) => fill.x === 4 && fill.h > 1)).toBe(true);
  });

  it("smooths small rises in every band without flattening their peaks and dips", () => {
    for (const channel of [0, 1, 2]) {
      const { ctx, fills } = recorder();
      const data = new Uint8Array(12);
      [77, 92, 90, 99].forEach((value, i) => { data[i * 3 + channel] = value; });
      drawBands(ctx, data, 16, 127, "detail");
      const heightAt = (x: number) => fills.find((fill) => fill.x === x)?.h ?? 0;
      expect(heightAt(0)).toBeCloseTo(77);
      expect(heightAt(2)).toBeCloseTo(84.5);
      expect(heightAt(4)).toBeCloseTo(92);
      expect(heightAt(8)).toBeCloseTo(90);
      expect(heightAt(10)).toBeCloseTo(94.5);
      expect(heightAt(12)).toBeCloseTo(99);
      expect(heightAt(15)).toBeCloseTo(99);
    }
  });
});

describe("the hot cue badges on a row preview", () => {
  /** A 2D context that records fills and text, in the order they happened. */
  function recorder() {
    const ops: (
      | { kind: "rect"; style: string; x: number; y: number; w: number; h: number }
      | { kind: "text"; style: string; text: string; x: number; y: number }
    )[] = [];
    const ctx = {
      fillStyle: "",
      font: "",
      textAlign: "",
      textBaseline: "",
      fillRect(x: number, y: number, w: number, h: number) {
        ops.push({ kind: "rect", style: String(this.fillStyle), x, y, w, h });
      },
      fillText(text: string, x: number, y: number) {
        ops.push({ kind: "text", style: String(this.fillStyle), text, x, y });
      },
    };
    return { ctx: ctx as unknown as CanvasRenderingContext2D, ops };
  }

  it("draws a 7pt square at the cue with a black letter centred in it", () => {
    // Measured off the 2x capture: 14x14 device pixels, left edge on the cue.
    const { ctx, ops } = recorder();
    drawPreviewCues(ctx, [["B", 90_000, "#F09235"]], 300_000, 200, 2);
    expect(ops).toEqual([
      { kind: "rect", style: "#F09235", x: 60, y: 0, w: 14, h: 14 },
      { kind: "text", style: "#000000", text: "B", x: 67, y: 7 },
    ]);
    expect(ctx.font).toBe('700 12px Arial, "Helvetica Neue", Helvetica, sans-serif');
    expect(ctx.textAlign).toBe("center");
    expect(ctx.textBaseline).toBe("middle");
  });

  it("falls back to rekordbox's default green without a colour", () => {
    const { ctx, ops } = recorder();
    drawPreviewCues(ctx, [["A", 0, null]], 300_000, 200, 1);
    expect(ops[0]).toMatchObject({ kind: "rect", style: "#3CEB50" });
  });

  it("keeps a badge at the very end inside the strip rather than cutting it off", () => {
    const { ctx, ops } = recorder();
    drawPreviewCues(ctx, [["H", 299_900, "#D9AC3A"]], 300_000, 200, 1);
    expect(ops[0]).toMatchObject({ kind: "rect", x: 193, w: 7 });
  });

  it("paints in the order given, so a later slot covers an earlier one", () => {
    // "Love To Give" carries D and H a millisecond apart, and the capture
    // shows H on top. The backend hands the cues over in slot order.
    const { ctx, ops } = recorder();
    drawPreviewCues(ctx, [["D", 162_486, "#77E866"], ["H", 162_485, "#D9AC3A"]], 288_000, 200, 1);
    const letters = ops.flatMap((op) => (op.kind === "text" ? [op.text] : []));
    expect(letters).toEqual(["D", "H"]);
  });

  it("draws nothing for a track with no cues, no length, or no width", () => {
    const { ctx, ops } = recorder();
    drawPreviewCues(ctx, [], 300_000, 200, 1);
    drawPreviewCues(ctx, [["A", 0, null]], 0, 200, 1);
    drawPreviewCues(ctx, [["A", 0, null]], 300_000, 0, 1);
    expect(ops).toEqual([]);
  });

  it("matches the tokens the stylesheet ships", () => {
    const css = readFileSync("src/styles/tokens.css", "utf8");
    const token = (name: string) => new RegExp(`${name}:\\s*([^;]+);`).exec(css)?.[1]?.trim();
    const { ctx, ops } = recorder();
    drawPreviewCues(ctx, [["A", 0, null]], 300_000, 200, 1);
    expect(ops[0]).toMatchObject({ w: Number.parseFloat(token("--s-preview-cue-badge") ?? "") });
    expect(ops[0]).toMatchObject({ style: token("--c-cue-hot")?.toUpperCase() });
    expect(ops[1]).toMatchObject({ style: token("--c-cue-hot-text")?.toUpperCase() });
    expect(ctx.font).toBe(`700 ${token("--f-size-preview-cue")} ${token("--f-ui")}`);
  });
});

describe("the BLUE and RGB palettes", () => {
  function recorder() {
    const fills: { style: string; x: number; y: number; w: number; h: number }[] = [];
    const ctx = {
      fillStyle: "",
      clearRect: () => undefined,
      fillRect(x: number, y: number, w: number, h: number) {
        fills.push({ style: String(this.fillStyle), x, y, w, h });
      },
    };
    return { ctx: ctx as unknown as CanvasRenderingContext2D, fills };
  }

  it("each palette reads its own tag, at its own bytes per column", () => {
    expect([waveformKindOf("3band", false), waveformKindOf("3band", true)]).toEqual(["bands", "bandsDetail"]);
    expect([waveformKindOf("blue", false), waveformKindOf("blue", true)]).toEqual(["mono", "monoDetail"]);
    expect([waveformKindOf("rgb", false), waveformKindOf("rgb", true)]).toEqual(["colour", "colourDetail"]);
    expect([strideOf("3band", true), strideOf("blue", true), strideOf("rgb", false), strideOf("rgb", true)])
      .toEqual([3, 1, 6, 2]);
  });

  it("BLUE reads five bits of height and three of whiteness from one byte", () => {
    // Full height, no whiteness: the bass blue. Then half height, all white.
    const { ctx, fills } = recorder();
    drawColumns(ctx, new Uint8Array([0x1f, 0x0f | (7 << 5)]), 2, 100, "blue", false);
    expect(fills).toHaveLength(2);
    expect(fills[0]!.style).toBe("rgb(0,85,225)");
    expect(fills[0]!.h).toBeCloseTo(100, 5);
    expect(fills[1]!.style).toBe("rgb(245,235,215)");
    expect(fills[1]!.h).toBeCloseTo((15 / 31) * 100, 5);
  });

  it("RGB reads PWV4's height and its three channels, the strongest at full", () => {
    // Height 127 of 127; channels low 40 (red), mid 10 (green), high 80
    // (blue) → blue strongest.
    const { ctx, fills } = recorder();
    drawColumns(ctx, new Uint8Array([127, 200, 30, 40, 10, 80]), 1, 100, "rgb", false);
    expect(fills).toHaveLength(1);
    expect(fills[0]!.style).toBe("rgb(128,32,255)");
    expect(fills[0]!.h).toBeCloseTo(100, 5);
  });

  it("RGB detail reads PWV5's rrrgggbbhhhhh00 word", () => {
    // r 7, g 0, b 3, height 16: 111 000 011 10000 00.
    const word = (7 << 13) | (0 << 10) | (3 << 7) | (16 << 2);
    const { ctx, fills } = recorder();
    drawColumns(ctx, new Uint8Array([word >> 8, word & 0xff]), 1, 62, "rgb", true);
    expect(fills).toHaveLength(1);
    expect(fills[0]!.style).toBe("rgb(255,0,109)");
    expect(fills[0]!.h).toBeCloseTo((16 / 31) * 62, 5);
  });

  it("a half waveform grows from the floor, a centred one from the middle", () => {
    const { ctx: a, fills: half } = recorder();
    drawColumns(a, new Uint8Array([0x1f]), 1, 100, "blue", false, true);
    expect(half[0]!.y + half[0]!.h).toBeCloseTo(100, 5);
    const { ctx: b, fills: centred } = recorder();
    drawColumns(b, new Uint8Array([0x10]), 1, 100, "blue", false);
    expect(centred[0]!.y + centred[0]!.h / 2).toBeCloseTo(50, 5);
  });
});


describe("preview memory cues", () => {
  it("centres red downward triangles at saved times and skips invalid positions", () => {
    const points: number[][] = [];
    let fills = 0;
    const ctx = {
      fillStyle: "", beginPath() {}, closePath() {},
      moveTo(x: number, y: number) { points.push([x,y]); },
      lineTo(x: number, y: number) { points.push([x,y]); },
      fill() { fills++; },
    };
    drawPreviewMemoryCues(ctx as unknown as CanvasRenderingContext2D, [250, -1, 1001, NaN], 1000, 200, 2);
    expect(ctx.fillStyle).toBe("#EA3323");
    expect(points).toEqual([[44,0],[56,0],[50,8]]);
    expect(fills).toBe(1);
  });
});

describe("a click on a row preview", () => {
  // rekordbox: PreviewComponent::clickWave starts at the click's fraction of
  // the width, clamped; cueRegionMouseDown starts at a clicked badge's cue.
  it("starts at the click's fraction of the track", () => {
    expect(previewClickMs(50, 10, 200, 300_000, [])).toBe(75_000);
    expect(previewClickMs(0, 10, 200, 300_000, [])).toBe(0);
  });

  it("clamps a click past either end to the track", () => {
    expect(previewClickMs(-4, 10, 200, 300_000, [])).toBe(0);
    expect(previewClickMs(260, 10, 200, 300_000, [])).toBe(300_000);
  });

  it("starts at a hot cue when its badge is clicked", () => {
    const cues = [["A", 30_000, null], ["B", 150_000, "#ff0000"]] as const;
    // B's badge is drawn from x = 100 for PREVIEW_BADGE pixels.
    expect(previewClickMs(100 + PREVIEW_BADGE - 1, 2, 200, 300_000, cues)).toBe(150_000);
    // Below the badges the same x is just a place in the track.
    expect(previewClickMs(100 + PREVIEW_BADGE - 1, PREVIEW_BADGE + 1, 200, 300_000, cues)).toBeCloseTo(159_000);
  });

  it("gives an overlapping click to the badge drawn on top", () => {
    const cues = [["A", 100_000, null], ["B", 101_000, null]] as const;
    expect(previewClickMs(68, 1, 200, 300_000, cues)).toBe(101_000);
  });

  it("finds the badge pushed in from the right edge", () => {
    const cues = [["H", 300_000, null]] as const;
    expect(previewClickMs(200 - PREVIEW_BADGE, 1, 200, 300_000, cues)).toBe(300_000);
  });

  it("is the top for a track with no length", () => {
    expect(previewClickMs(50, 1, 200, 0, [])).toBe(0);
  });
});
