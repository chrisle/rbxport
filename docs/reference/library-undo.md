# Library Undo coverage

[Documentation](../README.md)

Which user-visible actions that change the library, its playlists, a device
library or user files can be undone, and why the rest cannot. Audited for
#427 on 2026-10-16 against `dev` at `4a42662a`. Update the table when an
action is added or starts recording history.

Evidence markers follow the project convention: `[OBS]` is something read in
the code or observed, `[ASSUME]` is inferred, `[UNKNOWN]` is open.

## How library history works

- **One stack per session.** `AppState::edit_history` (`src-tauri/src/state.rs`)
  holds up to 50 `LibraryEdit` entries for Undo and a Redo stack. It is not
  saved; quitting, switching the Master Database (the app restarts) or a
  restore (run outside the app by the backup's scripts) leaves no history
  [OBS code].
- **Three ways to write.** In `src-tauri/src/commands.rs`:
  - `recorded_edit` writes and pushes one entry, clearing Redo.
  - `edit` writes without recording and clears Redo: "a non-recorded edit
    after an undo starts a new branch".
  - `permanent_edit` writes and clears both stacks, because an older entry
    could name rows the write removed for good.

  Writers that call `AppState::write` directly (imports, relocation, cues,
  analysis, LINK) record nothing. Since #427 the imports and relocations
  among them call `wrote_outside_history`, which clears Redo, or both stacks
  for a replace, then send `edit-history:changed` [OBS code].
- **Payloads are exact.** Entries keep row ids and before/after column values
  (`PlaylistDeletion`, `PlaylistRename`, `PlaylistMove`,
  `PlaylistTrackRemoval`, `TrackEdit`, `TrackTagEdit` in
  `crates/rbl-db/src/write.rs`). A multi-track edit is one entry with one
  step per track, undone last first [OBS code].
- **No conflict check.** Undo and Redo of a track edit write the stored value
  whether or not the row changed since. Playlist deletion refuses when a row
  is no longer in the state it expects [OBS code; #506].
- **Other histories.** Text fields keep WebKit's own history. Each track's
  beat grid has its own stack in `GridEditor` (`src-tauri/src/grid.rs`),
  which Edit › Undo reaches while a deck's grid editor is armed
  (`listenEditHistory`, `src/lib/editHistory.ts`) [OBS code].

## rekordbox

rekordbox has no library Undo to copy. In rekordbox 7.2.19 (macOS), no
browse, tree, collection or database code calls `juce::UndoManager`. Only
the lighting editors (`CLCE*`, `djplay::LightingPanel`, fixture maps),
`djplay::BeatGridAdjustment`, the JUCE text and code editors, and "Undo
track load" (`browse::ListViewer::undoTrackLoad`) do
[OBS static: every caller of `juce::UndoManager` in the full disassembly,
listed in `rbxport-worktrees/parity/issue-427/values-rekordbox-undomanager-callers.txt`
next to the main checkout; not committed]. Library Undo is
RBXport's own feature. Its scope is a product choice (#427), and each
follow-up names the contract it extends. rekordbox's confirmations ("Are you
sure you want to remove…") are matched where RBXport asks the same question.

## Audit

Status values:

- **Undo**: recorded, with a test.
- **Not recorded**: reversible in principle but not in the history yet. The
  follow-up ticket is listed.
- **Permanent**: deliberately has no Undo. The history is cleared and the app
  says so before acting.
- **External**: the change is to a device, a file outside the library, or a
  setting, where no truthful in-app Undo is possible.

"Destructive" means the action discards user data that the app does not keep
anywhere else. Soft-deleted rows count as kept only while an Undo entry
holds their ids.

### Collection and track metadata

| Action | Entry point | Writes | Status | Evidence | Destructive |
| --- | --- | --- | --- | --- | --- |
| Rating, colour, comment (one or many tracks) | `set_track_rating`, `set_track_color`, `set_track_comment` | `djmdContent` columns | Undo, one entry per selection | `library_history_names_and_reverses_each_supported_edit`; `a_failure_partway_through_several_tracks_puts_the_earlier_ones_back` (`commands.rs` unit) | No |
| Info panel fields (title, artist, album, genre, key, year…) | `details::set_track_field` | `djmdContent` columns, interned names | Undo | same test (title); multi-track genre unit test | No |
| Key with "Write the value to the ID3 tag" | same, `write_key_tag` | also the audio file's key tag | Undo restores the database only; the file keeps the new key | code (`details.rs`) | File tag: yes. Policy in #507 |
| BPM in the info panel | `set_track_field` `bpm` → `grid::set_tempo` | `.DAT` grid, `BPM` | Not recorded, and it clears the track's grid history | code | Grid: yes. #505 |
| My Tags | `details::set_my_tags` | `djmdSongMyTag` | Undo | code (`LibraryEdit::TrackTags`) [ASSUME: no command-level test] | No |
| Add / Delete Artwork (tracks) | `details::add_artwork`, `clear_artwork` | `ImagePath`; image copied into the share | Undo (path only; the copied file stays) | code [ASSUME: no command-level Undo test] | No |
| Reset DJ Play Count | `reset_play_count` | `DJPlayCount` | Undo | code [ASSUME: no command-level Undo test] | No |
| Reload Tag | `reload_tags` | Title, Comment, Artist, Album, Genre, Label, Key, Year, Track No. from the file | Not recorded | code | Yes: manual edits lost. #503 |
| Remove from Collection, Delete key | `remove_from_collection` | soft-deletes tracks and their memberships | Permanent: clears history, "This can't be undone" | `removing_from_collection_is_permanent_and_clears_history` | Yes, by design |
| Missing File Manager › Delete | `relocate::remove_missing_tracks` | same | Permanent | code | Yes, by design |
| Relocate (one track) | `relocate_track` | `FolderPath`, `FileNameL`, `rb_file_id` | Not recorded | code | Old path lost. #502 |
| Relocate by location, Auto Relocate | `relocate::relocate_by_location`, `auto_relocate` | same, many tracks | Not recorded; ends the redo branch | code | Old paths lost. #502 |
| Analyze (BPM, key, waveform, grid) | `analysis::analyse_track` | `.DAT/.EXT/.2EX`, BPM, key, analysis path | Not recorded [ASSUME: matches rekordbox, which has no analysis Undo] | code | Re-analysis replaces the old grid |
| Phrase edit | `analysis::edit_phrase` | `.EXT` phrase section | Not recorded | code. [UNKNOWN] whether rekordbox's phrase editing undoes; `editapp::ScoreEditManager::undo` exists but is not traced to it | Previous phrases lost |
| Beat grid edits, grid lock | `grid::grid_edit`, `grid_lock` | `.DAT/.EXT`, `BPM`, lock | Grid edits: own per-track Undo; lock: not recorded | `useGridEditor.test.tsx`, `e2e/grid-edit.spec.ts` | No |
| Cue and loop edits, Convert Memory Cues to Hot Cues | `cues::*` | `djmdCue`, ANLZ cue sections | Not recorded, and does not end the redo branch | code | Deleted cue lost. #504, #506 |

### Playlists, folders and intelligent playlists

| Action | Entry point | Writes | Status | Evidence | Destructive |
| --- | --- | --- | --- | --- | --- |
| Rename | `rename_playlist` | `Name` | Undo | `library_history_names_and_reverses_each_supported_edit` | No |
| Move (drag in the tree) | `move_playlist` | `ParentID`, sibling `Seq` | Undo | same | No |
| Delete playlist or folder (with contents) | `delete_playlist` | tombstones subtree and memberships | Undo | `a_deleted_playlist_tree_can_be_undone_and_redone` | No while the entry lives |
| Remove Tracks from Playlist | `remove_tracks_from_playlist` | tombstones memberships, renumbers | Undo | `library_history_names_and_reverses_each_supported_edit` | No |
| Add Tracks to Playlist (menu, drop, duplicates "Add") | `add_tracks_to_playlist` | new memberships | Not recorded | code | No. #499 |
| Reorder, Renumber Track Order | `reorder_playlist` | `TrackNo` of every member | Not recorded | code | Previous order lost. #499 |
| Create Playlist / Folder / Intelligent Playlist (also AppleScript `make playlist`) | `create_playlist`, `create_folder`, `create_smart_playlist` | new `djmdPlaylist` row | Not recorded | code | No. #500 |
| Edit Intelligent Playlist rule | `set_smart_rule` | `SmartList` | Not recorded | code | Old rule lost. #500 |
| Add Artwork (playlist or folder) | `details::add_playlist_artwork` | `ImagePath` | Not recorded | code | Old image reference lost. #500 |
| Tag List: add | `add_to_tag_list` | Tag List rows | Not recorded | code | No |
| Tag List: remove, clear | `remove_from_tag_list`, `clear_tag_list` | soft-deletes Tag List rows | Not recorded; asks first | code | Yes. #508 |
| Remove from History | `remove_from_history` | soft-deletes `djmdSongHistory` rows | Not recorded; asks first | code | Yes. #508 |
| A play recorded by the deck | `record_play` | history row, `DJPlayCount` | Not a user edit; not recorded | code | No |

### Imports

| Action | Entry point | Writes | Status | Evidence | Destructive |
| --- | --- | --- | --- | --- | --- |
| Import files or folders, drop onto the collection | `import_files`, `import_files_slice` | new tracks | Not recorded; ends the redo branch. Reversing it is Remove from Collection, which is Permanent | `an_import_ends_the_redo_branch_and_keeps_undo` | No |
| Folder drop onto the tree, new or "Import as copy" | `import_folder_playlist` | new tracks, playlist, memberships | Not recorded; ends the redo branch | `replacing_a_list_with_a_dropped_folder_ends_the_library_history` (copy part) | No |
| Folder drop, "Replace existing" | same, `replace` | tombstones the replaced subtree, then imports | Permanent for now: clears history (Undo of older entries could resurrect rows into a deleted list) | same test | Yes. #426 |
| Import XML / iTunes | `import_xml`, `import_itunes`, `import_itunes_selected` | tracks, rating, comment, cues, playlists | Not recorded; ends the redo branch | `an_xml_import_that_would_replace_lists_writes_nothing_until_told_to` | No |
| Import XML / iTunes, replace same-named lists | same, `replace` | rewrites members of existing lists (`set_tracks`) | Permanent for now: clears history | `an_xml_import_that_replaces_a_list_ends_the_library_history` | Yes. #501 |
| Import a device playlist into the library | `device_library::device_playlist_import` | tracks, playlist | Not recorded; ends the redo branch (`edit`) | code | No |
| USB import (cues, grids, histories, ratings, My Settings) | `usb_import::import_usb` | ANLZ files, `djmdCue`, histories, ratings, settings stash | Not recorded; ends the redo branch (also after a failure part way) | code | Cue/grid and rating overwrites are not kept |

### Devices, exports and other external effects

| Action | Entry point | Writes | Status | Evidence |
| --- | --- | --- | --- | --- |
| Device library: create, rename, delete, add, remove (on a stick) | `device_library::device_playlist_edit` | the stick's `export.pdb` / OneLibrary | External. rekordbox has no Undo here either and RBXport asks in rekordbox's words [OBS 7.2.14, `rekordbox-19`, cited in `App.tsx`] | code |
| Sync, Export to device, Export Track | `sync_devices`, `export_playlist`, `export_tracks_to_device` | files and databases on the stick; "delete unlisted music" removes audio there | External | `docs/reference/usb-export-pipeline.md` |
| Format, Eject | `format_device`, `eject_device` | the device | External, irreversible | code |
| Stick settings | `device_settings::save_device_settings`, `write_device_defaults` | `MYSETTING*.DAT` on the stick | External | code |
| Export XML, playlist file, loop WAV | `export_xml`, `export_playlist_file`, `export_loop_wav` | a file the person chose | External | code |
| Backups: create, delete, change folder | `back_up_library`, `delete_backup`, `set_backup_directory` | archives outside the library | External. Delete asks "This cannot be undone" | code |
| Edits from players over LINK (rating, Tag List, cues, history) | `link.rs` `StateSource::edit`, `history_write`, `player_cue_edit` | the library | Not recorded, and does not end the redo branch | code. #506 |
| Preferences | `preferences.rs`, `localStorage` | settings | Out of scope: not library data | — |

## Policy choices still open

These are the only new product decisions the audit found. Each is filed with
`needs-product-decision`:

1. What Undo/Redo should do when a recorded row was changed since by
   something outside the history (LINK, USB import, rekordbox): refuse,
   clear everything, or overwrite as now (#506).
2. Whether Undo of a key edit that also wrote the file tag should rewrite the
   tag, only say it did not, or not be offered (#507).

The other gaps (#426, #499 to #505, #508) extend the existing contract and
need no new policy.
