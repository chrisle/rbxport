# XDJ-R1 support in Sync Manager

Plan prepared October 15, 2026. This is proposed work, not an implemented or
hardware-validated feature.

Static-analysis update: the same day's follow-up mapped the principal Cbm
record fields, category serialization, text encodings, exporter bounds and
artwork transformation. The detailed evidence is in alphatheta-docs at
`platform/rekordbox-xdj-r1-cbm-layout.md`. The Cbm writer uses synthetic block
positions, not the MEP FAT-cluster path. The design below reflects that finding.

## Intended result

A user enables the global **XDJ-R1 support** preference (off by default),
selects playlists and a USB drive in Sync Manager, and clicks Sync. The completed drive contains an R1
Cbm DBH library matching the exported audio and playlist selection. Creation
of the exclusive library is part of Sync; no separate conversion step is needed.
Automatic synchronization uses the same saved setting. Eject occurs only after
all requested formats have been published and verified.

Accepted product behavior: persist a global preference that defaults to OFF,
including when older settings have no value. ON adds R1 output alongside
classic Device Library and OneLibrary (also called Device Library Plus),
sharing prepared audio. Capture the preference in each job and record the
applied output profile in its manifest. Detect existing Cbm libraries and
protect pending/foreign edits. Drive identities still need stable volume IDs;
never use a mount path alone.

## Evidence and prerequisites

The source reference is `platform/rekordbox-xdj-r1-dbh.md` in alphatheta-docs.
The current export integration is described in
[How a USB export runs](../reference/usb-export-pipeline.md).

- [OBS: static analysis of rekordbox 6.8.7] R1 uses six files in
  `PIONEER/DBH/`: `CbmDB.DBH`, `CbmPL.DBH`, `CbmTT.DBH`, `CbmAR.DBH`,
  `CbmAB.DBH`, and `CbmBP.DBH`. These differ from DeviceSQL, OneLibrary,
  and the MEP-4000 format.
- [OBS: static analysis] The common header is `0x9c5c` bytes with a
  `0x2001` marker and 10,000 offset slots. Contents records are `0x9f8`
  bytes; category TOC entries are `0x18` and directory records `0x1a0`.
  These sizes do not establish a complete writer or a device track limit.
- [OBS: follow-up static analysis] Contents metadata, text encodings, identity
  fields, category references, folder flattening, BPM bins, codec admission,
  10,000-track exporter bound, and artwork sibling generation are mapped.
  The contents header offset slots are zero; category slots point to directories.
- [OBS: extended static analysis] Schema-registered collation, album TRACKNO
  ordering, artwork directory allocation, empty/excluded-track branches, and
  native analysis/cue consumption are traced. Firmware 1.13 preserves serialized
  browse order on the examined native paths.
- [OBS: native reader] Content `+0x00c` is an optional cached first filesystem
  sector. Fresh zero invokes path resolution; the writer does not require audio
  files to occupy predetermined physical FAT sectors.
- [UNKNOWN] Some reserved intervals, remote artwork selection, and producer-side
  distinctions among playlist edit flags still need further evidence.
- [UNKNOWN] The reference has no complete runtime-generated fixture and no
  physical R1 acceptance result. Fixture capture is the next major dependency.
- [ASSUME: proposed product behavior] R1 can be an additional output alongside
  current libraries using shared audio. Prove coexistence, paths, and audio
  compatibility before enabling that behavior publicly.

## Static findings that affect implementation

Use the existing path-based `analysis_directory()` hash in `rbl-export` for R1
analysis placement. Firmware independently derives the same hash and shard,
checks `ANLZ0000.DAT` through `ANLZ0010.DAT`, and matches the stored `PPTH` audio
path. Audit collision allocation and any retained legacy ID-based directories.
The native loader consumes base DAT; preserve its ten memory-cue and three
hot-cue slots and Hot Cue Auto Load behavior.

Enforce the 10,000-slot category bound and retain a following zero sentinel;
the native reader scans until zero without a table-length guard. Reject malformed
offsets before indexing or reading.

The guarded native Cbm timing-load path clamps spans above 449,999 playback
frames to that value. Add long-track reference cases around that boundary and
report the resulting limitation accurately; do not equate this static clamp
with a universally enforced audio-file rejection. Native readers also narrow
serialized audio-offset remainder/BPM fields to 16 bits.

Emit the full 416-byte directory even though native browse paths read only its
400-byte prefix. Preserve unclassified bytes when importing existing R1 edits.
Desktop edit consumption resolves new/delete/rename and a common membership
rewrite for masks `0x08`, `0x10`, and `0x20`; do not invent separate operation
names for those three masks.

