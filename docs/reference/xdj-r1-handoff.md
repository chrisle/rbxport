# XDJ-R1 implementation handoff

- Host: chris-m2-mac, verified by ComputerName and Apple M2 CPU.
- Worktree: /Users/chrisle/code/rbxport-worktrees/xdj-r1-sync
- Branch: codex/xdj-r1-sync
- Base: f905eefaced796772448014c3217a17a8f8ef4ca (fetched origin/dev).
- Evidence: alphatheta-docs origin/main 60278d505f3a432e93cdc55522cdf146c27ad7ba.
  Its local main diverged, so ff-only pull was refused. Reference checkout:
  /Users/chrisle/code/alphatheta-docs-r1-reference-01a13e53-b.
  This task did not edit either original checkout; other work continued there
  independently. No reset, stash, implementation push, merge or release was used.

The global OFF-default preference is captured per job across every USB export
entry point. Enabled jobs add six Cbm files, native base DAT lookup/cues and
ordinary/320-square artwork while sharing audio with Device Library and
OneLibrary (also called Device Library Plus). Normal hidden roots retain their
spelling; R1 assets use the native PIONEER paths. Existing OFF digests remain
compatible. Cbm, dependent assets and ownership join retained publication,
verification and recovery. Foreign/pending/changed R1 output blocks destructive
sync. Turning OFF retires only unchanged owned R1 files and keeps shared assets.

The independent structural reader retains raw headers, contents, TOCs and full
directories. Safe deviations, byte layout, collation, bounds and the guarded
449,999-frame native timing clamp are documented in [xdj-r1.md](xdj-r1.md).
Instruction cross-check: PCM code low bits select sample rate, not bit depth.
Excluded rows do not renumber original Device Library content/category IDs.
Fresh unresolved fields are zero; existing opaque record bytes are retained by
the reader and changed/foreign libraries are not rewritten.

## Changed files

- Format/audio: crates/rbl-export/src/cbm.rs and r1.rs; crates/rbl-audio/src/legacy.rs.
- Pipeline/profile: rbl-export lib.rs, incremental.rs, plan.rs and verification.rs.
- Global preference/job capture: src/lib/preferences.ts, USB Export pane,
  Sync Manager, App export handlers, src/ipc and src-tauri/src/commands.rs.
- Three new UI strings in all 17 locale catalogs, plus generated ui.json.
- Generated codec/export fixtures, backend fixture tests, preference/UI tests
  and user/reference documentation. Cargo.lock only adds the existing image
  dependency to rbl-export.

## Validation

All fixture libraries and exports are disposable; RB_LITE_TEST=1 guards Rust
tests. No installed rekordbox library or real user's USB was written.
Generated fixtures are not stock captures or physical-device acceptance.

| Check | Result |
| --- | --- |
| Focused R1 export tests, final tree | 13 passed |
| Codec/parser and native cue-bound checks | 7 codec tests and 1 cue-bound test passed; all 46 export unit tests passed |
| Workspace Clippy, all targets, -D warnings | Passed |
| RB_LITE_TEST=1 cargo test --workspace | Exit 0: 1,796 passed, 8 ignored |
| Full rbl-export regression suite | 209 passed, 1 ignored; final affected R1 tests re-run above |
| Real backend command against fixture library, final tree | Passed, including ON then missing-option OFF |
| pnpm lint / build / budget | Passed |
| pnpm test | 116 files, 1,367 tests passed |
| Updated persistence/preferences checks | 37 passed |
| pnpm e2e --workers=2 | Exit 0: 864 passed, 1 flaky passed on retry, 15 skipped |

Validation logs are under /private/tmp/r1-*.log on the M2. pnpm is invoked
through corepack; the E2E child process used a temporary pnpm launcher on PATH
and E2E_PORT=16537 to avoid other worktrees' servers. No global tools were
installed or reconfigured.

## Remaining acceptance

Controlled stock six-file captures, differential validation and physical R1
discovery, browse, loading/playback, artwork, cue recall/Auto Load, timing and
device-edit round trips remain open. Remotebox was unavailable and was not a
prerequisite. Use an isolated stock 6.8.7 fixture collection and disposable
FAT32 media, retain before/after trees and hashes, then follow the concrete
physical acceptance steps in [xdj-r1.md](xdj-r1.md). Full R1 edit import remains
separate. No physical compatibility or complete unknown-field semantics is
claimed.


## Review validation, 2026-10-15

The review fixes AAC channel discovery and media-timebase calculations,
validates every MP4 chunk offset against media payloads, preserves cancellation
status, bounds artwork decoding and R1 database/sidecar allocations, and bumps
only the enabled request profile to v2. Old ownership and OFF digests remain
compatible. See [the evidence coverage and findings](xdj-r1-review.md).

