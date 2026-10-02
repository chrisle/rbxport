//! What the interface reads to know where a deck is.
//!
//! The audio callback is the only writer of `position`, and it writes it once
//! per callback rather than per frame. Everything else here is written by the
//! control side and read by both. Atomics rather than a lock: the callback is
//! realtime and must never wait for a reader.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};

/// One deck's shared state.
#[derive(Debug, Default)]
pub struct DeckClock {
    /// Frames of output produced since the track was loaded, at the device
    /// rate — the position of the playhead.
    position: AtomicU64,
    pre_roll: AtomicU64,
    /// The track's length in device-rate frames, or 0 when it is not known.
    total: AtomicU64,
    /// Bumped on every load and every seek. Blocks in the ring carry the
    /// generation they were decoded under, so the callback can drop the ones
    /// that belong to where the playhead used to be.
    generation: AtomicU32,
    /// The device rate, which is the rate everything downstream of the
    /// resampler is in.
    sample_rate: AtomicU32,
    playing: AtomicBool,
    scrubbing: AtomicBool,
    loaded: AtomicBool,
    /// The newest load the control side asked for. A decoder that finishes an
    /// older request discards it instead of installing the wrong track.
    requested_load: AtomicU64,
    /// The request that is actually installed, or 0 while empty/loading.
    load_id: AtomicU64,
    /// Serializes publishing a result with invalidating it. The audio thread
    /// never takes this; it is only shared by the caller and decode worker.
    load_gate: std::sync::Mutex<()>,
    /// The decode thread has pushed the last block it will push. The callback
    /// stops the deck when it has drained what is left.
    end_of_stream: AtomicBool,
    /// How fast the deck is playing, as a multiple of the file's own speed,
    /// in an `f32`'s bits. Read by the interface for the tempo readout, and by
    /// anything extrapolating the playhead between ticks.
    tempo: AtomicU32,
    master_tempo: AtomicBool,
    /// The key shift in semitones, as an `i8` widened: 0 is the track's own
    /// key. Read by the interface for its key readout.
    key_shift: AtomicI32,
    /// Output frames still to pass before a started deck makes a sound.
    ///
    /// Quantized play on a synced deck: the deck is playing as far as the
    /// transport is concerned, but the callback lets this many frames go by
    /// in silence first, so the first sound lands on the master's beat. Set
    /// with the start, counted down by the callback, and cleared by a pause,
    /// a seek or a load.
    start_in: AtomicU64,
    /// The loop's in and out points in device-rate frames, and whether the
    /// deck is inside it. A range of `0..0` is no loop. The decode thread
    /// reads these on every block and jumps back at the out point; the
    /// callback never looks at them.
    loop_in: AtomicU64,
    loop_out: AtomicU64,
    looping: AtomicBool,
}

/// A deck's state at one instant, for the tick the interface extrapolates from.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(clippy::struct_excessive_bools, reason = "a snapshot of the deck's flags, read together")]
pub struct DeckSnapshot {
    pub position_frames: u64,
    /// Track-rate frames of silence remaining before time zero.
    pub pre_roll_frames: u64,
    pub total_frames: u64,
    pub generation: u32,
    pub sample_rate: u32,
    pub playing: bool,
    pub loaded: bool,
    /// Which load request is installed, or 0 while empty/loading.
    pub load_id: u64,
    /// A multiple of the file's own speed: 1.0 is the track as recorded.
    pub tempo: f32,
    /// Whether the pitch is held while the speed changes.
    pub master_tempo: bool,
    /// Semitones the key is shifted by; 0 is the track's own.
    pub key_shift: i8,
    /// Output frames until a started deck sounds; 0 once it is under way.
    pub start_in_frames: u64,
    /// The loop's in and out points, device-rate frames; `0..0` for none.
    pub loop_in_frames: u64,
    pub loop_out_frames: u64,
    /// Whether the deck is inside the loop: RELOOP on, EXIT off.
    pub looping: bool,
}