Keep the planned journaled publication and recovery. Stock publication deletes
old finals in title, album, artist, BPM, playlist, contents order; its empty path
can remove finals without replacement and a publication failure has no rollback.
Those failure semantics are evidence about the reference exporter, not behavior
to reproduce in Sync Manager.

## 1. Capture a stock reference and close the schema

Use an isolated rekordbox 6.8.7 collection with generated audio and disposable
FAT32 media or an image exposed as USB. Do not use an installed user library.
Capture the complete volume, including filesystem layout, not only DBH files.
Record application/firmware versions, input metadata, export actions, file
tree, hashes, and generation time.

Capture empty and one-track exports, then controlled variants for title,
artist, album, BPM, duration, path, filename, audio properties, artwork,
playlist membership/order, duplicate membership, and folders. Include missing
metadata, Unicode, long strings, repeated exports, deletions, and relocated
or fragmented audio. Use rbl-difftool and independent byte inspection to map
each change. Observe stock limits rather than deriving them from table size.

Confirm the statically recovered path and synthetic-block model with reference
exports. The examined Cbm path does not consult FAT directory entries or cluster
chains; the similarly named MEP path does. Test renaming and relocation
separately to validate player behavior. Determine
which audio and analysis features the R1 actually consumes, including cues and
artwork. Map player edits before authorizing overwrites of existing R1 media.

Acceptance: a field map and fixture matrix account for all acceptance-relevant
bytes, with explicit [OBS], [ASSUME], and [UNKNOWN] status. No unexplained field
that may affect discovery or loading is filled with guessed values.

## 2. Implement an independent Cbm codec

Add a Tauri-independent `crates/rbl-cbm` crate with bounded recognition,
reading, writing, and structural/semantic validation. Keep it specific to Cbm;
do not copy MEP record fields or overload the DeviceSQL writer.

Use typed records and explicit little-endian serialization. Validate counts,
offsets, references, encodings, string boundaries, arithmetic, and output sizes.
Return contextual errors for unsupported or malformed data. Preserve unknown
fields when reading existing media where their meaning is not established;
refuse unsafe rewrites rather than discarding them.

Build all six files from one resolved track/playlist model and generation.
Introduce explicit R1 IDs if its identity rules differ from existing export IDs.
Implement category ordering, playlist/folder behavior, and artwork only from
the captured contract.

Acceptance: parser tests against stock fixtures, writer comparisons against
controlled exports with only documented volatile differences normalized, and
malformed/truncated input tests. Self-round-trip alone is insufficient.

## 3. Add an output profile to the shared export pipeline

Extend `ExportOptions` in `crates/rbl-export/src/lib.rs` with a database-output
profile. Keep it distinct from `CompatibilityFormat`, which concerns audio
conversion. Resolve the profile once per destination, then use it for both
space preview and execution.

Integrate R1 with:

| Area | Required change |
| --- | --- |
| `plan.rs`, `estimate.rs` | Include all six DBH files, R1 artwork, backups, and peak staging space; reject unsupported media/profile combinations before mutation. |
| `lib.rs` | Build Cbm from the reconciled selection and final exported audio properties, paths, and artwork, including retained device-only content. |
| `manifest.rs`, `incremental.rs` | Record output profile, codec version, DBH signatures and receipts, artwork recipe, device block size, and final audio metadata. Changing the profile invalidates the no-op shortcut. |
| `snapshot.rs`, `reconcile.rs` | Detect existing R1 state and outside edits; preserve device-only data or report a specific conflict before replacement. |
| `verification.rs` | Validate all requested formats, references, audio existence, category membership, playlist order, and generation consistency. |
| Recovery and progress | Include DBH in recoverable publication, cancellation, progress, final reports, and recovery validation. |

Initially rebuild the six small DBH databases together when relevant inputs
change. Reuse unchanged audio, analysis, and artwork. A repeat sync must become
a true no-op after content, paths, device block units and generated outputs are
proven stable.
Adding R1 support to an existing drive should reuse eligible audio.

Do not silently omit incompatible selected tracks and report R1 success. Use
the established conversion path when a proven R1 conversion profile exists;
otherwise return a clear preflight error listing affected tracks.

Acceptance: fresh export, profile enablement, unchanged sync, playlist-only
sync, metadata change, audio replacement, deletion, and missing-manifest cases
all produce the expected R1 library without unnecessary audio copying.

## 4. Extend recoverable publication

