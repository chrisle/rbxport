# rbxport

rbxport is a desktop app for a rekordbox DJ library. It reads and edits the
library in rekordbox's own `master.db`, analyses audio, plays tracks on two
decks, writes USB exports a CDJ can read, and serves the library to players
over Pro DJ Link ("LINK"). It runs on macOS, Windows and Linux, built with
Tauri 2, Rust and React.

This README is for working on the code. What the app does for its users is in
[docs/](docs/): [USB export](docs/usb-export.md), [backups](docs/backups.md),
[AppleScript](docs/applescript.md), [analysis settings](docs/analysis-settings.md)
and [waveform scrubbing](docs/waveform-scrubbing.md).

## Quick start

You need:

- Rust stable (the workspace's minimum is 1.85).
- Node 24 and pnpm 10 (`corepack enable` picks up the pinned version).
- The Tauri 2 prerequisites for your OS. On Linux that is
  `libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libgtk-3-dev libasound2-dev`.
- rekordbox 7 is optional. Without an installed library the app offers to
  create one.

```sh
pnpm install
pnpm dev          # the desktop app, against your installed library
pnpm dev:web      # the interface alone in a browser, against the mock backend
```

`pnpm dev:web` needs no Rust build and no library. The mock backend in
`src/ipc/backend-mock.ts` stands in for every command, and the Playwright
suite runs against it too.

**Before you run anything that writes, back up your library.** See
[Working with a real library](#working-with-a-real-library).

## Repository layout

| Path | What is there |
| --- | --- |
| `src/` | The React interface. `views/` holds the screens, `store/` the hooks that hold state, `ipc/` the only code allowed to call the backend, `i18n/` the strings. |
| `src-tauri/` | The app shell: Tauri commands, app state, windows, menus, updater, logging. `src/lib.rs` registers every command; most live in `src/commands.rs`. |
| `crates/` | The Rust libraries that do the work, all prefixed `rbl-`. They know nothing about Tauri. |
| `e2e/` | Playwright tests, run in Chromium and WebKit against the mock backend. |
| `docs/` | User documentation and notes on formats. |
| `design/` | Design tokens and reference material. `src/styles/tokens.css` is generated from `design/tokens/theme.json`. |
| `scripts/` | Version sync, locale build, icon and token generation, bundle-size check, release notes. |
| `public/locales/` | Generated translations. |

### The crates

| Crate | Job |
| --- | --- |
| `rbl-core` | Ids, time formats and durable file publishing shared by everything. |
| `rbl-db` | Opens rekordbox's SQLCipher `master.db`. `write.rs` holds every write the app makes; `fixture.rs` builds a throwaway library with the real schema for tests. |
| `rbl-index` | Loads the whole library into memory as columns. Sorting, filtering and searching happen here, not in the interface. |
| `rbl-anlz` | Reads and writes rekordbox's ANLZ analysis files (`.DAT`, `.EXT`, `.2EX`): grids, cues, waveforms. |
| `rbl-audio` | Decodes audio for analysis. |
| `rbl-analysis` | Tempo, beat grid, key and waveform analysis. |
| `rbl-deck` | Playback: two decks, the audio device, beat and key sync, scrubbing. |
| `rbl-export` | Writes a USB export: `export.pdb` through `rbl-pdb` and `exportLibrary.db` through `rbl-onelibrary`. |
| `rbl-pdb` | The DeviceSQL `export.pdb` format. |
| `rbl-onelibrary` | The SQLCipher `exportLibrary.db` (Device Library Plus / OneLibrary). |
| `rbl-devices` | Finds removable drives and reads what is already exported to them. |
| `rbl-backup` | Backup archives and restoring them. |
| `rbl-link` | LINK: the beacon, database server and file server a CDJ talks to, serving the library the way rekordbox does. |
| `rbl-prolink` | Pro DJ Link packets and the device table. |
| `rbl-dbserver` | The remote-database protocol a CDJ browses with (TCP 12523). |
| `rbl-nfs` | The read-only NFSv2 server a CDJ loads audio from. |
| `rbl-fakecdj` | A stand-in CDJ for testing the link servers over loopback. |
| `rbl-difftool` | Records what rekordbox itself changes in `master.db`, so writes copy rekordbox rather than guess. |

## How a request flows

1. A view calls a function on the backend object from `getBackend()` in
   `src/ipc/client.ts`. Inside Tauri that is `invoke(...)`; in a browser it is
   the mock.
2. The Tauri command in `src-tauri/src/` does blocking work on a worker
   thread and reads the library from `AppState` (`state.rs`).
3. The interface never receives the whole library. It opens a view (a sort, a
   filter, a search) that `rbl-index` evaluates, then fetches windows of rows
   by index as the table scrolls.

Edits go through one path. A command calls `edit(...)` in `commands.rs`,
which runs a `rbl_db::write::Writer` method inside `AppState::write_then`, then
`refresh_after_edit` re-reads only what changed (`Touched::Playlists`,
`Touched::Metadata`, and so on). The backend then emits `library:changed`
with a new generation, and the interface drops the pages it has cached.
Tag List edits emit `tag-list:changed` and keep the generation. The same path
serves edits made from a CDJ over LINK (`src-tauri/src/link.rs`).

## Working with a real library

The code is built so it cannot damage a library by accident. Keep it that way.

- `master.db` opens read-only unless a write is explicitly asked for, and a
  write is refused while rekordbox is running.
- `RB_LITE_TEST=1` refuses any write to the installed library, even from
  code that asks for one. CI sets it; set it when you run tests locally.
- Write tests use `rbl_db::fixture::build`, which makes a temporary library
  with the real schema. Never point a test at your own library.
- `RBX_DISABLE_READ_ONLY=1` exposes a session-only override: double-click the
  Read-only badge to allow writes while rekordbox runs. Both apps can then
  write the same file. It is for debugging only and is never saved.
- Read-only tools for looking at your own library:
  `cargo run -q -p rbl-db --example sql -- "SELECT ..."` runs one `SELECT`.
  `cargo run -p rbl-difftool -- record <name>` snapshots the database, waits
  while you do one thing in rekordbox, snapshots again and prints the diff.

## Checks

These are what CI runs before it builds a release. Run them before pushing.

```sh
cargo clippy --workspace --all-targets -- -D warnings
RB_LITE_TEST=1 cargo test --workspace

pnpm lint
pnpm build        # typecheck and production bundle
pnpm test         # Vitest unit tests
pnpm budget       # bundle size against perf-budgets.json
pnpm e2e          # Playwright; first run: pnpm exec playwright install chromium webkit
```

CI also runs `pnpm tokens` and `pnpm icons` and fails if
`src/styles/tokens.css` or `src/components/icons.tsx` changes. Regenerate and
commit them when you change their sources.

## Conventions

- **Rust lints are strict.** `unwrap`, `expect` and `panic!` are errors
  outside test modules. Nothing may panic across the IPC boundary. Clippy's
  `pedantic` group is on, and CI treats warnings as errors.
- **Frontend lint rules encode the architecture.** `eslint.config.js` forbids
  calling `invoke` outside `src/ipc`, polling, deep-cloning rows, and sorting
  or filtering row arrays in a view. Row ordering belongs to `rbl-index`.
- **Performance budgets are enforced.** `perf-budgets.json` sets bundle size,
  startup, scroll frame time, IPC response size and more. The bundle check and
  the e2e suite fail when a budget is exceeded.
- **Formats come from evidence.** Code that mirrors rekordbox or a CDJ says
  where each fact came from: `[OBS]` for something observed in a capture or a
  real library, `[ASSUME]` for an inference, `[UNKNOWN]` for what nobody has
  worked out yet. Keep the tags accurate when you change the code, and
  prefer measuring (`rbl-difftool`, a packet capture) to guessing.
- **Strings are translated.** Use `useTranslation()` from `src/i18n` for
  anything shown to the user. `pnpm locales` rebuilds `public/locales` from
  rekordbox's installed `.lang` files. It fails on strings rekordbox has no
  translation for until you run it with `--translate-missing`.
- **Commits** use Conventional Commits (`feat:`, `fix:`, `chore:`, ...), with
  a subject that says what changes for the user.

## Logs and environment

The app logs to stdout and to a daily file under
`~/Library/Application Support/rbxport/logs` (macOS) or
`%APPDATA%\rbxport\logs` (Windows). The last seven days are kept.

| Variable | Effect |
| --- | --- |
| `LOG_LEVEL` | Level for the app's own crates: `error`, `warn`, `info`, `debug` (default) or `trace`. `trace` adds LINK's packet-by-packet lines. |
| `RUST_LOG` | Replaces the whole filter, e.g. `RUST_LOG=rbl_link=trace,rbxport=info`. |
| `RBXPORT_LOG_DIR` | Writes the log file somewhere else. |
| `RB_LITE_TEST` | Refuses writes to the installed library. |
| `RBX_DISABLE_READ_ONLY` | Enables the session-only write override described above. |
| `E2E_PORT` | Moves the Playwright dev server so two checkouts can run e2e at once. |

## Releases

The version lives in `Cargo.toml` (`[workspace.package]`), `package.json` and
`src-tauri/tauri.conf.json`. `node scripts/sync-version.mjs --version X.Y.Z`
sets all three, and the workspace crates in `Cargo.lock`. Then add an entry
to `release-notes.json`. Each change starts with `(New)`,
`(Improved)` or `(Fixed)`, and the app shows these notes before it updates.

`pnpm dev` runs `sync-version.mjs` without arguments first, which sets the
version from the newest `v*` tag merged into your checkout. After a bump
that is not tagged yet, it rewrites those files back to the tagged version,
so don't commit them from a `pnpm dev` session.

`.github/workflows/ci.yml` runs the complete non-publishing release gate for
same-repository pull requests to `dev` and every `dev` push: pinned-current
Rust/Clippy, a separate Rust 1.85 MSRV compile check, Windows compilation, and
the frontend lint, build, unit, budget, generated-file, and Playwright checks.
Fork pull requests intentionally do not run on the self-hosted runners. A
manual Validate dispatch can preflight an exact SHA without building or
publishing installers.

Cut a release only from an already-green `dev` commit. First make one final
version and release-notes commit, then run `pnpm release:preflight --
--expect-untagged` and the manual Validate preflight on that exact commit.
Fast-forward `main` to the same SHA and create the immutable `vX.Y.Z` tag.
Pushing that tag runs `.github/workflows/release.yml`, which repeats the gate,
then builds, signs, and publishes installers with `latest.json` to the download
bucket. The pipeline creates no GitHub Release. If a tagged run has a transient
CI or publishing failure, dispatch Release with that existing `release_tag` to
retry the same source; do not bump the version merely to retry. If the source
needs a correction, make a new validated version commit and tag that new
immutable version.

`pnpm build` obfuscates the app's own JavaScript and omits source maps.
`pnpm dev` stays readable. Obfuscation makes the bundle harder to read but
protects no secrets, so none go in the frontend.

## License

rbxport is licensed under GPL-2.0-or-later. See [LICENSE](LICENSE) and
[LICENSING.md](LICENSING.md).

## Trademark notice

All third-party product names are the property of their respective owners.
rbxport is an independent project and is not affiliated with, endorsed by, or
sponsored by any third-party product or trademark owner.
