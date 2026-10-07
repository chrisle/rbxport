//! The engine end to end, on a sink with no device behind it.
//!
//! Every one of these drives the same audio callback the real device does, so
//! what they assert about position, seeking and silence is what a deck
//! actually produces.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss
)]

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rbl_deck::{Deck, DeckEvent, Engine, NullSink, Sink, FADE_FRAMES};

const RATE: u32 = 44_100;

/// Frames in a deck's ring, which is what the engine's own `RING_BLOCKS`
/// times `BLOCK_FRAMES` comes to.
const RING: usize = 16 * 512;

/// A 16-bit PCM WAV, so no fixture file is needed.
fn write_wav(path: &Path, sample_rate: u32, channels: u16, samples: &[f32]) {
    let bits = 16_u16;
    let block_align = channels * bits / 8;
    let byte_rate = sample_rate * u32::from(block_align);
    let data_len = u32::try_from(samples.len() * 2).unwrap();
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16_u32.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&((sample.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, out).unwrap();
}

/// A track whose sample at frame `n` says what `n` is, so a test can read the
/// output and say where in the file it came from.
fn ramp_at(path: &Path, rate: u32, frames: usize) {
    let mut samples = Vec::with_capacity(frames * 2);
    for frame in 0..frames {
        // 0.5 at frame 0 rising to 1.0 at the end, in both channels.
        let value = 0.5 + 0.5 * (frame as f32 / frames as f32);
        samples.push(value);
        samples.push(value);
    }
    write_wav(path, rate, 2, &samples);
}

fn ramp(path: &Path, frames: usize) {
    ramp_at(path, RATE, frames);
}

/// A track that is one steady loud value, so any step in the output came from
/// the transport rather than from the music.
///
/// 0.8 is most of full scale: it is what a kick drum is doing at the moment
/// somebody presses pause, and a cut from 0.8 to silence in one sample is the
/// click all of this exists to prevent.
const FLAT: f32 = 0.8;

fn flat(path: &Path, frames: usize) {
    write_wav(path, RATE, 2, &vec![FLAT; frames * 2]);
}

/// The largest jump between neighbouring output samples, on the left channel.
///
/// A click is a discontinuity and nothing else, so this is the whole of what
/// "does it click" means. A fade over `FADE_FRAMES` moves at most one
/// eighty-eighth of full scale a frame, so anything much above that is a step
/// somebody would hear.
fn worst_step(out: &[f32]) -> f32 {
    out.chunks_exact(2)
        .map(|frame| frame[0])
        .collect::<Vec<_>>()
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .fold(0.0_f32, f32::max)
}

/// How long the channel strip takes to go quiet after its input has.
///
/// Its crossovers are four-pole and one of them corners at 300 Hz, and no
/// filter stops faster than a few cycles of its own corner: measured at 186
/// frames, about four milliseconds, from the last audio in to −80 dB out. A
/// real mixer's EQ does the same thing — it is latency, not a defect.
///
/// Measured at 271 frames, about six milliseconds, from the end of a
/// two-millisecond fade to the last sample above −80 dB.
const STRIP_TAIL: usize = 320;

/// The master limiter's lookahead, which delays everything by this much
/// whether it is on or not: 1.5 ms at the harness's rate.
const LIMITER_TAIL: usize = 66;

/// Below this nothing is audible: −80 dB of full scale.
///
/// "Silent" is this rather than a hard zero because the channel strip's
/// filters ring on after the audio into them has stopped — an IIR's tail never
/// truly ends. It is 10,000 times below what the fade it follows started at.
const INAUDIBLE: f32 = 1e-4;

/// The most a fade is allowed to move in one frame, with room for the sample
/// rate conversion, the 16-bit quantisation of the fixture, and the channel
/// strip.
///
/// The strip is flat in level but not in phase, and its four-pole crossovers
/// overshoot the edges of a two-millisecond ramp: 0.0147 measured against the
/// ramp's own 0.0091 a frame. Twice the ramp's slope is the allowance, which
/// is still forty times below the step a real cut would make.
fn step_limit() -> f32 {
    FLAT / f32::from(FADE_FRAMES) * 2.0 + 0.005
}

struct Harness {
    engine: Engine,
    sink: Arc<NullSink>,
    events: Arc<Mutex<Vec<String>>>,
    loaded: Arc<AtomicU32>,
}

fn harness() -> Harness {
    let events: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let loaded = Arc::new(AtomicU32::new(0));
    let seen = Arc::clone(&events);
    let counted = Arc::clone(&loaded);
    let sink_slot: Arc<Mutex<Option<Arc<NullSink>>>> = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&sink_slot);

    let sink_events: rbl_deck::EventSink = Arc::new(move |event: DeckEvent| {
        if matches!(event, DeckEvent::Loaded { .. }) {
            counted.fetch_add(1, Ordering::SeqCst);
        }
        seen.lock().unwrap().push(format!("{event:?}"));
    });
    let engine = Engine::with_sink(
        move |render| {
            let sink = Arc::new(NullSink::new(RATE, render));
            *slot.lock().unwrap() = Some(Arc::clone(&sink));
            Ok(sink as Arc<dyn Sink>)
        },
        &sink_events,
    )
    .expect("engine");
    // The tests measure the decks' own levels: the master's decibel of
    // headroom would put a factor on every figure, so it is opened for them.
    engine.master().set_gain(1.0);

    let sink = sink_slot.lock().unwrap().clone().expect("sink");
    // These tests measure the deck path, and the ramp fixtures run to full
    // scale — over the limiter's ceiling. Off, the limiter is a fixed delay
    // of `LIMITER_TAIL` frames and nothing else; `the_limiter_*` tests turn
    // it back on.
    engine.limiter().set_enabled(false);
    Harness { engine, sink, events, loaded }
}

impl Harness {
    /// Waits for the deck to report itself loaded, rather than sleeping a
    /// guessed amount: opening a file is real I/O on another thread.
    fn wait_for_load(&self, count: u32) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.loaded.load(Ordering::SeqCst) < count {
            assert!(Instant::now() < deadline, "the deck never loaded: {:?}", self.events());
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Pulls until the deck's position has moved past `frames`, or gives up.
    ///
    /// Yields whenever a pull produced nothing new. A null sink drained in a
    /// tight loop takes the whole core, and the decode thread it is waiting on
    /// gets none of it — which on a loaded machine showed up as this returning
    /// short and the caller asserting against audio that had not arrived yet.
    /// One millisecond is nothing to the common case, where the ring is
    /// already full and the position moves every pull.
    fn play_until(&self, deck: Deck, frames: u64) -> Vec<f32> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut out = Vec::new();
        let mut last = self.position(deck);
        while self.position(deck) < frames {
            out.extend(self.sink.pull(512));
            let at = self.position(deck);
            if at == last {
                std::thread::sleep(Duration::from_millis(1));
            }
            last = at;
            if Instant::now() > deadline {
                break;
            }
        }
        out
    }

    /// `play_until`, pulled no faster than a device would: a debug build of
    /// the key-lock stretcher shifting a whole octave cannot outrun a loop
    /// that pulls as fast as it can, and an underrun is a fade, not a fault.
    fn play_until_paced(&self, deck: Deck, frames: u64) -> Vec<f32> {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut out = Vec::new();
        let mut next = Instant::now();
        while self.position(deck) < frames && Instant::now() < deadline {
            let now = Instant::now();
            if now < next {
                std::thread::sleep(next - now);
            }
            out.extend(self.sink.pull(512));
            next += Duration::from_micros(512 * 1_000_000 / u64::from(RATE));
        }
        out
    }

