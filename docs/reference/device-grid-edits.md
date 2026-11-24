# Device grid edits

The GRID panel can edit a track loaded from a Devices database. It sends the
loaded row's device path, database format and track ID with grid reads, edits,
undo and redo. The backend resolves that row again and checks that it still
names the loaded audio. Browsing another database containing the same audio
does not select a new grid target. An unbound device edit is refused.

The source and its analysis path also identify its session history. Switching
sources cancels queued edits, clears the old panel state until the new read
answers, and keeps another source's undo entries separate. Analysis Lock
remains a collection operation and is refused for device tracks.

## Write boundary

The application refuses device writes while rekordbox is running. A shared
USB lease covers the analysis snapshot and its backup; for an explicit edit
it creates a missing lock on a stock device. Read-only queries remain
observational. Linked or non-file lock paths refuse before recovery or lock
creation. The publication writer checks the original bytes under its exclusive lease. A concurrent export
that wins the handoff makes the edit refuse instead of overwriting its grid.
Both DAT and any present EXT are validated as plain on-device files before
reading or backing up either, including an EXT that needs no rewrite.
The browsed local track ID must be unique before any analysis backup; every
selected paired ID must also identify one live row. The current row must still name the same audio and analysis files. Relative
paths, plain files, and the permitted DAT/EXT companions are checked; symlinks,
wrong-track files, missing files and malformed replacement analysis are refused.

DAT/EXT originals are backed up in application data before replacement, with a
content digest in the backup name. The analysis and changed database rows use
one existing export-journal publication. A staged failure leaves published
files intact, and history is recorded only after publication succeeds.

A tempo edit patches the matching PDB row's four-byte tempo field and the
OneLibrary row's `bpmx100`. Other content columns and counters are preserved.
The paired row must be unique and name the same audio. Mismatched or ambiguous
origins refuse the entire write. It is selected by its master database/content IDs; the existing
fallback for two zero IDs uses its analysis path. That fallback is an
assumption. Additional duplicate-match criteria used by another writer are
not established here, so this is not a claim of complete writer parity.

After success the deck/panel receive `grid:changed`; a changed tempo also
refreshes device views through `devices:changed`. There is no polling loop.
Shared leases coordinate rbxport operations. They do not prevent an external
application from ignoring those locks; the running-app refusal, database
stamp checks and final analysis-byte check provide additional conflict checks.

## Coverage and limits

| Boundary | Fixture coverage |
| --- | --- |
| Exact loaded source | Native commands browse two databases sharing audio but naming different analysis, then edit/undo/redo only the loaded source; forged and unbound sources refuse. |
| Publication handoff | An analysis change after planning refuses with both databases and the newer analysis intact. |
| Write intent | A missing lock is created, a publisher waits through the planning lease, and a dangling lock link creates no outside file. |
| File association | Wrong-track, missing, outside-root and symlink-backed analysis refuse. Native linked-EXT fixtures preserve outside sentinels, create no backups, and change no grid/database/history. |
| Database writes | Both database formats, paired master IDs, unchanged non-tempo columns/counters, unchanged audio, and a paired-database failure. |
| Backup/history | Same-name media and media swapped at one mount retain their original bytes; device histories stay separate. |
| Recovery mechanism | Existing export publication/recovery suites; no new real-media crash test. |
| UI | Typed source forwarding, queued-source switches, device-view refresh, and mock-backed Chromium/WebKit grid controls. |

All writable validation uses disposable fixtures. Native fixture execution,
browser mocks and static code checks do not establish physical USB behavior,
Windows UI appearance, or stock application's database changes. Those remain
engineering evidence gaps; no personal library or real USB is needed for the
fixture suites.
