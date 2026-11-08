# XDJ-R1 USB export

Preferences > USB Export > Sync > **XDJ-R1 support (experimental)**
is global and defaults to
OFF, including settings saved before this option existed. Sync Manager's existing
Sync preferences button opens that section. Playlist, folder, Export Track,
AppleScript export and manual/automatic Sync Manager requests capture the
same preference before their job starts. Changing it during a job affects the
next job. It participates in the request digest, so enabling it rebuilds the
databases even when the selection and sources are unchanged. The corrected
category headers use request profile xdj-r1-v3, forcing one rebuild of an owned
v1/v2 R1 generation without recopying shared audio. Its ownership schema remains v1
so the rebuild still recognizes existing assets; OFF digests are unchanged.

Enabled exports also stage PIONEER/DBH/CbmDB.DBH, CbmTT.DBH, CbmAR.DBH,
CbmAB.DBH, CbmBP.DBH and CbmPL.DBH. Device Library (export.pdb) and
OneLibrary (exportLibrary.db, also called Device Library Plus) continue using
the same prepared audio. There is no third Device Library Plus output.
Unsupported R1 formats stay in the normal libraries and produce an R1 exclusion
warning. When normal libraries use .PIONEER, they retain that spelling; R1
DBH, analysis aliases and ordinary/large artwork use its native PIONEER lookup
paths. Audio is still shared. This is layout behavior, not an R1 filesystem
acceptance claim. Maximum CDJ compatibility can convert shared audio first; Cbm probes
the actual result instead of trusting Collection properties.

## Evidence

