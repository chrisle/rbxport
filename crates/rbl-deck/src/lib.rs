//! The playback engine: two decks, one audio device, one clock.
//!
//! Playback is here rather than on an `<audio>` element because the 2-player
//! view rekordbox has needs phase-locked beat sync, key sync and audible
//! drag-scrub, and a media element has a primitive for none of them. See
//! `docs/pre-release/player-engine.md`.
//!
//! # Threads
//!
//! - **The audio callback**, owned by the device. Reads each deck's ring,
//!   sums, writes the buffer, publishes each deck's position. It allocates
//!   nothing, locks nothing, and calls into nothing that can block; where it
//!   finds no audio it writes silence rather than whatever was in the buffer.
//! - **One decode thread per deck.** All the file I/O, demuxing, decoding and
//!   resampling. Blocks it produces carry the generation they belong to, which
//!   is what makes a seek exact rather than approximate.
//! - **The control side** — a Tauri command, or a test — which only ever sends
//!   messages and reads atomics. Nothing it does can block audio.
//!
//! Two decks, named rather than indexed. Going to four would rework the mixer
//! and the event payload; that is accepted (`docs/pre-release/player-engine.md` §4).

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "frame counts and sample rates convert between integers and f64 throughout"
)]

mod block;
mod clock;
mod deck;
/// Streaming decoder, also used by the close-zoom PCM waveform command.
pub mod decode;
mod fade;
mod limiter;
mod metronome;
mod meter;
mod mixer;
#[cfg(feature = "rubberband")]
mod rubberband;
mod scrub;
mod health;
pub use health::AudioHealth;
mod sink;
mod smooth;
mod stretch;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use rtrb::Consumer;

pub use clock::{DeckClock, DeckSnapshot};
pub use fade::FADE_FRAMES;
pub use sink::{
    default_output_device, output_devices, AudioDevice, CpalSink, NullSink, Render, Sink, StreamWish,
};

use block::{Block, RING_BLOCKS};
use fade::Ramp;
use smooth::Smoothed;

pub use limiter::{
    Limiter, LimiterSettings, DEFAULT_CEILING_DB, DEFAULT_INPUT_GAIN_DB, DEFAULT_RELEASE_MS,
    MAX_CEILING_DB, MAX_INPUT_GAIN_DB, MAX_RELEASE_MS, MIN_CEILING_DB, MIN_INPUT_GAIN_DB, MIN_RELEASE_MS,
};
pub use metronome::{ClickSound, ClickVolume, GridBeat, Metronome, MetronomeSettings};
pub use mixer::{Band, Channel, Curve, Fade, MixerSettings};
#[cfg(feature = "rubberband")]
pub use rubberband::RubberBand;
pub use stretch::{Stretcher, Varispeed, Wsola, MAX_RATIO, MIN_RATIO};

/// Frames the mixer works on at a time.
///
/// Each deck goes through its own strip before the two are summed, so each
/// needs a buffer of its own. One, allocated when the engine starts, and the
/// callback works in chunks of it — the audio callback allocates nothing, ever.
/// 4096 is what the device sink already chunks to.
const MIX_FRAMES: usize = 4096;

#[derive(Debug, thiserror::Error)]
pub enum DeckError {
    #[error("there is no audio output device")]
    NoDevice,
    #[error("the audio device could not be used: {0}")]
    Device(String),
    #[error("unsupported or unreadable audio: {0}")]
    Decode(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, DeckError>;

/// Which deck. Named rather than indexed, as the mixer and the interface are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Deck {
    A,
    B,
}

impl Deck {
    pub fn name(self) -> &'static str {
        match self {
            Deck::A => "a",
            Deck::B => "b",
        }
    }

    fn index(self) -> usize {
        match self {
            Deck::A => 0,
            Deck::B => 1,
        }
    }

    pub const ALL: [Deck; 2] = [Deck::A, Deck::B];
}

/// What the engine tells the interface about, outside the position tick.
#[derive(Debug, Clone)]
pub enum DeckEvent {
    Loaded { deck: Deck, load_id: u64, total_frames: u64, sample_rate: u32 },
    Error { deck: Deck, load_id: u64, message: String },
}

/// Where those go. Called from a decode thread, never from the audio callback.
pub type EventSink = Arc<dyn Fn(DeckEvent) + Send + Sync>;

/// Both decks at one instant, which is what one tick of the clock carries.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snapshot {
    pub a: DeckSnapshot,
    pub b: DeckSnapshot,
    pub sample_rate: u32,
}

impl Snapshot {
    pub fn any_playing(&self) -> bool {
        self.a.playing || self.b.playing
    }
}

/// The master level, and the peaks that came out of it.
///
/// One `AtomicU32` a value, holding an `f32`'s bits: the callback writes them
/// and the interface reads them, and neither ever blocks the other.
#[derive(Debug)]
pub struct Master {
    gain: AtomicU32,
    peak_left: AtomicU32,
    peak_right: AtomicU32,
    rms_left: AtomicU32,
    rms_right: AtomicU32,
    /// The lowest gain the limiter applied since the meter was last read, as
    /// a linear factor: 1.0 is a limiter that did nothing.
    reduction: AtomicU32,
    /// The device's rate, so the callback can smooth a fader in real time.
    ///
    /// Written once, by the engine, as soon as the sink is open — which is
    /// before the stream is started and so before any callback runs.
    rate: AtomicU32,
}

