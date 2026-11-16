/**
 * What the INFO tab beside the deck prints, worked out from the loaded row
 * and its record.
 *
 * Pure, so the rules are unit-tested without the deck: which source each
 * line comes from, what an empty deck shows, and what a record that has not
 * arrived yet shows.
 */
import type { RowDto, TrackDetails } from "@/ipc/types";
import { colorDot, colorNameOf, STOCK_COLOR_NAMES } from "@/lib/colorNames";
import { fileFacts } from "@/views/info/fields";

/** What the tab draws. Every string is ready to print. */
export interface DeckInfo {
  /** 0 to 5. */
  rating: number;
  /** The colour's name, or empty for none or not yet known. */
  color: string;
  /** The stock name its dot is painted by, whatever the library calls it. */
  dot: string;
  comment: string;
  /**
   * File type, size, sample rate, bit rate — the four lines the manual
   * lists, in its order. An empty deck prints a dash on each; a track whose
   * record has not arrived prints nothing, rather than the last track's.
   */
  file: [string, string, string, string];
}

const EMPTY = "—";

export function deckInfo(
  track: RowDto | null,
  details: TrackDetails | null,
  colorNames: readonly string[] = STOCK_COLOR_NAMES,
): DeckInfo {
  if (!track) {
    return { rating: 0, color: "", dot: "", comment: EMPTY, file: [EMPTY, EMPTY, EMPTY, EMPTY] };
  }
  // The rating and comment come from the row, which the browser overlays
  // the moment either is edited there; the record catches up only when the
  // library announces the write. The colour and the file are not on the row.
  const d = details && details.id === track.id ? details : null;
  const file = d ? fileFacts(d) : null;
  return {
    rating: track.rating,
    color: d ? colorName(d.color, colorNames) : "",
    dot: d ? colorDot(d.color) : "",
    comment: track.comment,
    file: file ? [file.type, file.size, file.sampleRate, file.bitrate] : ["", "", "", ""],
  };
}

/** `"6"` → `"Aqua"`, or what the library calls it; `"0"`, empty and anything else → nothing. */
export function colorName(id: string, colorNames: readonly string[] = STOCK_COLOR_NAMES): string {
  return colorNameOf(colorNames, id);
}