    fn position(&self, deck: Deck) -> u64 {
        match deck {
            Deck::A => self.engine.snapshot().a.position_frames,
            Deck::B => self.engine.snapshot().b.position_frames,
        }
    }

    fn events(&self) -> Vec<String> {
        self.events.lock().unwrap().clone()
    }
}

#[test]
fn a_loaded_deck_reports_its_length_and_stays_silent_until_it_is_played() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ramp.wav");
    ramp(&path, RATE as usize);

    let h = harness();
    h.engine.load(Deck::A, &path);
    h.wait_for_load(1);

    let snapshot = h.engine.snapshot();
    assert!(snapshot.a.loaded);
    assert_eq!(snapshot.a.total_frames, u64::from(RATE));
    assert!(!snapshot.a.playing);
    // Choosing a track must not make a sound, and a stopped device must not
    // even be pulled.
    assert!(!h.sink.running());
    assert!(h.sink.pull(256).iter().all(|s| *s == 0.0));
    assert_eq!(h.position(Deck::A), 0);
}

#[test]
fn rapid_loads_leave_only_the_newest_request_installed() {
    let dir = tempfile::tempdir().unwrap();
    let brief = dir.path().join("brief.wav");
    let long = dir.path().join("long.wav");
    ramp(&brief, RATE as usize);
    ramp(&long, RATE as usize * 3);

    let h = harness();
    // Browser selection can outrun disk opening by several tracks. Each new
    // request invalidates every older decoder result before it can replace
    // the deck, even when the same worker already started opening one.
    for request in 1..=25 {
        let path = if request % 2 == 0 { &brief } else { &long };
        h.engine.load_as(Deck::A, path, request);
    }

    let deadline = Instant::now() + Duration::from_secs(5);
    while h.engine.snapshot().a.load_id != 25 {
        assert!(Instant::now() < deadline, "the newest load never landed: {:?}", h.events());
        std::thread::sleep(Duration::from_millis(2));
    }
    let newest = h.engine.snapshot().a;
    assert!(newest.loaded);
    assert_eq!(newest.load_id, 25);
    assert_eq!(newest.total_frames, u64::from(RATE) * 3);

    // Give every queued command time to finish; none may overwrite request 25
    // after the interface has already begun acting on it.
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(h.engine.snapshot().a.load_id, 25);
    assert_eq!(h.engine.snapshot().a.total_frames, u64::from(RATE) * 3);
}

#[test]
fn playing_moves_the_clock_and_produces_the_files_own_audio() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ramp.wav");
    ramp(&path, RATE as usize);

    let h = harness();
    h.engine.load(Deck::A, &path);
    h.wait_for_load(1);
    h.engine.play(Deck::A);
    assert!(h.sink.running(), "playing must start the device");

    let audio = h.play_until(Deck::A, 4_096);
    assert!(h.position(Deck::A) >= 4_096, "the clock did not advance");
    // The ramp starts at 0.5; silence here would mean the ring never filled.
    let peak = audio.iter().fold(0.0_f32, |a, s| a.max(s.abs()));
    assert!(peak > 0.45, "peak was {peak}");
}

#[test]
fn a_start_held_for_the_beat_is_silent_for_exactly_that_long_and_then_sounds() {
    // Quantized play on a synced deck: the first sound lands the asked-for
    // number of output frames after the press, to the frame, and the clock
    // stands still until then.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ramp.wav");
    ramp(&path, RATE as usize);

    let h = harness();
    h.engine.load(Deck::A, &path);
    h.wait_for_load(1);
    // Off the start of the file, so the first frame played is not itself 0.
    h.engine.seek_frames(Deck::A, 2_048);
    std::thread::sleep(Duration::from_millis(50));
    h.engine.play_after(Deck::A, 1_300);
    assert!(h.sink.running(), "a held start still needs the device");
    assert!(h.engine.snapshot().a.playing, "the transport is playing, on hold");
    assert_eq!(h.engine.snapshot().a.start_in_frames, 1_300);

    // Three callbacks of 512: the first two and 276 frames of the third are
    // silence, and the clock has not moved.
    let mut out = Vec::new();
    for _ in 0..2 {
        out.extend(h.sink.pull(512));
    }
    assert!(out.iter().all(|s| *s == 0.0), "sound before the wait was over");
    assert_eq!(h.position(Deck::A), 2_048, "the clock moved during the wait");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut third = h.sink.pull(512);
    let mut retried = false;
    while third.iter().all(|s| *s == 0.0) && Instant::now() < deadline {
        // The ring may not have filled yet after the seek; the wait is over
        // either way, so what comes next is the first sound.
        retried = true;
        std::thread::sleep(Duration::from_millis(2));
        third = h.sink.pull(512);
    }
    assert_eq!(h.engine.snapshot().a.start_in_frames, 0);
    let first_sound = third.chunks_exact(2).position(|f| f[0] != 0.0);
    assert!(first_sound.is_some(), "no sound after the wait: {:?}", h.events());
    // When the third pull was the one the wait ran out in, the source starts
    // 276 frames into it. The master limiter adds its fixed lookahead before
    // that first audible frame, with a couple of frames for the fade-in.
    if !retried {
        let at = first_sound.unwrap();
        let expected = 276 + LIMITER_TAIL;
        assert!(
            (expected..=expected + 2).contains(&at),
            "the first sound was {at} frames in, not {expected}"
        );
    }
    assert!(h.position(Deck::A) > 2_048, "the clock did not move once the wait was over");

    // A pause clears a wait that has not run out.
    h.engine.play_after(Deck::A, 100_000);
    h.engine.pause(Deck::A);
    assert_eq!(h.engine.snapshot().a.start_in_frames, 0);
}

#[test]
fn pausing_stops_the_clock_and_the_device() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ramp.wav");
    ramp(&path, RATE as usize * 2);

    let h = harness();
    h.engine.load(Deck::A, &path);
    h.wait_for_load(1);
    h.engine.play(Deck::A);
    h.play_until(Deck::A, 2_048);

    h.engine.pause(Deck::A);
    let at = h.position(Deck::A);
    assert!(!h.sink.running(), "pausing must stop the device");

    // The stream is pulled a moment longer so the deck can fade out — see
    // `LINGER` — and what comes out of it is the fade and then silence.
    let tail = h.sink.pull(2_048);
    let fade = usize::from(FADE_FRAMES);
    assert!(
        tail
            .get((fade + STRIP_TAIL + LIMITER_TAIL) * 2..)
            .is_some_and(|rest| rest.iter().all(|s| s.abs() < INAUDIBLE)),
        "the deck was still sounding after the fade",
    );
    // The playhead moved by the fade and no further.
    assert!(
        h.position(Deck::A) - at <= u64::from(FADE_FRAMES),
        "the playhead ran on to {} from {at}",
        h.position(Deck::A),
    );
    // And it stays down: the linger is a fade, not a reprieve.
    assert!(h.sink.pull(4_096).iter().all(|s| *s == 0.0));
}