Source: alphatheta-docs main at
[60278d505f3a432e93cdc55522cdf146c27ad7ba](https://github.com/chrisle/alphatheta-docs/tree/60278d505f3a432e93cdc55522cdf146c27ad7ba),
particularly platform/rekordbox-xdj-r1-dbh.md,
platform/rekordbox-xdj-r1-cbm-layout.md,
platform/rekordbox-xdj-r1-native-reader.md, and linked writer-byte-map,
collation-constants and analysis-audit JSON evidence. These describe stock
rekordbox 6.8.7 and native R1 firmware 1.13 by static analysis.

[OBS] Headers are 0x9c5c bytes, contents records 0x9f8, TOCs 0x18 and
directories 0x1a0. Every category header +0x04 repeats the CbmDB content-table
end (confirmed by stock 6.8.7 capture). Directory slots reference directories,
not preceding TOCs.
Title contains ALL plus consecutive initial-character groups. Content and
category IDs remain those of Device Library even when earlier R1-ineligible
tracks are excluded. The 10,000-source-track bound is checked before exclusions. Artist/album
grouping uses exact source names. Sorting folds ASCII case/fullwidth ASCII
then compares mapped UTF-16 units. Album membership orders by album, track
number, title and content ID. Playlists flatten depth first, preserve track
order and retain empty named leaves. BPM bins run from 1 through 499.

[OBS] PCM format codes are 0x20/0x21 for WAV and 0x30/0x31 for AIFF:
the low bit selects 48 kHz rather than 44.1 kHz. Both 16- and 24-bit PCM are
admitted. This distinction is explicit in getBitrateIndex instructions
0x10170cf35..0x10170cfc1, even though the prose table's "respectively" can be
read ambiguously. The rate field separately carries PCM bits/second / 100.

[OBS] Synthetic block positions start at 150. The optional cached first sector
is zero. macOS reads diskutil's Device Block Size using the stock validation
rule (512 unless a larger multiple of 512 is reported). No raw FAT writes or
cluster inspection occurs. Non-macOS uses the stock 512-byte default [ASSUME].

[OBS] Native analysis lookup uses the existing UTF-16 hash/shard algorithm.
Base DAT candidates 0000 through 0010 need matching PPTH. Cbm has no embedded
analysis pointer. Mismatching candidates are preserved. Shared analysis or a
minimal base DAT carries the actual path and Collection cues. A-C hot cues
and memory cues/loops use PCOB/PCPT (at most three hot and ten memory slots); Auto Load is at content +0x132.
EXT/2EX continue serving other players and are not claimed as R1 inputs.

Ordinary artwork paths remain Artwork/#####/a<ID>.jpg. R1 adds 320-square
_l.jpg siblings from source artwork, centered on black before resizing.
[ASSUME] Resampling and JPEG encoding are equivalent in intent, not
byte-identical to JUCE. [UNKNOWN] Remote-client image selection is unverified;
writing both assets does not prove which one a client selects.

## Safety and deliberate deviations

The codec/reader live in Tauri-independent rbl-export; audio inspection is
in rbl-audio. Readers retain complete raw records, including unknown bytes.
Unknown nonzero intervals have no invented meanings. Readers accept legacy
RBXport category header +0x04 zero for safe owned v1/v2 migration; other
incorrect content-table ends are rejected. Foreign, incomplete, malformed, temporary or edited Cbm sets block export even with support OFF.
Full device-edit import is separate. Owned R1 outputs/assets have content
hashes in the generation profile, checked before reuse and publication.
Refusal protects audio still referenced by a foreign or edited library.

Cbm and assets join staging, conservative space estimates, verification and
the retained publication journal. Cancellation before publication leaves the
old set. Failed published validation restores it; rollback failure leaves
recovery pending. Turning OFF removes only unchanged owned R1 files through
the journal, keeping shared normal-library DATs/audio. Stale Cbm is not
advertised as a completed current profile.

Safe deviations from unsafe or unstable stock behavior:

- Reject oversized paths, files above u32 byte size, more than 10,000 tracks,
  directories or group entries, and invalid offsets instead of wrapping.
  All 10,000 directory slots can be occupied; both tail words remain zero.
- Bound R1 database/sidecar reads and Cbm output to 64 MiB per file,
  with 128 MiB per loaded Cbm set. Category sizes are checked before serialization; reads remain bounded even
  if a file grows after its metadata check. These are RBXport resource limits,
  not stock format or hardware limits.
- Empty/all-excluded selections produce a coherent six-file empty library,
  rather than stock's delete-before-replacement/empty-file publication.
- Preserve surrogate boundaries; never reproduce the 255-unit comment copy's
  erroneous cross-write into audio-path unit 252. Compact substitution
  retains the observed U+DFFF endpoint discrepancy without unsafe reads.
- Equal title keys retain deterministic source order; stock heap sort has
  no stable-order contract for those ties. Group equality stays separate.
- AAC MP4 requires one unambiguous chunk table. Every chunk offset must
  point inside an mdat payload, not an atom header or unrelated file bytes.
  Box/table scanning checks cancellation. Duration and payload bitrate use
  the media timebase, which can differ from the audio sample rate. If the
  demuxer omits AAC channels, codec configuration and one decoded packet
  establish them instead of guessing stereo or excluding ordinary M4A.
  Unsupported/ambiguous files are excluded or fail contextually.
- R1 artwork decoding is limited to 4096 pixels per dimension and 64 MiB of
  decoder allocations before RGB/square conversion. The square allocation
  is at most 48 MiB. This is an RBXport safety bound, not a stock format or
  hardware limit. Oversized artwork fails staging and preserves the old set.

[OBS] Native timing loads serialized start/end only when runtime values are
zero and block counts agree. Its guarded signed-span check clamps spans
**greater than 449,999 frames** to that span, just under 100 minutes at 75 Hz.
This is not a universal file rejection or playback limit. Tests at 449,998,
449,999 and 450,000 retain actual serialized duration on both sides.

## Validation and physical acceptance

All automated fixtures in tests/r1.rs and cbm unit tests are generated in
temporary directories, **not stock captures**. Tests cover offsets/references,
sentinels/bounds, text/collation, actual PCM properties, cues/artwork, toggles,
coexistence, incremental edits/removal, unknown-byte preservation,
pending/foreign edits, PPTH collisions, cancellation and journal rollback.
RB_LITE_TEST=1 guards library writes; command tests use
rbl_db::fixture::build. Those automated tests do not write an installed
library or USB.

[OBS] A two-track generated-WAV stock 6.8.7 capture and codec comparison is
recorded in [the handoff](xdj-r1-handoff.md). It does not cover the full
acceptance matrix below.

[UNKNOWN] Broader stock differential comparisons, physical R1
discovery/browse/load/playback, timing near 100 minutes, fragmentation/media
relocation, large files, device edits and remote-client behavior remain
unverified. Remotebox is unavailable and is not a prerequisite. Structural
test success does not establish physical compatibility.

For acceptance, use disposable FAT32 USB media and an isolated stock
rekordbox 6.8.7 fixture collection. Capture all six files and the complete tree
with hashes for empty, all-excluded, one-track and mixed-format exports.
Compare one field at a time and retain discrepancies. Test RBXport output
on firmware 1.13: discovery; ALL/letter/artist/album/BPM/playlist order; every
admitted audio family; memory/loop and A-C cue recall; Auto Load on/off;
artwork; and duration-boundary tracks. Preserve before/after images for device
edits and verify sync refuses them without deleting referenced audio. Repeat
incremental add/update/remove and interruption/recovery on disposable media.
Record firmware, filesystem and exact results.