Today's exporter verifies staged files, calls `Publication::commit_retained`,
then verifies the destination. Generate Cbm in that existing stage using final
exported paths, actual prepared audio bytes, and the destination's device block
size. The statically traced Cbm writer supplies no reason to introduce raw FAT
access or a post-placement database-generation step. Validate this model with
controlled exports and physical acceptance rather than copying MEP behavior.

Retain old generation files and rollback information until final verification
succeeds. Include all six DBH files and companion artwork in the existing
publication journal and its post-publication verification/recovery paths.

Stock rekordbox itself deletes/moves the final files sequentially after
generation; that is not an atomic filesystem transaction. Extend recovery to
restore or complete the whole generation, and document the boundary for removal
or power loss. A recovered mixed DBH family must never pass verification.

Acceptance: fault injection at each publication/finalization boundary proves
recovery to a coherent generation. Existing audio and other database families
remain recoverable on failure, cancellation, and insufficient space.

## 5. Wire Sync Manager and all export entry points

Add the default-off global R1 preference to the USB Export preferences pane
and make it discoverable from Sync Manager; use translated
strings through `useTranslation()`. Route it through `src/ipc/types.ts`,
`client.ts`, and `backend-mock.ts`, then the Tauri selection and write path in
`src-tauri/src/commands.rs`. Keep direct invoke calls out of views.

Use the same captured global R1 option and destination profile for manual sync, automatic sync,
playlist export, and individual-track export. Otherwise a later operation
could update audio or standard libraries while leaving R1 stale. Concurrent
multi-drive sync must resolve each drive's profile independently.

Show an R1 database creation stage, include its space requirements in preview,
and report success only when all requested outputs verify. Switching R1 off
must not silently leave stale Cbm files advertised as current or delete an
unowned library. Define explicit cleanup of app-owned files and surface any
foreign-family conflict.

Extend drive recognition in `rbl-devices` to detect Cbm-only media. Audit
device-library mutation and USB import paths so unsupported R1 edits/history
cannot be silently ignored or lost. Full R1 round-trip import can be separate
work, but preserve or block writes where unmapped player edits would be lost.

Acceptance: UI/mock and integration tests cover saved options, two drives with
different profiles, automatic sync, cancellation, errors, and eject ordering.

## 6. Validate and release

Use `RB_LITE_TEST=1` and `rbl_db::fixture::build` for source-library write tests.
Use temporary destinations for codec and exporter tests. FAT integration tests
must exercise real FAT images, including fragmentation and relocation, to
validate the recovered logical-block model. Ordinary host-directory tests
cannot establish physical-player behavior.

Run focused Cbm/export checks first. Before handing off implementation, run
the applicable repository gates: workspace clippy and fixture tests, `pnpm
lint`, `pnpm build`, `pnpm test`, `pnpm budget`, and `pnpm e2e`.

On a physical XDJ-R1, verify discovery, title/artist/album/BPM browsing,
playlist order, metadata, supported audio load and playback on both decks,
artwork, repeated sync, removal, reinsertion, and player edits where supported.
Record firmware and media details. Compare the same synthetic collection
exported by stock rekordbox and rbxport.

Internal codec/integration work can proceed with fixtures before hardware is
available. Public compatibility claims require the physical acceptance matrix;
fixture, emulator, and filesystem results remain separately identified.

## Delivery sequence

1. Reference captures validating the static field map, category rules and block model.
2. Cbm codec with independent fixture validation.
3. Shared export profile, reconciliation, incremental state, and recovery.
4. Sync Manager controls, consistent entry points, and drive recognition.
5. Full regression checks and physical R1 acceptance evidence.

The static map substantially reduces schema uncertainty. The remaining major
dependency is controlled stock output, followed by physical acceptance.
Estimate implementation effort after the fixture comparisons expose any
unresolved serialization details.

## Companion application acquisition, 2026-10-15

A focused remotebox acquisition pass did not obtain an IPA or app bundle.
The [acquisition audit](./assets/remotebox-acquisition-2026-10-15/acquisition-audit.json)
records Apple lookup results for US/UK/Japan, a public app-catalogue search, and
200 Internet Archive inventories containing 96,204 file entries. Filename
matches were unrelated applications. Blocked catalogue requests are recorded
as inaccessible, not as proof that no copy exists.

No client executable was acquired or statically analyzed, and this search
resolves none of the remaining field/edit/artwork unknowns. A matching IPA or
extracted app bundle is still required. Inspect identity and executable
encryption before treating a supplied archive as usable code evidence.
