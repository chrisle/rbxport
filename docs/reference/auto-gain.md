# Auto Gain existing-row writer (#889)

The backend-only `Writer::set_auto_gain_with_undo` accepts track IDs and finite
Auto Gain values in **dB**. It updates exactly one existing live `djmdMixerParam`
row per live track. Missing, duplicate or deleted rows are refused, atomically
for the whole batch. There is no UI/IPC integration or automatic analysis.

[OBS: third-party source] pyrekordbox's `DjmdMixerParam.Gain` describes the
rekordbox Grid Edit Auto Gain control and encodes dB as a linear amplitude
`10^(dB/20)`, rounded to IEEE-754 binary32, split into high/low unsigned 16-bit
words. We adapted that encoding, not the reporter's hypothetical `Gain` column.
Source pinned to [pyrekordbox 50b4604](https://github.com/dylanljones/pyrekordbox/blob/50b4604d98dfd52740aa9e76d80c3a32dfa451ba/pyrekordbox/masterdb/models.py),
class `DjmdMixerParam`; [published documentation](https://pyrekordbox.readthedocs.io/en/stable/_modules/pyrekordbox/masterdb/models.html#DjmdMixerParam).
The adapted encoding is covered by the [MIT notice](../licenses/pyrekordbox.txt).

Unlike upstream's negative-infinity sentinel, this API rejects all nonfinite
inputs, binary32 overflow and underflow to zero. These are storage safety
bounds, **not** a verified rekordbox UI gain range. Existing old NULL/bit values
are preserved exactly in opaque batch history, including on Undo.

Writes use the existing guarded Writer connection, first-write backup policy,
immediate transaction, local USN counter and timestamps. Callers must retain
normal backup policy. Undo/Redo first compares every affected row's ID, track
association and gain halves, then writes the whole batch or refuses conflicts.
Peak, UUID, creation time and unrelated fields are untouched. No new mixer row
is fabricated. The local USN scan includes mixer rows to avoid counter reuse.

[UNKNOWN] A live rekordbox roundtrip, actual UI range, new-row shape, cloud-sync
behavior and CDJ/export consumption remain unverified. Disposable SQLCipher
fixtures and third-party source agreement cannot establish physical compatibility.
This change must not be described as a live-tested rekordbox or player feature.
