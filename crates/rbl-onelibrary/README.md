# rbl-onelibrary

[Crate index](../README.md) · [Architecture](../../docs/development/architecture.md)

Reads and builds the SQLCipher `exportLibrary.db` used by Device Library Plus / OneLibrary. `rbl-export` coordinates it with the legacy `export.pdb` library.

## Start here

Start with `ExportLibrary::open_read_only` in [`src/lib.rs`](src/lib.rs), then `src/build.rs` for authoring. Inspect schemas and table counts through the read-only API.

Read [`tests/roundtrip.rs`](tests/roundtrip.rs) for existing cases and expected behavior.
The [manifest](Cargo.toml) lists dependencies and feature flags.

## Code map

| File in `src/` | Responsibility |
| --- | --- |
| [`lib.rs`](src/lib.rs) | Opening, cipher setup, and schema inspection. |
| [`build.rs`](src/build.rs) | Export database construction. |
| [`settings.rs`](src/settings.rs) | Export settings. |
| [`playlists.rs`](src/playlists.rs) | Playlist and playlist content rows of an existing stick, read and edited in place. |
| [`key.rs`](src/key.rs) | Passphrase derivation implementation. |

## Contracts and safety

Cipher initialization order is part of the file contract. Do not print or copy passphrases into docs or logs. Keep track identities, playlist membership, paths, and metadata consistent with DeviceSQL. Decryption alone does not establish firmware compatibility.

## Run focused checks

From the repository root:

```sh
RB_LITE_TEST=1 cargo test -p rbl-onelibrary
cargo clippy -p rbl-onelibrary --all-targets -- -D warnings
```

Follow the [test guide](../../docs/development/testing.md) for broader checks
and the [contribution guide](../../CONTRIBUTING.md) before preparing a change.

