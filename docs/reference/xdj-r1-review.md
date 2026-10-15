# XDJ-R1 implementation review

Evidence: alphatheta-docs 60278d505f3a432e93cdc55522cdf146c27ad7ba,
stock rekordbox 6.8.7 writer and stock R1 firmware 1.13 static analysis.
The R1 pages and linked bundles have no changes at the inspected origin/main
0bfc3849d0347019d3eeb9cd357566b17470fb51. No hardware ran.

| Contract | Evidence | Implementation and focused proof | Review |
| --- | --- | --- | --- |
| Six Cbm files, record fields and zero sentinel | cbm-layout, writer-byte-map.json; native reader 0x33cda/0x33e16 | cbm.rs, generated codec fixtures | Layout, admitted codes and references checked; bound file/set allocation before I/O and serialization |
| Text/collation/category traversal | collation-constants.json; createTitleDB, createArtistDB, createAlbumDB | cbm comparator, grouping/bounds tests | Static match; documented safe text/tie deviations remain |
| Audio properties and format admission | getBitrateIndex 0x10170ce90, setAudioInfoInTocHash | legacy.rs, PCM matrix and generated MP3/AIFF tests | Fixed MP4 media-offset checks, missing AAC channels, rational timebase and cancellation status |
| Native DAT lookup and cues | native 0x407ee/0x402e8/0x400e6/0x401f0 | r1.rs, collision/cue integration tests | Hash/shard/PPTH and slot bounds checked; device writes detected by owned hashes |
| Artwork | cbm-layout optional artwork conversion | r1.rs, generated artwork fixture | Fixed pre-decode limits and bounded square allocation |
| Guarded native timing | native-reader timing guard 0x3418e | serialized 449998/449999/450000 tests | Static boundary only; no universal rejection claim |
| Preference and immutable jobs | product architecture, no firmware transport requirement | preferences/IPC/commands, persistence/backend tests | Global default false, captured per job; v1 R1 request receipts rebuild under v2 without invalidating ownership or OFF digests |
| Ownership, OFF, publication/recovery | product safety contract, deliberate stock deviation | r1 protect, retained journal, toggle/edit/cancel/rollback tests | Existing foreign/changed libraries block; no delete-before-replacement |

Network request/reply envelopes are inapplicable to this initial USB writer.
Full device-edit import is deliberately unsupported and refuses changed output.
[UNKNOWN] Controlled stock captures, hardware browse/load/playback, fragmentation,
cue recall and remote-client artwork selection remain separate acceptance work.
All automated fixtures are generated, not stock captures.


## Findings and regression proof

- Ordinary generated AAC was incorrectly excluded when the MP4 demuxer left
  channels absent. Decode one packet using the codec configuration to obtain
  actual channels. The macOS generated AAC integration test now exports one
  Cbm record with format 0x41.
- MP4 n_frames and packet timestamps are in media-timebase units, not always
  sample-rate units. A generated AAC fixture doubles mdhd timescale/duration
  and stts deltas while preserving real duration. R1 duration, bitrate and
  data offset stay identical. Previously duration doubled and bitrate halved
  after the missing-channel refusal was bypassed.
- Only the first MP4 chunk offset was checked, and only against file size.
  Validate all stco/co64 entries against mdat payload extents, including
  extended boxes, nesting and duplicate-table refusal. Generated structural
  fixtures exercise non-first bad offsets, headers, end boundaries,
  truncation, duplicate tables, trailing bytes and cancellation.
- Artwork bounds ran after decode/RGB allocation. Decoder limits now precede
  decode and bound the later square allocation. Generated 4097x1 artwork
  is refused; 4096x1 is admitted and yields a 320-square JPEG.
- Cancellation in the audio inspector surfaced as a format conflict. The
  export adapter preserves ExportError::Cancelled. The affected export test
  cancels inside inspection and checks that the previous six-file set and
  normal libraries still verify.

- Untrusted DBH files were read in full before structural bounds were checked,
  and repeated playlist references could request very large serialized buffers.
  Reads now cap each file at 64 MiB and the set at 128 MiB, including concurrent
  growth. R1 analysis candidates and artwork comparisons use the same bounded
  reader. Category allocation is checked before serialization. Sparse oversized
  foreign Cbm and analysis-file tests check ON/OFF refusal and retained audio; a repeated-playlist
  fixture checks preflight refusal. These are explicit RBXport resource limits.

- The original R1 request key would otherwise reuse an unchanged v1 generation
  after the AAC fix. Bump only the enabled request profile to xdj-r1-v2. Keep
  the manifest ownership schema at v1 and the normal OFF digest unchanged.
  The migration fixture preserves exact v1 ownership and request receipts,
  proves a rebuild without recopying audio, then proves the next job is a no-op.

No Cbm format-code defect was found: the original hexadecimal range admits
exactly the documented codes. No codec layout rewrite was needed.


## Automated review result

Final workspace Clippy passes. The workspace test run passes with 1,803 tests
and 8 ignored; the complete final export suite passes with 219 tests and one
ignored, including all 48 export unit and 18 R1 integration tests. The final
backend preference/default fixture test passes. The workspace run preceded
the final allocation guards/profile migration; the final affected suite and
Clippy cover those changes. Frontend sources were unchanged and their checks
were not repeated. Detailed log paths and physical acceptance steps are in
[xdj-r1-handoff.md](xdj-r1-handoff.md).
