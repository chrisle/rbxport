# rbl-analysis

[Project documentation](../../docs/README.md) · [Crate documentation](docs/README.md)

`rbl-analysis` computes tempo, beat grids, key, waveforms, peak, and RMS from
mono `f32` audio at the file's sample rate. It contains offline DSP and has no
Tauri dependency. The same samples and options produce the same result.

## Start here

1. [Overview](docs/overview.md): what the grid algorithm does and why.
2. [API and code map](docs/development.md): inputs, outputs, stage ownership, and the app boundary.
3. [Pipeline](docs/pipeline.md): execution order and file-start handling.
4. [Algorithm references](docs/README.md#algorithm-references): details for the stage you are changing.
5. [Testing](docs/validation/README.md): synthetic checks, private evaluations, and result limits.

Read the project [code conventions](../../docs/development/conventions.md)
and [contribution process](../../CONTRIBUTING.md) before preparing a change.

## Use the API

```rust
let result = rbl_analysis::analyse(&mono_samples, sample_rate);
let options = rbl_analysis::AnalysisPreset::Rbxport.options();
let result = rbl_analysis::analyse_with(&mono_samples, sample_rate, options);
```

The snippet assumes an existing mono sample buffer and its sample rate.
`analyse` uses `AnalysisOptions::default()`; `analyse_with` accepts explicit
options. Both app presets use the 70–180 BPM range; the Normal
(`Rekordbox`) preset fits one constant tempo to the whole track, as
rekordbox's Normal analysis does, and the `Rbxport` preset follows tempo
changes.
The app can override the range and placement after selecting a preset.

`Analysis` returns `TempoResult`, an optional `MusicalKey`, a `Waveform`, and
peak/RMS. It does not write databases or ANLZ files. Application integration
in `src-tauri/src/analysis.rs` chooses requested stages and persists results.

## Run public checks

From the repository root:

```sh
RB_LITE_TEST=1 cargo test -p rbl-analysis
cargo clippy -p rbl-analysis --all-targets -- -D warnings
```

Public tests generate their own audio and require no reference music or
AlphaTheta Emulator. Private-playlist commands and their recorded scores are
in the [validation guide](docs/validation/README.md).

## Limits

The grid assumes 4/4 and is tuned for electronic music. Internal phrase-start
boundaries exist; phrase labels and vocal detection remain unimplemented.
The waveform overview is an empirical approximation, not byte-identical
rekordbox DSP. Historical evaluation scores do not establish current accuracy;
see [reference evaluation](docs/validation/reference-playlist.md).
