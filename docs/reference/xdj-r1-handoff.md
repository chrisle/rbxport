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
