# rbl-export

[Crate index](../README.md) · [Architecture](../../docs/development/architecture.md)

Coordinates USB-library export and synchronization: audio layout, analysis files, legacy DeviceSQL, Device Library Plus, manifests, and verification. Format encoding is delegated to sibling crates.

## Start here

Start with `SourceTrack`, `SourcePlaylist`, `ExportOptions`, and the export entry points in [`src/lib.rs`](src/lib.rs). Follow a fixture through `tests/export.rs` before changing the pipeline.

Read [`tests/export.rs`](tests/export.rs) for existing cases and expected behavior.
The [manifest](Cargo.toml) lists dependencies and feature flags.

## Code map

| File in `src/` | Responsibility |
| --- | --- |
| [`lib.rs`](src/lib.rs) | Export orchestration, layout, options, progress, and cancellation. |
| [`manifest.rs`](src/manifest.rs) | Exported track and playlist state. |
| [`sync_record.rs`](src/sync_record.rs) | Sync identities. |
| [`snapshot.rs`](src/snapshot.rs) | Export snapshots. |
| [`reconcile.rs`](src/reconcile.rs) | Sync reconciliation. |
| [`verification.rs`](src/verification.rs) | Export verification. |
| [`ext_pdb.rs`](src/ext_pdb.rs) | Extended database support. |
| [`device_library.rs`](src/device_library.rs) | A stick's own playlists, read and edited in one library at a time. |

## Contracts and safety

Writes are confined to the selected destination; use temporary destinations in tests. Preserve stable identities, cross-database path agreement, cancellation, and conflict handling. A database round trip, FAT32 filesystem validation, firmware playback, and physical-device behavior are separate results.

## Run focused checks

From the repository root:

```sh
RB_LITE_TEST=1 cargo test -p rbl-export
cargo clippy -p rbl-export --all-targets -- -D warnings
```

Follow the [test guide](../../docs/development/testing.md) for broader checks
and the [contribution guide](../../CONTRIBUTING.md) before preparing a change.

