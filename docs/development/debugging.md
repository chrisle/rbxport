# Debugging and local maintenance

[Documentation](../README.md) · Next: [Releases](releases.md)

Reproduce a problem at the smallest boundary first: browser/mock, Rust fixture,
protocol client, desktop app, firmware emulator, or physical device. Record
which boundary failed before comparing results from another one.

## Logs

The app writes stdout and a daily file under the OS data directory plus
`rbxport/logs`. Typical paths are `~/Library/Application Support/rbxport/logs`
on macOS and `%APPDATA%\rbxport\logs` on Windows. The newest five daily
files are retained. See `src-tauri/src/logging.rs` for filter and retention logic.

The same lines, the newest 10,000 since the app started, are kept in memory
for the Log window (`src-tauri/src/log_window.rs`, `src/views/log/`). Clicking
the status bar's message or error opens it, or brings the open one forward;
the messages the status bar shows are logged under `rbxport::status` (errors
at `warn`, the rest at `info`). The window filters by level and text, copies
or saves the lines it shows, and opens the log folder. Right-clicking the
app name in the status bar still opens the current log file itself.

| Variable | Effect |
| --- | --- |
| `LOG_LEVEL` | App-crate level: error, warn, info, debug (default), or trace. |
| `RUST_LOG` | Replaces the filter; for example `rbl_link=trace,rbxport_lib=info`. |
| `RBXPORT_LOG_DIR` | Overrides the log directory. |
| `RBXPORT_TEST` / `RB_LITE_TEST` | Refuses installed-library writes; does not create a fixture. |
| `RBXPORT_OPTIONS` | Names an options file, used by private harnesses to select a fixture library. |
| `E2E_PORT` | Selects the Playwright dev-server port; the production preview uses the next port. |
| `RBXPORT_PRIVATE_REPO` | Overrides the private test checkout location. |
| `ATEMU_DIR` | Selects the emulator checkout for private firmware harnesses. |

`pnpm dev` supplies a `RUST_LOG` filter in `package.json`. To change it for
that launch, adjust the invocation or run the Tauri command with your chosen
filter; setting `LOG_LEVEL` will not override an explicit `RUST_LOG`.

## Read-only diagnostics

The database SQL example accepts a SELECT for inspecting library state:

```sh
cargo run -q -p rbl-db --example sql -- "SELECT ID, Name FROM djmdPlaylist LIMIT 5"
```

Use `cargo run -p rbl-difftool -- record <name>` to compare snapshots around
one deliberate action in rekordbox. The tool observes that action; it is not
a writable development test. Keep captured user data in the private repository.

For analysis diagnostics that read an audio file and write separate output:

```sh
cargo run --release -p rbl-analysis --example waveform_probe -- AUDIO OUTPUT_DIRECTORY
```

Emulator and physical LINK investigations use the private evidence locations
listed in [Hardware coverage](../reference/link-testing.md).
[Sentry](sentry.md) covers optional crash reporting for packaged builds.

## Common setup failures

| Symptom | Check |
| --- | --- |
| Native Linux build cannot find WebKit or ALSA | Install the packages in [Getting started](getting-started.md). |
| Playwright cannot launch a browser | Run `pnpm exec playwright install chromium webkit`. |
| Production preview tests cannot find assets | Run `pnpm build` before `pnpm e2e`. |
| Two checkouts attach to the same test server | Give each an `E2E_PORT`. |
| A write test is refused | Confirm it uses a fixture; do not disable protection to target the real library. |
| Desktop startup changes version files | Inspect tag-derived synchronization in [Releases](releases.md). |
| Private test harness is missing | Check the sibling checkout or set `RBXPORT_PRIVATE_REPO`. |
| LINK ports are occupied | Quit rekordbox and other Pro DJ Link sources before a private firmware run. |

## Cleanup

Preview cleanup with `npm run clean -- --dry-run`, then run `npm run clean`
to remove generated build and test output from this checkout.

Optional flags extend it: `--dependencies` removes `node_modules`; `--app-data`
removes app caches and abandoned partial backups (close the app first);
`--git` prunes missing worktrees and local branches already merged into HEAD.
It preserves `main`, `dev`, the current branch, and checked-out worktree branches.

Cleanup preserves completed backups, preferences, logs, recovery journals,
and rekordbox libraries. Inspect the preview before using optional flags.