| Check on chris-m2-mac | Result |
| --- | --- |
| Workspace Clippy, all targets, -D warnings, final Rust tree | Passed |
| RB_LITE_TEST=1 cargo test --workspace | 1,803 passed, 8 ignored |
| Final affected rbl-export suite | 219 passed, 1 ignored |
| Final R1 integration subset | 18 passed |
| Final export unit subset | 48 passed |
| Audio malformed-MP4 structural test | Passed |
| Final backend R1 option/default fixture test | Passed |
| git diff --check | Passed |

The workspace run covers the AAC/artwork/cancellation fixes. The final
allocation/sidecar guards and profile migration were then covered by the
complete final export suite, Clippy and backend fixture test. All tests passed;
the existing block 0.1.6 future-incompatibility notice remains. Intermediate
development test/lint failures were corrected before these final checks.
No frontend sources or token/icon sources changed in this review, so frontend
checks were not repeated; the implementation run's results remain above.

Logs: /private/tmp/r1-review-workspace.log, r1-review-export-frozen.log,
r1-review-clippy-final.log, r1-review-integration-final.log,
r1-review-audio.log and r1-review-ipc-frozen.log on the M2.

Fixtures are generated disposable files and rbl_db::fixture libraries.
The macOS AAC test uses afconvert to generate audio and modifies its media
timescale without changing real duration. It is not a stock export capture.
No installed library or real user's USB was written. Resource bounds are
RBXport safety limits, not measured stock/device limits. Stock differential
captures and physical R1 acceptance remain open as described above.
No merge, push or release was performed.

## Stock rekordbox 6 test attempt, 2026-11-07

[OBS] On chris-m2-mac, cloned the stopped research VM `rb6-r1` to
`rb6-r1-capture-20261107`. Original VM, original `r1-fat32.img`, host
Collection and user USB media were preserved. Stock version 6.8.7.0303 has
executable SHA-256
`aab4838b9768860da941f232db3f31cfa397a7a79f6052cde1aa6584f2d5dc3c`,
matching the analyzed build. The VM's Collection contains stock factory
samples as well as two generated WAVs; no whole-Collection export was used.

[OBS] Generated two-track classic/OneLibrary and R1 exports from this branch.
Both exports verified; a separate Python parser checked all six generated
Cbm files, record/category references, extents and sentinels. These are
**generated RBXport outputs**, not stock captures. The new
`xdj_r1_fixture` example requires `RB_LITE_TEST=1`, refuses an existing
output directory, and accepts only the optional `--r1` flag. It expects
stereo 16-bit 44.1 kHz generated alpha.wav (3 seconds) and beta.wav
(4 seconds); it never opens an installed Collection. Example:

```sh
RB_LITE_TEST=1 cargo run -p rbl-export --example xdj_r1_fixture -- \
  /path/to/generated-wavs /path/to/fresh-output --r1
```

[OBS] Stock EXPORT mode opened without virtual USB. A mounted FAT32 image
was not listed under Devices. A custom Tart USB configuration repeatedly
restarted the guest. A minimal native Virtualization host without the explicit
XHCI controller reached the desktop with a 256 MiB FAT32 volume reported as
USB, external physical, 512-byte blocks. After stock launch, guest reboot
history advanced from 18:36 to 18:38. The guest then became unreachable
again. No stock Cbm set or panic report was captured. [UNKNOWN] The cause
of the guest restarts is unresolved. This is an environment blocker, not a
codec compatibility verdict. The disposable USB image was seeded with the
generated classic export, so the intended comparison was the unmodified
stock Cbm writer over identical prepared inputs.

Checks this attempt: example build and focused Clippy with `-D warnings`
passed; missing test guard, existing destination and unknown flag were
rejected without creating output. The initial example stdout lint was fixed.
Independent generated-output parsing and Python syntax checks passed.
No production/UI code changed; full workspace/frontend checks were not rerun.

Artifacts and resume inputs live at
`/Users/chrisle/r1-research/captures/20261107-stock-comparison`:
`run.json`, `compare.py`, `rbxport-r1`, `classic-input`,
`rbxport-independent-structure.json`, generated source hashes, test image,
VM configuration/source and diagnostics. VM console logs contain temporary
VNC connection credentials and must not be published wholesale. Test VM
hosts were stopped after the attempt.

