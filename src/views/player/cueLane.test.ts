/**
 * The detail waveform's cue lane (#393).
 *
 * rekordbox 7.2.14, observed on chris-win11 at 150% with a scratch library
 * (parity/issue-393/rekordbox-04-deck-paused-1x.png), draws from the top of
 * the detail band down: the memory cue's red triangle and the hot cue's
 * badge, the beat heads on the row after the badge's last, then the waveform.
 * No cue marker covers the audio. These pin that order in the tokens the
 * stylesheet and the canvas inset read, so a later change to one of them
 * cannot slide a badge back over the waveform.
 */
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const css = readFileSync("src/styles/tokens.css", "utf8");

function token(name: string): string {
  const match = new RegExp(`^\\s*${name}:\\s*([^;]+);`, "m").exec(css);
  if (!match?.[1]) throw new Error(`no ${name} in tokens.css`);
  return match[1].trim();
}
const px = (name: string) => Number.parseFloat(token(name));

describe("the detail's cue lane", () => {
  it("keeps the hot cue badge above the beat heads, as rekordbox does", () => {
    // rekordbox: badge y 144..162, beat heads from y 163.
    expect(px("--s-cue-badge-top") + px("--s-cue-badge")).toBeLessThanOrEqual(px("--s-beat-marker-top"));
    expect(px("--s-cue-badge-top")).toBeGreaterThanOrEqual(0);
  });

  it("keeps the memory cue triangle above the beat heads, starting over the badge", () => {
    // rekordbox: triangle y 134..148, over the badge's top rows.
    expect(px("--s-cue-marker-top")).toBeGreaterThanOrEqual(0);
    expect(px("--s-cue-marker-top")).toBeLessThan(px("--s-cue-badge-top"));
    expect(px("--s-cue-marker-top") + px("--s-cue-marker-h")).toBeLessThanOrEqual(px("--s-beat-marker-top"));
  });

  it("starts the waveform below every cue marker", () => {
    const wave = px("--s-wave-inset-top");
    expect(px("--s-cue-badge-top") + px("--s-cue-badge")).toBeLessThanOrEqual(wave);
    expect(px("--s-cue-marker-top") + px("--s-cue-marker-h")).toBeLessThanOrEqual(wave);
    // The beat markers still reach 15px above the waveform (by request, 2026-09-18).
    expect(wave - px("--s-beat-marker-top")).toBe(15);
  });

  it("paints the playhead in the colours rekordbox does", () => {
    // Two device columns of flat colour at 150%: paused and playing.
    expect(token("--c-playhead-armed").toUpperCase()).toBe("#FF0000");
    expect(token("--c-playhead").toUpperCase()).toBe("#E0E0E0");
  });
});
