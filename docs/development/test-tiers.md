# Test tiers

Which checks run, and when. [Testing](testing.md) says what each check proves;
this page says who runs it so the same work is not repeated and no check is
left to chance.

The tiers are defined in `scripts/ci/test-tiers.mjs`. A test
(`scripts/ci/test-tiers.test.mjs`) fails when a validation lane in
`scripts/ci/validation-plan.mjs` cannot be reached, never runs in full, or is
missing from this page.

## Tiers

| Tier | Status | Trigger | Runs |
| --- | --- | --- | --- |
| `local` | active | While working, by a person or an agent | Scoped commands only: `cargo test -p <crate> [filter]`, `pnpm vitest run <paths>`, one `pnpm e2e <spec>`. |
| `pull-request` | active | Each push to a pull request. A newer push cancels the older run. | The lanes the path plan selects, with Rust limited to the affected packages. |
| `integration` | planned | Push to `dev`; only the latest tip runs | Full Rust workspace and full frontend on Linux. Catches two pull requests that are green alone and broken together. |
| `nightly` | planned | Scheduled, only when `dev` changed since the last green run | The cross-platform lanes: Windows, Intel macOS, Linux window. |
| `release` | active | The Release workflow | Every lane, plus packaging, signing and installer checks. |

Only `active` tiers count as coverage. Until `integration` exists, the first
full run of a `dev` tip is the release, so a bad merge can wait that long to
be found.

Hardware, firmware and private suites (`rbxport-private`) are manual in every
tier. See [Testing](testing.md#private-suites).

## Lanes

Each lane is one output of the path plan. The pull-request tier runs a lane
only when a changed path selects it; the release tier runs all of them.

| Lane | Check |
| --- | --- |
| `frontend_lint` | `pnpm lint` |
| `frontend_typecheck` | TypeScript type check |
| `frontend_build` | `pnpm build` |
| `frontend_unit` | `pnpm test` |
| `frontend_e2e` | `pnpm e2e` |
| `frontend_budget` | `pnpm budget` |
| `scripts` | `node --test` for `scripts/` and workflow linting |
| `rust` | Clippy and `cargo test` on Linux |
| `windows_rust` | The Rust checks on Windows |
| `windows_file_drop` | Dropping files from Explorer on Windows |
| `linux_window` | The app window on Linux |
| `macos_intel` | The Intel macOS build |

## Rules

- **Do not repeat a tier.** An agent or person working a change runs `local`
  checks. The pull request runs `pull-request`. Nobody re-runs the whole
  workspace by hand to confirm what CI is about to run, except for a change
  CI cannot see (a fixture, a hardware path, a private suite).
- **A new lane is a decision.** Add it to `validationKeys`, give it a path in
  the plan, run it in the `release` tier, and list it above. The test fails
  until all four are done.
- **Skipping is explicit.** `skip_tests` on a release is for a source that was
  already validated in full, and the release notes should say which run.
- **A superseded run is wasted work.** The pull-request workflow cancels the
  older run for the same pull request when a new push arrives. A force-push or
  rebase therefore costs one run, not two.

## Not built yet

These are agreed but not implemented. Each is its own change.

1. **`integration` on push to `dev`**, debounced so only the latest tip runs.
   This adds runner load, so it waits for the disk and caching work.
2. **`nightly`**, gated on `dev` having changed.
3. **Releases that reuse a recorded pass** for the same source tree, so
   `skip_tests` is verified and not a checkbox.
4. **One compile for Clippy, its autofix and the tests.** Today the autofix
   job, the Clippy step and the test step each build the workspace cold on
   separate ephemeral runners.
5. **Clippy and tests as parallel jobs**, and a smaller CI build profile
   (`CARGO_PROFILE_DEV_DEBUG=0`) with `cargo test` skipping examples.
