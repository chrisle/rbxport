# Retained v1.9 export fixture

`legacy_overlap_v1_9.json.gz` contains six synthetic filesystem snapshots produced
by the ordinary exporter at `7fb4dad3f88a2cc0cecec0e718b4dde408fd7448`
(v1.9.0), with its original Cargo.lock (SHA256
`c0e4848f5ca4f98fa11eef31c8cf7a47631019e9e6995175c866f4df421e3cda`).
It contains generated database IDs, tiny generated audio payloads, generated ANLZ,
optional generated artwork payloads, and an unmanaged sentinel. There is no user
library, device image, playable-media claim, or player compatibility evidence.

The compressed fixture is 1,684,269 bytes; its SHA256 is
`5d5eb7dfed20b2d0c879dfc833deee6058d1a8c754a738fb97e38d88507caf2d`.
JSON format 1 pools file bytes by SHA256. Each case records the prior complete
file map and the naturally retained post-export file map with nanosecond mtimes.
The prior map is an independent test witness, never production recovery authority.
The loader reconstructs only fresh tempdirs, verifies pooled bytes, and preserves
captured times. Integration tests using these exact Linux timestamps run on Linux;
portable core tests separately exercise locked restoration and restart refusal.
Timestamp rounding on another filesystem does not weaken production validation.

## Historical generation

Each producer tree adds only `tests/ordinary_single_move.rs` to the exact v1.9
source. Retained producer commits are recorded per case:

- `f88b4e56c371fc3ce247d737dcc549511ee83f0c`: plain audio.
- `40d3a2f97e384e97d67e382df67952ffcac1d100`: changed artwork.
- `83fc350da55090bfd8413cbf1293c76d8dd12aca`: unchanged artwork and an untouched track.

Run the test-only source with the pinned toolchain/dependencies in an isolated
checkout, using `cargo test --locked -p rbl-export --test ordinary_single_move`.
Set `RBXPORT_641_EVIDENCE` to a new owned output directory to retain receipts and
`after-export-tree`. The variants set `RBXPORT_641_ARTWORK=1`; the last also sets
`RBXPORT_641_UNCHANGED_ARTWORK=1` and `RBXPORT_641_STABLE_TRACK=1`. These old tests
intentionally fail their current safety assertions after saving the actual result.
No crash hook, hand-built publication journal, or historical production fork is
used. Fresh runs vary in temporary source paths, encrypted DB salts and mtimes;
regeneration must preserve receipts and captured times rather than patching them.

The ordinary sequence first exports track 1 under artist A, renames it to Z, and
adds track 2 under A with the same basename. Both PIONEER layouts are included.
The old exporter legally accepts the move because Z is initially absent, then
stages the source replacement. In the replacement-first ordering its finalizer
copies the previous A back and retains `.incomplete`, while Z holds replacement
bytes. The opposite ordering automatically rolls back and is tested separately
against current production. Fabricated chains of moves are not these fixtures.

The recovery tests bind the original raw manifest checksum, old database/absence
receipts, moved/replacement hashes and required analysis bytes before mutation.
Affected artwork without byte authority refuses. Unchanged audio/artwork retain
the normal generation stamp trust; explicitly hashed required analysis is checked
once per recovery attempt. Restart tests interrupt durable boundaries, refuse
unknown/tampered states without writes, and compare the complete restored tree
and historical mtimes. Normal export retry and zero-copy repeat are also tested.