impl Default for Master {
    fn default() -> Self {
        Self {
            // The knob's 10: a decibel under full, the headroom every output
            // is given by default. 11 is full, past the detent.
            gain: AtomicU32::new(DEFAULT_MASTER_GAIN.to_bits()),
            peak_left: AtomicU32::new(0),
            peak_right: AtomicU32::new(0),
            rms_left: AtomicU32::new(0),
            rms_right: AtomicU32::new(0),
            reduction: AtomicU32::new(1.0_f32.to_bits()),
            rate: AtomicU32::new(0),
        }
    }
}

/// The master level the engine starts at: −1 dB, which is the knob at 10.
pub const DEFAULT_MASTER_GAIN: f32 = 0.891_250_9;
/// The master knob at 11: +2 dB.
pub const MAX_MASTER_GAIN: f32 = 1.258_925_4;

impl Master {
    pub fn gain(&self) -> f32 {
        f32::from_bits(self.gain.load(Ordering::Relaxed))
    }

    /// Sets the level, 0 to +2 dB. Anything outside is clamped rather than refused:
    /// a knob dragged past its end is a knob at its end.
    pub fn set_gain(&self, gain: f32) {
        let safe = if gain.is_finite() { gain.clamp(0.0, MAX_MASTER_GAIN) } else { 1.0 };
        self.gain.store(safe.to_bits(), Ordering::Relaxed);
    }

    /// The loudest sample of the last callback, per channel.
    ///
    /// Held rather than replaced: the callback runs every 11 ms and the
    /// interface reads whenever it likes, so a transient between two reads
    /// would be missed if each callback simply overwrote the last.
    fn report(&self, left: f32, right: f32) {
        Self::hold(&self.peak_left, left);
        Self::hold(&self.peak_right, right);
    }

    /// The limiter's floor for the last callback, held the same way, lowest
    /// wins: the moment it worked hardest is the one the meter should show.
    fn report_reduction(&self, floor: f32) {
        let mut current = self.reduction.load(Ordering::Relaxed);
        while floor < f32::from_bits(current) {
            match self.reduction.compare_exchange_weak(
                current,
                floor.to_bits(),
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(seen) => current = seen,
            }
        }
    }

    fn hold(slot: &AtomicU32, value: f32) {
        let mut current = slot.load(Ordering::Relaxed);
        while value > f32::from_bits(current) {
            match slot.compare_exchange_weak(
                current,
                value.to_bits(),
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(seen) => current = seen,
            }
        }
    }

    fn set_rate(&self, rate: u32) {
        self.rate.store(rate, Ordering::Relaxed);
    }

    fn rate(&self) -> u32 {
        self.rate.load(Ordering::Relaxed)
    }

    /// Reads the meters and clears them, so the next read is the next span.
    ///
    /// Peaks, not an average: an average of eleven milliseconds is a meter
    /// that never moves.
    pub fn peaks(&self) -> (f32, f32) {
        (
            f32::from_bits(self.peak_left.swap(0, Ordering::Relaxed)),
            f32::from_bits(self.peak_right.swap(0, Ordering::Relaxed)),
        )
    }

    /// Latest 400 ms unweighted RMS amplitudes, measured after output clipping.
    pub fn rms(&self) -> (f32, f32) {
        (f32::from_bits(self.rms_left.load(Ordering::Relaxed)),
         f32::from_bits(self.rms_right.load(Ordering::Relaxed)))
    }

    /// How far the limiter turned the sum down since the last read, in
    /// decibels, as a positive number: 0 is a limiter that did nothing. Clears
    /// on read, like the peaks.
    pub fn reduction_db(&self) -> f32 {
        let floor = f32::from_bits(self.reduction.swap(1.0_f32.to_bits(), Ordering::Relaxed));
        if floor >= 1.0 || floor <= 0.0 { 0.0 } else { -20.0 * floor.log10() }
    }
}

pub struct Engine {
    decks: [deck::DeckHandle; 2],
    sink: Arc<dyn Sink>,
    sample_rate: u32,
    master: Arc<Master>,
    mixer: Arc<MixerSettings>,
    limiter: Arc<LimiterSettings>,
    metronome: Arc<MetronomeSettings>,
    metronomes: [Arc<Metronome>; 2],
}

impl Engine {
    /// Opens the default output device and starts both decks' threads.
    pub fn new(events: &EventSink) -> Result<Self> {
        Self::on_device(events, None)
    }

    /// The same, on a chosen output.
    ///
    /// `device` is an id from `output_devices`. One that is not there any more
    /// falls back to the default, because an unplugged interface should not be
    /// a silent app.
    pub fn on_device(events: &EventSink, device: Option<String>) -> Result<Self> {
        Self::with_sink(
            move |render| Ok(Arc::new(CpalSink::open_named(render, device)?) as Arc<dyn Sink>),
            events,
        )
    }

