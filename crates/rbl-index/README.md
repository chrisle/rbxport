# rbl-index

[Crate index](../README.md) · [Architecture](../../docs/development/architecture.md)

Loads the library into a columnar in-memory snapshot and owns sorting, filtering, searching, playlists, and browse views. The frontend receives windows of rows rather than the whole library.

## Start here

Read `Library` and the re-exports in [`src/lib.rs`](src/lib.rs), then follow `load` into `ViewSpec`/`View`. Use `testing` for small synthetic libraries.

Read [`tests/views.rs`](tests/views.rs) for existing cases and expected behavior.
The [manifest](Cargo.toml) lists dependencies and feature flags.

## Code map

| File in `src/` | Responsibility |
| --- | --- |
| [`load.rs`](src/load.rs) | Snapshot loading and targeted refresh. |
| [`view.rs`](src/view.rs) | View selection, search, and sorting. |
| [`filter.rs`](src/filter.rs) | Track filters and counted values. |
| [`strings.rs`](src/strings.rs) | Packed strings and interners. |
| [`smart.rs`](src/smart.rs) | Smart playlist rules. |
| [`cache.rs`](src/cache.rs) | Snapshot caching. |
| [`related.rs`](src/related.rs) | Related-track selection. |
| [`device.rs`](src/device.rs) | A stick library's tracks as a view, ordered and searched. |
| [`xml_export.rs`](src/xml_export.rs) | XML export. |

## Contracts and safety

Keep list transformations here, not in React. Row indices are snapshot-local and must not survive a reload as persistent identities. Cover ordering, NULL/sentinel values, and search semantics with small fixtures.

## Run focused checks

From the repository root:

```sh
RB_LITE_TEST=1 cargo test -p rbl-index
cargo clippy -p rbl-index --all-targets -- -D warnings
```

Follow the [test guide](../../docs/development/testing.md) for broader checks
and the [contribution guide](../../CONTRIBUTING.md) before preparing a change.

