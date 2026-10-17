# macOS desktop regression tests

Run `pnpm desktop:test` in a logged-in macOS 14+ GUI session. The Apple Silicon
CI lane runs on `chris-m2-mac`. Requires Rust, Node/pnpm, the Swift compiler,
Finder, an audio output, and Accessibility/event-posting permission for the
runner. Missing input permission fails the suite; native cases are never
silently skipped.

`pnpm desktop:test --no-build` reuses the exact test binary checked by a SHA-256
stamp. `--expanded-only` is a development filter; CI always runs the full suite.
The main WKWebView is asserted to be **1200 × 800 logical pixels**, with the
normal app's overlay title bar. Test-only sizing and non-resizability do not
change the normal app configuration. Native drags put Finder beside the test
window; targets are measured from the current DOM and native window bounds.
An exclusive per-user lock prevents two desktop suites competing for input.

The embedded production frontend runs in a **debug Tauri shell**. Ordinary UI
cases dispatch DOM events through the existing debug-only loopback bridge, then
use real IPC/Rust/SQLCipher. Native drag cases use Accessibility to locate the
generated file in Finder and CoreGraphics to post mouse drags. They require
trusted native drag events, check the original imported path independently in
SQLCipher, and save event evidence. No DOM `File` or `DataTransfer` is fabricated.
Finder windows opened by the helper are closed; a reused existing window is
left open and its previous bounds are restored.

The full suite has 31 scenarios. Coverage includes:

- Default protection, hot and memory cues, keyboard editing, waveform/list
  refresh, repeated edits, playback, search, and full-process cue persistence.
- Playlist creation/rename and undo/redo; rating writes and undo/redo checked
  independently in the encrypted database.
- Finder drops of Unicode filenames, multiple files and nested folders;
  duplicate handling, cancelled drags, invalid targets and unsupported files.
- Missing audio refusal; analysis of deterministic 120 BPM audio with saved
  analysis artifacts; independent two-deck playback and beat-loop controls.
- Desktop-menu export and repeat export to a temporary volume directory, with
  independent parsing/verification of Device Library and OneLibrary.
- Preferences backup creation and ZIP integrity, cross-window Library Protection,
  and persistence of imported tracks, playlists, metadata and analysis.

Every run owns an encrypted fixture library, generated audio, state/backups and
export destination under a fresh temporary directory. `RB_LITE_TEST=1` and
`RBXPORT_TEST=1` remain enabled. Only the explicitly named, canonical temporary
fixture with the fixture key can write in a debug build. `RB_LITE_FAKE_VOLUMES`
replaces the device list with the temporary destination; no real disk is used.
A fresh UUID WebKit store is shared by test windows and reused across process
restarts. The first launch asserts that its preference store is empty.

Evidence goes to `desktop-test-results/<run>/`: scenario results, process logs,
native drag event JSON, parsed export read-back, and page HTML on failure when
the webview responds. The printed temporary fixture is retained for inspection.

Boundaries: DOM cases do not test physical pointer hit-testing or OS keyboard
accelerators. Native drags require an unlocked GUI session and exclusive input.
Signing, installers, physical USB/player behavior, RBXport Restore, large-library
endurance and the exact issue #486 freeze remain separate validation work.