#[test]
fn a_seek_lands_exactly_rather_than_playing_what_was_already_decoded() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ramp.wav");
    // Four seconds, so a seek is far past anything the ring can hold.
    ramp(&path, RATE as usize * 4);

    let h = harness();
    h.engine.load(Deck::A, &path);
    h.wait_for_load(1);
    h.engine.play(Deck::A);
    h.play_until(Deck::A, 2_048);

    let target = u64::from(RATE) * 3;
    h.engine.seek_frames(Deck::A, target);
    // The clock says so at once, before a frame of the new position is played.
    assert_eq!(h.position(Deck::A), target);

    let audio = h.play_until(Deck::A, target + 2_048);
    assert!(h.position(Deck::A) >= target, "the deck went backwards after a seek");

    // Three seconds into a four-second ramp is 0.875, not the 0.5 the ring was
    // holding from the start of the track. Stale blocks would show up here.
    let played: Vec<f32> = audio.into_iter().filter(|s| *s != 0.0).collect();
    let last = played.iter().rev().take(512).fold(0.0_f32, |a, s| a.max(*s));
    assert!(last > 0.8, "after seeking to 3 s the audio was at {last}");
}

#[test]
fn every_jump_lands_on_the_frame_it_asked_for_wherever_it_came_from() {
    // The seek is coarse and then decoded forward, because the demuxer's
    // accurate mode reads the file from a point it knows — seconds, on a long
    // track. What that must not cost is exactness, in either direction: a
    // coarse seek that overshoots is stepped back from, and a format that
    // overshoots anyway pays for the accurate seek instead. The ramp says
    // where audio came from, so a landing that is out reads as a wrong value.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ramp.wav");
    let frames = RATE as usize * 20;
    ramp(&path, frames);

    let h = harness();
    h.engine.load(Deck::A, &path);
    h.wait_for_load(1);
    h.engine.play(Deck::A);
    h.play_until(Deck::A, 2_048);

    // Forwards to the far end, back to the start, and about the middle.
    for seconds in [19_u64, 1, 10, 2] {
        let target = u64::from(RATE) * seconds;
        h.engine.seek_frames(Deck::A, target);
        assert_eq!(h.position(Deck::A), target, "the clock did not take the seek");

        // Pulled with the decode thread given room to refill, because a null
        // sink drained flat out outruns any decoder.
        let mut out = Vec::new();
        for _ in 0..40 {
            out.extend(h.sink.pull(512));
            std::thread::sleep(Duration::from_millis(2));
        }
        let left: Vec<f32> = out.chunks_exact(2).map(|frame| frame[0]).collect();

        // The end of what came back, against where the playhead says it came
        // from. Not the join — the channel strip's filters smear it, so there
        // is no silent frame to find the new audio by — and not the target
        // either, since the deck has played on since it landed there.
        let heard = left
            .get(left.len().saturating_sub(256)..)
            .expect("nothing was played after the seek")
            .iter()
            .fold(0.0_f32, |a, s| a.max(*s));
        let at = h.position(Deck::A);
        assert!(
            at >= target && at < target + u64::from(RATE),
            "seeking to {seconds} s left the playhead at {at}, not near {target}",
        );
        let want = 0.5 + 0.5 * (at as f32 / frames as f32);
        assert!(
            (heard - want).abs() < 0.01,
            "after seeking to {seconds} s the audio was {heard}, not {want}",
        );
    }
}

#[test]
fn a_seek_while_paused_moves_the_playhead_without_starting_the_device() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ramp.wav");
    ramp(&path, RATE as usize * 2);

    let h = harness();
    h.engine.load(Deck::A, &path);
    h.wait_for_load(1);

    h.engine.seek_ms(Deck::A, 1_000.0);
    assert_eq!(h.position(Deck::A), u64::from(RATE));
    assert!(!h.sink.running());
}

#[test]
fn a_track_that_ends_stops_the_deck_rather_than_running_on() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("short.wav");
    ramp(&path, 8_000);

    let h = harness();
    h.engine.load(Deck::A, &path);
    h.wait_for_load(1);
    h.engine.play(Deck::A);

    let deadline = Instant::now() + Duration::from_secs(5);
    while h.engine.snapshot().a.playing && Instant::now() < deadline {
        h.sink.pull(512);
    }
    assert!(!h.engine.snapshot().a.playing, "the deck never stopped at the end");
    assert!(h.position(Deck::A) >= 7_500, "it stopped at {}", h.position(Deck::A));
}

#[test]
fn moving_the_master_level_does_not_step() {
    // The level is read once a callback and used for the whole of it, so a
    // hand on the fader arrives as a staircase eleven milliseconds wide —
    // audible on anything loud, and a click on a fast move. Smoothed per
    // frame, the largest step is a fraction of what an ear picks out.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("flat.wav");
    flat(&path, RATE as usize * 2);

    let h = harness();
    h.engine.load(Deck::A, &path);
    h.wait_for_load(1);
    h.engine.play(Deck::A);
    // Past the fade in, so what is left is the fader and nothing else.
    h.play_until(Deck::A, RING as u64 + 4_096);

    let mut out = Vec::new();
    // Slammed from full to silence and back, a buffer apart: the worst a hand
    // can do, and further than a hand can actually move.
    for level in [0.0_f32, 1.0, 0.2, 1.0] {
        h.engine.master().set_gain(level);
        out.extend(h.sink.pull(512));
    }
    let worst = worst_step(&out);
    assert!(worst < step_limit(), "the level stepped by {worst}");
}

#[test]
fn the_two_decks_are_independent_and_sum() {
    let dir = tempfile::tempdir().unwrap();
    let one = dir.path().join("one.wav");
    let two = dir.path().join("two.wav");
    write_wav(&one, RATE, 2, &vec![0.4_f32; 44_100 * 2]);
    write_wav(&two, RATE, 2, &vec![0.4_f32; 44_100 * 2]);

    let h = harness();
    h.engine.load(Deck::A, &one);
    h.engine.load(Deck::B, &two);
    h.wait_for_load(2);

    h.engine.play(Deck::A);
    let only_a = h.play_until(Deck::A, 4_096);
    let a_peak = only_a.iter().fold(0.0_f32, |acc, s| acc.max(s.abs()));
    assert!((a_peak - 0.4).abs() < 0.02, "one deck peaked at {a_peak}");

    h.engine.play(Deck::B);
    let both = h.play_until(Deck::B, 4_096);
    let sum_peak = both.iter().fold(0.0_f32, |acc, s| acc.max(s.abs()));
    assert!(sum_peak > 0.7, "two decks summed to {sum_peak}");
    // And B's clock ran on its own rather than following A's.
    assert!(h.position(Deck::B) > 0);
    assert!(h.position(Deck::A) > h.position(Deck::B));
}

#[test]
fn the_device_stops_when_the_last_deck_pauses() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ramp.wav");
    ramp(&path, RATE as usize);

    let h = harness();
    h.engine.load(Deck::A, &path);
    h.engine.load(Deck::B, &path);
    h.wait_for_load(2);

    h.engine.play(Deck::A);
    h.engine.play(Deck::B);
    assert!(h.sink.running());
    h.engine.pause(Deck::A);
    // One deck still playing: the device stays up.
    assert!(h.sink.running());
    h.engine.pause(Deck::B);
    assert!(!h.sink.running(), "an idle app must not keep the device running");
}

