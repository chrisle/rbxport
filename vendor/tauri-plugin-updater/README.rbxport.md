# Compatible macOS updater quoting and placement backport

This directory retains the complete normalized `tauri-plugin-updater` 2.11.0
crate, including its original MIT/Apache licenses and generated permissions.
Published crate SHA-256:
`b28d8cabdeb0564f03ae261963de4bc3d98321cd3d213e76a81b7d344e5df606`.
Its original source matches official tag `updater-v2.11.0`, commit
`6aa2854f314481a459be1189b02c65a2450789ab`.

`src/updater.rs` and `src/error.rs` differ from that published crate. The macOS fallback
quotes each path for the shell and the resulting command for AppleScript,
and handles compilation, dispatch and reply failures without panicking.
These changes derive from official upstream commit
`2a9c29e004325db674fd904089749c38f4aac824` (included in updater 2.13.2).
A separate placement correction follows the same-filesystem staging and
backup/install/restore design in the official 2.13.2 implementation, without
its APFS atomic-swap path or a new dependency. Both temporary directories
are allocated on the install path's filesystem before moving the old bundle.
The unprivileged replacement is transactional: a failed placement restores
the old bundle; if restore also fails, its owned backup directory is retained
and the error exposes the recoverable path, even if another writer has
created a target meanwhile. The first backup move is distinguished from the
later placement move: only a permission refusal of the first can enter the
historical authorization fallback. A restored placement refusal cannot.

The protected-directory route retains the same checked AppleScript authorization
and dispatch boundary. Its shell command invokes the system JavaScript bridge
with only the target, staged bundle and reserved backup paths. Native macOS
`renamex_np(RENAME_EXCL)` moves the old bundle aside, places the new one, and
restores the old bundle if placement fails. Exclusive destination semantics
refuse an existing directory or symlink instead of nesting into it. If restore
also fails, the existing error exposes and retains the backup. Failure details
from authorization or the payload are logged. Only after successful placement
does the authorized helper remove the old backup. A cleanup refusal keeps the
installation successful and retains/logs the backup path. This is not a durable
crash-recovery protocol, and arbitrary process termination can leave a retained
backup requiring manual recovery.

A disposable macOS 15.7.7 arm64 guest exercised the protected path with
ad hoc-signed RBXport builds and a signed local update feed. Canceling the
administrator prompt preserved the old executable. Authorizing a retry
replaced the bundle in place, and Restart Now reopened the new version.
The original #457 install route and failure cause remain unverified.

A protected non-system-volume parent that cannot allocate staging safely
refuses before touching the original; it does not fall back to a cross-volume
copy. The standard protected Applications layout still stages in system temp
on the same filesystem and uses the existing authorization route.

The root Cargo patch keeps the existing Tauri and Wry dependencies. The app's
exact `=2.11.0` requirement prevents a Cargo update from bypassing this patch
with a newer registry version. Updater 2.13.2 requires Tauri 2.12; remove the
root patch, exact app requirement and this directory together when a reviewed
framework update can use that maintained release or later. The nested Cargo.lock is
retained from the published package and does not govern the app build.

The macOS quoting and generated-directory transaction tests run through:

```
cargo test -p tauri-plugin-updater --lib
```

The protected-command tests execute the exact inner shell/JavaScript payload as
the current user against generated generic directories, including literal
punctuation/Unicode paths, missing replacement and an injected destination
appearance. Ordinary transaction tests also use generated directories. They do
not install an application, request administrator privileges or restart. The ignored
cross-filesystem fixture requires an explicit disposable root on a different
filesystem from system temp:

```
RBX_UPDATER_CROSS_VOLUME_FIXTURE=/path/to/disposable/scratch \
  cargo test -p tauri-plugin-updater --lib cross_volume_default_temp \
  -- --ignored
```

It creates and removes its own child directory there, verifies the old
cross-device refusal, then uses same-filesystem staging for a generated
replacement. Keep this fixture separate from actual signed-update acceptance.
The crate remains outside the app's 19 workspace members, so invoke its tests
explicitly rather than relying on `cargo test --workspace`.
