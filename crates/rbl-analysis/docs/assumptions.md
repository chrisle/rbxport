# Analysis assumptions and policy

[Analysis guide](../README.md) · [Documentation](../../../docs/README.md)

These are algorithm assumptions and evaluation targets, not guarantees for arbitrary
music. Recorded measurements are in [Reference evaluation](validation/reference-playlist.md).

The rules for how this library's music is read, and what the code does
with each. They are general: none is about a particular track. Evidence
and measurements live in [Reference evaluation](validation/reference-playlist.md) and
[Key detection](algorithms/key.md).

## Reference and target

- **The private playlist `RBX-BPM-GRID-TEST` is the recorded reference.** Every track in it was
  analysed by rekordbox and its grid checked or set by hand.
- **The evaluation target is 99 % on BPM, downbeat/grid and key** against that
  playlist.
- **Where rekordbox versions disagree with each other, follow the current
  one.** The decoder in `rbl-audio` reproduces rekordbox 7's timeline.

## Time signature and tempo

- **The algorithm assumes 4/4.** A bar is four beats; beats are numbered 1–4
  and 1 is the downbeat. Nothing needs to detect a metre.
- **Drum & bass is counted at the fast tempo** (174, not 87). When two
  octaves are both plausible, the faster one wins if it carries the most
  rhythm at its rate.
- **Near-integer steady tempos snap to whole BPM when the music agrees.**
  A fitted line within a tenth of a whole number is snapped to it and
  re-phased through the same kicks, unless the whole number's line sits on
  less than 85 % as much of the music as the measured one: a track that
  really runs at 173.97 keeps 173.97, as rekordbox measures it. The beats of
  a gradual change keep the tempo they were measured at.
- **The Normal preset is one constant tempo**, as rekordbox's Normal
  analysis is. The RBXport preset follows tempo changes, ramps and
  re-phased returns as described below.
- **A tempo change is a new segment**, with the beat count carrying on 1–4
  across the join, as rekordbox writes it — whatever the new music does
  on that beat. Beat 1 is decided on the first tempo's own music, and the
  count runs on from there; an old-tempo beat within half a period of the
  new tempo's first beat is the same hit, and the new tempo keeps it.
- **Reliable kicks are the preferred timing reference.** A half-level
  incoming kick, isolated impact, arp, clap or snare roll does not establish
  a settled tempo by itself. If a transition cannot be walked, prefer a
  cut where the new kick pattern is reliable, at full level with the mix.
- **When a transition has no usable kick or click, emphasise the remaining
  transients.** Boost positive full-band flux rises by 4× with a 20 ms
  exponential release. Follow actual peaks above the hit floor, retaining
  their original timestamps; silence and release tails cannot supply beats.
  If no reliable kick run places a cut, use the transient support for the
  old and new grids, with ties going to the earliest supported new beat.
- **Where the music comes back at the same tempo on another phase, the
  grid cuts there.** A line through both halves of such a track is on one
  of them or on neither. Each half is put on its own kicks, and the cut
  goes at the first bar of hits on the new line; the old line holds up to
  it. A stretch that comes back on its own grid holds the line across
  whatever the breakdown did.
- **A slowdown or speed-up the line has lost is followed beat by beat
  where it leads somewhere.** With no kick to follow, the hits that
  remain (a bass line in eighths under a tape-stop) are walked from the
  last supported beat, each becoming a beat of its own length, up to the
  cut. Where the music comes back on the grid it left, the ramp is a
  breakdown and the grid holds.
- **A gradual tempo change between two settled tempos preserves each
  measured beat.** Walk from the last settled window at the old tempo,
  following kicks or the transient fallback, with up to 5% period drift
  per beat. Each interval has its own tempo, including on curved ramps.
  Resume the steady fit after four consecutive intervals agree with its
  period within 1% and its phase within 2 ms.

## Downbeat

- **The first beat is not the downbeat by default.** Beat 1 is where the
  music changes: drops, breakdowns, new bass lines, the starts of phrases
  eight and sixteen bars apart.
- **Beat 1 is on the kick, never on the off-beat**, whatever the hats or an
  off-beat bass line are doing.

## Key

- **Key detection is Ángel Faraldo's edmkey method**, as Essentia's
  `KeyExtractor` runs it: spectral peaks, whitening, a harmonic pitch
  class profile, a per-frame gate, detuning correction, and his profiles
  fitted on electronic dance music. The rules below are applied after it.
- **A toss-up between a major key and its parallel minor goes to the
  minor.** (D or Dm: Dm.) The code's `PreferMinor` rule adds a fixed bias
  to every minor key's score.
- **When the key is not easy to determine, listen to the bass.** In the
  first 45 and the last 45 seconds, and on the first two bars of a phrase,
  the bass is usually playing the root note. Within each beat, listen on
  the second eighth: the kick, tail included, takes the first sixteenth to
  eighth of the beat, so the second eighth is the bass line alone. The
  code's `BassRoot` and `BassVote` rules read the bass in each of those
  windows; which rules ship is decided by measurement ([Key detection](algorithms/key.md)).
- Key names are rekordbox's: `Dbm`, `F#m`, `Abm`, `Bbm` for the minors and
  `Db`, `F#`, `Ab`, `Bb`, `Eb` for the majors, matching `djmdKey.ScaleName`.

## Adding or changing a rule

A rule is a variant of `key::Rule` with its knobs, a match arm in
`key::apply`, and a line in this file, stated generally. `golden key` then
reports how often it fires, what it fixes and what it breaks, against the
shipped rules.

Write rules as general conditions, keep tuning and profile changes in the
owning options/rule code, and add public regressions. A new assumption needs
an explanation and identified evidence. Follow the [change workflow](development.md#make-a-change)
and report [evaluation results](validation/README.md#report-results).