#[test]
fn a_file_that_cannot_be_decoded_reports_an_error_and_leaves_the_deck_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.txt");
    std::fs::write(&path, b"not audio").unwrap();

    let h = harness();
    h.engine.load(Deck::A, &path);

    let deadline = Instant::now() + Duration::from_secs(5);
    while h.events().is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(2));
    }
    let events = h.events();
    assert!(events.iter().any(|e| e.contains("Error")), "{events:?}");
    assert!(!h.engine.snapshot().a.loaded);
    // And play on an empty deck does nothing at all.
    h.engine.play(Deck::A);
    assert!(!h.engine.snapshot().a.playing);
    assert!(!h.sink.running());
}

/// The budget for a track to be audible after it is asked for, in
/// milliseconds. From the plan's M7-P6 list.
const LOAD_TO_AUDIO_MS: u128 = 200;

#[test]
#[ignore = "environment-sensitive: a real 200ms load-to-audio budget, dependent on real disk/decode speed"]
fn a_track_is_audible_within_the_load_budget() {
    // Load to sound, on the same path the interface uses: `load` returns
    // immediately and the decode thread opens the file, so what is measured is
    // the whole of it — the open, the first blocks, and the fade in.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ramp.wav");
    ramp(&path, RATE as usize * 30);

    let h = harness();
    let asked = Instant::now();
    h.engine.load(Deck::A, &path);
    h.wait_for_load(1);
    h.engine.play(Deck::A);

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut heard = None;
    while heard.is_none() && Instant::now() < deadline {
        if h.sink.pull(512).iter().any(|s| s.abs() > 0.01) {
            heard = Some(asked.elapsed());
        }
    }
    let took = heard.expect("the deck never made a sound").as_millis();
    assert!(took <= LOAD_TO_AUDIO_MS, "load to audio took {took} ms");
}

#[test]
fn a_deck_played_fast_covers_more_of_the_track_in_the_same_time() {
    // What a tempo control is for. The playhead is in track time, so at +50%
    // the same number of output frames has to cover half as much again of the
    // file — and at −25%, three quarters of it.
    for (tempo, expected) in [(1.5_f32, 1.5_f64), (0.75, 0.75)] {
        for master_tempo in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("ramp.wav");
            ramp(&path, RATE as usize * 20);

            let h = harness();
            h.engine.load(Deck::A, &path);
            h.wait_for_load(1);
            h.engine.set_master_tempo(Deck::A, master_tempo);
            h.engine.set_tempo(Deck::A, tempo);
            h.engine.play(Deck::A);

            // A fixed number of output frames, and where the playhead
            // reached. Pulled at a third of a device's rate: drained faster
            // than the decode thread can stretch, the ring empties and the
            // playhead measures how fast the test ran rather than the deck.
            // At the device's own rate that happened about one run in three
            // with the rest of this suite running alongside; the ratio does
            // not depend on the pull rate, only on the ring never running dry.
            let pulls = 60;
            for _ in 0..pulls {
                h.sink.pull(512);
                std::thread::sleep(Duration::from_millis(36));
            }
            let covered = h.position(Deck::A) as f64;
            let out = f64::from(pulls * 512);
            let ratio = covered / out;
            assert!(
                (ratio - expected).abs() < 0.15,
                "at {tempo}x with master tempo {master_tempo} the playhead \
                 covered {ratio:.2} of the track per frame played, not {expected}",
            );
        }
    }
}

#[test]
fn master_tempo_holds_the_pitch_and_without_it_the_pitch_moves() {
    // The difference between the two, measured on the audio rather than
    // asserted: a tone through the deck at +50%, counted by its zero
    // crossings. With the key lock it is the same tone; without it, it is a
    // record played fast.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tone.wav");
    tone(&path, RATE as usize * 10);

    let mut heard = Vec::new();
    for master_tempo in [true, false] {
        let h = harness();
        h.engine.load(Deck::A, &path);
        h.wait_for_load(1);
        h.engine.set_master_tempo(Deck::A, master_tempo);
        h.engine.set_tempo(Deck::A, 1.5);
        h.engine.play(Deck::A);

        // Pulled at about the rate a device would: drained faster than the
        // decode thread can fill the stretcher, most of it would be silence
        // and there would be too little tone to measure.
        let mut out = Vec::new();
        for _ in 0..80 {
            out.extend(h.sink.pull(512));
            std::thread::sleep(Duration::from_millis(12));
        }
        // Past the fade in, and the left channel only.
        let left: Vec<f32> = out.chunks_exact(2).skip(4_096).map(|f| f[0]).collect();
        let sounding = left.iter().filter(|s| s.abs() > 0.05).count();
        assert!(
            sounding * 4 > left.len(),
            "the deck was mostly silent: {sounding} of {} samples",
            left.len(),
        );
        heard.push(tone_hz(&left, RATE));
    }

    let (locked, free) = (heard[0], heard[1]);
    // The fixture's tone is 220 Hz. Locked it stays there; free it goes up by
    // half, which is what a record does.
    assert!((locked - 220.0).abs() < 12.0, "with master tempo the tone was {locked} Hz");
    assert!((free - 330.0).abs() < 18.0, "without it the tone was {free} Hz");
}

#[test]
fn one_deck_going_wrong_leaves_the_other_playing() {
    // The case that matters in front of an audience: a file that will not
    // decode, or one that simply ends, must not take the other deck with it.
    // A stopped device or a frozen playhead on deck B because deck A hit a bad
    // file is the difference between a mistake and a silence.
    let dir = tempfile::tempdir().unwrap();
    let broken = dir.path().join("notes.txt");
    std::fs::write(&broken, b"not audio").unwrap();
    let good = dir.path().join("ramp.wav");
    ramp(&good, RATE as usize * 4);

    let h = harness();
    h.engine.load(Deck::B, &good);
    h.wait_for_load(1);
    h.engine.play(Deck::B);
    h.play_until(Deck::B, 4_096);
    let before = h.position(Deck::B);

    // Deck A is handed something that cannot be decoded, and asked to play it.
    h.engine.load(Deck::A, &broken);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !h.events().iter().any(|e| e.contains("Error")) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(h.events().iter().any(|e| e.contains("Error")), "{:?}", h.events());
    h.engine.play(Deck::A);

    // Deck B carries on: the device is still running, the playhead is still
    // moving, and there is still audio coming out of it.
    let audio = h.play_until(Deck::B, before + 4_096);
    assert!(h.sink.running(), "the device stopped when the other deck failed");
    assert!(h.position(Deck::B) > before, "deck B's playhead stopped");
    let peak = audio.iter().fold(0.0_f32, |a, s| a.max(s.abs()));
    assert!(peak > 0.4, "deck B went quiet: {peak}");
}

#[test]
fn a_track_ending_on_one_deck_leaves_the_other_playing() {
    // The same property, for the ordinary way a deck runs out: a track that
    // ends is not an error, and the deck beside it must not notice.
    let dir = tempfile::tempdir().unwrap();
    let brief = dir.path().join("brief.wav");
    ramp(&brief, RATE as usize / 4);
    let long = dir.path().join("long.wav");
    ramp(&long, RATE as usize * 4);

    let h = harness();
    h.engine.load(Deck::A, &brief);
    h.wait_for_load(1);
    h.engine.load(Deck::B, &long);
    h.wait_for_load(2);
    h.engine.play(Deck::A);
    h.engine.play(Deck::B);

    // Past the end of the short one.
    h.play_until(Deck::B, u64::from(RATE) / 2);
    assert!(h.engine.snapshot().a.position_frames >= u64::from(RATE) / 4 - 4_096);

    let before = h.position(Deck::B);
    let audio = h.play_until(Deck::B, before + 8_192);
    assert!(h.sink.running(), "the device stopped when the short track ended");
    assert!(h.position(Deck::B) > before, "deck B's playhead stopped");
    let peak = audio.iter().fold(0.0_f32, |a, s| a.max(s.abs()));
    assert!(peak > 0.4, "deck B went quiet: {peak}");
}

