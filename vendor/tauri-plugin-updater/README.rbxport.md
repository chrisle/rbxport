# Compatible macOS updater quoting backport

This directory retains the complete normalized `tauri-plugin-updater` 2.11.0
crate, including its original MIT/Apache licenses and generated permissions.
Published crate SHA-256:
`b28d8cabdeb0564f03ae261963de4bc3d98321cd3d213e76a81b7d344e5df606`.
Its original source matches official tag `updater-v2.11.0`, commit
`6aa2854f314481a459be1189b02c65a2450789ab`.

Only `src/updater.rs` differs from that published crate. The macOS fallback
quotes each path for the shell and the resulting command for AppleScript,
and handles compilation, dispatch and reply failures without panicking.
These changes derive from official upstream commit
`2a9c29e004325db674fd904089749c38f4aac824` (included in updater 2.13.2).
The original 2.11.0 installation sequence remains unchanged. The broader
upstream bundle-swap and recovery changes are not included.

The root Cargo patch keeps the existing Tauri and Wry dependencies. The app's
exact `=2.11.0` requirement prevents a Cargo update from bypassing this patch
with a newer registry version. Updater 2.13.2 requires Tauri 2.12; remove the
root patch, exact app requirement and this directory together when a reviewed
framework update can use that maintained release or later. The nested Cargo.lock is
retained from the published package and does not govern the app build.

The two added macOS unit tests run through:

```
cargo test -p tauri-plugin-updater --lib
```

Their shell commands replace `rm` and `mv` with logging functions. They do not
install an application, move files, request administrator privileges or restart.
The crate remains outside the app's 19 workspace members, so invoke its tests
explicitly rather than relying on `cargo test --workspace`.
