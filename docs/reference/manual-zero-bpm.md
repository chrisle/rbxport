# Manual zero BPM

Issue #643 uses zero as a track metadata convention for beatless audio.
A zero metadata BPM is distinct from a zero-tempo beat grid.

## Reference evidence

Read-only static inspection of the installed rekordbox 7.2.19.0342 arm64
executable establishes the Info metadata path:

- `browse::TrackInfoConcreteMediator::isValidText` at `0x10036fc3c` parses
  BPM and canonicalizes integer hundredths to `0..99999`.
- `BrowseHelper::roundToValidBpm(int)` at `0x101fcca58` applies that same
  range, including zero.
- `TrackInfoConcreteMediator::notifyBrowseInfo` at `0x10036c830` sends
  metadata column 26 to the database update path.
- `rekordboxDBController::modifyBPM` at `0x101421c60` forwards the value
  without a zero rejection. The directly traced metadata path updates the
  database; it does not retime the analysis file.

The inspected arm64 executable SHA-256 is
`2dfa9738b50e6c5ab3a6ca556dd7967c31ae4a6b06b52d0925f61fad8046f907`.
This is static evidence, not a running-app test of downstream callbacks.
No installed library was opened for writing.

## RBXport behavior

The single-track Info BPM field accepts zero and the existing native
positive grid-edit range of 40–499 BPM. Multiple-selection BPM remains
disabled. This change does not adopt the reference's broader positive
metadata range or change the device-grid editor's input contract.

Zero changes only the selected collection row's BPM. Existing DAT/EXT
bytes, beat times, positive grid tempo and analysis revision stay intact.
No-analysis tracks can receive the marker without creating a grid.
A dynamic grid also stays intact. GRID's tempo input continues to require
a positive tempo, so zero cannot reach beat-spacing division.

As required by #505, an analysed track's Info edit remains one action in
its grid history. Zero entries retain the before/after row BPM as well as
the grid. Undo of a later grid edit can therefore restore the zero marker
with its earlier positive grid. Zero-related Undo/Redo refuses conflicting
newer row values or a changed analysis association, even if the new file
has identical grid bytes. Writes compare the expected BPM and analysis association
in the database transaction; stale, deleted or missing rows are refused.
Metadata-only updates do not increment the analysis revision.

## Disposable acceptance coverage

`src-tauri/src/grid.rs` covers zero, positive tempo restoration, an existing
grid tempo, a dynamic grid, no analysis, external row conflicts, failed
writes, stale analysis association and negative/nonfinite/overflow input.
The original zero rejection makes the new native regression fail.
`crates/rbl-db/tests/writes.rs` checks guarded metadata and grid transactions,
revision preservation and refusal without a USN change.

Frontend tests keep metadata separate from grid tempo. The Chromium and
WebKit scenario in `e2e/grid-edit.spec.ts` uses the real Info field and grid
Undo/Redo against the disposable mock. Existing Info and device-grid
browser scenarios check the surrounding behavior. These tests establish
application and fixture behavior; they do not establish physical-player
or stock running-app acceptance.