#[test]
fn the_metronome_clicks_on_the_grid_and_only_while_playing() {
    // Two seconds of silence, so anything heard is the click.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("silence.wav");
    write_wav(&path, RATE, 2, &vec![0.0_f32; RATE as usize * 2 * 2]);
    let h = harness();
    h.engine.load(Deck::A, &path);
    h.wait_for_load(1);
    // Beats every half second, the first a downbeat.
    h.engine.set_metronome_grid(Deck::A, &[(0, true), (500, false), (1000, false), (1500, false)]);

    // Off: silence stays silence.
    h.engine.play(Deck::A);
    let quiet = h.play_until(Deck::A, u64::from(RATE) / 4);
    assert!(quiet.iter().all(|s| s.abs() < 1e-6), "no click with the metronome off");

    // On: the beat at 0.5 s is heard, and the silence between beats is silent.
    h.engine.set_metronome(Deck::A, true);
    assert!(h.engine.metronome_on(Deck::A));
    let out = h.play_until(Deck::A, u64::from(RATE) * 3 / 4);
    let loudest = out.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
    assert!(loudest > 0.05, "the click is heard: {loudest}");
    // The window began at 0.25 s: the first frames, before the beat, are silent.
    assert!(out[..1000].iter().all(|s| s.abs() < 1e-6), "nothing before the beat");

    // Paused: no clicks, however many beats the grid holds.
    h.engine.pause(Deck::A);
    std::thread::sleep(Duration::from_millis(50));
    let mut paused = Vec::new();
    for _ in 0..40 {
        paused.extend(h.sink.pull(512));
    }
    // The pause's own fade and the click's tail are over well within 40 pulls.
    let tail = &paused[paused.len() / 2..];
    assert!(tail.iter().all(|s| s.abs() < 1e-4), "a paused deck does not click");
}

/// Frequency of a tone from the median spacing of its rising zero crossings.
///
/// The median rather than a count over the window, because these tests pull
/// the null sink from a thread that can be descheduled on a busy machine. Each
/// underrun leaves a run of silence and a broken cycle either side of it, and
/// a count over the window measures those as well as the tone. The cycles the
/// deck did play are still the right length, and they are most of them.
fn tone_hz(left: &[f32], rate: u32) -> f32 {
    let rising: Vec<usize> = left
        .windows(2)
        .enumerate()
        .filter(|(_, w)| w[0] <= 0.0 && w[1] > 0.0)
        .map(|(i, _)| i)
        .collect();
    let mut periods: Vec<usize> = rising.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(periods.len() >= 8, "too few cycles to measure: {}", periods.len());
    periods.sort_unstable();
    rate as f32 / periods[periods.len() / 2] as f32
}

/// Frequency over the settled middle of a tone, left channel.
fn hz_of(out: &[f32], rate: u32) -> f32 {
    let left: Vec<f32> = out.chunks_exact(2).map(|f| f[0]).collect();
    tone_hz(&left[left.len() / 4..left.len() * 3 / 4], rate)
}

#[test]
#[cfg_attr(not(feature = "rubberband"), ignore = "only the Rubber Band backend shifts a key")]
fn a_key_shift_of_an_octave_doubles_the_pitch_and_keeps_the_tempo() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tone.wav");
    tone(&path, RATE as usize * 4);
    let h = harness();
    h.engine.limiter().set_enabled(false);
    h.engine.load(Deck::A, &path);
    h.wait_for_load(1);
    assert!(rbl_deck::Engine::shifts_key());

    // Master Tempo off, an octave up: the pitch doubles and the speed does not.
    h.engine.set_key_shift(Deck::A, 12);
    // The deck's own thread takes the shift; the snapshot says when it has.
    let deadline = Instant::now() + Duration::from_secs(2);
    while h.engine.snapshot().a.key_shift != 12 {
        assert!(Instant::now() < deadline, "the key shift never reached the deck");
        std::thread::sleep(Duration::from_millis(2));
    }
    h.engine.play(Deck::A);
    let out = h.play_until_paced(Deck::A, u64::from(RATE) * 2);
    // The last part only: what the ring held before the shift reached the
    // deck plays first, at the track's own pitch.
    let hz = hz_of(&out[out.len() * 3 / 4..], RATE);
    assert!((hz - 440.0).abs() < 8.0, "an octave up from 220 Hz reads {hz} Hz");
    // Two seconds of track took about two seconds of output: the tempo held.
    // Only frames that carry audio count, since an underrun on a busy machine
    // pads the output with silence that is not the deck playing slower.
    let frames_out = out.chunks_exact(2).filter(|f| f[0] != 0.0 || f[1] != 0.0).count();
    assert!((frames_out as f32 / RATE as f32 - 2.0).abs() < 0.25, "{frames_out} output frames for two seconds of track");

    // Back to the track's own key, still at its own speed.
    h.engine.set_key_shift(Deck::A, 0);
    let out = h.play_until_paced(Deck::A, u64::from(RATE) * 3);
    let hz = hz_of(&out[out.len() * 3 / 4..], RATE);
    assert!((hz - 220.0).abs() < 8.0, "the track's own 220 Hz reads {hz} Hz");
}

#[test]
fn unloading_clears_the_deck() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ramp.wav");
    ramp(&path, RATE as usize);

    let h = harness();
    h.engine.load(Deck::A, &path);
    h.wait_for_load(1);
    h.engine.play(Deck::A);
    h.play_until(Deck::A, 1_024);

    h.engine.unload(Deck::A);
    let deadline = Instant::now() + Duration::from_secs(5);
    while {
        let deck = h.engine.snapshot().a;
        (deck.loaded || deck.position_frames != 0) && Instant::now() < deadline
    } {
        std::thread::sleep(Duration::from_millis(2));
    }
    let snapshot = h.engine.snapshot();
    assert!(!snapshot.a.loaded);
    assert!(!snapshot.a.playing);
    assert_eq!(snapshot.a.position_frames, 0);
}