**Result: stock differential validation remains incomplete.** Resume with a
stable USB-capable isolated environment or dedicated disposable physical USB
and isolated fixture Collection. Retain the actual stock six-file set and
full tree/hashes before running `compare.py`; never substitute generated files
for stock inputs. The two-track comparison also does not cover the remaining
empty/excluded/mixed-format, text, cue/artwork and physical R1 acceptance
matrix above. Nothing was merged, pushed or released.

## Physical USB attempt, 2026-11-08

The user authorized TRIODE2 for testing and explicitly permitted erasure.
Verified M2 USB identity (62 GB, FAT32, initial volume UUID
67517EDC-C106-304C-9663-F4657D8BAE5B, disk9s1), then formatted only that
partition. The signed stock installer and writer hash matched the previous
attempt. No stock Cbm capture was obtained.

**Isolation incident:** a direct sandbox launch used a test-only HOME and
CFFIXED_USER_HOME. Initial checks proved normal Library reads and writes
outside the test/USB roots were denied. Selecting the extracted app through
CUA `getApp` nevertheless started a second LaunchServices instance outside
that confinement. It opened the normal host library and imported 18 factory
sampler rows. This violates the intended no-installed-library-write boundary.
Both copies and helpers were stopped promptly. Private copies of the library,
WAL and settings were retained. Read-only queries against those copies found
50 content rows, including 18 created during this attempt, all pointing to
Sampler files with factory sample names. The October 16 backup contains 32
rows; it is not assumed to be a complete immediate pre-incident baseline.
No blind restore or row removal was performed. Database and WAL hashes
remained unchanged after containment; the SHM file changed. Other settings,
cache and sample-file side effects have not been exhaustively compared.

A test-only wrapper subsequently forced LaunchServices through the same
sandbox and disabled optional helper launch paths; the original writer
Mach-O bytes were unchanged. That confined launch did not change the normal
DB/WAL but could not be controlled through CUA. It was stopped. Do not use
the initial direct-launch plus `getApp` approach again. A separate macOS
test account/session is the next isolation boundary before continuing.
A rekordbox account sign-in requirement has not yet been confirmed there.

Private artifacts:
`/Users/chrisle/r1-research/captures/20261108-triode2/run.json`,
`isolation-checks.json`, `usb-format.log`, and `incident/`.
Incident copies contain real library/settings data and must not be published.
TRIODE2 is now test media with stock-created initialization files, not a
verified R1 export. Stock differential and physical player acceptance remain
open. This attempt changed no production code and ran no new product test
suite; the sandbox refusal probes and read-only incident queries are
operational diagnostics, not export validation. No merge, push or release.

## Authorized regular-account test checkpoint, 2026-11-08

The user clarified that the M2 is not used for their regular rekordbox work
and explicitly authorized the normal macOS account/library plus TRIODE2 for
this test, with Cloud Library Sync off. Restored the extracted app's original
signed layout and verified its deep signature and the recorded writer hash.
No login prompt was required. `EnableLibrarySync=0` was checked before import
and after export; cloud sync was not enabled.

Imported only the generated alpha/beta WAV playlist, exported it to TRIODE2,
then used rekordbox's Remove USB device control. Observed stock “Create
library” progress, its completion and TRIODE2 disappearing from Devices.
The device was fully ejected and its node disappeared from macOS. **Actual
six-file capture/comparison is pending physical reinsertion**, which was
requested from the user. Do not infer byte parity from the UI completion.

Retained the classic export before finalization in the private artifact
folder's `stock-before-eject`. Exported WAV hashes match the generated input
hashes. Added `xdj_r1_compare_pdb`, a guarded codec comparison example that
reads only that copied classic PDB and actual WAVs, preserving PDB IDs,
paths, playlist order and metadata. It never reads Cbm as construction input
or opens a Collection. It deliberately accepts only two alpha/beta WAVs,
no named categories/artwork, and one flat playlist; it is not a general
foreign-library converter or full pipeline acceptance test.

The example built, regenerated six structurally valid Cbm files, passed
focused Clippy with `-D warnings`, and refused missing RB_LITE_TEST/existing
output. A separate Python parser validated the generated files. No product
code changed; full workspace/frontend checks were not repeated. Artifacts:
`authorized-host-test.json`, `rbxport-from-stock-pdb`,
`rbxport-from-stock-pdb-structure.json`, and build/Clippy logs under the
20261108-triode2 artifact root. Once TRIODE2 returns, preserve its actual
files/tree/hashes before doing any further export, then compare against
`rbxport-from-stock-pdb`. Stock differential and physical-player acceptance
remain open. No merge, push or release.

## Completed stock capture and header fix, 2026-11-08

