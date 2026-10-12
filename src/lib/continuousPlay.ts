/**
 * What deck 1 plays when its track runs out: the next row of the list it
 * was loaded from.
 *
 * rekordbox does this in EXPORT mode without a setting. At the end of a
 * track the player asks the browser for the next one, but only in the
 * layouts that show one player [OBS rekordbox 7.2.19 static,
 * `UiPlayer::timerSongTime` @0x101ac07f0: posts the TRACK SEARCH switch
 * when the deck is playing, not looping and at its end, and in EXPORT mode
 * only while `WindowLayout` is `singleplayer.xml`, `simpleplayer.xml` or
 * `hideplayer.xml`]. The switch becomes `ListViewer::loadNextTrack` with
 * `force` off (`WidgetMixer::operateCommBrowse` @0x1019f1664,
 * `WidgetBrowse::operate` @0x100c89c84), and `canLoadNextTrack`
 * @0x10040f380 decides by panel, list and layout (see `continuesFrom`)
 * without reading Preferences › Controller › Continuous Playing, which is a
 * PERFORMANCE-mode setting (manual 7.2.18 p. 241) and is not among the
 * EXPORT-mode preferences [OBS Winrig, rekordbox 7.2.14].
 */
import type { DeckTick, TrackSource, ViewSpec } from "@/ipc/types";
import type { PlayerLayout } from "@/lib/layout";

/** Where a loaded track came from: the list as it was shown, and its row. */
export interface ListPosition {
  spec: ViewSpec;
  index: number;
}

/**
 * Whether deck 1 goes on into the next row of the list its track came from,
 * in this layout. rekordbox decides this at the end of the track in
 * `ListViewer::canLoadNextTrack` @0x10040f380, and in EXPORT mode it asks
 * two things [OBS rekordbox 7.2.19 static]:
 *
 * - Which panel the list is in (`BrowseHeader::getHeaderType`, vtable
 *   +0x340): 0 the browser's track list and the sub-browser, 1 Tag List,
 *   2 palette, 3 Related Tracks panel, 4 Track Suggestion, 5 Automix,
 *   7 Request Catalog (`createBrowseHeader` @0x100b6d770 and each header's
 *   `getHeaderType`). Panels 3 and 4 never continue (@0x10040f3f0).
 * - The layout. Anything but `singleplayer.xml` (1 PLAYER) continues from
 *   every other panel without looking at the list (@0x10040f468-78), so
 *   SIMPLE PLAYER goes on from the Collection, the Explorer and histories.
 *   In 1 PLAYER the track list continues only for a playlist
 *   (`BrowseProperty::isPlayList`, list type 2) or a Related Tracks node
 *   (`isRelatedList`, type 10), and not for a playlist of browse type 4,
 *   which the import-from-bridge code uses [ASSUME rekordbox xml]; a USB
 *   stick is browse type 2 and its playlists continue. A palette stops; the
 *   Tag List goes on (@0x10040f540).
 *
 * 2 PLAYER never asks (`UiPlayer::timerSongTime`). FULL BROWSER does in
 * rekordbox, but here it has no deck to load the track into.
 *
 * Related Tracks are left out: they are worked out from the track on deck 1,
 * so loading the next one changes the list it came from [UNKNOWN] what
 * rekordbox's list does then. A playlist folder's list type in 1 PLAYER is
 * [UNKNOWN] too, so it stops there.
 */
export function continuesFrom(source: TrackSource, layout: PlayerLayout): boolean {
  if (layout !== "one" && layout !== "simple") return false;
  if (source.kind === "related") return false;
  if (layout === "simple") return true;
  switch (source.kind) {
    case "playlist":
    case "tagList":
      return true;
    case "device":
      // Playlist "0" is the stick's All Tracks, not a playlist.
      return source.playlist !== "0";
    default:
      return false;
  }
}

/**
 * Whether the deck has just played to the end of its track, between two
 * ticks: it was playing, and the engine has stopped it because the track ran
 * out. A pause is not one, and neither is a loop, which never reaches the
 * end. A build that does not say falls back on the head standing at the
 * length the file declares.
 */
export function playedToEnd(before: DeckTick | null, after: DeckTick): boolean {
  if (before === null || !before.playing || after.playing) return false;
  if (!after.loaded || after.looping) return false;
  if (before.loadId !== undefined && after.loadId !== before.loadId) return false;
  return after.ended ?? (after.totalFrames > 0 && after.frames >= after.totalFrames);
}

/**
 * The row after the loaded one, as rekordbox picks it
 * (`ListViewer::getNextTrackIndex` @0x10040f920):
 *
 * - the row the track was loaded from, when it still holds that track, so
 *   a track listed twice moves on from the copy that was played;
 * - otherwise the track's first row in the list;
 * - a track no longer in the list starts the list again from its first row;
 * - after the last row comes the first.
 *
 * `ids` are the list's rows in the order the browser shows them. -1 for an
 * empty list.
 */
export function nextRowIndex(ids: readonly string[], trackId: string, loadedFrom: number): number {
  return skipRowIndex(ids, trackId, loadedFrom, "next");
}

/**
 * How far into its track deck 1 must be for the previous-track button to go
 * back to the start of it rather than to the row before. rekordbox compares
 * `DjEngineIF::getPlayingTime` with 1000 [OBS rekordbox 7.2.19 static,
 * `ListViewer::getNextTrackIndex` @0x10040fb04]; that it is milliseconds of
 * the head's place is [ASSUME].
 */
export const RESTART_AFTER_MS = 1000;

/**
 * Whether the previous-track button goes back to the start of the loaded
 * track: it has played more than `RESTART_AFTER_MS` of it. Clicked again from
 * there, it goes to the row before. rekordbox's manual (7.2.18 p.68): "Move
 * to the beginning of the track currently playing. Click twice to move to the
 * previous track."
 */
export function previousRestarts(positionMs: number): boolean {
  return positionMs > RESTART_AFTER_MS;
}

/**
 * The row the track skip buttons load, as rekordbox picks it
 * (`ListViewer::getNextTrackIndex` @0x10040f920, called with `next` set for
 * the next button and clear for the previous one): from the loaded track's
 * row, found as `nextRowIndex` describes, the next row with the last one
 * wrapping to the first, or the row before with the first one wrapping to the
 * last. A track no longer in the list starts it again from its first row
 * either way. -1 for an empty list.
 *
 * The previous button's restart is not a row: see `previousRestarts`.
 */
export function skipRowIndex(
  ids: readonly string[], trackId: string, loadedFrom: number, direction: "previous" | "next",
): number {
  if (ids.length === 0) return -1;
  const at = ids[loadedFrom] === trackId ? loadedFrom : ids.indexOf(trackId);
  if (at < 0) return 0;
  if (direction === "next") return at === ids.length - 1 ? 0 : at + 1;
  return (at > 0 ? at : ids.length) - 1;
}
