# Library location

How rbxport decides which rekordbox library to open, how it switches to
another one, and what it does when the library is on a drive that is not
connected. It does each the way rekordbox does, with rekordbox's screens and
no others. The code is `crates/rbl-db/src/locate.rs`; drive discovery is
`crates/rbl-devices/src/libraries.rs`.

## What rekordbox does

Evidence is static analysis of `/Applications/rekordbox 7` 7.2.11 on macOS
(symbols present, disassembled with `lldb`), its locale files, and rekordbox
7.2.x on Windows observed on chris-win11.

- [OBS] rekordbox's record of its library is `masterDbDirectory` in
  `rekordbox3.settings` (`~/Library/Application Support/Pioneer/rekordbox6/`
  on macOS), for example
  `<VALUE name="masterDbDirectory" val="/Users/x/Library/Pioneer/rekordbox"/>`.
  It is read by `SettingIF::getCurrentMasterDbDirectory` and written by
  `SettingIF::setMasterDbDirectory`. The default folder is
  `~/Library/Pioneer/rekordbox`; `master.db` and `share/` live in the folder.
- [OBS] `MainAppWindow::judgeIfExistDatabase`, at launch: a value that is not
  an absolute path is reset to the default (`File::isAbsolutePath`, then
  `setMasterDbDirectory(ConfigPath())`). When the folder is not the default
  and its database is missing, it shows `MasterDbMissingWindow`, a window
  with no title, the text "Cannot find Master Database. / Launch rekordbox
  after connecting a drive where Master Database is stored. / Do you want to
  open Master Database in the default drive?" and the buttons Yes and No.
  - Yes asks "Location of Master Database will be changed to the default
    drive. / The location can be changed at [Advanced] tab of [Preferences]
    window." with OK and Cancel. OK calls
    `setMasterDbDirectory(ConfigPath())` and the launch carries on with the
    default folder.
  - No returns false from `judgeIfExistDatabase`, ending the launch
    [ASSUME: the caller quits, as the text "Launch rekordbox after
    connecting…" says; the caller was not traced].
  - It never makes a library on the missing drive. (The window also offers
    "Open backup file" when the drive's `ExtDriveBackup` folder holds a
    backup; rbxport has no such backup, so that button is not built.)
- [OBS] rekordbox has no other library picker. Preferences › Advanced ›
  Database, last on the page under "The settings below work when tracks in
  an external drive are used", has **Database management**: a "?" help
  icon, the heading "Select a drive", a drive list and **Move Database**.
  - The list (`DetailDatabaseManagement::setup`) is the writable drives that
    hold a library (`DatabaseIF::getWritableDriveList`, `hasMasterDB`): a
    drive's library is `<drive>/PIONEER/Master/master.db`, or
    `.PIONEER/Master` on HFS (`getMasterDbDirectoryPath`), and `/` stands for
    the default folder. Each is shown by its volume label
    (`File::getVolumeLabel`). On Windows the label is run into the drive
    letter: the only entry on chris-win11 read `C:BOOTCAMP`, greyed out
    because it was the only one [OBS chris-win11 2026-10-08, screenshot].
  - Choosing another drive (`comboBoxChanged`, `selectDrive`) asks "Are you
    sure you want to switch Master Database? / This operation may require
    long time." with OK and Cancel; OK stops playback and calls
    `DatabaseIF::selectLibrary`. Failure says "Failed to switch Master
    Database."
- [OBS] `rekordboxAgent/storage/options.json` is written by rekordbox for its
  agent. `CloudAgentAPI::Agent::start` deletes and rewrites it from
  `masterDbDirectory` on every launch, and rekordbox never reads `db-path`
  back: in the x86_64 slice the `"db-path"` string has one code reference,
  where it is quoted into the JSON that `FileOutputStream::writeText` writes.
- [OBS] Windows keeps the same `masterDbDirectory` in
  `%APPDATA%\Pioneer\rekordbox6\rekordbox3.settings`, written with forward
  slashes: `C:/Users/chris/AppData/Roaming/Pioneer/rekordbox` on chris-win11
  (file read only, 2026-10-08). rbxport writes Windows folders the same way.
  [UNKNOWN] The Windows binary itself was not analysed.

## What rbxport does

### Which library opens

1. `RBXPORT_OPTIONS`, when set, is the only source: a test harness pointing
   the app at a fixture. Nothing is ever written under it.
2. `masterDbDirectory`, when it is an absolute path.
3. Else `options.json`'s `db-path`, read only. An rbxport release before
   this one recorded a library it made, on a machine without rekordbox,
   there; rekordbox itself rewrites the file from `masterDbDirectory`.
4. Else the default folder.

When that `master.db` is missing and is not the default folder's, the window
asks rekordbox's "Cannot find Master Database" question, word for word with
"RBXport" for "rekordbox", Yes or No; Yes asks rekordbox's OK/Cancel
confirmation and then sets `masterDbDirectory` to the default folder. No
quits. Nothing is made on the missing drive. When the default folder has no
library either, the existing question to create one, or quit, follows.

### Switching

Preferences › Advanced › Database ends with **Database management**,
"Select a drive", listing the default drive when it holds a library and
every connected drive holding `PIONEER/Master/master.db` or
`.PIONEER/Master/master.db`, by volume label (`C:LABEL` on Windows). It is
greyed out with one entry, and while rekordbox runs. Choosing a drive asks
rekordbox's "Are you sure you want to switch Master Database?" with OK and
Cancel. OK opens the chosen `master.db` read-only to check its key and
schema, sets `masterDbDirectory` to its folder, and starts rbxport again on
it. A file that is not a library is refused with "Failed to switch Master
Database." and nothing is written. The "?" help and Move Database are not
built.

### Writing `rekordbox3.settings`

Switching writes the one setting rekordbox itself reads, so rekordbox and
rbxport open the same library afterwards, as after a switch made in
rekordbox. Only the `masterDbDirectory` entry changes; every other byte of
the file is kept, the value is escaped as JUCE writes it (`&#8217;` for a
non-ASCII character), and the file is written whole and renamed into
place. A missing entry is added before `</PROPERTIES>`; a file without
`</PROPERTIES>` is not touched. On a machine without rekordbox (the
issue #49 reporter's Linux machine) the file is created with only that
entry, in rbxport's equivalent of rekordbox's settings folder
(`~/.config/Pioneer/rekordbox6/` on Linux).

It is never written while rekordbox runs (rekordbox writes its settings back
when it quits), never under `RB_LITE_TEST`/test mode, and never under
`RBXPORT_OPTIONS`; tests use a temporary machine
(`locate::Sources::under`). The library itself is only opened read-only to
switch to it.

A library is opened with the passphrase from `options.json`'s `dp` when that
file exists, else rekordbox's standard `dp`. The value was the same on every
install observed (macOS 7.2.11, Windows 7.2.14) [OBS 2026-09-24].

## Drive discovery

rbxport looks for `PIONEER/Master/master.db` and `.PIONEER/Master/master.db`
on every mounted volume: `/Volumes/*` on macOS, drive letters on Windows,
and on Linux the mounted disks plus the folders in `/media`, `/media/$USER`,
`/run/media/$USER` and `/mnt`. Both folder names are checked on every
filesystem, so a drive moved between machines is still found. Discovery only
checks that the files exist; nothing is opened until a drive is chosen.
