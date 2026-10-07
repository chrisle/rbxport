# GitHub Actions layout

The entry-point workflows describe orchestration. Reusable workflows and local
composite actions own implementation details.

Pull requests use a focused validation profile selected from changed paths.
Manual full validation and the release workflow keep every input at its default
and therefore run the exhaustive cross-platform, browser, budget, MSRV, and
workspace test gates before deployment.

## Entry points

- `ci.yml` uses `scripts/ci/validation-plan.mjs` to classify changed files,
  applies deterministic fixes, and requests only the relevant validation lanes.
- `release.yml` resolves and records a version, validates it, promotes `main`,
  builds installers, publishes them, and requests Store submission.
- `label-issue-platform.yml` labels incoming issues independently of delivery.

## Reusable validation workflows

- `validation.yml` coordinates the aggregate validation gate.
- `validate-rust.yml` runs current-toolchain tests and the MSRV check.
- `validate-windows.yml` checks Windows-native compilation and file paths.
- `validate-linux.yml` proves the Tauri shell opens a native Linux window.
- `validate-macos.yml` proves the Intel build launches through Rosetta.
- `validate-frontend.yml` independently selects lint, build, unit, browser, and
  bundle-budget checks.
- `validate-scripts.yml` runs the dependency-free Node script tests.

On pull requests, Rust test and Clippy selection is narrowed by
`scripts/ci/affected-rust-packages.mjs` to changed workspace crates and their
reverse dependents. Deploy validation runs workspace-wide Clippy, tests, and
MSRV checks.

## Reusable release workflows

- `build-installer.yml` coordinates one platform package.
- `release-publish.yml` publishes and verifies the R2 feed, prunes old payloads,
  and announces the release.
- `release-store.yml` verifies and submits the MSIX to Partner Center.

Installer implementation is further divided under `.github/actions/` into
builder setup, signing setup, packaging, verification, and cleanup. Keep
platform-specific commands in those actions so the release state machine stays
readable and every cleanup path remains explicit.
