# rbl-pdb

[Crate index](../README.md) · [Architecture](../../docs/development/architecture.md)

Reads and builds the legacy DeviceSQL database stored as `PIONEER/rekordbox/export.pdb`. This is the binary-format layer, not USB export orchestration.

## Start here

Read `Pdb::parse`, `PageType`, and row accessors in [`src/lib.rs`](src/lib.rs), then `build::FileBuilder` and the row encoders used by `rbl-export`.

Read [`tests/roundtrip.rs`](tests/roundtrip.rs) for existing cases and expected behavior.
The [manifest](Cargo.toml) lists dependencies and feature flags.

## Code map

| File in `src/` | Responsibility |
| --- | --- |
| [`lib.rs`](src/lib.rs) | Page chains, presence maps, row lookup, and decoding. |
| [`build.rs`](src/build.rs) | Page/table construction, and replacing one table of an existing file in place. |
| [`rows.rs`](src/rows.rs) | Row encoders. |
| [`reference.rs`](src/reference.rs) | Reference-format helpers. |

## Contracts and safety

Presence bits determine which row offsets are live; deleted rows may leave offsets behind. Keep page bounds, string pointers, and row layouts covered by round-trip tests. Successful parsing does not prove a player accepts an export.

## Run focused checks

From the repository root:

```sh
RB_LITE_TEST=1 cargo test -p rbl-pdb
cargo clippy -p rbl-pdb --all-targets -- -D warnings
```

Follow the [test guide](../../docs/development/testing.md) for broader checks
and the [contribution guide](../../CONTRIBUTING.md) before preparing a change.

