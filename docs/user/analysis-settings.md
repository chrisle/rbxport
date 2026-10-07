# Track analysis settings

[Documentation](../README.md) · [Analysis internals](../../crates/rbl-analysis/README.md)

Use **Analyze Track** from a track or player menu, or the analyse-selection
shortcut. **Analysis Setting** opens before any work is queued and captures
the selected track IDs. OK queues that batch; Cancel or Escape starts no work.
Changing the browser selection while the dialog is open does not change it.

## Choose what to analyse

| Control | Effect |
| --- | --- |
| Waveform | Regenerates waveform sections without changing BPM, the beat grid, or key. |
| BPM / Grid | Replaces tempo/grid analysis without changing waveforms unless Waveform is also selected. Disabling it also disables the timing controls. |
| High precision analysis | Uses attack detection for beat placement; otherwise uses the onset envelope. Enabled by default. |
| Analysis Mode | Normal (`rekordbox`) and RBXport (`rbxport`) currently select the same underlying RBXport options. A separate Normal implementation is not present. Initially follows Preferences. |
| BPM Range | Limits tempo search to 70–180 (default), 98–195, 118–236, or 58–115. |
| KEY | Updates detected key; disabling it preserves the existing key. |

Select at least Waveform, BPM / Grid, or KEY. Phrase labels, vocal detection, and automatic
cue generation are not available in this dialog. Full analysis preserves
existing cues and other supported sections via the existing-file inputs.

## Preservation and locks

Waveform-only analysis replaces the waveform sections while carrying existing
beat-grid sections through byte-for-byte and preserving BPM and key metadata.
Grid-only analysis similarly carries existing waveform sections through.

Key-only analysis changes key metadata without regenerating files, moving
beats, changing BPM, or marking an unanalysed track as grid-analysed. If no
key is detected, the existing key remains.

The app's local analysis lock and the library analysis-lock flag both prevent
analysis. Locks are checked before decoding and before writing. A refused
track contributes to the queue's failure count without stopping other tracks.

## Queued settings

Each queued track holds a copy of its settings. Later preference changes or
other batches cannot alter pending work. Manual choices apply to that batch
and do not overwrite global preferences.

Automatic imports queue directly with the preferred preset, waveform, BPM/grid and key
enabled, high precision enabled, and the 70–180 range. Eligible tempo
transitions use the automatic transient fallback with either preset; see
[Beat grid](../../crates/rbl-analysis/docs/algorithms/beat.md#6-grid-the-change).

The modal contains keyboard focus and makes the background inert. Closing
it restores focus to the previous control.

## Code and tests

The request travels from `src/views/analysis/AnalysisDialog.tsx` through
`src/app/App.tsx`, `src/store/useAnalysis.ts`, and the typed IPC backend to
`src-tauri/src/analysis.rs`. `QueueItem.analysis` holds the copied settings.
The backend validates ranges and selected stages; omitted settings retain
full-analysis defaults.

Coverage lives in `e2e/analysis.spec.ts`, `e2e/app.spec.ts`,
`src/store/useAnalysis.test.tsx`, and backend tests in `analysis.rs`.
Use temporary fixtures when testing writes; see [Testing](../development/testing.md).
