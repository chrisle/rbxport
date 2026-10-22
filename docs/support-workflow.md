# RBXport support workflow

This is the contract for `#rbx-support`, GitHub Issues, pull requests, and
releases. GitHub Issues own the work items. Discord threads own conversations
with reporters. A thread may report several work items, and several threads may
report the same work item. Lexie and automation project verified work state
back to Discord; a Discord tag is never evidence that code was merged.

## Records and authority

| Fact | Authority | Projection |
| --- | --- | --- |
| A report and its conversation | Discord thread | Issue source link and Lexie cache |
| Ticket identity, duplicate disposition, assignment | GitHub Issue | Lexie acknowledgement |
| A PR that actually fixes a ticket | Explicit, reviewed issue ↔ PR link | Issue activity and status computation |
| Code on `dev` or `main` | Git merge commit ancestry | Issue status label and Discord tag |
| Publicly shipped code | Public `latest.json` version plus immutable `v<version>` ancestry | One release reply per linked Discord thread |

The GitHub issue is the durable home for Discord thread links. Lexie's database
indexes them for quick processing and delivery retries. A database row alone
must never be the only copy of a link. The issue keeps explicit links to every
associated thread; a thread can appear on multiple issues. A duplicate intake
issue links to its canonical issue, and its thread follows the canonical issue's
work state. Closing an issue as `duplicate`, `invalid`, or `wontfix` never means
the fix shipped.

## Status contract

Status is computed from work facts, then written to one canonical `status:*`
label and one Discord workflow tag. `needs-review` is a separate triage flag.
It does not mean the fix reached `dev`.

Under the [review and execution policy](development/product-decisions.md#review-and-execution-policy),
routine investigation, fixes, verification, documentation, and execution of
recorded decisions are accepted by default across the backlog. Do not route
technical uncertainty or unfinished work back to the maintainer using
`needs-review`. Reserve `needs-product-decision` for an exact, genuinely
unresolved product choice, preserving existing holds and scoped restrictions.

USB database errors bypass product review and go directly to investigation,
fixing, and validation under the [standing product decision](development/product-decisions.md#usb-database-errors).
Do not add `needs-review` for an unknown cause, unsupported schema, missing
diagnostics, or incomplete reproduction. This does not bypass explicit holds,
ownership, library safety, or normal PR review. A genuine new product choice
uses `needs-product-decision` for that choice alone.

[Drag-and-drop failures](development/product-decisions.md#drag-and-drop-failures)
also go directly to investigation, fixing, and validation without `needs-review`.
Missing reproduction details or an uncertain transfer path do not require
product approval. Proposals for new capabilities still follow the recorded
product scope; preserve unrelated concerns in reports with multiple symptoms.

[Startup, file-access, platform, preference, and saved-setting work](development/product-decisions.md#startup-file-access-platform-behavior-preferences-and-saved-settings)
also bypasses `needs-review`. Investigate failures and establish Rekordbox
behavior for settings directly. Apply existing product-scope and platform-support
decisions; this routing does not grant new capabilities or unsupported-platform
support, release explicit holds, or bypass normal PR review.

| Status | Required evidence | Discord tag |
| --- | --- | --- |
| Intake / triage | No verified active work or fix PR | No workflow tag |
| In progress | Assigned to Chris Le and an agent has accepted active code work | `in-progress` |
| Under review | A reviewed fix PR is merged to `dev`, and the fix is not in the current public release | `under-review` |
| Done | The verified fix is present on both `dev` and `main` | `done` |

`done` takes precedence when a fix is on both branches, including the short
interval before publication. Publication is a separate notification. An open PR,
issue closure, a PR that merely mentions an issue, a timestamp comparison, or a
manually set status label cannot establish merge or release inclusion. If a PR
fixes only part of a ticket, the ticket stays in its earlier state until the
remaining work has verified fix links. A thread with multiple active issues
shows the least advanced issue state; it reaches `done` only when all linked
work items are done. Each issue's status and release reply identifies its
number so the combined tag cannot hide partial progress.

## Events and recovery

1. Discord's thread creation event starts intake immediately. Lexie records a
   durable processing attempt, searches for likely existing issues, files an
   intake issue when needed, stores the thread link on the issue, and replies
   with the issue number. A failed acknowledgement is retried without creating
   another issue.
2. `triage-issues` decides whether the intake issue is new, a duplicate, or
   several distinct issues. Candidate matches are suggestions, never automatic
   duplicate closures. A confirmed duplicate links its thread to the canonical
   issue and tells the reporter where the work is tracked.
3. `take-tickets` records Chris's assignment and active agent work. The status
   projection then becomes `in-progress`.
4. `human-pr-review` and `review-contributor-prs` verify which issue each PR
   actually fixes. Merging to `dev` updates the issue and Discord to
   `under-review`; a PR reference without this decision does not promote it.
5. Fast-forwarding the verified fix to `main` makes it `done`. Once the public
   feed names an immutable tag containing the fix, Lexie posts a single
   versioned release reply to every linked reporter thread.

GitHub issue, PR, and branch events should trigger reconciliation promptly.
The release publisher triggers the publication pass only after checking the
public feed and artifacts. A periodic reconciliation pass repairs missed
events, Discord outages, and manual GitHub changes. All handlers are
idempotent: store the event/delivery key before retrying side effects, and
compare current tags before editing them. Preserve non-workflow forum tags.

## Operational invariants

- Use exactly one canonical label for each workflow state. Retire the old
  `status:in-review`/`status:under-review` split after migrating existing
  issues. Discord tag names remain `in-progress`, `under-review`, and `done`.
- Review links before promoting a ticket. A PR body that says “related” or
  “may cover” is not a fix claim.
- Verify `main` and the published tag independently. The public feed can lag
  behind `main`; issue status and release notification answer different
  questions.
- Reconcile every linked thread, including archived threads. Preserve its
  unrelated tags and restore archive state after a tag update.
- Keep a per-thread, per-issue delivery record for acknowledgements, status
  replies, and release replies. A failed Discord call remains pending.
- Report missing links and unverifiable fix claims as exceptions for triage;
  never guess a status from issue age or closure time.
