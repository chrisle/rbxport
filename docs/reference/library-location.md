# Library location

How rbxport decides which rekordbox library to open, and what it does when
that library is on a drive that is not connected. The code is
`crates/rbl-db/src/locate.rs`; drive discovery is
`crates/rbl-devices/src/libraries.rs`.

## What rekordbox does

Evidence is static analysis of `/Applications/rekordbox 7` 7.2.11 on macOS,
with symbols present.

- [OBS] rekordbox's record of its library is `masterDbDirectory` in
  `~/Library/Application Support/Pioneer/rekordbox6/rekordbox3.settings`, for
  example `<VALUE name="masterDbDirectory" val="/Users/x/Library/Pioneer/rekordbox"/>`.
  It is read by `SettingIF::getCurrentMasterDbDirectory` and
  `DatabaseMediator::getLibraryFolderPath`. When it is unset the folder is
  `~/Library/Pioneer/rekordbox`. The database is `<folder>/master.db` and
  the analysis root is `<folder>/share`.
- [OBS] `rekordboxAgent/storage/options.json` is written by rekordbox for its
  agent. `CloudAgentAPI::Agent::start` deletes and rewrites it from
  `masterDbDirectory` on every launch, and rekordbox never reads `db-path`
  back to choose a library. Writing it does not move rekordbox's library.
- [OBS] rekordbox has no library picker at startup. Preferences > Advanced >
  Database management has a Drive list that offers only drives already
  holding `<volume>/PIONEER/Master/master.db`, or
  `<volume>/.PIONEER/Master/master.db` on HFS volumes
  (`getMasterDbDirectoryPath`; `getDriveFileSystemType` returns 4 for a
  filesystem label starting `HFS`), plus `/` for the default. Choosing one
  switches the library after "Are you sure you want to switch Master
  Database?". Move Database copies the library to another drive.
- [OBS] When `masterDbDirectory` is not the default and its files are missing
  at launch (`MainAppWindow::judgeIfExistDatabase`), rekordbox says it cannot
  find the Master Database, asks for the drive to be connected, and offers to
  open the Master Database on the default drive. It never makes a library on
  the missing drive.
- [UNKNOWN] The Windows binary was not analysed. [ASSUME] Windows keeps the
  same `masterDbDirectory` in `%APPDATA%\Pioneer\rekordbox6\rekordbox3.settings`,
  with drive letters. rbxport already reads `DropboxSharingPath` from that
  file through the same path. A missing value falls through to the next
  source rather than failing.

## The order rbxport uses

1. `RBXPORT_OPTIONS`, when set, is the only source. It points a test harness
   at a fixture library.
2. rekordbox's library: `masterDbDirectory`, else `options.json`'s `db-path`
   when the settings file says nothing, else the default folder. It is used
   whenever its `master.db` exists, because rbxport must work on the library
   rekordbox uses. The default folder counts as rekordbox's only when
   rekordbox has run on the machine, which leaves `rekordbox3.settings` or
   `options.json` behind (the agent rewrites `options.json` at every launch).
   Without either, a library there was made by rbxport and is handled in
   step 4.
3. The library chosen in rbxport, saved as `library.json` in rbxport's data
   folder (`~/Library/Application Support/rbxport/` on macOS,
   `%APPDATA%\rbxport\` on Windows, `~/.local/share/rbxport/` on Linux). It
   is used when it exists and rekordbox's library does not, for example on a
   machine without rekordbox, or when rekordbox's drive is not connected.
4. On a machine where rekordbox has not run, a library in the default folder,
   unless a saved choice names a library elsewhere.
5. Otherwise nothing is opened and the window asks:
   - A configured library that is missing is reported as unavailable. The
     window asks for the drive to be connected (Try Again), offers the
     default folder's library (opened if there, created if not), libraries
     found on connected drives, or a `master.db` picked by hand. No library
     is ever created at the missing location.
   - With nothing configured, the window offers libraries on connected
     drives, a `master.db` picked by hand, or a new library in the default
     folder.

rbxport never writes `rekordbox3.settings` or `options.json`. Choosing a
library in rbxport does not change rekordbox's choice. A chosen library that
rekordbox's own library would override at the next start is refused with a
message naming rekordbox's library, rather than saved to no effect.

A library is opened with the passphrase from `options.json`'s `dp` when that
file exists, else rekordbox's standard `dp`. The value was the same on every
install observed (macOS 7.2.11, Windows 7.2.14) [OBS 2026-09-24]. A chosen
library is opened read-only, and its key and schema are checked before the
choice is saved.

## Drive discovery

rbxport looks for `PIONEER/Master/master.db` and `.PIONEER/Master/master.db`
on every mounted volume: `/Volumes/*` on macOS, drive letters on Windows,
and on Linux the mounted disks plus the folders in `/media`, `/media/$USER`,
`/run/media/$USER` and `/mnt`. Both folder names are checked on every
filesystem, so a drive moved between machines is still found. Discovery only
checks that the files exist; nothing is opened until a library is chosen.
The startup window refreshes the list when a drive is connected or removed.
