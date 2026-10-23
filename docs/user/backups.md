# Backups

[Documentation](../README.md)

Preferences → Backups creates and lists library backups. Quit rekordbox before
creating one. Music files are excluded, so keep a separate backup of your audio.
If a backup says the database is busy, close rekordbox or another app using
the library and try again. Leave Library Protection enabled; creating a backup
does not require write access to the library.

## Contents and format

A new backup is a ZIP file named `rbexport-YYYYMMDD-HHMM.zip` using local
24-hour time. It contains:

- The library database, including playlists, tags, ratings, and history.
- The complete `PIONEER/USBANLZ` analysis folder, including cues, grids, waveforms, phrases, and vocals.
- `PIONEER/Artwork` images and thumbnails.
- Sync Manager selections and the Automix playlist, when present.
- `summary.json`, recording track/playlist/cue counts and component sizes.

ZIP creation uses maximum Deflate compression and parallel work based on CPU
capacity. A same-minute backup is never overwritten. Backups stay until deleted.

## Storage and estimates

The saved-backup list shows date, time, and size. **Default backup folder →
Change folder…** changes where future backups are written and which folder
is listed. Existing archives remain in their original folders. The setting
persists across restarts.

The Rekordbox Data bar saves its last successful estimate. It refreshes a
week-old estimate on opening Backups or while that pane stays open.
**Refresh** requests an immediate estimate.

## Restore

Restore through the separate RBXport Restore app. Quit both RBXport and
rekordbox first. The restore app lists backups and uses `summary.json` when
available to show their contents.

Restore the whole backup or selected components: database, analysis, artwork,
or Sync Manager/Automix selections. If interrupted, the next RBXport or
RBXport Restore startup rolls back the operation, or completes it when new
files were already in place, before opening the library.

## Developer entry points

Backup integration lives in `src-tauri/src/backups.rs` and the adjacent
backup helper modules, with shared archive logic in `crates/rbl-backup/`. [How library backups work](../reference/backups.md)
describes the implementation and its design.
Validate backup/restore changes against disposable data, following [Testing](../development/testing.md).