#[test]
fn a_seek_is_honoured_at_once_even_while_the_decoder_runs_flat_out() {
    // A minute, so the far end is far past anything already decoded, and a
    // deck that decoded its way there would take a very long time about it.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("long.wav");
    // At 48 kHz against a 44.1 kHz device, so the resampler runs: that is what
    // most of the library will do on a real machine, and a deck that decodes
    // faster than anything can drain it never starves its own commands.
    ramp_at(&path, 48_000, 48_000 * 60);

    let h = harness();
    h.engine.load(Deck::A, &path);
    h.wait_for_load(1);
    // Straight into playing and seeking, with nothing waited for in between:
    // the decode thread is filling from the start of the track at the moment
    // the seek arrives, which is when it used to ignore it.
    h.engine.play(Deck::A);

    // Fifty seconds into a sixty-second ramp is 0.917; the start is 0.5.
    let target = u64::from(RATE) * 50;
    h.engine.seek_frames(Deck::A, target);

    // Frames of audio from the *old* position that get played before the new
    // position arrives. Silence is not counted: an underrun while the decode
    // thread refills is a different thing from playing the wrong music, and
    // counting pulls rather than audio would just measure how busy the machine
    // is.
    let mut stale = 0_u64;
    let mut arrived = false;
    let deadline = Instant::now() + Duration::from_secs(20);
    while !arrived && Instant::now() < deadline {
        // A whole ring at a time, which is what makes this a test of the
        // starvation: a consumer that takes less than the decode thread
        // produces lets the ring fill, and a full ring is what used to be the
        // only thing that sent the thread back to its channel.
        let audio = h.sink.pull(RING);
        arrived = audio.iter().any(|sample| *sample > 0.9);
        stale += audio.iter().filter(|sample| **sample > 0.4 && **sample < 0.9).count() as u64 / 2;
    }
    assert!(arrived, "audio from the new position never arrived");
    // A ring is 8,192 frames, and the decode thread checks its channel once a
    // ring, so two is the most that can already be in flight. Anything near a
    // second means the deck decoded its way to the seek point instead of
    // jumping — which is what happened while the decode loop could starve its
    // own command channel.
    assert!(stale < u64::from(RATE) / 2, "played {stale} frames from the old position");
}

#[test]
fn starting_opens_from_silence_rather_than_stepping_into_the_music() {
    // Press play on a loud passage and the first sample handed to the device
    // is whatever the waveform was doing. Straight from silence, that is a
    // click; this is the fade that makes it a start.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("flat.wav");
    flat(&path, RATE as usize);

    let h = harness();
    h.engine.load(Deck::A, &path);
    h.wait_for_load(1);

    // The silence the device was putting out before play was pressed. Without
    // it the measurement starts at the first sample of music and has nothing
    // to compare it to, which is the one thing this test is about.
    let mut audio = h.sink.pull(64);
    assert!(audio.iter().all(|s| *s == 0.0), "a stopped deck must be silent");
    h.engine.play(Deck::A);
    audio.extend(h.play_until(Deck::A, 4_096));

    let peak = audio.iter().fold(0.0_f32, |acc, s| acc.max(s.abs()));
    assert!(peak > FLAT - 0.05, "the deck never reached full level: {peak}");
    let worst = worst_step(&audio);
    assert!(worst <= step_limit(), "starting stepped by {worst}");
}

#[test]
fn stopping_closes_to_silence_rather_than_cutting_the_waveform() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("flat.wav");
    flat(&path, RATE as usize * 2);

    let h = harness();
    h.engine.load(Deck::A, &path);
    h.wait_for_load(1);
    h.engine.play(Deck::A);
    let mut audio = h.play_until(Deck::A, 8_192);

    h.engine.pause(Deck::A);
    audio.extend(h.sink.pull(2_048));

    let worst = worst_step(&audio);
    assert!(worst <= step_limit(), "stopping stepped by {worst}");
    // And it ends at zero, which is the only place silence can start from.
    assert_eq!(audio.last().copied(), Some(0.0));
}

#[test]
fn a_seek_neither_cuts_what_was_playing_nor_opens_on_the_new_position() {
    // Cueing, jumping a beat and dragging the overview are all this seek. Both
    // ends of it are a step in the waveform without the fade: the old position
    // is cut wherever it was, and the new one opens wherever it starts.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("flat.wav");
    flat(&path, RATE as usize * 8);

    let h = harness();
    h.engine.load(Deck::A, &path);
    h.wait_for_load(1);
    h.engine.play(Deck::A);
    let mut audio = h.play_until(Deck::A, 8_192);

    // Far past anything the ring holds, so the deck really does jump.
    h.engine.seek_frames(Deck::A, u64::from(RATE) * 5);
    let landed = h.play_until(Deck::A, u64::from(RATE) * 5 + 8_192);
    audio.extend(landed);

    let worst = worst_step(&audio);
    assert!(worst <= step_limit(), "the seek stepped by {worst}");
    // The music did come back, rather than the deck simply going quiet.
    let peak = audio
        .chunks_exact(2)
        .skip(8_192)
        .map(|frame| frame[0].abs())
        .fold(0.0_f32, f32::max);
    assert!(peak > FLAT - 0.05, "nothing was heard after the seek: {peak}");
}

#[test]
fn several_seeks_in_a_row_still_do_not_click() {
    // A beat jump held down is a seek every few milliseconds, arriving while
    // the last one is still fading. Each one has to wait its turn rather than
    // cutting the fade in front of it.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("flat.wav");
    flat(&path, RATE as usize * 8);

    let h = harness();
    h.engine.load(Deck::A, &path);
    h.wait_for_load(1);
    h.engine.play(Deck::A);
    let mut audio = h.play_until(Deck::A, 4_096);

    for beat in 1..=6_u64 {
        h.engine.seek_frames(Deck::A, u64::from(RATE) * beat / 2);
        audio.extend(h.sink.pull(256));
    }
    audio.extend(h.play_until(Deck::A, u64::from(RATE) * 3 + 4_096));

    let worst = worst_step(&audio);
    assert!(worst <= step_limit(), "a run of seeks stepped by {worst}");
}

#[test]
fn the_end_of_a_track_fades_rather_than_being_cut_off() {
    // The last sample of a file is wherever the music was, and the silence
    // after it is a step down from there.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("short.wav");
    flat(&path, 8_000);

    let h = harness();
    h.engine.load(Deck::A, &path);
    h.wait_for_load(1);
    h.engine.play(Deck::A);

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut audio = Vec::new();
    while h.engine.snapshot().a.playing && Instant::now() < deadline {
        audio.extend(h.sink.pull(512));
    }
    assert!(!h.engine.snapshot().a.playing, "the deck never stopped at the end");
    let worst = worst_step(&audio);
    assert!(worst <= step_limit(), "the end of the track stepped by {worst}");
}

/// A 220 Hz sine, which steps between neighbouring samples all by itself.
fn tone(path: &Path, frames: usize) {
    let mut samples = Vec::with_capacity(frames * 2);
    for frame in 0..frames {
        let t = frame as f32 / RATE as f32;
        let value = 0.8 * (t * 220.0 * std::f32::consts::TAU).sin();
        samples.push(value);
        samples.push(value);
    }
    write_wav(path, RATE, 2, &samples);
}

/// How far a step has to stand out from its neighbours to be a click.
///
/// An absolute threshold says nothing about music: a loud high note steps by
/// most of full scale between samples all by itself. A click is a step that
/// does not belong to what is around it. The same measure `scrubcheck` uses.
const CLICK_RATIO: f32 = 12.0;

/// Samples either side a step is compared against: about two milliseconds.
const NEIGHBOURHOOD: usize = 96;

/// Steps that stand out from the music around them.
fn clicks(out: &[f32]) -> usize {
    let frames: Vec<f32> = out.chunks_exact(2).map(|frame| frame[0]).collect();
    let steps: Vec<f32> = frames.windows(2).map(|pair| (pair[1] - pair[0]).abs()).collect();
    let mut found = 0;
    for i in NEIGHBOURHOOD..steps.len().saturating_sub(NEIGHBOURHOOD) {
        let around: f32 = steps[i - NEIGHBOURHOOD..i]
            .iter()
            .chain(&steps[i + 1..i + 1 + NEIGHBOURHOOD])
            .sum::<f32>()
            / (NEIGHBOURHOOD * 2) as f32;
        // A silent neighbourhood has nothing to stand out from.
        if around > 1e-4 && steps[i] > around * CLICK_RATIO {
            found += 1;
        }
    }
    found
}

