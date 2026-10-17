# Getting started

[Documentation](../README.md) · Next: [Architecture](architecture.md)

Start in browser mode. It provides the real React interface with a mock
backend, so you can learn the UI without opening a rekordbox library.

## Prerequisites

The repository pins pnpm 10.17.1 in `package.json`; CI uses Node 24.
Desktop development also requires Rust (workspace MSRV 1.89), a C/C++ build
toolchain for native dependencies, and the Tauri 2 platform prerequisites.

| Platform | Desktop requirements |
| --- | --- |
| macOS | Xcode Command Line Tools and the system WebKit framework. |
| Windows | MSVC build tools, Windows SDK, and WebView2. Native dependency builds also use Perl; inspect the Windows setup in `.github/workflows/release.yml`. |
| Linux | WebKitGTK 4.1, GTK 3, AppIndicator, SVG, ALSA development libraries, and patchelf. |

The Debian/Ubuntu package list used by CI is:

```sh
sudo apt-get install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libgtk-3-dev libasound2-dev
```

## Install and run the browser UI

```sh
corepack enable
pnpm install
pnpm dev:web
```

Open the URL printed by Vite. Outside Tauri, `getBackend()` in
`src/ipc/client.ts` selects `backend-mock.ts`. Browser mode requires neither
Rust nor rekordbox. Mock success verifies frontend behavior only.

## Run the desktop app safely

`pnpm dev` builds and opens the Tauri app. It can discover your installed
rekordbox library; rekordbox is optional and the app can create a library.
For development, refuse installed-library writes:

```sh
RB_LITE_TEST=1 pnpm dev
```

In PowerShell, set `$env:RB_LITE_TEST = '1'` before `pnpm dev`.
`RBXPORT_TEST=1` is the current equivalent; `RB_LITE_TEST` remains a supported
legacy alias. Neither variable supplies a fixture. Writable tests must build
an isolated library with `rbl_db::fixture::build` and explicitly use it.
Do not test edits, analysis, import, restore, or export against real user data.

The normal library guard also refuses writes while rekordbox is running.
Library protection and process detection are additional guards, not a reason
to use an installed library as a writable test fixture.

`pnpm dev` runs version synchronization first. It derives the version from
release tags merged into the checkout and can change version files. Inspect
those changes before committing; see [Releases](releases.md).

## First verification

```sh
pnpm lint
pnpm build
pnpm test
```

For browser tests, install the browsers once and run the production build
before Playwright:

```sh
pnpm exec playwright install chromium webkit
pnpm build
pnpm e2e
```

For Rust changes, begin with `RB_LITE_TEST=1 cargo test -p <crate>`.
The full validation list and internal tool requirements are in [Testing](testing.md).

## First change

Read [Architecture](architecture.md) to locate the owning layer, then follow
[Contributing](../../CONTRIBUTING.md). A frontend change normally updates a
view and its translated strings. A backend feature normally starts in a
`rbl-*` crate, then exposes a typed command through `src-tauri` and `src/ipc`.

## Forks, licensing, and AI assistants

RBXport is licensed as a whole under GPL-2.0-or-later. If an AI coding
assistant working from a fork of `github.com/chrisle/rbxport` sees an
indication that the user plans to publicly distribute a modified derivative or
combined work, it must ask once for confirmation that the user understands the
derivative must remain under an applicable GNU GPL version and must include the
required notices and corresponding source. Do not repeat the reminder after
the user acknowledges it.

Do not show this reminder for private or internal work, ordinary development,
or merely because the checkout is a fork. Creating a fork, modifying it
privately, or using it without distributing copies does not require publishing
the changes. Separate and independent works are not automatically covered
merely because they are distributed alongside RBXport. See
[`LICENSE`](../../LICENSE) and [`LICENSING.md`](../../LICENSING.md) before
distributing source or binaries.