    /// The same engine on a sink of the caller's choosing, which is how it is
    /// tested without an audio device.
    #[allow(clippy::too_many_lines, reason = "the render callback is one closure, and it reads as one")]
    pub fn with_sink<F>(open: F, events: &EventSink) -> Result<Self>
    where
        F: FnOnce(Render) -> Result<Arc<dyn Sink>>,
    {
        let clocks: [Arc<DeckClock>; 2] =
            [Arc::new(DeckClock::default()), Arc::new(DeckClock::default())];

        let mut producers = Vec::with_capacity(2);
        let mut readers = Vec::with_capacity(2);
        for clock in &clocks {
            let (producer, consumer) = rtrb::RingBuffer::<Block>::new(RING_BLOCKS);
            producers.push(producer);
            readers.push(DeckReader {
                consumer,
                clock: Arc::clone(clock),
                current: None,
                offset: 0,
                ramp: Ramp::silent(),
                last: (0.0, 0.0),
                generation: 0,
            });
        }

        // The callback owns the readers outright: nothing else touches them,
        // so it never has to take a lock to read one.
        // The master level and what came out of it. Atomics rather than a
        // lock: the callback is realtime and must never wait for the interface
        // to finish reading a meter.
        let master = Arc::new(Master::default());
        let mixing = Arc::clone(&master);
        let mixer = Arc::new(MixerSettings::default());
        let strip = Arc::clone(&mixer);
        let limiter = Arc::new(LimiterSettings::default());
        let limiting = Arc::clone(&limiter);
        // The metronome: settings shared by both decks, a grid and a switch
        // per deck, and the clicks in flight owned by the callback.
        let metronome = Arc::new(MetronomeSettings::default());
        let metronomes: [Arc<Metronome>; 2] =
            [Arc::new(Metronome::new(Arc::clone(&metronome))), Arc::new(Metronome::new(Arc::clone(&metronome)))];
        let clicking = metronomes.clone();
        let mut voices = [metronome::MetronomeVoice::default(), metronome::MetronomeVoice::default()];
        // Built at the first callback with the channels, for the same reason:
        // its lookahead is a number of frames, and that needs the rate.
        let mut limit: Option<Limiter> = None;
        // One buffer per deck, allocated here rather than in the callback,
        // and one for the metronome's clicks, which join after the master
        // level: the click is a reference, and turning the music down must
        // not turn it down too.
        let mut scratch = vec![0.0_f32; MIX_FRAMES * 2];
        let mut click_scratch = vec![0.0_f32; MIX_FRAMES * 2];
        let mut channels: Option<[Channel; 2]> = None;
        // The level a callback is given is one number for the whole buffer, so
        // a hand on the fader arrives as a staircase eleven milliseconds wide.
        // Smoothed per frame instead: see `smooth`. Built at the first
        // callback, because the device's rate is not known until the sink is
        // open and the sink is opened with this closure.
        let mut level: Option<Smoothed> = None;
        let mut rms = meter::WindowRms::new();
        let render: Render = Box::new(move |out: &mut [f32]| {
            let rate = mixing.rate();
            rms.set_rate(rate);
            let channels =
                channels.get_or_insert_with(|| [Channel::new(rate), Channel::new(rate)]);
            let curve = strip.curve();
            let (fade_a, fade_b) = strip.fader();
            // Each deck through its own strip and then summed, which is what a
            // mixer is: a per-channel EQ applied to the sum would be one EQ.
            let target = mixing.gain();
            let level = level.get_or_insert_with(|| Smoothed::new(target, mixing.rate()));
            for chunk in out.chunks_mut(MIX_FRAMES * 2) {
                let Some(buffer) = scratch.get_mut(..chunk.len()) else { continue };
                let Some(click_buffer) = click_scratch.get_mut(..chunk.len()) else { continue };
                click_buffer.fill(0.0);
                for (i, (reader, channel)) in
                    readers.iter_mut().zip(channels.iter_mut()).enumerate()
                {
                    buffer.fill(0.0);
                    let before = reader.clock.position();
                    reader.mix_into(buffer);
                    let after = reader.clock.position();
                    let fader = if i == 0 { fade_a } else { fade_b };
                    let Some(settings) = strip.channels.get(i) else { continue };
                    channel.process(buffer, settings, curve, fader);
                    // The click after the strip, so an EQ cut does not muffle
                    // it, and only while the deck is playing: a scrub crosses
                    // beats too, and nobody wants it clicking.
                    if reader.clock.playing() && !reader.clock.scrubbing() {
                        if let (Some(metro), Some(voice)) = (clicking.get(i), voices.get_mut(i)) {
                            voice.render(metro, before, after, click_buffer, rate);
                        }
                    }
                    for (sample, add) in chunk.iter_mut().zip(buffer.iter()) {
                        *sample += *add;
                    }
                }
                // The master level over the music, then the clicks on top at
                // their own volume, whatever the knob says.
                for (frame, click) in chunk.chunks_exact_mut(2).zip(click_buffer.chunks_exact(2)) {
                    let gain = level.step(target);
                    for (sample, add) in frame.iter_mut().zip(click.iter()) {
                        *sample = *sample * gain + *add;
                    }
                }
            }
            // Two decks at full level sum past 1.0. The limiter sits after
            // the master level, as a DJM's does: turning the master down is
            // then a way to limit less, and what it protects is the output.
            let limit = limit.get_or_insert_with(|| Limiter::new(mixing.rate()));
            limit.process(out, &limiting);
            mixing.report_reduction(limit.take_floor());
            // The limiter lets nothing over its ceiling through, so this
            // never engages — but a hot sum with the limiter off must not
            // reach the device as a wrap.
            let (mut left, mut right) = (0.0_f32, 0.0_f32);
            for frame in out.chunks_exact_mut(2) {
                for (channel, sample) in frame.iter_mut().enumerate() {
                    let value = sample.clamp(-1.0, 1.0);
                    *sample = value;
                    // The meter reads what the device is given, after the
                    // level and the limiter: a meter before the fader tells
                    // you about the file rather than about what anyone can
                    // hear.
                    if channel == 0 {
                        left = left.max(value.abs());
                    } else {
                        right = right.max(value.abs());
                    }
                }
            }
            for frame in out.chunks_exact(2) {
                if let [left, right] = frame { rms.push(*left, *right); }
            }
            let [rms_left, rms_right] = rms.levels();
            mixing.rms_left.store(rms_left.to_bits(), Ordering::Relaxed);
            mixing.rms_right.store(rms_right.to_bits(), Ordering::Relaxed);
            mixing.report(left, right);
        });

        let sink = open(render)?;
        let sample_rate = sink.sample_rate();
        // Before the stream is started, so the first callback already knows it.
        master.set_rate(sample_rate);

        let mut handles = Vec::with_capacity(2);
        for (deck, (clock, producer)) in Deck::ALL.into_iter().zip(clocks.iter().zip(producers)) {
            clock.set_sample_rate(sample_rate);
            handles.push(deck::spawn(
                deck,
                Arc::clone(clock),
                producer,
                sample_rate,
                Arc::clone(events),
            )?);
        }

        let decks: [deck::DeckHandle; 2] = handles
            .try_into()
            .map_err(|_| DeckError::Device("could not start both decks".to_owned()))?;
        Ok(Self { decks, sink, sample_rate, master, mixer, limiter, metronome, metronomes })
    }