#[test]
fn no_transport_move_stands_out_from_the_music_around_it() {
    // The flat-track tests measure the step exactly; this one asks the
    // question the way an ear does, on a signal that is stepping anyway.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tone.wav");
    tone(&path, RATE as usize * 8);

    let h = harness();
    h.engine.load(Deck::A, &path);
    h.wait_for_load(1);

    let mut audio = h.sink.pull(64);
    h.engine.play(Deck::A);
    audio.extend(h.play_until(Deck::A, 8_192));
    // A cue, then a beat jump, then the overview dropped somewhere else.
    for at in [u64::from(RATE) * 4, u64::from(RATE) * 2, u64::from(RATE) * 6] {
        h.engine.seek_frames(Deck::A, at);
        audio.extend(h.play_until(Deck::A, at + 8_192));
    }
    h.engine.pause(Deck::A);
    audio.extend(h.sink.pull(2_048));

    let found = clicks(&audio);
    assert_eq!(found, 0, "{found} steps stood out from the music");
    assert_eq!(audio.last().copied(), Some(0.0), "the stream must end at silence");
}

#[test]
fn a_release_onto_the_cue_lands_on_it_or_after_it_but_never_before() {
    // Letting go of CUE: the deck is previewing, and the release seeks back to
    // the cue point and pauses in the same breath. The playhead must not end up
    // *before* the cue — the report was "sometimes when I release the C key it
    // rewinds too far", measured landing 36 ms in front of it, which is outside
    // `CUE_TOLERANCE` and so moved the cue point there on the next press.
    //
    // **This does not reproduce the intermittent case.** That needs the control
    // thread to move the clock while the audio callback is mid-block on the
    // ring it just invalidated, and the window is too narrow to open on demand
    // from a test that owns the callback. It was measured in the app instead:
    // worst error 36 ms before the fixes, 4.8 ms after, and the cue point stops
    // drifting. What is left here is the invariant, which a gross regression
    // would still break.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ramp.wav");
    ramp(&path, RATE as usize * 4);

    let h = harness();
    h.engine.load(Deck::A, &path);
    h.wait_for_load(1);

    let cue = u64::from(RATE) * 2;
    for attempt in 0..20 {
        h.engine.seek_frames(Deck::A, cue);
        h.engine.play(Deck::A);
        h.play_until(Deck::A, cue + 1_024);

        h.engine.seek_frames(Deck::A, cue);
        h.engine.pause(Deck::A);
        for _ in 0..8 {
            let _ = h.sink.pull(512);
        }
        let landed = h.position(Deck::A);
        assert!(
            landed >= cue,
            "attempt {attempt}: released onto {landed}, {} frames before the cue at {cue}",
            cue.saturating_sub(landed),
        );
    }
}

/// Drags the head from `from` to `to` over `steps` pointer moves a frame's
/// worth apart, pulling the sink as a device would, and returns what came out.
fn drag(h: &Harness, deck: Deck, from: u64, to: u64, steps: u64) -> Vec<f32> {
    let ms = |frames: u64| frames as f64 * 1000.0 / f64::from(RATE);
    h.engine.scrub_begin(deck);
    let mut out = Vec::new();
    for step in 0..=steps {
        let at = from + (to - from) * step / steps;
        h.engine.scrub_to_ms(deck, ms(at));
        // A pointer move every ~12 ms, the way a trackpad delivers them, and
        // the callback draining between them.
        let until = Instant::now() + Duration::from_millis(12);
        while Instant::now() < until {
            out.extend(h.sink.pull(512));
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    h.engine.scrub_end(deck);
    out
}

#[test]
fn a_drag_sounds_on_a_deck_that_has_only_been_loaded_never_played() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tone.wav");
    tone(&path, RATE as usize * 4);

    let h = harness();
    h.engine.load(Deck::A, &path);
    h.wait_for_load(1);
    assert!(!h.sink.running());

    // Straight from the load: no play, no pause, a hand on the overview.
    let audio = drag(&h, Deck::A, u64::from(RATE), u64::from(RATE) * 2, 40);
    assert!(h.sink.running() || !audio.is_empty(), "the drag never started the device");
    let peak = audio.iter().fold(0.0_f32, |a, s| a.max(s.abs()));
    assert!(peak > 0.1, "a drag on a loaded deck made no sound: peak {peak}");
    let landed = h.position(Deck::A);
    let target = u64::from(RATE) * 2;
    assert!(
        landed.abs_diff(target) < u64::from(RATE) / 10,
        "the drag left the head at {landed}, not near {target}"
    );
    // Letting go leaves the transport as it was found: stopped, and the
    // device released.
    assert!(!h.engine.snapshot().a.playing);
}

/// The tone at full scale, so two of them sum to twice what the device takes.
fn loud_tone(path: &Path, frames: usize) {
    let mut samples = Vec::with_capacity(frames * 2);
    for frame in 0..frames {
        let t = frame as f32 / RATE as f32;
        let value = 0.99 * (t * 220.0 * std::f32::consts::TAU).sin();
        samples.push(value);
        samples.push(value);
    }
    write_wav(path, RATE, 2, &samples);
}

/// Both decks playing the same full-scale tone, pulled for a moment.
fn two_decks_summed(h: &Harness) -> Vec<f32> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("loud.wav");
    loud_tone(&path, RATE as usize * 4);
    h.engine.load(Deck::A, &path);
    h.engine.load(Deck::B, &path);
    h.wait_for_load(2);
    h.engine.play(Deck::A);
    h.engine.play(Deck::B);
    // Past the fade in, input smoothing, and the 250 ms limiter release.
    h.play_until(Deck::A, u64::from(RATE));
    // Measure the overlap, excluding the initial input-gain smoothing ramp.
    h.engine.master().reduction_db();
    let mut out = Vec::new();
    for _ in 0..40 {
        out.extend(h.sink.pull(512));
        std::thread::sleep(Duration::from_millis(2));
    }
    out
}

#[test]
fn the_limiter_keeps_two_full_decks_under_its_ceiling() {
    let h = harness();
    h.engine.limiter().set_enabled(true);
    let out = two_decks_summed(&h);
    let ceiling = 10.0_f32.powf(rbl_deck::DEFAULT_CEILING_DB / 20.0);
    let loudest = out.iter().fold(0.0_f32, |a, s| a.max(s.abs()));
    assert!(loudest <= ceiling + 1e-5, "{loudest} reached the device, over {ceiling}");
    assert!(loudest > ceiling * 0.9, "{loudest}: the sum was turned down, not limited");
    // Four dB of input headroom leaves about two dB to catch with both decks.
    let reduction = h.engine.master().reduction_db();
    assert!((1.2..2.5).contains(&reduction), "the meter read {reduction} dB of reduction");
    // And no flat tops: the sum is a sine, and a sine's steps are smooth.
    assert_eq!(clicks(&out), 0);
}

#[test]
fn the_limiter_off_leaves_the_clamp_to_flat_top_the_sum() {
    let h = harness();
    let out = two_decks_summed(&h);
    let flat = out.iter().filter(|s| s.abs() >= 1.0).count();
    assert!(flat > 100, "only {flat} samples hit the rail: the sum was not clipped");
    assert!(h.engine.master().reduction_db() < 1e-6, "off, the limiter reported work");
}

