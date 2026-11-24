# Auto Gain writer (#889)

The backend-only `Writer::set_auto_gain_with_undo` accepts track IDs and finite
Auto Gain values in **dB**. It updates the existing live `djmdMixerParam` row or
creates one with Peak 1.0 when a live track has no live mixer row. Missing or
deleted tracks, duplicate input IDs and duplicate live mixer rows refuse the
whole batch. Existing tombstones are preserved. There is no UI/IPC integration
or automatic analysis; the requested API can be used independently of a UI.

[OBS: third-party source] pyrekordbox's `DjmdMixerParam.Gain` describes the
rekordbox Grid Edit Auto Gain control and encodes dB as a linear amplitude
`10^(dB/20)`, rounded to IEEE-754 binary32, split into high/low unsigned 16-bit
words. We adapted that encoding, not the reporter's hypothetical `Gain` column.
Source pinned to [pyrekordbox 50b4604](https://github.com/dylanljones/pyrekordbox/blob/50b4604d98dfd52740aa9e76d80c3a32dfa451ba/pyrekordbox/masterdb/models.py),
class `DjmdMixerParam`; [published documentation](https://pyrekordbox.readthedocs.io/en/stable/_modules/pyrekordbox/masterdb/models.html#DjmdMixerParam).
The adapted encoding is covered by the [MIT notice](../licenses/pyrekordbox.txt).

[OBS: primary static code, rekordbox 7.2.19.0342 macOS arm64]
`AppSyncDBController::addAutoGainInfo` at `0x100a6cfb0` generates separate
dashed UUIDs for `ID` and `UUID`, associates `ContentID`, splits each supplied
binary32 gain/peak into high/low unsigned 16-bit words, obtains a local USN,
and inserts `djmdMixerParam` through its SQLite query path.
`rekordboxDBController::setTrackAutoGainInfo` at `0x1017f9e5c` also reaches
the add path when no row exists. This establishes the missing-row storage
shape. Peak 1.0 is the default requested in #889, not a measured universal
stock default or an assertion about automatic gain analysis.

Unlike upstream's negative-infinity sentinel, this API rejects all nonfinite
inputs, binary32 overflow and underflow to zero. These are storage safety
bounds, **not** a verified rekordbox UI gain range. Existing old NULL/bit values
are preserved exactly in opaque batch history, including on Undo.

Writes use the existing guarded Writer connection, first-write backup policy,
immediate transaction, local USN counter and timestamps. Callers must retain
normal backup policy. Undo/Redo first compares every affected row's ID, track
association and gain halves, then writes the whole batch or refuses conflicts.
Existing Peak, UUID, creation time and unrelated fields are untouched. New rows
use fresh UUIDs and unity Peak (`PeakHigh=16256`, `PeakLow=0`). Undo soft-deletes
only this batch's created rows; Redo restores those same identities. Both
operations check the created row's entire stored shape, excluding only the
liveness/local-revision fields they own, and refuse outside edits or a live
replacement before changing any row. The local USN scan includes mixer rows
to avoid counter reuse. Disposable tests verify mixed update/create batches,
backup, exact Undo/Redo, trigger rollback and preservation of track metadata.

[UNKNOWN] A live rekordbox roundtrip, actual UI range, cloud-sync
behavior and CDJ/export consumption remain unverified. Disposable SQLCipher
fixtures and static primary code cannot establish physical compatibility.
This change must not be described as a live-tested rekordbox or player feature.
