# Track analysis settings

[Documentation](../README.md) · [Analysis internals](../../crates/rbl-analysis/README.md)

Use **Analyze Track** from a track or player menu, or the analyse-selection
shortcut. **Analysis Setting** opens before any work is queued and captures
the selected track IDs. OK queues that batch; Cancel or Escape starts no work.
Changing the browser selection while the dialog is open does not change it.

## Choose what to analyse

| Control | Effect |
| --- | --- |
| BPM / Grid | Replaces tempo/grid analysis and regenerates waveforms. Disabling it also disables the timing controls. |
| High precision analysis | Uses attack detection for beat placement; otherwise uses the onset envelope. Enabled by default. |
| Analysis Mode | Normal (`rekordbox`) fits one constant tempo to the whole track, as rekordbox's Normal analysis does; use it for music played by a band or with a drifting tempo. RBXport (`rbxport`) follows tempo changes, ramps, and re-phased returns, for DJ edits and electronic music. Rekordbox's Dynamic and Auto modes are not available. Initially follows Preferences. |
| BPM Range | Limits the tempo search to one of rekordbox's ten ranges: 70-180, 48-95, 58-115, 68-135, 78-155, 88-175, 98-195, 108-215, 118-235, or 128-255. Music faster than the top of the range is counted at half speed, so 180–200 BPM music needs 98-195, 108-215, or 118-235. Initially follows Preferences (70-180 by default). |
| Add memory cue at first beat | Adds a memory cue on the first beat of the new grid. Skipped when a memory cue or memory loop already starts within 5 ms of that beat, so re-analysis does not stack duplicates. Requires BPM / Grid. Initially follows Preferences; off by default. |
| KEY | Updates detected key; disabling it preserves the existing key. |

Select at least BPM / Grid or KEY. Phrase labels, vocal detection, and other
automatic cue generation are not available in this dialog. Full analysis
preserves existing cues and other supported sections via the existing-file
inputs.

The first-beat cue is written to the library after the analysis files are
published. If that write is refused, the new grid stays and the track is
reported as failed with a message saying the cue was not added.

## Preservation and locks

Key-only analysis changes key metadata without regenerating files, moving
beats, changing BPM, or marking an unanalysed track as grid-analysed. If no
key is detected, the existing key remains.

The app's local analysis lock and the library analysis-lock flag both prevent
analysis. Locks are checked before decoding and before writing. A refused
track contributes to the queue's failure count without stopping other tracks.

## Keep keys from another program

Preferences > Analysis > **Key Analysis** > **Analyze key** is on by default.
Turn it off to keep keys that come from another source, such as Mixed In Key,
so RBXport never overwrites them:

- Automatic analysis and queued analysis skip the key and leave each track's
  existing key as it is.
- The Analysis Setting window starts with KEY unticked, whatever it was last
  confirmed with. Tick it to analyse the key for that batch only.

## Write the key into the audio file

Preferences > Analysis > **Key Analysis** > **Write the value to the ID3 tag**
is rekordbox's setting of the same name (`writeKeyInTag`), and is off by
default as it is there. With it on, the key also goes into the track's own
file at the two moments rekordbox writes it:

- when an analysis with KEY selected finds a key, and
- when the key is edited in the information panel (a cleared key empties the
  file's key). Only the tracks whose key the edit changed are written: a
  selected track that already had that key, or had no key to clear, keeps its
  file as it was, as in rekordbox. Analysis writes the key it finds each time,
  even when it is the key the track already had, as rekordbox does.

| File | Where the key goes |
| --- | --- |
| MP3 | ID3v2 `TKEY` (the tag is saved as ID3v2.4) |
| AIFF | `TKEY` in the file's `ID3 ` chunk |
| FLAC | Vorbis comment `INITIALKEY` |
| M4A, WAV, others | Nothing; rekordbox writes no key into these either |

After analysis the key is written in View > Key display format: `Dbm` under
Classic, `12A` under Alphanumeric, as rekordbox writes it. An edited key is
written as edited. Files you may not write (read-only, or owned by another
user without write access for you) are passed over, as rekordbox passes them
over; the log says so.

The file is never rewritten in place. A copy beside it is written, read back
and compared with the original — the audio byte for byte and every other tag
item as it was — and only then renamed over it, keeping the original's owner,
group (where you belong to it; otherwise the log says so) and creation date. These files are left unchanged, and the log says
why:

- a tag that cannot be read completely;
- an ID3 tag that repeats a frame of the same kind (two `TKEY` frames, two
  comments with the same language and description, two `TXXX` with the same
  description), which rekordbox keeps but the rewrite could not;
- a file with other hard links, which a replacement would split off;
- a file owned by another user that rbx could not hand back to them.

The evidence is in `crates/rbl-db/src/key_tag.rs`.

## Queued settings

Each queued track holds a copy of its settings. Later preference changes or
other batches cannot alter pending work. Manual choices apply to that batch
and do not overwrite global preferences.

Automatic imports queue directly with the preferred preset, BPM/grid and key
enabled, high precision enabled, the Preferences BPM Range, and the Preferences
first-beat memory cue choice. Eligible tempo
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