impl DeckClock {
    pub fn snapshot(&self) -> DeckSnapshot {
        DeckSnapshot {
            position_frames: self.position.load(Ordering::Relaxed),
            pre_roll_frames: self.pre_roll(),
            total_frames: self.total.load(Ordering::Relaxed),
            generation: self.generation.load(Ordering::Relaxed),
            sample_rate: self.sample_rate.load(Ordering::Relaxed),
            playing: self.playing.load(Ordering::Relaxed),
            loaded: self.loaded.load(Ordering::Relaxed),
            load_id: self.load_id(),
            tempo: self.tempo(),
            master_tempo: self.master_tempo(),
            key_shift: self.key_shift(),
            start_in_frames: self.start_in.load(Ordering::Relaxed),
            loop_in_frames: self.loop_in.load(Ordering::Relaxed),
            loop_out_frames: self.loop_out.load(Ordering::Relaxed),
            looping: self.looping.load(Ordering::Relaxed),
        }
    }

    /// The loop, when there is one: `(in, out)` with in before out.
    pub fn loop_range(&self) -> Option<(u64, u64)> {
        let (from, to) = (self.loop_in.load(Ordering::Relaxed), self.loop_out.load(Ordering::Relaxed));
        (to > from).then_some((from, to))
    }

    /// Sets the loop; `None` clears it and leaves the loop off.
    pub fn set_loop(&self, range: Option<(u64, u64)>) {
        let (from, to) = range.filter(|(from, to)| to > from).unwrap_or((0, 0));
        // Out first: a reader between the two stores sees either the old
        // range or `from..old_out`, never a range that ends before it starts
        // when the new one lies past the old.
        self.loop_out.store(to, Ordering::Relaxed);
        self.loop_in.store(from, Ordering::Relaxed);
        if range.is_none() {
            self.looping.store(false, Ordering::Relaxed);
        }
    }

    pub fn looping(&self) -> bool {
        self.looping.load(Ordering::Relaxed)
    }

    pub fn set_looping(&self, on: bool) {
        self.looping.store(on && self.loop_range().is_some(), Ordering::Relaxed);
    }

    /// Frames of silence the callback has still to let pass before the deck
    /// sounds.
    pub fn start_in(&self) -> u64 {
        self.start_in.load(Ordering::Relaxed)
    }

    pub fn set_start_in(&self, frames: u64) {
        self.start_in.store(frames, Ordering::Relaxed);
    }

    /// Lets up to `frames` of the wait pass; returns how many did.
    pub fn pass_start(&self, frames: u64) -> u64 {
        let waiting = self.start_in.load(Ordering::Relaxed);
        let passed = waiting.min(frames);
        if passed > 0 {
            self.start_in.store(waiting - passed, Ordering::Relaxed);
        }
        passed
    }

    /// A tempo of zero means nothing has set one; a fresh clock plays a track
    /// at its own speed.
    pub fn tempo(&self) -> f32 {
        let stored = f32::from_bits(self.tempo.load(Ordering::Relaxed));
        if stored > 0.0 && stored.is_finite() { stored } else { 1.0 }
    }

    pub fn set_tempo(&self, tempo: f32) {
        self.tempo.store(tempo.to_bits(), Ordering::Relaxed);
    }

    pub fn master_tempo(&self) -> bool {
        self.master_tempo.load(Ordering::Relaxed)
    }

    pub fn set_master_tempo(&self, on: bool) {
        self.master_tempo.store(on, Ordering::Relaxed);
    }

    pub fn key_shift(&self) -> i8 {
        i8::try_from(self.key_shift.load(Ordering::Relaxed)).unwrap_or(0)
    }

    pub fn set_key_shift(&self, semitones: i8) {
        self.key_shift.store(i32::from(semitones), Ordering::Relaxed);
    }

    pub fn pre_roll(&self) -> u64 {
        self.pre_roll.load(Ordering::Relaxed)
    }

    pub fn set_pre_roll(&self, frames: u64) {
        self.pre_roll.store(frames, Ordering::Relaxed);
    }