    /// Device callback deadlines, without opening or polling the device.
    pub fn audio_health(&self) -> AudioHealth { self.sink.health() }

    /// The metronome's click and volume, shared by both decks.
    pub fn metronome(&self) -> &Arc<MetronomeSettings> {
        &self.metronome
    }

    /// Switches a deck's metronome on or off.
    pub fn set_metronome(&self, deck: Deck, on: bool) {
        if let Some(metro) = self.metronomes.get(deck as usize) {
            metro.set_on(on);
        }
    }

    pub fn metronome_on(&self, deck: Deck) -> bool {
        self.metronomes.get(deck as usize).is_some_and(|m| m.is_on())
    }

    /// The beats a deck's metronome clicks on, in milliseconds from the
    /// start of the track with whether each is a downbeat. Given on a load;
    /// converted to output frames here, where the rate is known.
    pub fn set_metronome_grid(&self, deck: Deck, beats: &[(u32, bool)]) {
        let Some(metro) = self.metronomes.get(deck as usize) else { return };
        let rate = u64::from(self.sample_rate);
        metro.set_grid(
            beats
                .iter()
                .map(|&(ms, downbeat)| GridBeat { frame: u64::from(ms) * rate / 1000, downbeat })
                .collect(),
        );
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// How fast a deck plays, as a multiple of the file's own speed.
    ///
    /// 1.0 is the track as recorded, 1.06 is six percent fast. Clamped to what
    /// a deck offers rather than refused.
    pub fn set_tempo(&self, deck: Deck, tempo: f32) {
        if let Some(handle) = self.decks.get(deck.index()) {
            handle.send(deck::Command::SetTempo(tempo));
        }
    }

    /// Master Tempo: whether the pitch is held while the speed changes.
    ///
    /// Off is what a record does. On costs the stretcher's work — a fifth of a
    /// percent of realtime for two decks — and holds the key.
    pub fn set_master_tempo(&self, deck: Deck, on: bool) {
        if let Some(clock) = self.decks.get(deck.index()).map(deck::DeckHandle::clock) {
            clock.set_master_tempo(on);
        }
        if let Some(handle) = self.decks.get(deck.index()) {
            handle.send(deck::Command::SetMasterTempo(on));
        }
    }

    /// The key, in semitones from the track's own: a CDJ's key shift.
    ///
    /// Only the Rubber Band backend holds a pitch to within a cent; the
    /// WSOLA one is out by up to a quarter of a semitone, so a build without
    /// Rubber Band reports `shifts_key` false and the interface greys the
    /// buttons rather than offer a wrong key.
    pub fn set_key_shift(&self, deck: Deck, semitones: i8) {
        if let Some(handle) = self.decks.get(deck.index()) {
            handle.send(deck::Command::SetKeyShift(semitones));
        }
    }

    /// Whether this build can shift a key without moving the tempo.
    #[must_use]
    pub fn shifts_key() -> bool {
        cfg!(feature = "rubberband")
    }

    /// The channel strips and the crossfader.
    pub fn mixer(&self) -> &Arc<MixerSettings> {
        &self.mixer
    }

    /// The master level and its meters.
    pub fn master(&self) -> &Arc<Master> {
        &self.master
    }

    /// The master limiter: on or off, its ceiling and its release.
    pub fn limiter(&self) -> &Arc<LimiterSettings> {
        &self.limiter
    }

    fn deck(&self, deck: Deck) -> Option<&deck::DeckHandle> {
        self.decks.get(deck.index())
    }

    /// Points a deck at a file. Readiness arrives as `DeckEvent::Loaded`,
    /// because opening one means reading from a disk that may be asleep.
    pub fn load(&self, deck: Deck, path: &Path) {
        let request = self
            .deck(deck)
            .map_or(1, |handle| handle.clock().requested_load().wrapping_add(1).max(1));
        self.load_as(deck, path, request);
    }

    /// The same load under an identity supplied by a caller. Only the newest
    /// identity may finish; stale opens are discarded by the deck worker.
    pub fn load_as(&self, deck: Deck, path: &Path, request: u64) {
        let request = request.max(1);
        if let Some(handle) = self.deck(deck) {
            handle.clock().request_load(request);
            handle.send(deck::Command::Load { request, path: PathBuf::from(path) });
        }
        self.settle_device();
    }

    pub fn unload(&self, deck: Deck) {
        if let Some(handle) = self.deck(deck) {
            // Invalidates an open already in progress before its worker can
            // publish it, not only when the queued unload is handled.
            handle.clock().request_load(0);
            handle.send(deck::Command::Unload);
        }
        self.settle_device();
    }

    pub fn play(&self, deck: Deck) {
        let Some(handle) = self.deck(deck) else { return };
        if !handle.clock().loaded() {
            return;
        }
        // Playing to the end and pressing play again starts it over, which is
        // what a deck does rather than sitting silently at the end.
        if handle.clock().end_of_stream() && handle.clock().position() >= handle.clock().total() {
            handle.send(deck::Command::Seek(0));
        }
        handle.clock().set_start_in(0);
        handle.clock().set_playing(true);
        handle.send(deck::Command::Wake);
        self.settle_device();
    }

    /// Starts a deck after `frames` of output have passed in silence.
    ///
    /// Quantized play on a synced deck: the press comes when it comes, and
    /// the first sound is held until the master's next beat. Counted in
    /// output frames by the callback, which is the only clock that lands a
    /// sound where it says it will; a timer on any other thread would be
    /// out by a callback or two, and a beat is only a few of those.
    pub fn play_after(&self, deck: Deck, frames: u64) {
        let Some(handle) = self.deck(deck) else { return };
        if !handle.clock().loaded() {
            return;
        }
        if handle.clock().end_of_stream() && handle.clock().position() >= handle.clock().total() {
            handle.send(deck::Command::Seek(0));
        }
        handle.clock().set_start_in(frames);
        handle.clock().set_playing(true);
        handle.send(deck::Command::Wake);
        self.settle_device();
    }

    pub fn pause(&self, deck: Deck) {
        let Some(handle) = self.deck(deck) else { return };
        handle.clock().set_start_in(0);
        handle.clock().set_playing(false);
        handle.send(deck::Command::Wake);
        self.settle_device();
    }

    fn seek_with_pre_roll(&self, deck: Deck, frame: u64, pre_roll: u64) {
        let Some(handle) = self.deck(deck) else {
            return;
        };
        // The clock moves now rather than when the first block after the seek
        // is played: a seek while paused must show where it landed.
        handle.clock().set_pre_roll(pre_roll);
        handle
            .clock()
            .set_position(frame.min(handle.clock().total().max(frame)));
        // A head moved by hand is no longer where the beat was worked out
        // from; whatever wait was pending is over.
        handle.clock().set_start_in(0);
        // And the generation with it, here rather than on the deck thread.
        //
        // The callback treats "the clock's generation is ahead of mine" as
        // "a seek is in flight" and stops moving the playhead for audio the
        // seek has replaced. Bumping only when the deck thread got round to
        // the command left a window — position already moved, generation not
        // yet — in which the callback still believed the old blocks in the
        // ring were current and stamped their position back over the one that
        // had just been set. Measured releasing CUE: a cue at 1.4346 s read
        // back as 1.4106, 24 ms in front of it, and the next press then moved
        // the cue point there because it was outside `CUE_TOLERANCE`.
        handle.clock().bump_generation();
        handle.send(deck::Command::Seek(frame));
        handle.send(deck::Command::Wake);
    }

    /// Moves the playhead, in frames at the device rate.
    pub fn seek_frames(&self, deck: Deck, frame: u64) {
        self.seek_with_pre_roll(deck, frame, 0);
    }

    /// Moves the playhead, in milliseconds.
    ///
    /// Fractional, for the same reason `scrub_to_ms` is: a whole millisecond
    /// is 44 frames at 44.1 kHz and 96 at 96, and a cue point is a place in
    /// the music rather than a rounded one. Returning to a cue used to land
    /// 0.3 ms in front of it every time, measured, because the position went
    /// out as a rounded integer.
    /// Sets a loop between two points, in milliseconds, and turns it on. A
    /// head already past the out point is sent back to the in point.
    pub fn set_loop_ms(&self, deck: Deck, in_ms: f64, out_ms: f64) {
        let to_frames = |ms: f64| (ms.max(0.0) * f64::from(self.sample_rate) / 1000.0) as u64;
        self.set_loop_frames(deck, to_frames(in_ms), to_frames(out_ms));
    }

    /// [`set_loop_ms`](Self::set_loop_ms) in device-rate frames.
    pub fn set_loop_frames(&self, deck: Deck, from: u64, to: u64) {
        let Some(handle) = self.deck(deck) else { return };
        if to <= from {
            return;
        }
        handle.clock().set_loop(Some((from, to)));
        handle.clock().set_looping(true);
        if handle.clock().position() >= to || handle.clock().position() < from {
            self.seek_frames(deck, from);
        }
        handle.send(deck::Command::Wake);
    }

    /// RELOOP and EXIT: back into the loop from its in point, or out of it
    /// with the range kept for the next RELOOP.
    pub fn set_looping(&self, deck: Deck, on: bool) {
        let Some(handle) = self.deck(deck) else { return };
        let Some((from, _)) = handle.clock().loop_range() else { return };
        handle.clock().set_looping(on);
        if on {
            self.seek_frames(deck, from);
        }
        handle.send(deck::Command::Wake);
    }

    /// Forgets the loop.
    pub fn clear_loop(&self, deck: Deck) {
        let Some(handle) = self.deck(deck) else { return };
        handle.clock().set_loop(None);
    }

    /// Moves the playhead by `ms` from where it is now. The move is worked
    /// out here, from the clock, so it does not land late by the time the
    /// command took to arrive. The phase lock and a grid shift on a synced
    /// deck use it. A head in the pre-roll before zero moves from there, as
    /// `seek_ms` puts it.
    pub fn move_ms(&self, deck: Deck, ms: f64) {
        let Some(handle) = self.deck(deck) else { return };
        let clock = handle.clock();
        #[allow(clippy::cast_precision_loss, reason = "a track is far under 2^52 frames")]
        let frames = clock.position() as f64 - clock.pre_roll() as f64;
        self.seek_ms(deck, frames * 1000.0 / f64::from(self.sample_rate) + ms);
    }

    pub fn seek_ms(&self, deck: Deck, ms: f64) {
        let frames = (ms.max(0.0) * f64::from(self.sample_rate) / 1000.0) as u64;
        let pre_roll = ((-ms).clamp(0.0, 5000.0) * f64::from(self.sample_rate) / 1000.0) as u64;
        self.seek_with_pre_roll(deck, frames, pre_roll);
    }

    pub fn snapshot(&self) -> Snapshot {
        let a = self.decks.first().map(|d| d.clock().snapshot()).unwrap_or_default_snapshot();
        let b = self.decks.get(1).map(|d| d.clock().snapshot()).unwrap_or_default_snapshot();
        Snapshot { a, b, sample_rate: self.sample_rate }
    }

    pub fn any_playing(&self) -> bool {
        self.decks.iter().any(|deck| deck.clock().playing())
    }

    /// Whether anything needs the device: playing, or being dragged.
    pub fn any_sounding(&self) -> bool {
        self.decks.iter().any(|deck| deck.clock().sounding())
    }

    /// Starts a drag on a deck. Audio follows the pointer until `scrub_end`.
    pub fn scrub_begin(&self, deck: Deck) {
        let Some(handle) = self.deck(deck) else { return };
        if !handle.clock().loaded() {
            return;
        }
        handle.clock().set_scrubbing(true);
        handle.send(deck::Command::ScrubBegin);
        self.settle_device();
    }

    /// Where the pointer is now, in milliseconds.
    /// Fractional milliseconds, deliberately.
    ///
    /// A whole millisecond is 44 frames, and the read head's speed is worked
    /// out from how far the pointer has moved since the last one. Rounding the
    /// position to a millisecond therefore quantises the *speed*: a hand
    /// moving at a twentieth of playback advances a third of a millisecond
    /// between moves, so the rounded position repeats and then jumps, and the
    /// head stalls and lurches instead of turning evenly. Measured on a
    /// simulated drag: rounding took the rate's spread from 13% of its mean to
    /// 38%, and stopped the head outright in 21 blocks out of 258.
    pub fn scrub_to_ms(&self, deck: Deck, ms: f64) {
        let Some(handle) = self.deck(deck) else {
            return;
        };
        let frames = (ms.max(0.0) * f64::from(self.sample_rate) / 1000.0) as u64;
        // Stamped here, on the way in, rather than counted in blocks on the
        // way out: the same argument as the fractional milliseconds above, on
        // the other axis. Distance over time is the speed, and rounding either
        // of them quantises it.
        let pre_roll = ((-ms).clamp(0.0, 5000.0) * f64::from(self.sample_rate) / 1000.0) as u64;
        handle.send(deck::Command::ScrubTo(
            frames,
            pre_roll,
            std::time::Instant::now(),
        ));
    }

    /// Ends a drag. The playhead lands under the pointer, not on the read
    /// head, which the audible rate cap leaves behind on a fast drag.
    pub fn scrub_end(&self, deck: Deck) {
        let Some(handle) = self.deck(deck) else { return };
        handle.clock().set_scrubbing(false);
        handle.send(deck::Command::ScrubEnd);
        handle.send(deck::Command::Wake);
        self.settle_device();
    }

    /// Starts the device when a deck needs it and stops it when neither does.
    ///
    /// A stream that is left running costs a callback every 11 ms for silence,
    /// which is exactly the sort of idle cost the budget rules out.
    fn settle_device(&self) {
        let wanted = self.any_sounding();
        let result = if wanted { self.sink.start() } else { self.sink.stop() };
        if let Err(e) = result {
            tracing::error!(error = %e, "the audio device would not change state");
        }
    }
}

/// `Option<DeckSnapshot>` with the empty case spelled out, so a missing deck
/// reads as a stopped one rather than unwrapping.
trait OrEmptySnapshot {
    fn unwrap_or_default_snapshot(self) -> DeckSnapshot;
}

impl OrEmptySnapshot for Option<DeckSnapshot> {
    fn unwrap_or_default_snapshot(self) -> DeckSnapshot {
        self.unwrap_or(DeckSnapshot {
            position_frames: 0,
            pre_roll_frames: 0,
            total_frames: 0,
            generation: 0,
            sample_rate: 0,
            playing: false,
            loaded: false,
            load_id: 0,
            tempo: 1.0,
            master_tempo: false,
            key_shift: 0,
            start_in_frames: 0,
            loop_in_frames: 0,
            loop_out_frames: 0,
            looping: false,
        })
    }
}

/// One deck's side of the audio callback.
///
/// Realtime: no allocation, no lock, no syscall, no `unwrap`. Where anything
/// is missing it writes nothing and leaves silence behind.
struct DeckReader {
    consumer: Consumer<Block>,
    clock: Arc<DeckClock>,
    /// The block being played, and how far into it.
    current: Option<Block>,
    offset: usize,
    /// The envelope every frame this deck contributes goes through.
    ///
    /// Sound is only ever started or cut where this reads zero, which is what
    /// makes play, pause, cue, a beat jump and a seek silent at the join
    /// rather than a click. See `fade`.
    ramp: Ramp,
    /// The last frame actually played, held so that running dry can be faded
    /// rather than cut. See the underrun in `mix_into`.
    last: (f32, f32),
    /// The generation being rendered, which trails the clock's across a seek.
    ///
    /// The clock moves the moment a seek is asked for. The reader keeps
    /// playing what it was playing until the ramp has taken it to zero, and
    /// only then takes the new one up: without that the jump lands in the
    /// middle of a waveform, at whatever height the old one was cut at.
    generation: u32,
}

impl DeckReader {
    fn mix_into(&mut self, out: &mut [f32]) {
        // A start held for the beat: the frames of the wait pass in silence
        // — nothing is added to `out` — and the rest of the callback, if
        // any, is played as usual. The wait is counted here, in the output's
        // own frames, so the first sound lands where the wait was measured
        // to. A drag is heard regardless: the hand is not waiting for a beat.
        let out = if self.clock.playing() && !self.clock.scrubbing() && self.clock.start_in() > 0 {
            let frames = out.len() / 2;
            let passed = usize::try_from(self.clock.pass_start(frames as u64)).unwrap_or(frames);
            if passed >= frames {
                return;
            }
            out.get_mut(passed * 2..).unwrap_or(&mut [])
        } else {
            out
        };
        // Before the file starts, retain its queued audio and move only the
        // silent lead-in. A held scrub or paused deck keeps its position.
        let out = if self.clock.pre_roll() > 0 {
            let held = self.clock.scrubbing() || !self.clock.playing();
            let rate = f64::from(self.clock.tempo());
            let available = out.len() / 2;
            let waiting = (self.clock.pre_roll() as f64 / rate).ceil() as usize;
            let passed = if held { available } else { available.min(waiting) };
            // Fade the last sample into silence without consuming the song.
            for frame in out[..passed * 2].chunks_exact_mut(2) {
                if self.ramp.silent_now() { break; }
                let gain = self.ramp.step(false);
                frame[0] += self.last.0 * gain;
                frame[1] += self.last.1 * gain;
            }
            if self.ramp.silent_now() {
                self.adopt(self.clock.generation());
            }
            if held { return; }
            self.clock.pass_pre_roll((passed as f64 * rate).ceil() as u64);
            if passed == available {
                return;
            }
            out.get_mut(passed * 2..).unwrap_or(&mut [])
        } else {
            out
        };
        let target = self.clock.generation();
        let sounding = self.clock.sounding();
        // Silent and asked for nothing. Whatever a seek left behind is dropped
        // here rather than handed back when the deck starts again.
        if !sounding && self.ramp.silent_now() {
            self.adopt(target);
            return;
        }

        let frames = out.len() / 2;
        let mut position = self.clock.position();
        let mut done = 0;
        while done < frames {
            // Faded out and the clock has moved on: this is where a seek, a
            // cue or a beat jump actually takes effect, with nothing sounding
            // across the join.
            if self.generation != target && self.ramp.silent_now() {
                self.adopt(target);
            }
            let changing = self.generation != target;
            // Down for a stop, for a seek that has not landed yet, and for the
            // last frames of a track; up for anything else.
            let open = sounding && !changing && !self.ending();
            // Mid-changeover the ring holds where the deck is *going*, so only
            // what is already in hand may be faded out; taking a new block
            // would fade out audio from the seek target and lose it.
            let Some((left, right, at)) = self.next_frame(!changing) else {
                // Nothing to play: the decode thread has not kept up, or the
                // file has ended somewhere the fade did not see coming. The
                // last frame is held and faded down rather than cut — two
                // milliseconds of a held sample decaying is inaudible, and the
                // step it replaces is not. The playhead does not move for it:
                // no audio from the track was played.
                if self.ramp.silent_now() {
                    if changing {
                        self.adopt(target);
                        continue;
                    }
                    break;
                }
                let gain = self.ramp.step(false);
                if let Some(slot) = out.get_mut(done * 2) {
                    *slot += self.last.0 * gain;
                }
                if let Some(slot) = out.get_mut(done * 2 + 1) {
                    *slot += self.last.1 * gain;
                }
                done += 1;
                continue;
            };
            self.last = (left, right);
            let gain = self.ramp.step(open);
            if let Some(slot) = out.get_mut(done * 2) {
                *slot += left * gain;
            }
            if let Some(slot) = out.get_mut(done * 2 + 1) {
                *slot += right * gain;
            }
            // Only for audio the deck still stands behind. Mid-changeover
            // these frames are the tail of where it *was*, fading out under a
            // seek that has already replaced them, and they sit behind the
            // ring — stamping them walks the playhead backwards past the point
            // it was just sent to.
            //
            // Releasing CUE is where it shows. That sends a seek and a pause as
            // two commands, and when the seek is handled first the fade-out
            // that follows it stamped the old audio's position over the cue.
            // Measured in the app: a release onto a cue at 1.4513 s reported
            // 1.4367 s, 14 ms in front of it.
            //
            // The same rule the underrun above already follows: no audio from
            // where the deck is going was played, so the playhead does not
            // move for it.
            if !changing {
                position = at;
            }
            done += 1;
            // A stop that has finished fading stops consuming: the frames past
            // it belong to wherever the deck is resumed from.
            if !sounding && self.ramp.silent_now() {
                break;
            }
        }

        self.clock.set_position(position);

        // Ran dry with nothing more coming: the track has ended. An underrun
        // that is not the end leaves the deck playing and writes silence,
        // which is a glitch rather than a stop.
        if done < frames
            && self.current.is_none()
            && self.consumer.is_empty()
            && self.clock.end_of_stream()
        {
            self.clock.set_playing(false);
        }
    }

