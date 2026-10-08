# Releases

[Documentation](../README.md) · [Contributing](../../CONTRIBUTING.md)

The release entry point is `npm run deploy` from a clean, pushed `dev` checkout.
A normal push does not publish a release. Read the deployment script and
workflow before operating them; they create tags, promote branches, publish
installers, and submit a Microsoft Store update.

## Versions and release notes

The version is recorded in `Cargo.toml` (`[workspace.package]`), `package.json`,
and `src-tauri/tauri.conf.json`. `node scripts/sync-version.mjs --version X.Y.Z`
updates these files and workspace crate versions in `Cargo.lock`.
`release-notes.json` contains app-visible notes prefixed with `(New)`,
`(Improved)`, or `(Fixed)`.

`pnpm dev` first synchronizes the version from the newest merged `v*` tag.
It can reset an untagged version bump, so review incidental version changes
before committing them. Existing release tags are immutable.

## Validation and release sequence

Same-repository PRs to `dev` and `main` use `.github/workflows/ci.yml` to call
the release workflow in validation-only mode. CI can commit machine-applicable
Clippy fixes and missing English locale fallbacks before validating the updated
PR SHA. Fork PRs do not run on the self-hosted fleet.

The release validation gate covers Rust tests and Clippy, the Rust 1.89 minimum
supported version, Windows compilation and desktop checks, Linux window smoke,
and frontend lint/build/unit/budget/Playwright and Intel macOS runtime checks.
See [Testing](testing.md) for local checks and their evidence boundaries.

The deployment script checks the clean, pushed `dev` tip and that `main` is
its ancestor. It asks the local Codex CLI to curate notes from the release diff
with read-only checkout access, attaches the result to the source commit as a
git note (`refs/notes/release-notes`), then dispatches the Release workflow,
which reads the note (a dispatch input would be dropped while `main` does not
declare it) and falls back to commit-derived notes when there is none. To give
the curator extra instructions for one release, run
`npm run deploy -- --notes-instructions "…"` or set
`RELEASE_NOTES_INSTRUCTIONS`.

After validation, the workflow records the version and notes on `dev`, creates
an immutable tag, and fast-forwards `main`. Installer jobs build from that
promoted commit. Validation failure creates no release metadata or tag.

Installers and update feeds are published to R2; the workflow does not create
a GitHub Release. Payloads precede the `latest.json` update. When `CDN_BASE_URL`
is configured, publication checks the live feeds' versions and installer URLs.
Only after publication succeeds does it prune old versioned artifacts, keeping
the five newest versions, and announce the notes on Discord.

When `PACKAGING_NIX` is set, the publish job also writes `packaging/nix/pin.json`
(the published Linux AppImage's version and SRI hash) and pushes
`chore(nix): seed <version>` to `dev`, so the Nix package seeds that release.

## Microsoft Store

The `store-submit` job runs after the release publication job succeeds. It
downloads the same run's Windows artifact and verifies the MSIX identity,
publisher, x64 architecture, and language list before submission.

The `microsoft-store` environment supplies `MS_STORE_PRODUCT_ID` and Partner
Center credentials. The job configures the Microsoft Store Developer CLI and
publishes the package for certification, then queries submission status.
Certification is asynchronous: successful submission does not establish that
the update is available in the Store. The former manual draft workflow is
replaced by this release job.

## Operation and verification

Read `node scripts/deploy.mjs --help` for supported inputs. `SKIP_TESTS=true`
skips validation; `SKIP_VERSION_BUMP=true` reuses the current tagged version.
These options do not establish that omitted checks passed.

Report local checks, CI, public-download verification, and Store certification
separately. A workflow dispatch is the start of a release; confirm the exact
published version and downloads before reporting completion.

Production frontend builds obfuscate app JavaScript and omit source maps.
Development builds remain readable. Obfuscation provides no protection for
secrets; keep credentials out of the frontend.
