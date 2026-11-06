# Using RBXport on Linux

[Documentation](../README.md) · [FAQ](faq.md) · [Library location](../reference/library-location.md) · [USB export](usb-export.md)

You do not need rekordbox, a Windows or macOS virtual machine, or a copied
`master.db` to use RBXport on Linux. RBXport can make its own, empty library
the first time it starts, and export it to a USB stick.

Evidence markers follow the repository convention: **[OBS]** is something
read in the code or docs named beside it (or observed, where said),
**[ASSUME]** is an inference, and **[UNKNOWN]** was not checked. Nothing here
has been tried with a physical stick or player on Linux.

## First start: no library yet

1. [OBS `crates/rbl-db/src/lib.rs` `default_library_dir`] On Linux, as on
   macOS, the default library folder is `~/Library/Pioneer/rekordbox` (so the
   database is `~/Library/Pioneer/rekordbox/master.db`). It is not directly in
   your home folder, and it is not under `~/.config`.
2. [OBS `crates/rbl-db/src/locate.rs`] If nothing names another library and
   that `master.db` does not exist, the start-up check reports "no library
   here, one may be made". It does not look for rekordbox.
3. [OBS `src/views/library/NewLibraryDialog.tsx`] The window then shows **No
   rekordbox Library** with the text "rekordbox isn't installed and there is no
   rekordbox database. Would you like to create a new database?", the path it
   would use, and the buttons **Create** and **Quit**. This is the expected
   first-run question on a machine without rekordbox, not an error.
4. [OBS `crates/rbl-db/src/new_library.rs`] **Create** makes the folders
   `share/PIONEER/Artwork` and `share/PIONEER/USBANLZ` and an empty
   `master.db` in that folder. The database is built beside its final name
   and moved into place only when complete, and an existing `master.db` is
   never replaced.
5. [OBS `src-tauri/src/lib.rs` `spawn_library_load`] The library is loaded on a
   background thread, so the window is not blocked while it is read.

If the window does not appear at all, or the whole machine becomes
unresponsive, that is not the flow above. See
[issue #848](https://github.com/chrisle/rbxport/issues/848); the cause of that
report is [UNKNOWN] (see the PR notes for what was checked).

## Copying `master.db` from another computer is optional

[OBS] The flow above needs no file from rekordbox. If you already have a
rekordbox library on Windows or macOS you may choose to use it, but this is
never required to export a stick, and how best to move one to Linux has not
been tested [UNKNOWN]. RBXport opens the library named by `masterDbDirectory`
in `rekordbox3.settings`, else the default folder
([Library location](../reference/library-location.md)). Do not copy a library
that rekordbox is still using; RBXport refuses to change a library while
rekordbox is running.

## Import music and make playlists

- [OBS `src-tauri/src/menu.rs`] **Import** (Ctrl+O) adds audio files and
  **Import Folder…** (Ctrl+Shift+O) adds every audio file under a folder.
- [OBS `crates/rbl-db/src/import.rs`] The extensions imported are mp3, m4a,
  aac, flac, wav, aiff, aif, ogg and opus.
- [OBS `src/lib/contextMenus.ts`] Right-click the collection to **Create New
  Playlist**, **Create New Intelligent Playlist** or **Create New Folder**.
  Tracks are added to a playlist with the Add To Playlist command
  ([OBS `src-tauri/src/lib.rs` registers `add_tracks_to_playlist`]).
- Whether Auto Analysis runs on imported tracks follows the Preferences
  setting ([OBS `src/app/App.tsx` `runImport`]).

## Export to a rekordbox-compatible USB stick

1. Format the stick yourself. [OBS `crates/rbl-devices/src/format.rs`]
   RBXport's own formatter exists only on macOS; on Linux the call reports
   that formatting is not supported. FAT32 is the format its code describes as
   readable on macOS, Windows, Linux and CDJs; exFAT and HFS+ are also handled
   by the export planner ([OBS `crates/rbl-export/src/plan.rs`,
   `docs/reference/usb-export-pipeline.md`]).
2. Mount it. [OBS `crates/rbl-devices/src/lib.rs`] A volume is offered as an
   export target when it is reported removable or is mounted under `/media/`,
   `/run/media/` or `/mnt/`. Desktop auto-mounting normally uses the first
   two [ASSUME]. A stick mounted elsewhere, such as under your home folder,
   is not offered unless it reports itself as removable.
3. Right-click a playlist or folder, choose **Export Playlist** or **Export
   Folder**, then the stick; or use **Sync Manager** to make the stick hold
   exactly the ticked playlists ([OBS `docs/user/usb-export.md`]).
4. [OBS `crates/rbl-export/src/lib.rs`] Your rekordbox My Settings files
   (`MYSETTING.DAT`, etc.) are copied to the stick only when rekordbox's
   settings folder exists on the machine; without rekordbox they are simply
   not copied, and the export goes ahead.
5. [OBS `docs/reference/usb-export-db.md` "Root selection"] A blank stick
   gets the `PIONEER` folder (`.PIONEER` on a blank HFS+ volume). The
   preference **Setup PIONEER folder on USB drives** (Preferences → USB
   Export → Sync) controls creating the folders on a new drive.
6. Eject with the app's eject action, which uses `udisksctl unmount`
   ([OBS `crates/rbl-devices/src/eject.rs`]), and do not unplug while saving.

## Limits

- [OBS `docs/reference/usb-export-pipeline.md`, "Not yet shown"] No physical
  player has loaded a stick written by this pipeline, and Linux physical
  sticks are unverified. Emulator results are not physical-device results.
- [OBS] RBXport does not format sticks on Linux.
- [UNKNOWN] Behaviour on specific distributions (including Fedora), kernel
  filesystem drivers for exFAT or HFS+, and auto-mount locations other than
  those listed has not been tested.
- RBXport is not performance software and does not register controllers
  ([FAQ](faq.md)).
