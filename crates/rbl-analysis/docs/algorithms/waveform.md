# Waveforms

[Analysis documentation](../README.md) · [Code map](../development.md)

`src/waveform.rs` produces detail columns and an independently measured overview
from mono audio. Changing one representation should not silently change the
other. The app and ANLZ author consume these results through their own layers.

## Input and output

`waveform::compute(samples, sample_rate)` returns `Waveform`:

- `columns`: low, mid, high, and overall peak values at 150 columns per second.
- `columns_per_sec`: the detail-column rate.
- `overview`: 1,200 low/mid/high buckets already in seven-bit range.

Empty input or a zero sample rate returns no detail columns and a zero-filled
1,200-bucket overview. See the implementation and its tests for short-input
and impulse handling.

## Detail and overview

Detail uses band peaks. The overview instead measures energy directly:
compressed bass RMS, rectified mid/high energy, track-level normalization,
and a short trailing average. Reducing the detail's peaks produced an
undersized, spiky overview, so it is not the source for PWV6.

The overview is calibrated empirically against nine rekordbox references.
It is not recovered rekordbox DSP or byte-identical parity. The
[calibration reference](../../../../docs/reference/waveform-analysis.md)
records the algorithms, measured errors, and remaining limitations.

## Packing and persistence

The app writes `PWVC`, `PWV3`, `PWV4`, `PWV5`, `PWV6` and `PWV7` as rekordbox
computes them itself, from `src/wave_creator.rs`; see
[rekordbox's colour and 3-band waveforms](rekordbox-waveforms.md). The columns
here still give `PWAV` and `PWV2`, and the helpers for the other
representations remain for callers that have only columns.

The crate has helpers for PWAV, PWV2, PWV3, PWV4, and PWV5 representations.
Their counts and bit layouts are tested in `src/waveform.rs`. Two PWV4 bytes
remain `[UNKNOWN]` in that column encoding and are written zero; rekordbox's
own values for them are in the `wave_creator` reference.

Complete ANLZ framing and production PWV6/PWV7 encoding belong to
`rbl-anlz`, not this crate. Use the [USB format reference](../../../../docs/reference/usb-export-db.md#waveforms)
when changing persisted tags. A changed algorithm affects new analysis;
existing analysis files are not automatically rewritten by a DSP code change.

## Validation

```sh
RB_LITE_TEST=1 cargo test -p rbl-analysis waveform
```

Check column counts, band separation, preview packing, and silence/impulse
behavior before a private calibration comparison. Keep calibration error
separate from structural packing correctness. For read-only diagnostics:

```sh
cargo run --release -p rbl-analysis --example waveform_probe -- AUDIO OUTPUT_DIRECTORY
```

This writes diagnostics separately from the library. Manual file repair is
documented in the calibration reference and must use disposable data.
