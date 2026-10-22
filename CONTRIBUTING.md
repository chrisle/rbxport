# Contributing

Start with [Getting started](docs/development/getting-started.md), then read
[Architecture](docs/development/architecture.md) and
[Development conventions](docs/development/conventions.md).
The [documentation index](docs/README.md) links to feature-specific references.

## Report or choose an issue

Use the GitHub bug or feature request form in `.github/ISSUE_TEMPLATE/`.
For a bug, provide reproduction steps, expected and actual behavior, app
version, OS, and exact hardware/firmware when relevant. Attach a redacted log
excerpt or report. Keep credentials, private databases, and unrelated personal
data out of issues and the public repository.

For a substantial change, explain the problem and proposed behavior before
implementation. Keep the change scoped so a reviewer can understand its
trigger, result, and validation without reading a chat history.

## Feature proposals

rbxport aims to be a fast rekordbox clone with a deliberately limited feature
set. It trades feature breadth for speed and responsiveness, including on older
machines. The core focuses on library management, USB exporting, and PRO DJ
LINK, reproducing rekordbox behavior within that scope.

The core does not aim to become a full DJ performance application, support
other DJ software, or provide advanced library management beyond what
rekordbox offers. An established rekordbox counterpart is necessary for a new
feature, but does not guarantee inclusion. Check the
[product decisions](docs/development/product-decisions.md) before proposing work.

Providing a useful foundation for forks is also a project goal. You are welcome
to use rbxport to build your own DJ software with broader features or a
different focus. Keep reusable components clear and maintainable without
expanding the core product's scope to accommodate every fork.

Every contribution that adds a feature is carefully considered against this
vision, and a working, well-tested feature can still be declined. Before writing
code, open an issue that explains the problem and why it belongs in the core
project. Bug fixes, performance improvements, compatibility work, and simplifying
internal code are always in scope.

A change must also respect two commitments:

- **Compatibility.** rbxport aims to be fully compatible with the current
  version of rekordbox and all the hardware that rekordbox supports. A change
  must not break existing libraries, USB exports, or supported devices.
- **Performance.** rbxport maintains its speed through the performance budget
  in `perf-budgets.json`. A change must stay within it; run `pnpm budget` for
  interface changes. Do not raise a budget gate to fit a new feature. See
  [Development conventions](docs/development/conventions.md).

## Create a branch

Branch from the target branch for the change, normally `dev`:

```sh
git fetch origin
git switch -c fix/short-description origin/dev
```

Preserve unrelated working-tree changes. Use a separate checkout or worktree
when another task needs a different baseline. Do not include generated logs,
private fixtures, or incidental version synchronization in the patch.

## Implement and validate

Use the owning layer, typed IPC, translated strings, and temporary fixtures
as described in [Conventions](docs/development/conventions.md).
Never write to an installed rekordbox library during development or tests.
Set `RB_LITE_TEST=1` (or `RBXPORT_TEST=1`) for Rust tests and supply fixtures
for write coverage.

Run the smallest relevant checks first. Before handing off a substantial
change, run the applicable checks in [Testing](docs/development/testing.md),
including frontend lint/build/unit/budget/Playwright checks for interface
changes and workspace Clippy/tests for Rust changes. Run affected internal
firmware suites when tools and firmware are available. Report unavailable
checks and unresolved device behavior explicitly.

Update the appropriate user guide or technical reference, along with generated
output when its source changed. Validate new documentation links and commands.

## Commit and open a pull request

Use Conventional Commit titles such as `fix: preserve playlist order` or
`feat: add export progress`. Write a concise, imperative subject and a short
body explaining what changed and why. Keep commits grouped by logical change.

Push the topic branch and open a PR against its target. The description should
state the problem, resulting behavior, relevant checks, and any material
limitations. Include issue references and separate local, CI, emulator, and
physical-device evidence. Public contributors do not need access to private
firmware or test assets.

Same-repository PR CI may commit machine-applicable Clippy fixes and missing
English locale fallbacks to the branch, then validate that updated SHA.
Inspect those changes and use the newest PR head when reporting validation.
Fork PRs are excluded from self-hosted runner execution by the workflow;
a maintainer must arrange appropriate validation before integration.

## Review and integration

Address review findings and rerun checks affected by the updates. Keep `main`
and `dev` linear: rebase the topic branch onto its target, then integrate with
a fast-forward. Do not create merge commits between them. A rebased topic
branch may be force-pushed with `--force-with-lease`, including one with an
open PR. Never force-push `main` or `dev`, and never move or replace a
release tag.

Opening or merging a PR does not publish a release. Release operators follow
[Releases](docs/development/releases.md) from a clean, pushed `dev` checkout.