    /// Takes `generation` as the one being rendered, dropping what is older.
    ///
    /// Only ever called with the ramp at zero, which is what makes the change
    /// silent: the audio before the seek has already been faded out and the
    /// audio after it is faded in from nothing.
    fn adopt(&mut self, generation: u32) {
        self.generation = generation;
        if self.current.as_ref().is_some_and(|block| block.generation < generation) {
            self.current = None;
            self.offset = 0;
        }
    }

    /// The next frame of this deck's audio, and where it leaves the playhead.
    ///
    /// `pop` says whether the ring may be drawn on. Blocks decoded before the
    /// last seek are dropped rather than played: that is what makes a seek
    /// exact.
    fn next_frame(&mut self, pop: bool) -> Option<(f32, f32, u64)> {
        loop {
            if let Some(block) = self.current.as_ref() {
                if self.offset < block.frames as usize {
                    let i = self.offset * 2;
                    let filled = block.filled();
                    let left = filled.get(i).copied().unwrap_or(0.0);
                    let right = filled.get(i + 1).copied().unwrap_or(0.0);
                    self.offset += 1;
                    return Some((left, right, block.position + self.offset as u64));
                }
                self.current = None;
                self.offset = 0;
            }
            if !pop {
                return None;
            }
            match self.consumer.pop() {
                Ok(block) if block.generation >= self.generation => {
                    self.current = Some(block);
                    self.offset = 0;
                }
                Ok(_) => {}
                Err(_) => return None,
            }
        }
    }

