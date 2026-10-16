# Development conventions

[Documentation](../README.md) · Next: [Testing](testing.md)

Follow the neighboring code's layout and naming. The enforced rules live in
`Cargo.toml`, `eslint.config.js`, and `tsconfig.json`; keep changes focused
instead of reformatting unrelated files.

## Rust

- Keep Tauri commands and native app integration in `src-tauri/`. Shared `rbl-*` crates remain independent of Tauri.
- Return contextual errors. Workspace linting forbids `unwrap`, `expect`, and `panic!` outside tests unless a file explicitly opts out; do not add an exemption to hide a production failure.
- No panic may cross the IPC boundary. Use the app's error conversion and blocking-worker patterns when exposing crate operations.
- Avoid holding locks across `await`. Workspace Clippy also rejects unnecessary ownership, cloning, large enum variants, debug macros, and stdout printing.
- Keep database mutations in `rbl-db` and route app edits through `AppState::write_then` and refresh/event handling.
- Preserve publication and recovery behavior when changing files. A successful write to a staging file is not proof that final publication succeeded.

Clippy's performance lints are denied, its pedantic group is enabled, and
validation treats warnings as errors. Before changing Rust formatting across
a file, check its existing style; the repository has no dedicated rustfmt
configuration or formatting gate in the documented validation commands.

## TypeScript and React

- Use strict types. `tsconfig.json` enables unchecked-index protection, exact optional properties, and unused symbol checks.
- Put backend calls in `src/ipc/`. Views use the typed backend interface; do not import Tauri `invoke` into them.
- Add the corresponding mock behavior when extending the backend interface, so browser development remains useful.
- Sorting, filtering, and searching the library belong in `rbl-index`. Render fetched row windows rather than transforming the collection in React.
- Use events and observers for updates. ESLint forbids `setInterval` polling and deep-cloning row arrays.
- Follow React hook dependency rules. A leading underscore marks an argument or variable deliberately unused by a required signature.
- A keyboard action is a command. Register what it does with `useCommands` (`src/lib/commands.ts`) in the component that owns its state, and have the key handler, menu item and any other entry point call `runCommand`; a key handler only decides whether a press is the command. See [Keyboard commands](../reference/shortcut-commands.md).
- Use `console.warn` or `console.error` for meaningful diagnostics; frontend linting rejects ordinary console output outside test exceptions.

## User-facing strings and generated files

Use `useTranslation()` from `src/i18n` for all user-visible strings. Inspect
existing keys before adding new ones. `pnpm locales` rebuilds
`public/locales/` from installed rekordbox `.lang` files; the script's
`--translate-missing` option handles missing translations. CI can add English
fallbacks, which keep the catalog complete but are not native translations.

Commit generated output when its source changes. The local agent guide also
calls for `pnpm tokens` or `pnpm icons` after token/icon source changes;
those commands are not currently in the public `package.json`. Locate the
owning generator in the relevant checkout rather than documenting or running
an unavailable public command.

## Performance

`perf-budgets.json` distinguishes measured baselines, enforced gates, and
manual targets. `pnpm budget` checks budget schema and production bundle size;
`npm run perf:gate` runs isolated browser frame checks.
`npm run perf:measure` builds and measures the production preview.

Do not materialize the full library in the frontend, add idle polling, or
silently loosen a budget to make a regression pass. When rebaselining, record
the environment, command, observed result, and reason.

## Formats and protocol evidence

Keep `[OBS]`, `[ASSUME]`, and `[UNKNOWN]` accurate. Identify whether evidence
comes from a library diff, packet capture, static firmware analysis, an
emulator, or a physical device. Static source behavior and a passing emulator
run are different evidence layers.

Use `rbl-difftool` and captures to establish rekordbox behavior before changing
format writers. A result for one firmware/model does not establish compatibility
with another. Technical references belong beside the code or in `docs/`;
private captures and detailed incident investigations belong in `rbxport-private`.

## Tests and comments

Test observable behavior at the smallest useful boundary. Use generated audio
and temporary database fixtures for writable tests. Add regression coverage
for a bug's trigger and expected result, rather than assertions that mirror
the implementation.

Comments explain a constraint, evidence source, or non-obvious decision.
Keep usage instructions short and executable. Update the relevant guide when
a command, interface boundary, or user behavior changes.
