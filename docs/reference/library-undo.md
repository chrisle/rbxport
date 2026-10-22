# Library Undo coverage

[Documentation](../README.md)

Which user-visible actions that change the library, its playlists, a device
library or user files can be undone, and why the rest cannot. Audited for
#427 on 2026-10-16 against `dev` at `4a42662a`. Update the table when an
action is added or starts recording history. Folder replacement coverage was
updated during integration on 2026-10-21 to retain #426 support already in `dev`.
Cue and loop edits were added to the history for #504 on 2026-10-22.
Playlist, folder and Intelligent Playlist creation, rule edits and playlist
artwork were added for #500 on 2026-10-22.
XML and iTunes Replace existing became one Undo step for #501 on 2026-10-22.
Add Tracks to Playlist and playlist reordering were added for #499 on
2026-10-22.

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

  Writers that call `AppState::write` or `write_then` directly (imports,
  relocation, analysis, grid saves, LINK, deck plays) record nothing.
  Since #506 `write_then` itself ends the Redo branch whenever the write
  changed a row (SQLite `total_changes` on the writer's connection), and
  tells the window through the listener `lib.rs` sets
  (`AppState::on_history_changed` sends `edit-history:changed`). A write
  that changed nothing (a probe for a path) leaves Redo. The code that keeps
  the history itself (`recorded_edit`, `edit`, `permanent_edit`, Undo,
  Redo, and cue edits, which record their own entry under the edit gate
  since #504) uses `write_history_then`, which does not. Since #501 no
  path clears both stacks with `wrote_outside_history(_, true)`: XML/iTunes
  Replace existing records its own entry instead [OBS code].
- **Payloads are exact.** Entries keep row ids and before/after column values
  (`PlaylistDeletion`, `PlaylistRename`, `PlaylistMove`,
  `PlaylistTrackRemoval`, `PlaylistTrackAddition`, `PlaylistReorder`,
  `PlaylistCreation`, `PlaylistEdit`, `MembershipReplacement`, `TrackEdit`,
  `TrackTagEdit`, `CueRevision` in
  `crates/rbl-db/src/write.rs`). A multi-track edit is one entry with one
  step per track, undone last first [OBS code].
- **Conflict check (#506).** Before Undo or Redo writes, the rows it would
  write must still hold what the entry expects: the edit's result for Undo,
  what it replaced for Redo. Track fields compare by SQL `IS` (NULL equals
  NULL); My Tags as a set; a rename by name; a move by parent and place
  among its siblings; a track removal or addition, and an XML/iTunes
  replacement of a playlist's members, by the playlist's live members in
  order (an addition with their `TrackNo`); a reorder by those members and
  their `TrackNo`; deletion, Sort Items and folder-drop replacement by the rows they
  name. An entry of several tracks is checked whole before any is written.
  When something outside the history changed one since (a player over LINK,
  a USB import, rekordbox while RBXport stayed open), nothing is written,
  the newer value stays, only that entry leaves its stack, and the command
  fails with `ErrorKind::Conflict`: "Can't undo Track Edit: the track has
  changed since. The newer change was kept." The rest of the history
  stays; each entry checks its own rows when it runs. Policy chosen by
  Chris on 2026-10-22 (#506, option A) [OBS code;
  `undo_keeps_a_rating_a_player_set_over_link_and_the_rest_of_the_history`
  in `link.rs`, `redo_refuses_a_rating_changed_by_another_program_and_keeps_the_rest`
  in `tests/commands.rs`].
  A cue entry (#504) is checked in `check_history` through
  `Writer::cue_revision_conflict`, and again in the transaction that
  writes: every row it names must hold what the entry left there, and a hot
  cue pad (or the Active Loop) it would fill must not be held by another
  cue now ("Can't undo Delete Cue: Hot Cue B has been set since. The newer
  change was kept.") [OBS code; cue tests below].
  A creation (#500) is checked through `Writer::creation_conflict`, and
  again in the transaction that writes: Undo only tombstones a row that is
  still as it was made (same parent, name, rule, no artwork) and holds no
  tracks and no playlists, so tracks or lists put in it since are never
  deleted with it ("Can't undo Create Playlist: tracks have been added to
  the playlist since. The newer change was kept."). Redo is refused when
  the row is live again or the folder it was made in is gone. A rule or
  artwork entry compares the column by SQL `IS`, as a track field does
  [OBS code; creation tests below].
- **Other histories.** Text fields keep WebKit's own history. Each track's
  beat grid has its own stack in `GridEditor` (`src-tauri/src/grid.rs`),
  which Edit › Undo reaches while a deck's grid editor is armed
  (`listenEditHistory`, `src/lib/editHistory.ts`) [OBS code].

### Cue and loop Undo (#504)

A cue edit made on a deck or in the cue list is undone with Edit › Undo
(Cmd+Z on macOS, Ctrl+Z elsewhere) and redone with Edit › Redo
(Shift+Cmd+Z, Ctrl+Shift+Z), when no text field has focus and no deck's
grid editor is armed (an armed grid editor owns Undo for its grid). The menu names the edit: Undo Set Hot Cue, Undo
Move Cue, Undo Delete Cue, Undo Convert Memory Cues to Hot Cues, and so on.

- An entry holds every `djmdCue` column a cue edit can change, for exactly
  the rows that command changed, read before and after it under the edit
  gate (`CueRevision` in `crates/rbl-db/src/write.rs`). Other cues of the
  track are not named and are left as they are.
- Undo of an add soft-deletes the same row; Undo of a delete brings the same
  row (same id) back. Restored rows get a new `rb_local_usn`, as any edit.
- The decks and the browser row follow through `cues:changed` for the track,
  as after the edit itself.

### Playlist creation, rule and artwork Undo (#500)

- **Create New Playlist / Folder / Intelligent Playlist** (tree menu, menu
  bar, AppleScript `make playlist`) are one entry each, labelled Create
  Playlist, Create Folder and Create Intelligent Playlist. The entry keeps
  the new row's id and its parent's children as `(ID, Seq)` before and
  after. Undo soft-deletes the same row and, when the parent holds exactly
  the children it had, gives each the `Seq` it had (a playlist made at a
  place renumbers the run). Redo brings the same row back with its rule, in
  its place. AppleScript `make` at an index is one entry, not a create and a
  move.
- **Edit Intelligent Playlist** (saving the rule) is one entry holding the
  `SmartList` text before and after; Undo puts back the exact text. A
  rename saved from the same dialog stays its own Rename Playlist entry.
- **Add Artwork** on a playlist or folder is one entry holding `ImagePath`
  before and after. Undo puts the previous reference back; the copied image
  file stays, as track artwork Undo leaves it.

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
| BPM in the info panel | `set_track_field` `bpm` → `grid::set_tempo` | `.DAT/.EXT` grid, `BPM` | One "Set Tempo" entry in the track's grid history, as the grid editor's own; earlier grid entries stay; ends the library redo branch | `a_typed_bpm_is_one_grid_undo_step_and_keeps_the_earlier_ones` (`grid.rs` unit) | No (#505) |
| My Tags | `details::set_my_tags` | `djmdSongMyTag` | Undo | code (`LibraryEdit::TrackTags`) [ASSUME: no command-level test] | No |
| Add / Delete Artwork (tracks) | `details::add_artwork`, `clear_artwork` | `ImagePath`; image copied into the share | Undo (path only; the copied file stays) | code [ASSUME: no command-level Undo test] | No |
| Reset DJ Play Count | `reset_play_count` | `DJPlayCount` | Undo | code [ASSUME: no command-level Undo test] | No |
| Reload Tag | `reload_tags` | Title, Comment, Artist, Album, Genre, Label, Key, Year, Track No. from the file | Not recorded | code | Yes: manual edits lost. #503 |
| Remove from Collection, Delete key | `remove_from_collection` | soft-deletes tracks and their memberships | Permanent: clears history, "This can't be undone" | `removing_from_collection_is_permanent_and_clears_history` | Yes, by design |
| Missing File Manager › Delete | `relocate::remove_missing_tracks` | same | Permanent | code | Yes, by design |
| Relocate (one track) | `relocate_track` | `FolderPath`, `FileNameL`, `rb_file_id` | Not recorded | code | Old path lost. #502 |
| Relocate by location, Auto Relocate | `relocate::relocate_by_location`, `auto_relocate` | same, many tracks | Not recorded; ends the redo branch | code | Old paths lost. #502 |
| Analyze (BPM, key, waveform, grid) | `analysis::analyse_track` | `.DAT/.EXT/.2EX`, BPM, key, analysis path | Not recorded [ASSUME: matches rekordbox, which has no analysis Undo]; ends the redo branch | code | Re-analysis replaces the old grid |
| Phrase edit | `analysis::edit_phrase` | `.EXT` phrase section | Not recorded | code. [UNKNOWN] whether rekordbox's phrase editing undoes; `editapp::ScoreEditManager::undo` exists but is not traced to it | Previous phrases lost |
| Beat grid edits, grid lock | `grid::grid_edit`, `grid_lock` | `.DAT/.EXT`, `BPM`, lock | Grid edits: own per-track Undo, which refuses (`ErrorKind::Conflict`) and drops the top entry when the grid on disk is no longer the one the entry expects, keeping the newer grid; lock: not recorded. Both end the library redo branch | `useGridEditor.test.tsx`, `e2e/grid-edit.spec.ts`, `an_undo_over_a_grid_changed_elsewhere_is_refused_and_keeps_the_newer_grid` (`grid.rs` unit) | No |
| Cue and loop edits: Set Memory Cue / Hot Cue / Loop, move, move to another pad, colour, comment, Active Loop, delete | `cues::add_cue`, `add_loop`, `move_cue`, `move_hot_cue`, `set_cue_colour`, `set_cue_comment`, `set_active_loop`, `delete_cue` | `djmdCue` rows of one track | Undo, one entry per command; refused if the cue changed since | `each_cue_and_loop_edit_is_one_undo_that_restores_the_exact_rows`, `undo_of_a_cue_changed_since_is_refused_and_keeps_the_newer_change`, `undo_of_a_delete_is_refused_when_the_pad_was_set_again`, `redo_of_a_cue_is_refused_when_the_pad_was_set_since` (`src-tauri/tests/commands.rs`) | No while the entry lives |
| Convert Memory Cues to Hot Cues | `cues::convert_memory_cues_to_hot` | new `djmdCue` rows | Undo, one entry for the whole track | `a_conversion_is_one_undo_for_the_whole_track` | No |

### Playlists, folders and intelligent playlists

| Action | Entry point | Writes | Status | Evidence | Destructive |
| --- | --- | --- | --- | --- | --- |
| Rename | `rename_playlist` | `Name` | Undo | `library_history_names_and_reverses_each_supported_edit` | No |
| Move (drag in the tree) | `move_playlist` | `ParentID`, sibling `Seq` | Undo | same | No |
| Delete playlist or folder (with contents) | `delete_playlist` | tombstones subtree and memberships | Undo | `a_deleted_playlist_tree_can_be_undone_and_redone` | No while the entry lives |
| Remove Tracks from Playlist | `remove_tracks_from_playlist` | tombstones memberships, renumbers | Undo | `library_history_names_and_reverses_each_supported_edit` | No |
| Add Tracks to Playlist (menu, drop, duplicates "Add", AppleScript `add`) | `add_tracks_to_playlist` | new memberships | Undo, one entry per drop or menu choice: the window's slices of 100 share a `transaction` and join the entry while each carries on from the last. Undo soft-deletes exactly the rows the add made (an earlier copy of a duplicate stays); Redo brings the same rows back at their numbers. Tracks imported on the way in stay in the collection. Refused if the playlist's tracks changed since | `an_add_sent_in_slices_is_one_undo_and_one_redo`, `undo_of_an_add_with_duplicates_leaves_the_tracks_already_there`, `undo_of_an_add_or_reorder_refuses_a_playlist_changed_since` (`src-tauri/tests/commands.rs`); `an_add_undoes_and_redoes_the_exact_rows_it_made` and the other #499 tests in `crates/rbl-db/tests/writes.rs` | No while the entry lives (#499) |
| Reorder, Renumber Track Order | `reorder_playlist` | `TrackNo` of every member | Undo, one "Reorder Playlist" entry: every member's `TrackNo` before and after. Refused if the playlist's tracks or numbers changed since | `a_reorder_is_one_undo_and_one_redo`, `undo_of_an_add_or_reorder_refuses_a_playlist_changed_since`; `a_reorder_undoes_and_redoes_every_number` (`writes.rs`) | No (#499) |
| Create Playlist / Folder / Intelligent Playlist (also AppleScript `make playlist`) | `create_playlist`, `create_folder`, `create_smart_playlist` | new `djmdPlaylist` row, sibling `Seq` | Undo, one entry each; refused once tracks or playlists were put in it, or it was renamed, moved or deleted outside the history | `each_creation_is_one_undo_and_one_redo_that_restore_the_exact_tree`, `undo_of_a_creation_keeps_the_tracks_and_playlists_added_to_it_since`, `undo_and_redo_of_a_creation_refuse_a_playlist_changed_since` (`src-tauri/tests/commands.rs`) | No (#500) |
| Edit Intelligent Playlist rule | `set_smart_rule` | `SmartList` | Undo, the exact previous rule text; refused if the rule changed since | `a_rule_edit_and_playlist_artwork_undo_to_the_exact_previous_values` | No while the entry lives (#500) |
| Add Artwork (playlist or folder) | `details::add_playlist_artwork` | `ImagePath`; image copied into the share | Undo (reference only; the copied file stays) | same | No while the entry lives (#500) |
| Tag List: add | `add_to_tag_list` | Tag List rows | Not recorded | code | No |
| Tag List: remove, clear | `remove_from_tag_list`, `clear_tag_list` | soft-deletes Tag List rows | Not recorded; asks first | code | Yes. #508 |
| Remove from History | `remove_from_history` | soft-deletes `djmdSongHistory` rows | Not recorded; asks first | code | Yes. #508 |
| A play recorded by the deck | `record_play` | history row, `DJPlayCount` | Not a user edit; not recorded | code | No |

### Imports

| Action | Entry point | Writes | Status | Evidence | Destructive |
| --- | --- | --- | --- | --- | --- |
| Import files or folders, drop onto the collection | `import_files`, `import_files_slice` | new tracks | Not recorded; ends the redo branch. Reversing it is Remove from Collection, which is Permanent | `an_import_ends_the_redo_branch_and_keeps_undo` | No |
| Folder drop onto the tree, new or "Import as copy" | `import_folder_playlist` | new tracks, playlist, memberships | Not recorded; ends the redo branch | code (`import_folder_playlist`) | No |
| Folder drop, "Replace existing" | same, `replace` | tombstones the replaced subtree, then imports | Undo: one replacement entry restores the old subtree and memberships | `a_folder_drop_replacement_is_one_undo_and_one_redo` | No while the entry lives |
| Import XML / iTunes | `import_xml`, `import_itunes`, `import_itunes_selected` | tracks, rating, comment, cues, playlists | Not recorded; ends the redo branch | `an_xml_import_that_would_replace_lists_writes_nothing_until_told_to` | No |
| Import XML / iTunes, replace same-named lists | same, `replace` | rewrites members of existing lists (`set_tracks_with_undo`) | Undo, one "Replace Playlist Contents" entry per import: restores every replaced playlist's own membership rows and order; Redo the file's. The tracks and lists the import added stay. Refused if a replaced list's members changed since (#506) | `an_xml_import_that_replaces_lists_is_one_undo_and_one_redo`, `an_itunes_import_that_replaces_a_list_is_one_undo_and_one_redo`, `undo_of_an_xml_replacement_changed_since_is_refused_and_keeps_the_newer_change` (`src-tauri/tests/commands.rs`); `a_set_of_playlists_tracks_undoes_and_redoes_with_the_same_rows` (`rbl-db` `writes.rs`) | No while the entry lives (#501) |
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
| Edits from players over LINK (rating, Tag List, cues, history) | `link.rs` `StateSource::edit`, `history_write`, `player_cue_edit` | the library | Not recorded; ends the redo branch, and an Undo that would overwrite one is refused | `undo_keeps_a_rating_a_player_set_over_link_and_the_rest_of_the_history` (#506) |
| Preferences | `preferences.rs`, `localStorage` | settings | Out of scope: not library data | — |

## Policy choices

The audit found two new product decisions:

1. What Undo/Redo should do when a recorded row was changed since by
   something outside the history (LINK, USB import, rekordbox). Decided on
   2026-10-22: refuse that entry with a message, keep the newer value and the
   rest of the history (#506; see "Conflict check" above).
2. Whether Undo of a key edit that also wrote the file tag should rewrite the
   tag, only say it did not, or not be offered (#507). Still open, filed with
   `needs-product-decision`.

The other gaps (#499, #501 to #505, #508) extend the existing contract and
need no new policy.