    /// Whether what is left in hand is the last of the track, and short enough
    /// that the fade has to start now to reach zero by the end of it.
    ///
    /// The end of a file is a cut like any other: the last sample is wherever
    /// the music was, and the silence after it is a step down from there.
    fn ending(&self) -> bool {
        if !self.clock.end_of_stream() || !self.consumer.is_empty() {
            return false;
        }
        let left = self
            .current
            .as_ref()
            .map_or(0, |block| (block.frames as usize).saturating_sub(self.offset));
        left <= usize::from(FADE_FRAMES)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp, reason = "exact values are set and read back")]
mod master_tests {
    use super::*;

    #[test]
    fn a_new_master_is_a_decibel_under_full_and_silent() {
        let master = Master::default();
        assert_eq!(master.gain(), DEFAULT_MASTER_GAIN);
        assert!((20.0 * master.gain().log10() + 1.0).abs() < 0.001);
        assert_eq!(master.peaks(), (0.0, 0.0));
    }

    #[test]
    fn a_knob_dragged_past_its_end_is_a_knob_at_its_end() {
        let master = Master::default();
        master.set_gain(2.5);
        assert_eq!(master.gain(), MAX_MASTER_GAIN);
        master.set_gain(-1.0);
        assert_eq!(master.gain(), 0.0);
        master.set_gain(f32::NAN);
        assert_eq!(master.gain(), 1.0, "a NaN level would silence the app");
    }

    #[test]
    fn the_meters_read_what_the_device_was_given() {
        let master = Master::default();
        master.report(0.5, 0.25);
        assert_eq!(master.peaks(), (0.5, 0.25));
    }

    #[test]
    fn a_transient_between_two_reads_is_not_lost() {
        // The callback runs every 11 ms and the meter is read when it is read.
        // Overwriting each time would drop the loud callback in between.
        let master = Master::default();
        master.report(0.2, 0.2);
        master.report(0.9, 0.8);
        master.report(0.1, 0.1);
        assert_eq!(master.peaks(), (0.9, 0.8));
    }

    #[test]
    fn reading_the_meters_clears_them_for_the_next_span() {
        let master = Master::default();
        master.report(0.7, 0.7);
        assert_eq!(master.peaks(), (0.7, 0.7));
        assert_eq!(master.peaks(), (0.0, 0.0), "silence since the last read");
    }
}
