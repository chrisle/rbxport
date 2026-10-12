/**
 * What deck 1 plays when its track runs out: the next row of the playlist
 * it was loaded from.
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
 * @0x10040f380 lets it through in EXPORT mode for a playlist without
 * reading Preferences › Controller › Continuous Playing, which is a
 * PERFORMANCE-mode setting (manual 7.2.18 p. 241) and is not among the
 * EXPORT-mode preferences [OBS Winrig, rekordbox 7.2.14].
 */
import type { DeckTick, TrackSource, ViewSpec } from "@/ipc/types";

/** Where a loaded track came from: the list as it was shown, and its row. */
export interface ListPosition {
  spec: ViewSpec;
  index: number;
}

/**
 * Whether a list continues into its next row. rekordbox's 1 PLAYER lets a
 * playlist or a Related Tracks list through and nothing else
 * (`canLoadNextTrack`); a device's playlists are turned away. Related
 * Tracks are left out here: they are worked out from the track on deck 1,
 * so loading the next one changes the list it came from [UNKNOWN] what
 * rekordbox's list does then.
 */
export function continuesFrom(source: TrackSource): boolean {
  return source.kind === "playlist";
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
  if (ids.length === 0) return -1;
  const at = ids[loadedFrom] === trackId ? loadedFrom : ids.indexOf(trackId);
  if (at < 0) return 0;
  return at === ids.length - 1 ? 0 : at + 1;
}
