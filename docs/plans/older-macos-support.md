# Older macOS support implementation and maintenance plan

Date: October 15, 2026. Related ticket: [#145](https://github.com/chrisle/rbxport/issues/145). Related regression: [#247](https://github.com/chrisle/rbxport/issues/247).

## Outcome and support contract

Target **macOS 10.13.6, fully patched with Safari/WebKit 13.1.2, on Intel x86_64**. Retain **macOS 11.0+ on Apple Silicon**. This is the oldest practical target while retaining Tauri 2 and its custom-protocol IPC architecture. Supporting 10.12 or earlier would require a different shell/transport project; it is outside this plan. Do not promise all installations of 10.13: the required OS patch and WebKit versions are part of the contract.

Keep one product, one frontend source tree, and the existing separate Intel/ARM downloads. Prefer baseline-compatible implementations everywhere, with narrowly scoped capability fallbacks where necessary. Do not create a permanently frozen, unmaintained legacy branch.

Support means installation, visible usable windows, library operations, playback, analysis, export, LINK, backup/restore, and secure updating pass the gates below. A live process, successful cross-build, or modern Playwright run alone is insufficient. Modern features may have cosmetic fallbacks; correctness, accessibility, data integrity, and transport security must remain intact.

The configured release minimum stays 11.0 until the compatibility candidate passes. This request authorizes planning; it does not change the issue's HOLD state or publish a lower support claim. The October 12 decision in #145 explicitly permits future research/implementation without another product decision.

## Evidence and current gaps

Repository inspected at `96921d33ea3814a0da0af6549c48898e4b87e313` (local working tree, not a claim about every published artifact).

| Finding | Evidence | Implication |
| --- | --- | --- |
| [OBS] Configured minimum is 11.0 | `src-tauri/tauri.conf.json` | Current declared floor, not proof all features work on 11 |
| [OBS] Vite targets Safari 16 / Edge 110 | `vite.config.ts` | Lowering bundle metadata cannot make the frontend compatible |
| [OBS] Modern runtime calls exist | `.at`, `Object.hasOwn`, `crypto.randomUUID`, `replaceAll`, observers in `src/` | Inventory and bootstrap compatibility before application evaluation |
| [OBS] Modern styling and native dialogs exist | `src/styles/reset.css`, player/device/sync CSS; multiple `<dialog>` users | CSS and modal behavior require structural work |
| [OBS] Intel smoke uses Rosetta on an ARM host, debug shell, ten-second process check | `.github/workflows/validate-macos.yml` | Does not verify old WebKit, a visible UI, or an actual Intel CPU |
| [OBS] Updater 2.11.0 uses reqwest 0.13.4; lockfile includes platform verifier | `Cargo.lock`, `src-tauri/Cargo.toml` | Re-resolve features and inspect the produced binary |
| [OBS, historical] #145 reports a 10.13 release cross-build and a `SecTrustEvaluateWithError` dependency | Issue investigation comments | Useful lead; rebuild and verify current lockfile, do not treat as new runtime evidence |
| [UNKNOWN] Actual 10.13 launch, request bodies, IPC, rendering, playback and updater behavior | No target runtime run performed for this plan | First milestone is a feasibility experiment |

Do not carry forward the earlier comment's claim that Catalina needs only a one-line change: current syntax, CSS, runtime APIs, dependencies and runtime tests must all pass there too. Safari 15.4 alone also does not establish support for every API mentioned in that comment. Do not copy its old occurrence counts; create a current machine-generated inventory excluding tests/mocks and distinguishing custom methods from built-ins.

## 1. Prove the native shell and transport before the UI port

Deliver an isolated candidate configuration and reproducible probe report. Build on a maintained macOS host with the locked toolchain/dependencies. Old test machines consume artifacts; they need not run Node, pnpm, cargo, or GitHub Actions.

1. Establish an Intel Mac test host with a logged-in GUI session. Provision a clean 10.13.6 guest with verified WebKit 13.1.2 and snapshot it; record OS build, Safari/WebKit bundle versions, CPU, RAM and graphics configuration. Verify guest behavior in WKWebView, not just Safari's version string. Use Apple-provided installation media. Inventory existing hardware before choosing virtualization software; the 2017 MacBook mentioned in #145 is a lead, not verified available infrastructure.
2. Use an isolated config, application-data directory, temporary fixture library and disposable export destination. Audit the test-mode/path handling before launch: neither `RBXPORT_TEST=1` nor XDG variables alone prove all macOS library paths are redirected. Use `RB_LITE_TEST=1` and `rbl_db::fixture::build` for write tests; fail closed if a path resolves into the installed rekordbox library.
3. Explicitly build x86_64 with deployment target 10.13 and candidate bundle minimum 10.13.6. Keep ARM at 11.0. Verify the actual Info.plist, Mach-O minimum versions, architecture and every bundled executable/dylib; match the published minimum to what the artifact really requires.
4. Inventory Cocoa/Objective-C selectors, framework imports, SQLCipher/OpenSSL, RubberBand/C++, audio, MIDI, drag/drop, window state, scripting, telemetry and updater dependencies. Inspect resolved Cargo features and C/C++ compiler deployment flags. Do not enable `target-cpu=native` or assume an AVX-capable CPU. Check transitive strong imports and delayed call paths; symbol inspection cannot establish runtime selector availability.
5. Ship a small baseline-compatible probe page in the candidate that avoids the normal frontend's modern syntax/CSS. Through the same `src/ipc/` and Tauri transport, test empty and nonempty commands, nested JSON, Unicode, null/false/zero values, representative large payloads, errors, cancellation, events/channels, and custom asset/audio requests. Compare payload hashes/lengths both ways and exercise the largest real command/event shapes. The probe must work even before React loads.
6. Trigger startup and manual update checks and telemetry error handling. Exercise audio and MIDI initialization, file dialogs and drag/drop. Collect native crashes, JS failures, screenshots, readiness assertions and structured probe results.

**Gate:** the exact release-profile native shell launches, shows a window and passes payload/asset probes without unavailable symbols or crashes on 10.13. Debug probes may help diagnosis, but release LTO can change reachable symbols and must also be tested. If request bodies fail, first test a narrowly scoped upstream fix or maintained backport with transport regression coverage. If no safe maintainable solution exists, report the blocker and test the next candidate OS; do not silently declare 10.15 supported or close the ticket just because one probe failed.

## 2. Remove newer-OS dependencies from all HTTPS paths

Preferred outcome: retain the updater with a maintained Rust TLS configuration and bundled roots that works on 10.13. Inspect the exact updater/reqwest feature graph before selecting flags. Cargo features are additive: adding a roots feature does not necessarily remove platform-verifier/default features enabled elsewhere. If configuration cannot remove the unsupported path, use a small reviewed upstream patch/backport rather than a broad downgrade.

Audit updater, Sentry, licensing/reporting and any other network clients independently. Verify all TLS paths and delayed symbol bindings on the target. A native OS-version check around automatic updates is insufficient if the unsupported symbol is strongly imported or another client still calls it.

Tests: valid production-equivalent certificate chain; hostname mismatch; expired and untrusted certificate; offline/timeout; redirect policy; cancelled and truncated download; insufficient disk space; invalid updater signature; successful download/install/relaunch from version N to N+1. HTTPS trust and updater signature verification are separate requirements and both must pass. Exercise automatic and manual entry points. Never disable certificate checks or signature verification. Document any enterprise trust-store behavior change if bundled roots replace platform trust.

Only consider disabling automatic updating as an explicit scoped fallback with a tested secure manual distribution path and truthful translated UI. It is not completion of the preferred full-support contract. Keep CA roots and TLS dependencies on the normal maintenance/update cadence.

## 3. Establish the frontend baseline and bootstrap order

Create a checked-in compatibility manifest (proposed `config/compatibility.json`) describing Intel OS/WebKit floor, ARM floor, build targets, approved shims, protected APIs, tests and owners. Configure Safari 13.1 as the compatibility baseline while preserving existing Windows/Linux requirements. Confirm the installed Vite/esbuild versions' target syntax; use a compatible Safari 13 syntax target if that is the supported spelling.

The production frontend must parse before it can polyfill anything. Inspect final output including dependencies, Tauri initialization, lazy chunks, workers if present, and the obfuscator's output. Keep custom protocol loading, CSP and production asset rewriting in the test. Vite syntax targeting does not supply DOM features or built-in methods. High Sierra's updated WebKit may support the required native module path; prove that first. Do not add SystemJS/legacy chunks automatically: evaluate Vite's legacy plugin only if the real module loader requires it, and validate CSP/protocol compatibility.

Refactor `src/main.tsx` into a minimal bootstrap that installs synchronous shims and awaits necessary DOM setup before dynamically importing the application entry. Its current static imports evaluate React/App/Sentry before entry-body code, so inserting a polyfill call below those imports is unsafe. Every secondary window and every alternate HTML entry must use the same bootstrap. Render a small baseline-compatible translated startup error with native logging if initialization fails; never leave a blank window.

## 4. Explicit polyfill and fallback policy

This is the initial inventory, not a promise that scanning a few strings discovers every incompatibility. Scan direct and transitive production code and validate each entry in the actual engine.

| Feature | Planned treatment | Required regression |
| --- | --- | --- |
| `Array.prototype.at`, `Object.hasOwn` | Narrow maintained `core-js` imports or small local replacements; feature detect and preserve native implementations | Negative/out-of-range indexes; inherited properties; null-prototype objects |
| `String.replaceAll` | Verify baseline implementation and app usage; add narrow shim only if needed | Unicode, literal metacharacters, replacement semantics |
| `crypto.randomUUID` | Shared helper using `crypto.getRandomValues`, correct UUID v4 bits; native method when available | Format/version/variant, missing-native branch; never use `Math.random` |
| `dialog.showModal` and backdrop | Shared dialog adapter; evaluate maintained dialog polyfill against actual UI, otherwise implement accessible modal primitive | Focus trap/restore, Escape, cancel/close, stacking, backdrop, nested scrolling, screen reader behavior |
| `inert` | Feature-tested maintained shim or equivalent shared modal isolation | Dynamic children, keyboard/pointer exclusion, aria-hidden restoration, mutation cleanup |
| `:focus-visible` | Visible `:focus` baseline; optional maintained modality shim | Keyboard focus always visible, mouse behavior acceptable |
| `ResizeObserver` | Verify native behavior; maintained shim only for demonstrated missing/broken cases | Virtual table resize, hidden-to-visible, window resize, observer disconnect/loop behavior |
| AbortController/signals, events, URL, TextEncoder/Decoder, Intl, canvas, clipboard, drag/file APIs | Inventory usage and third-party assumptions; test capabilities; narrow shim or native IPC adapter where necessary | Cancellation, locale output, binary/Unicode round trips, native file paths |
| Future APIs such as structuredClone, findLast, toSorted, newer Intl/Promise/AbortSignal methods | Deny unreviewed usage; add approved implementation and tests when introduced | Semantics and performance in missing-native branch and target WKWebView |

Avoid blanket polyfill bundles. Vendor/package all shims locally, pin with lockfile, review license/maintenance and account for size/CPU cost. No CDN scripts or `eval` exceptions. Never substitute an unsafe JSON clone for structured cloning or weak randomness for secure identifiers. Keep behavioral decisions in wrappers; keep native IPC behind `src/ipc/`.

## 5. CSS and interaction port

| Area | Implementation | Verification |
| --- | --- | --- |
| `@layer` | Use a verified build-time layer transform or carefully rewrite cascade; preserve layer precedence, unlayered rules and reversed important-rule priority | Computed-style snapshots and modern/old screenshots; simply deleting wrappers is unsafe |
| `:has()` | Express existing state as React classes/data attributes on the relevant ancestor | Pending artwork, disabled sync options, device layout states |
| `:is()` / `:where()` | Expand selectors or use proven transforms while preserving intended specificity | Overlap/override cases, not only screenshots of default state |
| Flex gap | Baseline margin wrappers/utilities or explicit grid where equivalent | Rows/columns, wrapping, hidden children, reverse order, RTL, edge spacing; CSS.supports(gap) cannot distinguish old grid-only support |
| `inset`, individual translate/rotate/scale | Expand edges and combine transforms with existing transforms in the proper order | Positioning, animations, zoom, overlays and hit targets |
| aspect ratio | Explicit dimensions or stable wrapper sizing | Artwork/device images at all supported scale factors |
| color-mix and newer colors | Compatible precomputed token/fallback before enhanced declaration | Themes, selection, disabled controls, contrast |
| accent-color, containment, newer overflow/viewport units | Native accessible control baseline and safe layout fallback; capability-specific enhancements | Checkbox/radio visibility, clipping, scrolling, waveform sizing |

Audit CSS Modules, shared reset/base, generated tokens, inline styles, SVG, dependency styles and native form controls. Prefix where required using the declared baseline. Build-time CSS targeting/autoprefixing alone cannot implement unsupported selectors or layout behavior. Extend existing token generation if sources change and commit generated outputs using the repository-required generation command.

Port all windows and states: main browser, both deck layouts, waveform/grid editing, track information, preferences, sync, device settings, missing files, backup/restore, reports, updates, empty/loading/error and long translated text. Keep localization through `useTranslation()`. Preserve backend sorting/filtering/search in `rbl-index`.

## 6. Test infrastructure and critical acceptance matrix

Modern host orchestrates copying artifacts and driving the old GUI over a restricted authenticated channel. Extend the existing debug-only test-port approach for diagnosis. Release verification must use the real signed release candidate without a production-accessible remote test endpoint; use external accessibility/UI automation and observable state/files/logs. Require a logged-in GUI session, explicit readiness, bounded timeouts and screenshots on failure. A headless SSH process check is insufficient.

| Test | Exact success criterion | Cadence |
| --- | --- | --- |
| Clean install and launch | Quarantined downloaded DMG mounts; signed/notarized app copies and opens; first usable window and IPC-ready assertion succeed | Every candidate on 10.13 and modern Intel/ARM |
| Window recovery (#247) | Clean and saved off-screen/oversized/minimized state yields visible usable windows; changed display, reopen and secondary windows work | PR regression suite; real Big Sur and floor guest before release |
| IPC/assets | Payload round trips match; events, errors, cancellation, artwork/audio routes work under production CSP | Every floor candidate |
| Library integrity | Fixture read/write/playlist/cue/grid edits persist exactly; failures leave valid DB, backups and recovery state | Rust suite plus floor integration |
| Playback and analysis | Supported formats load; two decks/cues/seek/tempo/grid timing remain within existing correctness tolerances; no panic or unexplained dropouts | Floor physical Intel before support claim and relevant changes |
| USB publication | Disposable FAT32/exFAT/HFS+ destinations; Unicode/long names; full/removed device; cancellation and interrupted publication recover without corruption | Release integration; real disposable media where filesystem/device behavior matters |
| LINK/MIDI/audio devices | Discovery, browse/load and transfers; reconnect, interface changes, audio-device change, sleep/wake and MIDI initialization work | Network fixtures each candidate; hardware verification for affected paths |
| Backup/restore | Backup verifies; isolated restore yields expected fixture data; interruption recovery succeeds | Every floor candidate |
| Accessibility and dialogs | Keyboard can reach/operate controls; modals isolate background and restore focus; native dialogs and VoiceOver work | UI changes plus release checklist |
| Secure update N→N+1 | Correct version installs/relaunches; malformed signatures/TLS failures never install; old OS receives only compatible artifact | Every release candidate |
| Sustained operation | One-hour two-deck + browse + background-analysis workload has no crash, deadlock or monotonic resource leak | Before support claim and scheduled regression |

Use reproducible small/10k/100k-track fixture catalogs and rights-cleared audio, with content hashes. Measure cold start to usable library, search/scroll/input latency, waveform frame times, peak/resident memory, analysis throughput, export throughput and audio underruns. Before implementation, freeze a reference Intel machine and numerical budgets in the existing performance policy; proposed starting targets are p95 input response under 100 ms, p95 interactive frame time under 33 ms during normal browsing, and zero audio dropouts during the controlled sustained-playback run. Validate feasibility in the initial benchmark and record any approved adjustment before final acceptance. Compare modern systems to their own pre-port baselines using existing performance gates. VM graphics/virtual audio results are functional evidence; they cannot establish physical audio latency or old Intel performance.

Runtime matrix: mandatory 10.13.6/WebKit 13.1.2 Intel floor, 10.14.6 Intel, 10.15.7 Intel, Big Sur Intel, oldest supported ARM environment, and current macOS on Intel where supported/ARM. Intervening supported releases need at least install/launch/IPC/update smoke; maintain representative UI coverage across WebKit versions. A maintained physical Intel machine verifies audio, USB, CPU instruction baseline and graphics. Rosetta remains an additional architecture smoke, never a replacement for the floor guest. Modern Playwright WebKit is an additional regression tool, never a Safari 13 emulator.

## 7. CI and permanent prevention

1. Add ESLint compatibility checks and Stylelint unsupported-feature checks tied to the shared manifest. Review rules against known false negatives: flex-gap support, selector specificity, property/value combinations and dynamic access. Allow exceptions only with an adjacent fallback, regression test and documented rationale.
2. Scan final assets as well as source: syntax parsing against baseline, unsupported built-ins/dependency code, CSS transforms, lazy imports and post-obfuscation output. Unit tests remove configurable native APIs or inject capabilities to force fallback branches; they supplement real-engine tests.
3. Add artifact audit for per-architecture minimums, linked libraries/symbols, CPU flags, bundle metadata, notarization/signing and updater manifest architecture/version routing. A clean scan is evidence, not a substitute for execution.
4. Extend `scripts/ci/validation-plan.mjs`, `scripts/ci/test-tiers.mjs`, reusable validation workflows and their tests so frontend, CSS, lockfile, native dependency, IPC, packaging and updater changes schedule compatibility lanes. Missing/offline floor infrastructure makes a required check pending/failed, never a silent success.
5. Run quick floor install/launch/readiness/IPC/window tests for affected PRs; run full functional/visual matrix nightly and on release candidates; run physical device suite before the first claim and when relevant subsystems change. Cache by artifact hash and environment identity, not just branch name.
6. Pin supported build toolchain/SDK and retain reproducible runners. Every dependency upgrade must pass compatibility checks; prefer maintained upstream fixes. Do not hold vulnerable dependencies indefinitely to retain an OS claim. If support becomes unsustainable, make a deliberate published support-policy change and prevent incompatible automatic updates.
7. Store artifact SHA, source commit, lockfile hashes, OS/WebKit builds, test outcomes, screenshots, crash logs and performance data in a compatibility report linked from each release. Record explicit skips as incomplete coverage.

Before merging substantial implementation, run the applicable repository checks:

```sh
cargo clippy --workspace --all-targets -- -D warnings
RB_LITE_TEST=1 cargo test --workspace
pnpm lint
pnpm build
pnpm test
pnpm budget
pnpm e2e
```

Add compatibility lint/asset audit to existing commands or required workflow jobs. Keep Windows and Linux validation: shared bootstrap/polyfills/CSS must not regress those platforms.

## 8. Delivery order, estimates and release gates

Estimates are engineering effort, not a delivery promise; provisioning and upstream fixes can extend elapsed time.

| Slice | Deliverable / exit gate | Estimate |
| --- | --- | --- |
| A: inventory and lab | Current API/CSS/dependency inventory, fixture isolation, working Intel floor guest, baseline measurements | 2–4 days |
| B: feasibility and TLS | Release-profile launch, IPC/assets and secure HTTPS on floor; native blockers resolved or precisely documented | 2–5 days |
| C: compatibility foundation | Shared manifest, minimal bootstrap, approved polyfills/adapters, static checks and artifact scan | 2–4 days |
| D: UI/layout migration | Cascade/selectors/flex layout/modal accessibility work, every-window visual report | 4–8 days |
| E: functional and performance | Full target matrix, physical Intel tests, recoverability and updater N→N+1 | 3–6 days |
| F: release integration | Required CI lanes, signed candidate gates, accurate docs/feed/download metadata and evidence report | 1–3 days |

Planning range: **14–30 engineering days**, plus hardware provisioning and any native/WebKit backport work. Use independently reviewable topic PRs, rebased onto `dev` with linear integration. Keep compatibility configs experimental until the release gate. An interim Mojave/Catalina floor can ship only after its own matrix passes; it is not required to reach High Sierra.

Release acceptance requires all mandatory matrix rows green for the exact candidate artifact; safe library isolation; proven secure upgrade path; acceptable physical Intel performance; no unexplained blank windows or payload loss; and maintained CI ownership. Then lower the Intel release minimum, retain ARM minimum, update README/system requirements/release notes and distribution metadata together, and run final artifact checks again. Verify download/feed hashes and installation of the published artifact; distinguish candidate validation from public release evidence.

If final packaging/signing changes any bytes after an earlier test, revalidate the final signed artifact. If compatibility is blocked, keep the current supported floor and record the exact failing layer plus next candidate to test. Never let an old system auto-update to a release it cannot run; retain signed last-compatible artifacts and version-aware update routing if the floor rises later. Application rollback must account for database/schema compatibility: do not blindly downgrade a modified library.

## References

- [Ticket and historical dependency investigation](https://github.com/chrisle/rbxport/issues/145).
- [Tauri macOS bundle baseline and minimumSystemVersion](https://v2.tauri.app/distribute/macos-application-bundle/).
- [Rust Darwin platform support](https://doc.rust-lang.org/rustc/platform-support/apple-darwin.html).
- [Apple WKURLSchemeHandler API](https://developer.apple.com/documentation/webkit/wkurlschemehandler).
- [Apple Safari 13.1.2 availability](https://support.apple.com/en-by/103116).
- [Vite build target limitations](https://vite.dev/guide/build).
- [Tauri updater TLS feature documentation](https://docs.rs/tauri-plugin-updater/latest/tauri_plugin_updater/).

Official documentation describes upstream capabilities, not proof that this dependency-locked app works on an old OS. No runtime compatibility tests were performed and no implementation, ticket state, release setting or support claim was changed while preparing this plan.
