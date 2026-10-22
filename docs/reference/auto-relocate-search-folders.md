# Auto Relocate Search Folders

[Documentation](../README.md)

Research record for #487 (settings to add Auto Relocate search folders). It
compares rekordbox's "Auto Relocate Search Folders" preferences with what
rbxport has. It is research only: Auto Relocate stays hidden and its settings
are not exposed, because the product decision on #415 is still open.

Tags: `[OBS]` observed or read in evidence, `[ASSUME]` inferred,
`[UNKNOWN]` not settled. The Windows rig (chris-win11) was off-limits for this
ticket, so nothing here was observed live by this work.

## What rekordbox does

Source: `../alphatheta-docs/platform/rekordbox-auto-relocate.md`, sections
"Preferences" and "Search engine" (static analysis of rekordbox 7.2.19 macOS,
live view-only observation of 7.2.14 Windows).

- [OBS] Location: Preferences > Advanced > Database, section "Auto Relocate
  Search Folders" (`DetailAutoRelocate`, ctor `0x1006b4894`).
- [OBS] Controls: toggles Music, Movies (Windows 7.2.14 shows "Video"),
  Desktop, "Specified user folders"; a non-editable combo box of folders with
  Add and Del buttons. With "Specified user folders" off the combo, Add and Del
  are disabled.
- [OBS] Add opens a native folder chooser; the folder is inserted at the top of
  the list (case-sensitive de-dupe), saved and selected. Del removes the
  shown folder with no confirmation.
- [OBS] Storage in `rekordbox3.settings`: `AutoRelocateSelectMusicFolder`,
  `...MovieFolder`, `...DesktopFolder` (default true), `AutoRelocateSelectUserFolder`
  (default false), `AutoRelocateSearchPaths` (`;`-joined string).
- [OBS] Search order for Auto Relocate (mode 2): file-identifier lookup
  first; otherwise folders in order: folder of the last track relocated in the
  run, Specified user folders that exist, Music, Movies, Desktop. Each is
  walked recursively (hidden files included) for a file whose
  name equals `FileNameL` exactly; the first match ends the search. Folders
  are not de-duplicated.
- [ASSUME] The walk follows symbolic links: the source marks this as a
  hypothesis (JUCE enum value 2 read as "yes"), from static analysis only.
- [OBS] Dependencies: Library Protection blocks the action; a match that is
  already in the collection is listed under "Track exists in the collection"
  instead of relocated; `relocateTrack` rewrites the `djmdContent` row and the
  analysis-file audio path.

## What rbxport has

[OBS] from reading the code at the branch base, not run:

| rekordbox | rbxport | Where |
|---|---|---|
| Four toggles, defaults true/true/true/false | Present in prefs model, same defaults | `src/lib/preferences.ts` (`relocateMusic`, `relocateVideo`, `relocateDesktop`, `relocateUserFolders`) |
| Folder list | `relocateFolders: string[]` | same |
| Preferences section with Add/Del | Not shown in Preferences (by design for now) | `src/app/App.tsx` comment at `relocateSearch` |
| Search order and exact-name recursive walk | Implemented | `src-tauri/src/relocate.rs` (`search_roots`, `Walks`, `plan`) |
| Hidden files, first match wins; links followed is [ASSUME] for rekordbox | Implemented, tested | `relocate.rs` tests |
| Duplicate-in-collection list | Implemented | `Plan::in_collection` |
| File-ID lookup | macOS only (APFS/HFS) | `relocate.rs` `by_file_id` |

So the settings model and the search already follow rekordbox. The only missing
piece for #487 is the Preferences UI (Add/Del list and toggles), which is the
part that is not to be exposed yet.

## Known gaps in the dependencies

Recorded on #415 (2026-10-22 check of dev), tracked under #454, not changed here:

- Non-macOS file-ID lookup returns none. [OBS] Windows rekordbox does have the
  step (`../alphatheta-docs/platform/rekordbox-auto-relocate.md`, "Windows"):
  `OpenFileById` and `GetFileInformationByHandleEx` in the function at
  `0x141d1e720`, called from the counterpart of the macOS lookup at
  `0x1419385a0`. The `find -inum` bool has no effect there. So this is a real
  parity gap on Windows. The ID only means something on the machine and volume
  that wrote it, so a library moved between macOS and Windows gets no help from
  it in either app.
- rbxport caps the walk at 2,000,000 directory entries; rekordbox is uncapped
  and cancellable.
- Relocation writes do not yet cover `rb_data_status`, `ContentLink` and the
  analysis-file path rewrite.

## Unknowns

- [UNKNOWN] Windows 7.2.14 preference key names: the executable has no
  symbols; only the key names and component name were matched. A view-only Winrig check of the Add/Del behavior would settle it.
- [UNKNOWN] Whether the folder list persists `;`-containing paths correctly in
  rekordbox (hypothesis in the source doc); rbxport stores an array so the
  question does not arise.
- [UNKNOWN] Whether the section should be shown at all: waits on #415.

## If #415 decides to restore the UI

Add only what rekordbox has: the section under Advanced > Database with the
four toggles, the folder combo box, Add (native folder chooser titled "Choose a
new fullpath for", insert at top, no duplicates) and Del (no confirmation), and
disable the combo, Add and Del while "Specified user folders" is off. The
storage and search sides need no new work.
