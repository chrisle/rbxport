/**
 * The eight memory cue colours, and the colour a memory cue's marker is drawn
 * in on the deck's waveforms.
 *
 * rekordbox stores a memory cue's colour in `djmdCue.Color`, 0 to 7 in the
 * order below, and 255 for none. The backend sends the MEMORY list's dot
 * colour for it (`track_cues`), or `null` for none.
 *
 * [OBS] static, rekordbox 7.2.19 (arm64): both deck waveforms draw a memory
 * cue from one sprite sheet, `skins/memoryCueImageDown{1,2}.png`, a row per
 * colour (pink, red, orange, yellow, green, aqua, blue, purple, then white).
 * `WidgetWaveView::drawHotCueGL` @`0x10212245c` (detail) and
 * `WidgetLoudView::drawTextures` @`0x100fd1fcc` (overview) both take the row
 * for `Color` when it is 0-7 and the red row otherwise, so an uncoloured
 * memory cue is drawn exactly as a red one. The browser preview does the same
 * with `skins/p2254_mempink.png` … `p2261_mempurple.png`
 * (`BrowseHelper::getCueImageForPreview` @`0x101fc96c4`).
 *
 * The sprite rows are sRGB `#F870F8 #FF0000 #F8A030 #F8E330 #00E000 #00C0F8
 * #0050F8 #9808F8`, and the preview images the same but for red, `#F80000`.
 * The values here are those colours as the Display P3 captures the other cue
 * colours were measured from show them: the list's are exactly the preview
 * images converted, and the deck waveform's red, `#FF0000`, is `#EA3323`, the
 * `--c-cue-head` an uncoloured memory cue already took. So the waveform
 * marker is the list colour, but for red. The row preview uses the same: its
 * red, `#F80000`, is the list's, and drawing it in the deck's red, as the
 * preview's uncoloured cues always were, keeps none and Red alike as
 * rekordbox does at a difference of 6 in the red channel.
 */

/** Name and MEMORY list colour, in `djmdCue.Color` order. */
export const MEMORY_CUE_COLOURS = [
  ["Pink", "#E778F1"], ["Red", "#E33122"], ["Orange", "#EBA44A"], ["Yellow", "#F4E458"],
  ["Green", "#66DD42"], ["Aqua", "#56BDF3"], ["Blue", "#204FEF"], ["Purple", "#8B1EEF"],
] as const;

/** The MEMORY list colour for a `djmdCue.Color`, or `null` for none (-1, 255). */
export function memoryCueListColour(color: number | null | undefined): string | null {
  return color === null || color === undefined ? null : MEMORY_CUE_COLOURS[color]?.[1] ?? null;
}

/**
 * A row's `memoryCueColours` from its memory cues' list colours, as the
 * backend builds it: each `djmdCue.Color`, 255 for none, and `undefined`
 * when none has one.
 */
export function rowMemoryCueColours(colours: readonly (string | null)[]): number[] | undefined {
  const indices = colours.map((colour) => {
    const index = MEMORY_CUE_COLOURS.findIndex(([, hex]) => hex === colour?.toUpperCase());
    return index < 0 ? 255 : index;
  });
  return indices.some((index) => index !== 255) ? indices : undefined;
}

/** The deck waveforms' red: `memoryCueImageDown`'s `#FF0000`, `--c-cue-head`. */
const WAVEFORM_RED = "#EA3323";

/**
 * The colour a memory cue's marker is drawn in on the deck waveforms, from
 * the colour the backend sent: red for none, as rekordbox draws it.
 */
export function memoryCueMarkerColour(colour: string | null | undefined): string {
  if (!colour) return WAVEFORM_RED;
  const upper = colour.toUpperCase();
  return upper === MEMORY_CUE_COLOURS[1][1] ? WAVEFORM_RED : upper;
}
