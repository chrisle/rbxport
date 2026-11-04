# rbl-deck

[Project documentation](../../docs/README.md) · [Architecture](../../docs/development/architecture.md)

This crate owns two-deck playback: decoding, transport, the audio callback,
mixing, stretching, scrubbing, and the clocks read by the interface. It has no
Tauri dependency. The app integrates it through `src-tauri/src/player.rs`.

## Start here

1. Read the public API and thread model in [`src/lib.rs`](src/lib.rs).
2. Follow the [code map](#code-map) for the behavior you are changing.
3. Read [Scrubbing](../../docs/user/waveform-scrubbing.md) for its filter contract.
4. Use the [validation guide](docs/testing.md) and [fixture guide](tests/fixtures/README.md).
5. Follow the project [conventions](../../docs/development/conventions.md) and [contribution process](../../CONTRIBUTING.md).

## Thread model

The control side sends commands and reads clocks. Each deck's decode thread
owns file I/O, decoding, and resampling. The audio callback reads ring-buffer
blocks, mixes them, fills missing audio with silence, and publishes positions.
Blocks carry generation information so seeking can discard stale decoded data.

A saved Active Loop is armed separately from the current deck loop. The app
supplies only a marked Kind-4 memory loop while Active Loop Playback is enabled.
The decoder limits a read at its IN frame, then starts that loop without a
seek. An already playing manual, hot-cue, or beat loop keeps its range. EXIT
disarms the current pass; a seek before IN permits a later crossing. Clearing
the mark or disabling playback exits a loop that this candidate started.
Arming after a load refills buffered audio from the audible head when that
head is still before IN. Synthetic fixtures check this path; native audio and
physical-player behavior require separate observation.

Keep blocking work and locks out of the callback. The steady-state allocation
test counts allocations after callback initialization; do not move file access,
format decoding, or control-thread work into that path.

## Code map

| Code | Responsibility |
| --- | --- |
| `lib.rs` | Engine API, render callback, master state, and integration. |
| `deck.rs`, `block.rs` | Deck commands, decode workers, and queued audio blocks. |
| `decode.rs` | Streaming decode, seek alignment, and resampling. |
| `clock.rs`, `health.rs` | Position/snapshot and audio-health state. |
| `sink.rs` | Device and caller-supplied sink interfaces. |
| `mixer.rs`, `limiter.rs`, `meter.rs` | Channel processing, output limiting, and meters. |
| `stretch.rs`, `rubberband.rs` | Varispeed, WSOLA, and optional Rubber Band processing. |
| `scrub.rs`, `fade.rs`, `smooth.rs` | Scrub audio and continuous transport/control changes. |
| `metronome.rs` | Grid-based click generation. |

`Engine::new` opens the default device; `Engine::on_device` selects a device.
`Engine::with_sink` supplies a test sink, so tests can exercise the render
callback without opening physical hardware. See `tests/engine.rs` for usage.

## Build features and limits

The default `rubberband` feature builds the vendored Rubber Band library.
Without it, playback uses the WSOLA fallback and does not offer accurate key
shifting. See [Licensing](../../LICENSING.md) for the bundled component terms.
The crate is a two-deck engine; do not imply four-deck behavior from a firmware
model exposing more players.

Generated-audio tests check the engine and DSP. A physical output-device run
and a player-firmware run are separate evidence layers.