    pub fn pass_pre_roll(&self, frames: u64) {
        #[allow(deprecated)] // `try_update` requires Rust 1.95; the workspace MSRV is 1.85.
        let _ = self
            .pre_roll
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                Some(left.saturating_sub(frames))
            });
    }

    pub fn position(&self) -> u64 {
        self.position.load(Ordering::Relaxed)
    }

    /// Written by the audio callback, once per callback.
    pub fn set_position(&self, frames: u64) {
        self.position.store(frames, Ordering::Relaxed);
    }

    pub fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    pub fn set_total(&self, frames: u64) {
        self.total.store(frames, Ordering::Relaxed);
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate.load(Ordering::Relaxed)
    }

    pub fn set_sample_rate(&self, rate: u32) {
        self.sample_rate.store(rate, Ordering::Relaxed);
    }

    pub fn playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    pub fn set_playing(&self, playing: bool) {
        self.playing.store(playing, Ordering::Relaxed);
    }

    /// Whether a drag is running.
    ///
    /// Separate from `playing`, because a scrub makes a sound whether or not
    /// the deck was playing and must not be mistaken for the transport having
    /// been started: the callback mixes it, the device stays awake for it, and
    /// letting go puts the transport back exactly as it was.
    pub fn scrubbing(&self) -> bool {
        self.scrubbing.load(Ordering::Relaxed)
    }

    pub fn set_scrubbing(&self, scrubbing: bool) {
        self.scrubbing.store(scrubbing, Ordering::Relaxed);
    }

    /// Whether the callback should be pulling audio from this deck at all.
    pub fn sounding(&self) -> bool {
        self.playing() || self.scrubbing()
    }

    pub fn loaded(&self) -> bool {
        self.loaded.load(Ordering::Relaxed)
    }

    pub fn set_loaded(&self, loaded: bool) {
        self.loaded.store(loaded, Ordering::Relaxed);
    }

    pub fn requested_load(&self) -> u64 {
        self.requested_load.load(Ordering::Acquire)
    }

    /// Makes `request` the only load whose result may be installed.
    pub fn request_load(&self, request: u64) {
        let _gate = self.load_gate.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        self.requested_load.store(request, Ordering::Release);
        self.load_id.store(0, Ordering::Release);
        self.loaded.store(false, Ordering::Release);
        self.playing.store(false, Ordering::Release);
    }

    pub fn load_id(&self) -> u64 {
        self.load_id.load(Ordering::Acquire)
    }

    pub fn set_load_id(&self, request: u64) {
        self.load_id.store(request, Ordering::Release);
    }

    /// Publishes a decoder only if nothing newer was requested. Holding the
    /// same gate as `request_load` closes the last-instruction race between
    /// checking an id and marking its audio loaded.
    pub fn install_load(&self, request: u64) -> bool {
        let _gate = self.load_gate.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.requested_load.load(Ordering::Acquire) != request {
            return false;
        }
        self.load_id.store(request, Ordering::Release);
        self.loaded.store(true, Ordering::Release);
        true
    }

    /// An open failure belongs to the current selection only while its request
    /// still does. A newer selection must not receive the older error.
    pub fn finish_load_error(&self, request: u64) -> bool {
        let _gate = self.load_gate.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.requested_load.load(Ordering::Acquire) != request {
            return false;
        }
        self.load_id.store(0, Ordering::Release);
        self.loaded.store(false, Ordering::Release);
        true
    }

    pub fn end_of_stream(&self) -> bool {
        self.end_of_stream.load(Ordering::Acquire)
    }

    pub fn set_end_of_stream(&self, ended: bool) {
        self.end_of_stream.store(ended, Ordering::Release);
    }

    pub fn generation(&self) -> u32 {
        self.generation.load(Ordering::Acquire)
    }

    /// Release ordering, and always before the first block of the new
    /// generation is pushed: the callback must never see a block from a
    /// generation the counter has not reached.
    pub fn bump_generation(&self) -> u32 {
        self.generation.fetch_add(1, Ordering::AcqRel).wrapping_add(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_clock_is_at_the_start_and_stopped() {
        let clock = DeckClock::default();
        let snapshot = clock.snapshot();
        assert_eq!(snapshot.position_frames, 0);
        assert!(!snapshot.playing);
        assert!(!snapshot.loaded);
    }

    #[test]
    fn the_generation_moves_forward_on_every_bump() {
        let clock = DeckClock::default();
        assert_eq!(clock.bump_generation(), 1);
        assert_eq!(clock.bump_generation(), 2);
        assert_eq!(clock.generation(), 2);
    }
}