**TRIODE2 operational constraint: NEVER EJECT IT AGAIN.** The user explicitly
allows mounting/unmounting only. Do not use rekordbox's Remove USB device
control on TRIODE2: it ejects the device. After their physical reinsertion,
this phase only read the mounted stick. All comparison/build/tests use saved copies or
disposable temporary exports. Stock rekordbox was stopped before reinsertion.

[OBS] Captured all six actual stock 6.8.7.0303 DBH files and the complete
post-finalization tree, with SHA-256 hashes. The stock writer operated on two
generated PCM WAVs, a flat beta/alpha playlist, no artwork or named metadata,
BPM 150 and Auto Load on. This is a controlled stock capture, not an RBXport
fixture relabeled as stock. The exported audio hashes match the inputs.
Only the six DBH files, hashes and provenance are committed under
`crates/rbl-export/tests/fixtures/xdj-r1-stock-6.8.7`; private device settings
and the full tree stay outside Git.

[OBS] The initial raw comparison found CbmDB fully identical except its
four-byte timestamp. Each category had exactly two other differing bytes:
header +0x04 was zero in RBXport and 0x0000b04c in stock. This is the CbmDB
content-table end, not the category file length. The initial parser's
semantic-field comparison did not include this word; its byte comparison
correctly exposed the gap. The independent parser now validates that
cross-file header reference as well.

Fixed the writer to populate this word in every category. Readers reject
incorrect nonzero values while retaining legacy zero-header compatibility;
existing ownership/hash protection still rejects foreign or changed sets.
The enabled request profile advances to `xdj-r1-v3`, regenerating owned v1/v2
output on unchanged input without recopying audio. OFF request digests and
ownership schema stay unchanged. A regression test reconstructs zero-header
v2 files plus their genuine hashes and checks safe migration/no-op behavior.

[OBS] Rebuilt Cbm from the saved stock classic PDB and actual exported WAVs,
without using Cbm as construction input. Independent Python structure and
byte comparison now passes: **all six files are byte-identical except bytes
0x10..0x13, the per-file timestamp**. The checked-in stock regression tests
also compare every other byte. This covers content IDs/paths/timing/blocks,
unknown groups, title letters, BPM, flat playlist order and blank fields for
this particular two-WAV selection. It does not validate other audio families,
nontrivial metadata/text, artwork, actual cue recall, or physical R1 playback.
Broader stock fixtures and the physical firmware 1.13 acceptance matrix
remain required before claiming full compatibility.

Artifacts in the existing 20261108-triode2 capture directory:
`stock-after-eject`, `stock-tree-sha256.json`, initial `stock-comparison.json`,
`rbxport-fixed-stock-pdb`, `stock-comparison-fixed.json`, and `compare.py`.
Original captures remain unchanged; the initial comparison is retained with
its semantic scope explained. Cloud Library Sync remained off; no login was
needed. This supersedes the earlier pending-capture checkpoint, not the
historical isolation-incident record.

Validation of the final header/profile fix:

| Check | Result |
| --- | --- |
| Independent Python stock structure/byte comparison | All six match except timestamp bytes 0x10..0x13 |
| Focused R1 integrations | 19 passed, including legacy v2 migration and OFF behavior |
| Actual stock-byte/header regression tests | 2 passed |
| cargo clippy --workspace --all-targets -- -D warnings | Passed |
| RB_LITE_TEST=1 cargo test --workspace | Exit 101: 1,748 passed, 1 failed, 8 ignored |
| Failed backup-restore test, exact rerun after agent cleanup | Passed, exit 0 |
| RB_LITE_TEST=1 cargo test --workspace --doc | Passed, exit 0; no doc tests defined |
| git diff --check | Passed |

The workspace failure was the unchanged backup restore safety check reporting
“Quit rekordbox before restoring this backup.” The stock test's background
rekordboxAgent remained after the main application stopped. Stopped that
specific test agent and its helpers; the exact failed test then passed.
No backup code changed and the full workspace command was not repeated after
cleanup. Do not report its original run as exit 0. All R1 tests passed in that
workspace run as well. Existing duplicate example-output-name and block 0.1.6
future-compatibility notices remain. An initial new test-only truncating-cast
lint was corrected before final Clippy. No frontend/token/icon sources changed,
so their checks/generators were not repeated. Rust checks used the available
1.99.0 toolchain directly because this host's rustup launcher was absent.

Logs are `header-fix-focused.log`, `header-fix-workspace-clippy-final.log`,
`header-fix-workspace-tests.log`, `header-fix-backup-rerun.log`, and
`header-fix-workspace-doc-tests.log` in the private capture directory.
TRIODE2 remains mounted with the stock capture intact. No merge, push or
release was performed.
