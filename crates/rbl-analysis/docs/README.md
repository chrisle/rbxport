# Analysis documentation

[Crate](../README.md) · [Project documentation](../../../docs/README.md)

Begin with [Overview](overview.md), then [API and code map](development.md).
Read [Assumptions](assumptions.md) and [Pipeline](pipeline.md) before changing
stage behavior. Commands run from the repository root unless stated otherwise.

## Algorithm references

| Guide | Owning code | Use it for |
| --- | --- | --- |
| [Beat grid](algorithms/beat.md) | `onset.rs`, `attack.rs`, `tempo.rs` | Tempo candidates, attack placement, ramps, cuts, and gaps. |
| [Downbeat](algorithms/downbeat.md) | `downbeat.rs`, `lib.rs` | Bar phase, off-beat correction, beat numbering, and boundary adjustment. |
| [Phrase boundaries](algorithms/phrase.md) | `downbeat.rs`, `phrase.rs`, `vocal.rs` | Internal boundaries and unimplemented label/vocal interfaces. |
| [Key](algorithms/key.md) | `key.rs` | Spectral extraction, profiles, rule ordering, and key naming. |
| [Waveforms](algorithms/waveform.md) | `waveform.rs` | Detail columns, independent overview, and encoding ownership. |
| [rekordbox's waveforms](algorithms/rekordbox-waveforms.md) | `wave_creator.rs` | `PWVC`, `PWV6`, `PWV7`, `PWV4`, `PWV5` and `PWV3` as rekordbox computes them. |

## Validation

[Testing and evaluation](validation/README.md) explains the checks to run for
a change. It separates synthetic public tests from evaluations requiring
private reference audio or a disposable library.

- [Reference playlist](validation/reference-playlist.md): read-only cache/scoring, metrics, and historical misses.
- [Multi-tempo evaluation](validation/multitempo.md): manually comparing imported copies, grids, and tempo changes.

## Related app and format guides

- [Analysis settings](../../../docs/user/analysis-settings.md): batch options, locks, and preservation.
- [Waveform calibration](../../../docs/reference/waveform-analysis.md): measured overview comparison.
- [USB analysis format](../../../docs/reference/usb-export-db.md#7-the-analysis-bundle): persisted ANLZ tags.
- [Code conventions](../../../docs/development/conventions.md) and [Contributing](../../../CONTRIBUTING.md): shared development rules.
