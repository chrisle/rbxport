# Documentation

New developers should follow the [developer reading path](../README.md#developer-reading-path).
Commands in these guides run from the repository root unless stated otherwise.

## Development

| Guide | Use it when |
| --- | --- |
| [Getting started](development/getting-started.md) | Setting up a checkout and choosing a safe run mode. |
| [Architecture](development/architecture.md) | Finding the owner of a feature or tracing a request. |
| [Conventions](development/conventions.md) | Writing code that fits the project. |
| [Testing](development/testing.md) | Choosing checks and interpreting their results. |
| [Contributing](../CONTRIBUTING.md) | Reporting an issue or preparing a change for review. |
| [Debugging](development/debugging.md) | Diagnosing failures and managing local output. |
| [Releases](development/releases.md) | Understanding versions, CI, and release operations. |
| [Sentry](development/sentry.md) | Configuring crash reporting for a build. |

## Using the app

- [Track analysis settings](user/analysis-settings.md): choosing stages and queuing a batch.
- [USB export](user/usb-export.md): compatibility conversion and playlist cleanup.
- [Backups](user/backups.md): backup contents, storage, and restoration.
- [Waveform scrubbing](user/waveform-scrubbing.md): drag behavior and its audio filter.
- [AppleScript](user/applescript.md): macOS automation, objects, commands, and examples.

## Technical references

- [Library location](reference/library-location.md): which rekordbox library opens, drive discovery, and missing drives.
- [USB export format](reference/usb-export-db.md): files, binary records, implementation, verification, and unknowns.
- [LINK behavior and hardware coverage](reference/link-testing.md): device dialects, behavior catalog, and physical checks.
- [Analysis crate](../crates/rbl-analysis/README.md): algorithm reading path and evaluation tools.
- [Waveform calibration](reference/waveform-analysis.md): measured overview behavior and its limits.
- [Playback crate](../crates/rbl-deck/README.md): thread model, code map, and test workflow.
- [Playback test fixtures](../crates/rbl-deck/tests/fixtures/README.md): generated audio and regeneration.
- [Licensing](../LICENSING.md): bundled library terms.

## Internal material

The sibling `rbxport-private` checkout contains firmware harnesses, private test
assets, captures, and investigations. Run registered suites through
`npm run tests:private` in this checkout. The private repository's README
explains its layout; its `verification/` directory holds local run evidence.
Keep credentials, installed library backups, and copyrighted test audio out
of the public repository.

## Brand assets

`assets/brand/` contains the app icons, graphics, wordmarks, combined logos,
and reference board. See its `README.txt` for format details. The centered
README logo uses the existing color/white lockup.
