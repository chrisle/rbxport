# Product decisions

## Review and execution policy

Routine investigation, defect correction, verification, documentation, and
execution of recorded decisions are accepted by default. They do not require
`needs-review` or another maintainer approval merely because technical work
or evidence is incomplete. Apply this rule across the backlog, not only to
the specific categories listed below.

Only a genuinely unresolved product decision requires a new maintainer choice.
State the exact choice and established evidence under `needs-product-decision`.
An unknown cause, missing reproduction, inaccessible diagnostic archive,
unfinished acceptance check, or failing CI is not by itself a product decision.
Investigate those constraints and carry out the existing scoped work.

Preserve explicit holds, research-only restrictions, ownership, normal PR
review, safety boundaries, and recorded product decisions. This policy does
not approve a new feature outside scope, override an explicit visibility
restriction, waive failing checks, or grant release/deployment or reporter-
messaging authorization. Keep unresolved work open and verify actual completion.

## Rekordbox-only scope

RBXport aims to be a fast Rekordbox clone with a deliberately limited set of
features. It trades feature breadth for speed and responsiveness, including on
older machines. The core focuses on library management, USB exporting, and
PRO DJ LINK; it does not aim to become a full DJ performance application or
an advanced library manager beyond what Rekordbox offers. A Rekordbox
counterpart is necessary for a new feature, but does not guarantee inclusion.

The project also aims to provide a useful foundation for others to fork and
create their own DJ software. Forks can expand the product in other directions
while the core stays focused and fast.

RBXport is solely for Rekordbox. Integrations, library import/export, and export
databases for other DJ software, including Mixxx and Engine DJ, are outside
scope. An opt-in setting or shared audio files does not change that decision.
Existing safety, correctness, and compatibility work remains necessary.

## macOS Big Sur support

macOS Big Sur is not yet supported. Reports specific to Big Sur are outside
the current support scope; do not describe them as fixed without a verified
correction. This does not rule out future support. Reassess those reports if
Big Sur support is introduced.

## Vocal and phrase detection

RBXport will not add vocal or phrase detection, including custom vocal/phrase-marked
export waveforms. It should remain useful on older machines.

Bundling or downloading AI models adds size and resource costs. Commercial
licensing requirements for suitable separation models would also complicate
keeping RBXport open source. These concerns govern the decision regardless of
how AlphaTheta packages or prices the features.

Existing analysis support and phrase data already present in a library remain
within the existing compatibility scope.

## Advanced metadata management

RBXport will not add an advanced metadata cleanup suite: batch find/replace,
automatic capitalization, conditional transformations, whitespace cleanup,
or automatic artist/title splitting. This functionality is outside the
Rekordbox feature scope RBXport aims to reproduce.

For advanced library and database management, we recommend the excellent
[Lexicon DJ](https://www.lexicondj.com/) app. Ordinary metadata editing remains
within RBXport's existing scope; a Lexicon integration does not.

## USB database errors

USB database errors go directly to investigation and repair. They do not
require a product-review decision and must not be flagged `needs-review`
merely because their cause, schema, reproduction, or fix is not yet known.

Inspect diagnostics, trace the relevant read/write paths, reproduce with
disposable fixtures, and implement and validate the established correction.
When a possible fix is already merged, verify coverage before closing the
report. An inaccessible archive or incomplete reproduction is an investigation
constraint, not a reason to ask for routine permission or declare the error fixed.

Preserve existing ownership, explicit holds, library safety, and normal PR
review. If research reveals a genuine new product choice, isolate that choice
under `needs-product-decision`; continue independent defect investigation.

## Drag-and-drop failures

Drag-and-drop failures go directly to investigation and repair without
`needs-review`. Missing reproduction details, an unknown platform-specific
cause, or uncertainty about the failing transfer path are investigation work,
not product-review blockers.

Trace the source payload, destination, coordinate handling, import or membership
operation, and resulting view as relevant. Reproduce and validate the established
correction, keeping distinct symptoms separate until a shared cause is proven.
Preserve every concern in reports that also describe other failures.

This applies to broken interactions within the agreed product scope. A proposal
for a new drag-and-drop capability still follows the Rekordbox-only scope rule.
Preserve existing ownership, explicit holds, library safety, and normal PR review.
Isolate genuine new product choices under `needs-product-decision` while
continuing independent defect investigation.

## Startup, file access, platform behavior, preferences, and saved settings

Startup failures, file-access errors, platform failures, preferences, and saved
settings do not require `needs-review`. Investigate and correct failures within
the supported product scope directly. Missing diagnostics, platform-specific
reproduction gaps, or uncertainty about persistence are investigation work.

For preferences and saved settings, establish the corresponding Rekordbox
behavior and match it within the agreed feature subset. Verify existing merged
fixes before closing reports. Apply recorded scope exclusions and unsupported
platform boundaries rather than inventing new capabilities or support promises.

Preserve each concern in multi-symptom reports, existing ownership, explicit
holds, library safety, and normal PR review. Isolate a genuinely new product
choice under `needs-product-decision` while continuing independent investigation.