#[test]
fn a_loop_rounds_at_its_out_point_with_no_gap_and_exit_plays_on() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ramp.wav");
    // Four seconds of a ramp, so the audio says where in the file it is.
    ramp(&path, RATE as usize * 4);

    let h = harness();
    h.engine.load(Deck::A, &path);
    h.wait_for_load(1);
    // A loop from 1 s to 1.5 s, set before play so the first pass is exact.
    let from = u64::from(RATE);
    let to = u64::from(RATE) + u64::from(RATE) / 2;
    h.engine.set_loop_frames(Deck::A, from, to);
    assert_eq!(h.position(Deck::A), from, "setting a loop behind the head sends it to the in point");
    let snap = h.engine.snapshot().a;
    assert_eq!((snap.loop_in_frames, snap.loop_out_frames, snap.looping), (from, to, true));

    h.engine.play(Deck::A);
    // Three passes' worth of audio: the head must never reach the out point.
    let audio = h.play_until_paced(Deck::A, from + 100);
    let mut out = audio;
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut highest = 0;
    let mut pulled = 0_usize;
    while pulled < RATE as usize * 3 / 2 && Instant::now() < deadline {
        let chunk = h.sink.pull(512);
        pulled += chunk.len() / 2;
        out.extend(chunk);
        highest = highest.max(h.position(Deck::A));
        std::thread::sleep(Duration::from_micros(512 * 1_000_000 / u64::from(RATE)));
    }
    assert!(highest < to + 600, "the head ran past the out point to {highest} (out is {to})");
    assert!(highest >= from, "the head never reached the loop");
    // Past the fade-in, the audio stayed at the ramp's 0.625..0.6875 band:
    // nothing from outside the loop was heard. The seam is the ramp's own
    // step from 0.6875 back to 0.625, which the output smoother rings on for
    // a hundred frames, and never a silence.
    let played: Vec<f32> = out.into_iter().skip(4096).filter(|s| *s != 0.0).collect();
    assert!(played.len() > RATE as usize, "not enough audio to judge: {}", played.len());
    let (lo, hi) = played.iter().fold((1.0_f32, 0.0_f32), |(lo, hi), &s| (lo.min(s), hi.max(s)));
    assert!(lo > 0.58 && hi < 0.74, "the loop played audio from {lo} to {hi} of the ramp");
    let silent = played.windows(64).any(|w| w.iter().all(|s| s.abs() < 1e-3));
    assert!(!silent, "a loop seam went silent");

    // EXIT: the head carries on past the out point.
    h.engine.set_looping(Deck::A, false);
    h.play_until_paced(Deck::A, to + 4_096);
    assert!(h.position(Deck::A) > to, "after EXIT the head stayed inside the loop");
    assert!(!h.engine.snapshot().a.looping);
    // RELOOP: back to the in point, looping again.
    h.engine.set_looping(Deck::A, true);
    assert_eq!(h.position(Deck::A), from);
    assert!(h.engine.snapshot().a.looping);
    h.engine.clear_loop(Deck::A);
    assert_eq!(h.engine.snapshot().a.loop_out_frames, 0);
}

#[test]
fn the_metronome_keeps_its_volume_when_the_master_is_turned_down() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("silence.wav");
    write_wav(&path, RATE, 2, &vec![0.0_f32; RATE as usize * 2 * 2]);
    let h = harness();
    h.engine.load(Deck::A, &path);
    h.wait_for_load(1);
    h.engine.set_metronome_grid(Deck::A, &[(0, true), (500, false), (1000, false), (1500, false)]);
    h.engine.set_metronome(Deck::A, true);
    h.engine.play(Deck::A);
    let loud = h.play_until(Deck::A, u64::from(RATE) * 3 / 4);
    let at_full = loud.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);

    // The master down to a tenth: the click at 1.0 s is as loud as the one
    // at 0.5 s was, because the click joins after the master level.
    h.engine.master().set_gain(0.1);
    let quiet = h.play_until(Deck::A, u64::from(RATE) * 5 / 4);
    let at_tenth = quiet.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
    assert!(at_full > 0.05, "the click is heard at full: {at_full}");
    assert!((at_tenth - at_full).abs() < 0.02, "the click changed with the master: {at_full} then {at_tenth}");
}

#[test]
fn negative_seek_is_bounded_pauses_and_plays_through_zero() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lead-in.wav");
    flat(&path, RATE as usize * 2);
    let h = harness();
    h.engine.load(Deck::A, &path);
    h.wait_for_load(1);
    h.engine.seek_ms(Deck::A, -9000.0);
    assert_eq!(h.engine.snapshot().a.pre_roll_frames, u64::from(RATE) * 5);
    h.sink.pull(512);
    assert_eq!(h.engine.snapshot().a.pre_roll_frames, u64::from(RATE) * 5);
    h.engine.seek_ms(Deck::A, -100.0);
    std::thread::sleep(Duration::from_millis(30));
    h.engine.play(Deck::A);
    let silence = h.sink.pull(4410);
    assert!(silence.iter().all(|sample| sample.abs() < INAUDIBLE));
    assert_eq!(h.engine.snapshot().a.pre_roll_frames, 0);
    assert_eq!(h.position(Deck::A), 0);
    let audio = h.play_until(Deck::A, 1024);
    assert!(audio.iter().any(|sample| sample.abs() > 0.1));
    assert!(
        h.position(Deck::A) < 2048,
        "lead-in must not consume the song"
    );
}

#[test]
fn negative_scrub_lands_before_zero_and_positive_seek_clears_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scrub-lead-in.wav");
    flat(&path, RATE as usize * 2);
    let h = harness();
    h.engine.load(Deck::A, &path);
    h.wait_for_load(1);
    h.engine.scrub_begin(Deck::A);
    h.engine.scrub_to_ms(Deck::A, -8000.0);
    h.engine.scrub_end(Deck::A);
    let deadline = Instant::now() + Duration::from_secs(5);
    while h.engine.snapshot().a.pre_roll_frames == 0 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(h.engine.snapshot().a.pre_roll_frames, u64::from(RATE) * 5);
    assert_eq!(h.position(Deck::A), 0);
    assert!(!h.engine.snapshot().a.playing);
    h.engine.seek_ms(Deck::A, 500.0);
    assert_eq!(h.engine.snapshot().a.pre_roll_frames, 0);
}


#[test]
fn lead_in_obeys_tempo_and_pause() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tempo-lead-in.wav");
    flat(&path, RATE as usize * 2);
    let h = harness();
    h.engine.load(Deck::A, &path);
    h.wait_for_load(1);
    h.engine.set_tempo(Deck::A, 1.5);
    h.engine.seek_ms(Deck::A, -1000.0);
    std::thread::sleep(Duration::from_millis(30));
    h.engine.play(Deck::A);
    h.sink.pull(4410);
    assert_eq!(h.engine.snapshot().a.pre_roll_frames, 44100 - 6615);
    h.engine.pause(Deck::A);
    h.sink.pull(4410);
    assert_eq!(h.engine.snapshot().a.pre_roll_frames, 44100 - 6615);
    h.engine.play(Deck::A);
    h.sink.pull(4410);
    assert_eq!(h.engine.snapshot().a.pre_roll_frames, 44100 - 13230);
}
