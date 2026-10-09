//! One deck: the thread that decodes for it, and the handle that steers it.
//!
//! All blocking work — opening a file, demuxing, decoding, resampling — is on
//! this thread. The audio callback only ever reads blocks that were finished
//! and checked here.
//!
//! While a deck is paused and its ring is full there is nothing that can
//! change except a command, so the thread blocks on the channel rather than
//! polling. That is what keeps an idle app at no measurable CPU.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rtrb::Producer;

use crate::block::{Block, BLOCK_FRAMES, RING_BLOCKS};
use crate::clock::DeckClock;
use crate::decode::Streamer;
use crate::scrub::{PcmWindow, Scrubber, WINDOW_REACH};
use crate::stretch::{Stretcher, Varispeed, Wsola};
use crate::{Deck, DeckError, DeckEvent, EventSink};

/// What the control side asks a deck to do.
pub enum Command {
    Load { request: u64, path: PathBuf },
    /// Both play and pause are already in the clock when this arrives; the
    /// command exists to wake the thread from its blocking wait.
    Wake,
    Seek(u64),
    /// A drag has started. The deck decodes a window around the playhead and
    /// starts producing from it at whatever rate the drag asks for.
    ScrubBegin,
    /// Where the pointer is, in device-rate frames, and when it said so.
    ///
    /// The instant is stamped where the report enters the engine rather than
    /// counted in blocks on the way out. Blocks are 11.6 ms and a hand
    /// crossing thirty pixels a second reports every 33 ms, so rounding the
    /// gap between reports to whole blocks quantises the measured speed by a
    /// sixth — the same mistake on the time axis that rounding the position to
    /// whole milliseconds was on the distance axis. See `Engine::scrub_to_ms`.
    ScrubTo(u64, u64, Instant),
    /// The drag is over: the streamer picks up where the head was left.
    ScrubEnd,
    /// How fast to play, as a multiple of the file's own speed.
    SetTempo(f32),
    /// Master Tempo: whether the pitch is held while the speed changes.
    SetMasterTempo(bool),
    /// The key, in semitones from the track's own.
    SetKeyShift(i8),
    Unload,
    Quit,
}

/// How far the key can be shifted either way, in semitones: an octave.
pub const KEY_SHIFT_RANGE: i8 = 12;

/// How long the thread waits between top-ups while the ring is full and audio
/// is running. Short enough that a device buffer can never outrun it.
const TOP_UP_WAIT: Duration = Duration::from_millis(2);

/// Blocks kept queued while a drag is running: about 35 ms.
///
/// Playback wants the full ring, which absorbs a decode hiccup. A drag wants
/// the opposite — every block already queued is a block of the pointer's past.
const SCRUB_BLOCKS: usize = 3;

/// The longest loop whose audio is decoded once and kept, in device-rate
/// frames: about six seconds at 44.1 kHz and three at 96 kHz, 2 MiB.
///
/// A loop goes round by going back to its in point, and for the streamer that
/// is a demuxer seek and a decode from the packet before it. Once a pass that
/// is cheap enough. A 1/64-beat loop goes round every few milliseconds, more
/// often than a block, and an accurate MP3 seek can scan the file from its
/// start; so a short loop is read from memory instead. A longer one is sought
/// once a pass, as before.
const LOOP_AUDIO_FRAMES: u64 = 1 << 18;

pub struct DeckHandle {
    commands: Sender<Command>,
    clock: Arc<DeckClock>,
}

impl DeckHandle {
    pub fn clock(&self) -> &Arc<DeckClock> {
        &self.clock
    }

    pub fn send(&self, command: Command) {
        // A deck whose thread has gone is a deck that cannot be steered; the
        // clock stops moving, which the interface already draws as stopped.
        if self.commands.send(command).is_err() {
            tracing::error!("a deck's decode thread has stopped");
        }
    }
}

impl Drop for DeckHandle {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Quit);
    }
}

/// Starts a deck's decode thread and returns the handle to it.
pub fn spawn(
    deck: Deck,
    clock: Arc<DeckClock>,
    producer: Producer<Block>,
    device_rate: u32,
    events: EventSink,
) -> std::io::Result<DeckHandle> {
    let (tx, rx) = std::sync::mpsc::channel();
    let worker_clock = Arc::clone(&clock);
    std::thread::Builder::new()
        .name(format!("rbl-deck-{}", deck.name()))
        .spawn(move || {
            let mut worker = Worker::new(deck, worker_clock, producer, device_rate, events);
            worker.run(&rx);
        })?;
    Ok(DeckHandle { commands: tx, clock })
}

struct Worker {
    deck: Deck,
    clock: Arc<DeckClock>,
    producer: Producer<Block>,
    device_rate: u32,
    events: EventSink,
    streamer: Option<Streamer>,
    generation: u32,
    /// The read head, present only while a drag is running.
    scrubber: Option<Scrubber>,
    /// When the pointer last reported, so the next one can be timed.
    last_report: Option<Instant>,
    /// Decoded audio around that head. Emptied when the drag ends, because
    /// several megabytes for a gesture that is over is several megabytes
    /// nobody asked for.
    window: PcmWindow,
    /// The two ways to play at another speed: with the pitch held, and with
    /// the pitch moving as a record's does. Both are built at load, because
    /// building one takes an allocation and the switch between them is a
    /// button somebody presses mid-track.
    ///
    /// Boxed because which one holds the pitch is a build-time decision — see
    /// [`key_lock`] — and the hot path already went through `&mut dyn
    /// Stretcher`, so this costs one allocation at load and nothing per block.
    keylock: Box<dyn Stretcher>,
    varispeed: Varispeed,
    tempo: f32,
    master_tempo: bool,
    /// Semitones from the track's own key. Any shift puts the key-lock
    /// stretcher in the path, since only it can move the pitch on its own;
    /// with Master Tempo off it is asked for the pitch a record would have
    /// at this speed, and the shift on top of that.
    key_shift: i8,
    /// Where the audio coming out of the stretcher sits in the track, in
    /// input frames.
    ///
    /// Not the streamer's position: by the time a block comes out of the
    /// stretcher the streamer has read a bufferful past it, and a playhead
    /// that ran 60 ms ahead of the sound would put every cue in the wrong
    /// place. Advanced by what the output consumes instead.
    head: f64,
    /// Frames of decoded audio waiting to go into the stretcher.
    feed: Vec<f32>,
    /// How much of `feed` has been handed over.
    fed: usize,
    /// The audio of a loop no longer than [`LOOP_AUDIO_FRAMES`], interleaved,
    /// decoded the first time the loop went round. Shorter than the loop when
    /// the track ends inside it.
    loop_audio: Vec<f32>,
    /// The loop `loop_audio` holds, as `(in, out)`.
    loop_audio_range: Option<(u64, u64)>,
    /// Where in `loop_audio` the next frame of input comes from, in frames;
    /// `None` while it comes from the streamer. While it is `Some`, the
    /// streamer waits where `loop_audio` ends, which is where the track goes
    /// on from after EXIT.
    from_loop_audio: Option<usize>,
    /// Passes of a loop the stretcher has been fed that the head has not
    /// reached yet, oldest first, as the `(in, out)` each went round at.
    ///
    /// The input goes round the moment it reaches the out point, a
    /// stretcher's buffer ahead of the sound, and nothing is reset there: the
    /// stretcher hears the loop as one continuous signal. The head goes round
    /// when the output reaches the same point, and this is what tells it to.
    wraps: VecDeque<(u64, u64)>,
}

impl Worker {
    fn new(
        deck: Deck,
        clock: Arc<DeckClock>,
        producer: Producer<Block>,
        device_rate: u32,
        events: EventSink,
    ) -> Self {
        Self {
            deck,
            clock,
            producer,
            device_rate,
            events,
            streamer: None,
            generation: 0,
            scrubber: None,
            last_report: None,
            window: PcmWindow::empty(),
            keylock: key_lock(device_rate),
            varispeed: Varispeed::new(device_rate),
            tempo: 1.0,
            master_tempo: false,
            key_shift: 0,
            head: 0.0,
            feed: vec![0.0; BLOCK_FRAMES * 2],
            fed: 0,
            loop_audio: Vec::new(),
            loop_audio_range: None,
            from_loop_audio: None,
            wraps: VecDeque::with_capacity(64),
        }
    }

    fn run(&mut self, commands: &Receiver<Command>) {
        loop {
            // A ring's worth at most, then look at the channel again.
            //
            // Unbounded, this starves its own commands: a consumer draining as
            // fast as this fills — anything faster than realtime — means the
            // ring is never full, `produce` never returns false, and a seek
            // sits in the channel while the thread decodes the rest of the
            // track. Seeking four minutes into a track took two seconds
            // because of it.
            let mut produced = false;
            for _ in 0..RING_BLOCKS {
                if !self.produce() {
                    break;
                }
                produced = true;
            }

            let waiting = if produced || self.clock.sounding() {
                // Playing: top up as the callback drains, without spinning.
                match commands.recv_timeout(TOP_UP_WAIT) {
                    Ok(command) => Some(command),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            } else {
                // Paused with nothing to decode: nothing but a command can
                // change that, so block rather than poll.
                match commands.recv() {
                    Ok(command) => Some(command),
                    Err(_) => return,
                }
            };

            if let Some(command) = waiting {
                if !self.handle(command) {
                    return;
                }
            }
        }
    }

    /// Returns false when the deck has been told to quit.
    fn handle(&mut self, command: Command) -> bool {
        match command {
            Command::Load { request, path } => self.load(request, &path),
            Command::Seek(frame) => self.seek(frame),
            Command::ScrubBegin => self.scrub_begin(),
            Command::ScrubTo(frame, pre_roll, at) => self.scrub_to(frame, pre_roll, at),
            Command::ScrubEnd => self.scrub_end(),
            Command::SetTempo(tempo) => self.set_tempo(tempo),
            Command::SetMasterTempo(on) => {
                // The two hold different audio, so switching between them
                // starts the new one from where the old one had reached
                // rather than from what it happened to have buffered.
                let was = self.keylock_in_path();
                self.master_tempo = on;
                self.apply_pitch();
                if was != self.keylock_in_path() {
                    self.restart_stretch();
                }
            }
            Command::SetKeyShift(semitones) => {
                let was = self.keylock_in_path();
                self.key_shift = semitones.clamp(-KEY_SHIFT_RANGE, KEY_SHIFT_RANGE);
                self.clock.set_key_shift(self.key_shift);
                self.apply_pitch();
                if was != self.keylock_in_path() {
                    self.restart_stretch();
                }
            }
            Command::Unload => self.unload(),
            Command::Wake => {}
            Command::Quit => return false,
        }
        true
    }

    fn load(&mut self, request: u64, path: &std::path::Path) {
        // Requests waiting behind a slow open are cheap to skip. The newest
        // path is the only one a person still has selected.
        if self.clock.requested_load() != request {
            return;
        }
        self.clock.set_start_in(0);
        self.clock.set_loop(None);
        self.clock.set_playing(false);
        self.streamer = None;
        self.forget_loop_audio();
        self.clock.set_loaded(false);
        self.clock.set_end_of_stream(false);

        let opened = Streamer::open(path, self.device_rate);
        // Opening can block on a sleeping disk. If another selection arrived
        // meanwhile, never publish this stale file or its error.
        if self.clock.requested_load() != request {
            return;
        }
        match opened {
            Ok(streamer) => {
                let total = streamer.total_frames();
                self.generation = self.clock.bump_generation();
                self.clock.set_pre_roll(0);
                self.clock.set_position(0);
                self.head = 0.0;
                self.restart_stretch();
                self.clock.set_total(total);
                self.clock.set_sample_rate(self.device_rate);
                self.streamer = Some(streamer);
                if !self.clock.install_load(request) {
                    self.streamer = None;
                    return;
                }
                (self.events)(DeckEvent::Loaded {
                    deck: self.deck,
                    load_id: request,
                    total_frames: total,
                    sample_rate: self.device_rate,
                });
            }
            Err(e) => {
                if !self.clock.finish_load_error(request) {
                    return;
                }
                self.clock.set_total(0);
                self.clock.set_pre_roll(0);
                self.clock.set_position(0);
                (self.events)(DeckEvent::Error {
                    deck: self.deck,
                    load_id: request,
                    message: e.to_string(),
                });
            }
        }
    }

    /// The loop the deck is inside, if it is inside one.
    fn active_loop(&self) -> Option<(u64, u64)> {
        if self.clock.looping() { self.clock.loop_range() } else { None }
    }

    /// Back to the loop's in point with the stretcher emptied, quietly: no
    /// generation change, so the callback plays straight on with nothing
    /// faded.
    ///
    /// Only for a stretched head that reached the out point without the input
    /// going round first, which is a loop set behind what the stretcher had
    /// already been fed. Every other pass goes round in the input, where
    /// nothing is reset; see `wrap_input`.
    fn loop_jump(&mut self, frame: u64) {
        self.forget_input();
        let Some(streamer) = self.streamer.as_mut() else { return };
        match streamer.seek(frame) {
            Ok(landed) => {
                self.clock.set_end_of_stream(false);
                self.head = landed as f64;
                self.restart_stretch();
            }
            Err(e) => self.report(&e),
        }
    }

    /// Tells the interface that the file could not be read.
    fn report(&self, e: &DeckError) {
        (self.events)(DeckEvent::Error {
            deck: self.deck,
            load_id: self.clock.load_id(),
            message: e.to_string(),
        });
    }

    /// The input is about to come from the streamer at wherever it is sent
    /// next, and no pass of a loop is waiting for the head.
    fn forget_input(&mut self) {
        self.from_loop_audio = None;
        self.wraps.clear();
    }

    /// A different file, or none: the loop audio held is not this one's.
    fn forget_loop_audio(&mut self) {
        self.forget_input();
        self.loop_audio.clear();
        self.loop_audio_range = None;
    }

    /// Where the next frame of input sits in the track.
    fn input_at(&self) -> u64 {
        if let (Some(at), Some((from, _))) = (self.from_loop_audio, self.loop_audio_range) {
            return from + at as u64;
        }
        self.streamer.as_ref().map_or(0, Streamer::next_frame)
    }

    /// At most `wanted` frames, and none past the loop's out point.
    ///
    /// Input already past the out point is not held to it: it was read before
    /// the loop was set, and the head deals with that when it gets there.
    fn input_room(&self, wanted: usize) -> usize {
        let Some((_, to)) = self.active_loop() else { return wanted };
        let at = self.input_at();
        if at >= to {
            return wanted;
        }
        usize::try_from(to - at).map_or(wanted, |left| left.min(wanted))
    }

    /// Takes the input back to the in point when it is at the out point.
    ///
    /// Unstretched the input is the head, and anywhere at or past the out
    /// point goes round. Stretched, only input that stopped on the out point
    /// does: input past it was fed to the stretcher before the loop was set,
    /// and the head going round has to empty the stretcher (`wrap_head`).
    /// False when the file could not be read.
    fn wrap_input(&mut self, stretching: bool) -> bool {
        let Some((from, to)) = self.active_loop() else { return true };
        let at = self.input_at();
        if at < to || (stretching && at > to) {
            return true;
        }
        if to - from <= LOOP_AUDIO_FRAMES {
            if self.loop_audio_range != Some((from, to)) && !self.decode_loop_audio(from, to) {
                return false;
            }
            self.from_loop_audio = (!self.loop_audio.is_empty()).then_some(0);
        } else {
            self.from_loop_audio = None;
            let Some(streamer) = self.streamer.as_mut() else { return false };
            if let Err(e) = streamer.seek(from) {
                self.clock.set_end_of_stream(true);
                self.report(&e);
                return false;
            }
        }
        if stretching {
            self.wraps.push_back((from, to));
        }
        self.clock.set_end_of_stream(false);
        true
    }

    /// Decodes `from..to` into `loop_audio`, leaving the streamer at `to`, or
    /// at the end of the track if that comes first.
    fn decode_loop_audio(&mut self, from: u64, to: u64) -> bool {
        self.from_loop_audio = None;
        self.loop_audio_range = None;
        let Some(streamer) = self.streamer.as_mut() else { return false };
        let length = usize::try_from(to - from).unwrap_or(0);
        self.loop_audio.clear();
        self.loop_audio.resize(length * 2, 0.0);
        let mut filled = 0;
        let mut failed = streamer.seek(from).err();
        while failed.is_none() && filled < length {
            let Some(rest) = self.loop_audio.get_mut(filled * 2..) else { break };
            match streamer.fill(rest) {
                Ok(0) => break,
                Ok(frames) => filled += frames,
                Err(e) => failed = Some(e),
            }
        }
        if let Some(e) = failed {
            self.loop_audio.clear();
            self.clock.set_end_of_stream(true);
            self.report(&e);
            return false;
        }
        self.loop_audio.truncate(filled * 2);
        self.loop_audio_range = Some((from, to));
        true
    }

    /// Reads input into `out`, from the loop audio or the streamer, stopping
    /// at the out point. Returns where the first frame sits in the track and
    /// how many were read; `None` when the file could not be read.
    fn read_input(&mut self, out: &mut [f32]) -> Option<(u64, usize)> {
        let room = self.input_room(out.len() / 2);
        if let (Some(at), Some((from, _))) = (self.from_loop_audio, self.loop_audio_range) {
            let held = self.loop_audio.len() / 2;
            let frames = room.min(held.saturating_sub(at));
            if let (Some(src), Some(dst)) =
                (self.loop_audio.get(at * 2..(at + frames) * 2), out.get_mut(..frames * 2))
            {
                dst.copy_from_slice(src);
            }
            // Past the end of what is held, the streamer is already there.
            self.from_loop_audio = (at + frames < held).then_some(at + frames);
            return Some((from + at as u64, frames));
        }
        let streamer = self.streamer.as_mut()?;
        let target = out.get_mut(..room * 2)?;
        match streamer.fill(target) {
            // Stamped after the fill, not before: the first fill after a seek
            // starts by discarding what the demuxer overshot by, so the
            // position beforehand is the packet boundary it landed on, not the
            // frame the block's audio begins at. Stamping that put the
            // playhead a few hundred frames early for one block after every
            // seek.
            Ok(frames) => Some((streamer.position() - frames as u64, frames)),
            Err(e) => {
                // A decode that fails mid-track stops the deck rather than
                // playing whatever was left in the buffer.
                self.clock.set_end_of_stream(true);
                self.report(&e);
                None
            }
        }
    }

    /// Whether the input has nothing more to give: it is the streamer's, and
    /// the streamer is finished.
    fn input_finished(&self) -> bool {
        self.from_loop_audio.is_none() && self.streamer.as_ref().is_some_and(Streamer::finished)
    }

    fn seek(&mut self, frame: u64) {
        self.forget_input();
        let Some(streamer) = self.streamer.as_mut() else { return };
        match streamer.seek(frame) {
            Ok(landed) => {
                self.clock.set_end_of_stream(false);
                // The position first, then the generation: the callback reads
                // them in that order, and must never take a new generation's
                // blocks while still believing the old position.
                self.clock.set_position(landed);
                self.head = landed as f64;
                self.restart_stretch();
                // Adopted, not bumped: `seek_frames` already moved the counter
                // when it moved the position, so that the callback stopped
                // trusting the ring at the same instant. Bumping again here
                // would start a second changeover for one seek.
                self.generation = self.clock.generation();
            }
            Err(e) => self.report(&e),
        }
    }

    /// How fast to play, as a multiple of the file's own speed.
    fn set_tempo(&mut self, tempo: f32) {
        let safe = if tempo.is_finite() {
            tempo.clamp(crate::stretch::MIN_RATIO, crate::stretch::MAX_RATIO)
        } else {
            1.0
        };
        self.tempo = safe;
        self.keylock.set_ratio(safe);
        self.varispeed.set_ratio(safe);
        self.clock.set_tempo(safe);
        self.apply_pitch();
    }

    /// Whether the key-lock stretcher, rather than plain resampling, is
    /// playing: Master Tempo on, or a key shift that only it can make.
    fn keylock_in_path(&self) -> bool {
        self.master_tempo || self.key_shift != 0
    }

    /// The pitch the key-lock stretcher is asked for: the shift, on top of
    /// the pitch a record would have at this speed when Master Tempo is off.
    fn apply_pitch(&mut self) {
        let shift = 2_f32.powf(f32::from(self.key_shift) / 12.0);
        let base = if self.master_tempo { 1.0 } else { self.tempo };
        self.keylock.set_pitch_scale(base * shift);
    }

    /// Whichever of the two is in the path.
    fn stretcher(&mut self) -> &mut dyn Stretcher {
        if self.keylock_in_path() { &mut *self.keylock } else { &mut self.varispeed }
    }

    /// Empties both, so nothing of the last position or the last mode is
    /// played after a seek or a switch.
    fn restart_stretch(&mut self) {
        self.keylock.reset();
        self.varispeed.reset();
        self.fed = 0;
        self.feed.fill(0.0);
    }

    fn unload(&mut self) {
        self.scrubber = None;
        self.window = PcmWindow::empty();
        self.clock.set_scrubbing(false);
        self.clock.set_start_in(0);
        self.clock.set_loop(None);
        self.clock.set_playing(false);
        self.streamer = None;
        self.forget_loop_audio();
        self.generation = self.clock.bump_generation();
        self.clock.set_loaded(false);
        self.clock.set_load_id(0);
        self.clock.set_pre_roll(0);
        self.clock.set_position(0);
        self.clock.set_total(0);
        self.clock.set_end_of_stream(false);
    }

    /// Starts a drag: the head begins where the playhead is.
    fn scrub_begin(&mut self) {
        if self.streamer.is_none() {
            return;
        }
        let at = self.clock.position();
        self.scrubber = Some(Scrubber::new(at, self.device_rate));
        // A new drag times its reports from scratch; the gap since the last
        // one is however long ago the previous drag was, which is not a speed.
        self.last_report = None;
        // The blocks already in flight belong to normal playback and are at the
        // wrong place and the wrong speed; a new generation drops them.
        self.generation = self.clock.bump_generation();
        // The window is read from wherever the streamer is sent, and the end
        // of the drag seeks from there.
        self.forget_input();
        self.fill_window(at);
        self.clock.set_scrubbing(true);
    }

    fn scrub_to(&mut self, frame: u64, pre_roll: u64, at: Instant) {
        self.clock.set_pre_roll(pre_roll);
        // Output frames since the report before this one, which is what the
        // head's speed is measured over. Nothing for the first report of a
        // drag: there is no report before it to measure against.
        let since = self.last_report.replace(at).map_or(0.0, |last| {
            at.saturating_duration_since(last).as_secs_f64() * f64::from(self.device_rate)
        });
        if let Some(scrubber) = self.scrubber.as_mut() {
            scrubber.aim(frame, since);
        }
    }

    /// Ends a drag, leaving the playhead under the pointer.
    ///
    /// Where the pointer is, not where the read head got to. The head is
    /// capped at `MAX_RATE` so the drag stays audible, which leaves it behind
    /// the hand on a fast one and barely moved at all on a click; landing on
    /// it turned a click on the overview into a jump that sprang back.
    fn scrub_end(&mut self) {
        let Some(scrubber) = self.scrubber.take() else {
            return;
        };
        self.window = PcmWindow::empty();
        // The streamer has been sitting wherever the window was filled from,
        // so it has to be put where the drag finished before playback resumes.
        let total = self.clock.total();
        let at = if total > 0 {
            scrubber.target().min(total)
        } else {
            scrubber.target()
        };
        self.clock.bump_generation();
        self.seek(at);
        self.clock.set_scrubbing(false);
    }

    /// Decodes the window a drag reads from, centred on `at`.
    ///
    /// One demuxer seek and a few seconds of decoding, which is what buys the
    /// thousands of reads a drag makes without touching the file again.
    fn fill_window(&mut self, at: u64) {
        let Some(streamer) = self.streamer.as_mut() else { return };
        let start = at.saturating_sub(WINDOW_REACH);
        if streamer.seek(start).is_err() {
            self.window = PcmWindow::empty();
            return;
        }
        let wanted = (WINDOW_REACH * 2) as usize;
        let mut samples = vec![0.0_f32; wanted * 2];
        let mut filled = 0_usize;
        while filled < wanted {
            let Some(chunk) = samples.get_mut(filled * 2..) else { break };
            // A short read or a decode error both mean the window is as long
            // as it is going to get; a drag past its end hears silence, which
            // is what the end of a record sounds like.
            match streamer.fill(chunk) {
                Ok(frames) if frames > 0 => filled += frames,
                _ => break,
            }
        }
        samples.truncate(filled * 2);
        // A short fill is the end of the track; a start of 0 is its top. On
        // either the window can go no further, and the head is not asked to
        // keep a margin from an edge that is the track's own.
        let at_end = filled < wanted;
        self.window = PcmWindow { start, samples, at_start: start == 0, at_end };
    }

    /// One block of a drag. False when there is nothing to add.
    fn produce_scrub(&mut self) -> bool {
        // A shallow ring, not the full sixteen blocks. What is already queued
        // has to play out before the drag's next move is heard, and 186 ms of
        // that is a drag that answers the pointer a fifth of a second late and
        // keeps sounding that long after it stops.
        if RING_BLOCKS - self.producer.slots() >= SCRUB_BLOCKS {
            return false;
        }
        let generation = self.generation;
        let Some(scrubber) = self.scrubber.as_mut() else { return false };
        // Nothing until the pointer has said where it is going. Producing
        // at-rest blocks in the meantime fills the ring with silence, and that
        // silence has to play out before the first sound of the drag.
        if !scrubber.started() {
            return false;
        }
        // Refill before the head reaches the edge, not after: a demuxer seek
        // costs more than a block, and running off the end is silence.
        let at = scrubber.cursor();
        let comfortable = self.window.comfortable(at as f64, WINDOW_REACH / 4);
        if !comfortable {
            self.fill_window(at);
        }
        let Some(scrubber) = self.scrubber.as_mut() else { return false };
        let mut block = Block::empty(generation, scrubber.cursor());
        let frames = scrubber.render(&self.window, &mut block.samples);
        block.frames = u16::try_from(frames.min(BLOCK_FRAMES)).unwrap_or(0);
        self.producer.push(block).is_ok()
    }

    /// Decodes one block into the ring. False when there is nothing to add.
    fn produce(&mut self) -> bool {
        if self.scrubber.is_some() {
            return self.produce_scrub();
        }
        if self.producer.is_full() {
            return false;
        }
        let generation = self.generation;
        let stretching = self.stretching();
        if stretching {
            // A head at or past the out point with no pass of the loop fed
            // for it: a loop set behind the head, or a seek past its end
            // while it is on. Round again before another frame is decoded.
            if self.wraps.is_empty() {
                if let Some((from, to)) = self.active_loop() {
                    if self.head >= to as f64 {
                        self.loop_jump(from);
                    }
                }
            }
        } else {
            // Unstretched the head is the input, so nothing waits for it.
            self.wraps.clear();
            if !self.wrap_input(false) {
                return false;
            }
        }
        if self.streamer.is_none() {
            return false;
        }
        if self.input_finished() {
            self.clock.set_end_of_stream(true);
            return false;
        }

        // Through the stretcher whenever the speed or the key is not the
        // file's own; at unity with no shift the decoded audio goes straight.
        if stretching {
            return self.produce_stretched(generation);
        }
        let mut block = Block::empty(generation, 0);
        // Only up to the out point, so the next block is the in point: a
        // block that ran past it would play the audio beyond the loop first,
        // and a loop shorter than a block would play a block long.
        let Some((start, frames)) = self.read_input(&mut block.samples) else { return false };
        if frames == 0 {
            if self.input_finished() {
                self.clock.set_end_of_stream(true);
            }
            return false;
        }
        block.position = start;
        self.head = (start + frames as u64) as f64;
        block.frames = u16::try_from(frames.min(BLOCK_FRAMES)).unwrap_or(0);
        self.producer.push(block).is_ok()
    }

    /// Whether the audio goes through a stretcher rather than straight: the
    /// speed or the key is not the file's own.
    fn stretching(&self) -> bool {
        (self.tempo - 1.0).abs() > f32::EPSILON || self.key_shift != 0
    }

    /// The same, with the tempo control in the path.
    ///
    /// The block's position is not the streamer's: by the time audio comes out
    /// of a stretcher the streamer has read a bufferful past it, and a
    /// playhead running ahead of its own sound puts every cue in the wrong
    /// place. It is counted forward instead, by what each block of output
    /// consumed at the speed it was played at.
    fn produce_stretched(&mut self, generation: u32) -> bool {
        let tempo = f64::from(self.tempo);
        let mut block = Block::empty(generation, self.head as u64);
        let mut frames = 0;
        let limit = self.block_limit(tempo);

        // Feed, take what came of it, feed again — until the block is full or
        // there is nothing left to feed it with. One pass is not enough for
        // every backend: Rubber Band takes the block it asked for, hands back
        // what that block made, and wants nothing more until it has been
        // drained, so a single feed-then-pull filled exactly half of every
        // block. WSOLA fills one in a pass and leaves this loop after it.
        while frames < limit {
            if !self.top_up_stretcher() {
                break;
            }
            let Some(rest) = block.samples.get_mut(frames * 2..limit * 2) else { break };
            let got = self.stretcher().pull(rest);
            if got == 0 {
                break;
            }
            frames += got;
        }

        if frames == 0 {
            return false;
        }
        self.head += frames as f64 * tempo;
        block.frames = u16::try_from(frames.min(BLOCK_FRAMES)).unwrap_or(0);
        let pushed = self.producer.push(block).is_ok();
        self.wrap_head();
        pushed
    }

    /// Output frames this block may hold before the head reaches the next out
    /// point, so that no block runs across a seam and the playhead never reads
    /// past the out point. The head is counted in input frames, so the block
    /// ends on the first output frame that takes it to or over the out point:
    /// over by less than one frame's worth of tempo, which `wrap_head` carries
    /// into the next pass rather than dropping.
    fn block_limit(&self, tempo: f64) -> usize {
        let out = self.wraps.front().map(|&(_, to)| to).or_else(|| self.active_loop().map(|(_, to)| to));
        let Some(to) = out else { return BLOCK_FRAMES };
        let left = to as f64 - self.head;
        if left <= 0.0 || tempo <= 0.0 {
            return BLOCK_FRAMES;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "positive, and clamped to a block")]
        let frames = (left / tempo).ceil().min(BLOCK_FRAMES as f64) as usize;
        frames.clamp(1, BLOCK_FRAMES)
    }

    /// Takes the head round when it reaches the out point of the oldest pass
    /// the input has already gone round, keeping what it went over by.
    ///
    /// With no such pass and the loop on, the stretcher was fed past the out
    /// point before the loop was set, and has to be emptied: the old jump.
    fn wrap_head(&mut self) {
        if let Some(&(from, to)) = self.wraps.front() {
            if self.head >= to as f64 {
                self.head = from as f64 + (self.head - to as f64);
                self.wraps.pop_front();
            }
            return;
        }
        if let Some((from, to)) = self.active_loop() {
            if self.head >= to as f64 {
                self.loop_jump(from);
            }
        }
    }

    /// Gives the stretcher everything it asks for that there is input for.
    ///
    /// False when the stream is over and there is nothing more to give, which
    /// is what stops the loop above rather than a short pull.
    fn top_up_stretcher(&mut self) -> bool {
        loop {
            let wanted = self.stretcher().wants();
            if wanted == 0 || self.stretcher().ready(BLOCK_FRAMES) {
                break;
            }
            if self.fed == 0 {
                // The input stops on the out point and goes round from there,
                // so the stretcher is fed the loop over and over as one
                // continuous signal and never anything past it.
                if !self.wrap_input(true) {
                    return false;
                }
                // Lent out and put back, so reading into it allocates nothing.
                let mut feed = std::mem::take(&mut self.feed);
                let read = self.read_input(&mut feed);
                self.feed = feed;
                let Some((_, frames)) = read else { return false };
                if frames == 0 {
                    if self.input_finished() {
                        self.clock.set_end_of_stream(true);
                    }
                    break;
                }
                self.fed = frames;
            }
            let take = self.fed.min(wanted);
            // The fields taken apart by hand rather than through `stretcher`,
            // so the buffer can be lent to the stretcher without copying it:
            // one method borrowing all of `self` would have meant a fresh
            // allocation for every block a stretched deck plays.
            let stretcher: &mut dyn Stretcher =
                if self.master_tempo || self.key_shift != 0 { &mut *self.keylock } else { &mut self.varispeed };
            let Some(from) = self.feed.get(..take * 2) else { break };
            let taken = stretcher.feed(from);
            // What was not taken stays at the front for the next pass.
            self.feed.copy_within(taken * 2..self.fed * 2, 0);
            self.fed -= taken;
        }
        true
    }
}

/// The stretcher behind MASTER TEMPO.
///
/// Rubber Band R3 where the `rubberband` feature is on, which is the default
/// and the GPL build; the WSOLA backend written for this crate otherwise, and
/// also if Rubber Band will not allocate, because a deck that plays with the
/// pitch drifting is better than a deck that does not play.
fn key_lock(device_rate: u32) -> Box<dyn Stretcher> {
    #[cfg(feature = "rubberband")]
    if let Some(stretcher) = crate::rubberband::RubberBand::new(device_rate) {
        return Box::new(stretcher);
    }
    Box::new(Wsola::new(device_rate))
}

/// The loop path, rendered offline: a worker driven by hand, its ring drained
/// as it fills, with no device, no callback and no clock time. What comes out
/// is exactly what the decode thread would queue, whatever the machine's load.
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    /// Frames in the fixture: one 16-bit step per frame, so every sample says
    /// exactly which frame of the file it is.
    const TOTAL: u64 = 32_768;

    /// What one step of the fixture wraps at.
    const STEPS: u64 = 65_536;

    /// One beat at 128 BPM is 20,671.875 frames at 44.1 kHz, so 1/64 of a beat
    /// is 323 frames and 1/32 is 646: both shorter than two blocks, and 1/64
    /// shorter than one.
    const BEAT_64: u64 = 323;
    const BEAT_32: u64 = 646;

    const FROM: u64 = 10_000;

    /// A 16-bit stereo WAV of `total` frames whose sample at frame `n` is
    /// `n - 32768`, wrapping every 65,536 frames.
    fn staircase(path: &std::path::Path, total: u64) {
        let data_len = u32::try_from(total * 4).unwrap();
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16_u32.to_le_bytes());
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&2_u16.to_le_bytes());
        out.extend_from_slice(&RATE.to_le_bytes());
        out.extend_from_slice(&(RATE * 4).to_le_bytes());
        out.extend_from_slice(&4_u16.to_le_bytes());
        out.extend_from_slice(&16_u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for frame in 0..total {
            let sample = ((frame % STEPS) as i32 - 32_768) as i16;
            out.extend_from_slice(&sample.to_le_bytes());
            out.extend_from_slice(&sample.to_le_bytes());
        }
        std::fs::write(path, out).unwrap();
    }

    /// Which frame of the fixture an output sample came from, fractional
    /// where the stretcher interpolated between two.
    fn source(sample: f32) -> f64 {
        f64::from(sample) * 32_768.0 + 32_768.0
    }

    struct Render {
        /// The left channel, as frames of the fixture.
        frames: Vec<f64>,
        /// Each block's position and length, in the order queued.
        blocks: Vec<(u64, usize)>,
    }

    struct Rig {
        worker: Worker,
        clock: Arc<DeckClock>,
        ring: rtrb::Consumer<Block>,
        _dir: tempfile::TempDir,
    }

    impl Rig {
        fn new(tempo: f32, master_tempo: bool) -> Self {
            Self::with_length(tempo, master_tempo, TOTAL)
        }

        fn with_length(tempo: f32, master_tempo: bool, total: u64) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("staircase.wav");
            staircase(&path, total);
            let clock = Arc::new(DeckClock::default());
            let (producer, ring) = rtrb::RingBuffer::<Block>::new(RING_BLOCKS);
            let events: EventSink = Arc::new(|event| {
                assert!(!matches!(event, DeckEvent::Error { .. }), "the deck reported {event:?}");
            });
            let mut worker = Worker::new(Deck::A, Arc::clone(&clock), producer, RATE, events);
            clock.request_load(1);
            worker.load(1, &path);
            assert!(clock.loaded(), "the fixture did not load");
            worker.handle(Command::SetTempo(tempo));
            worker.handle(Command::SetMasterTempo(master_tempo));
            Self { worker, clock, ring, _dir: dir }
        }

        /// Sets a loop and sends the head to its in point, as the engine does.
        fn set_loop(&mut self, from: u64, to: u64) {
            self.clock.set_loop(Some((from, to)));
            self.clock.set_looping(true);
            self.clock.bump_generation();
            self.worker.handle(Command::Seek(from));
        }

        /// Everything the decode thread queues, until at least `frames`.
        fn render(&mut self, frames: usize) -> Render {
            let mut out = Render { frames: Vec::new(), blocks: Vec::new() };
            let mut idle = 0;
            while out.frames.len() < frames {
                let produced = self.worker.produce();
                let mut popped = false;
                while let Ok(block) = self.ring.pop() {
                    popped = true;
                    out.blocks.push((block.position, usize::from(block.frames)));
                    out.frames.extend(block.filled().chunks_exact(2).map(|frame| source(frame[0])));
                }
                idle = if produced || popped { 0 } else { idle + 1 };
                assert!(idle < 4, "the deck stopped producing after {} frames", out.frames.len());
            }
            out
        }
    }

    /// Output frames between successive returns to the in point.
    ///
    /// Inside a pass the source frame only rises. A return is where it starts
    /// to fall, which can take two frames when the stretcher interpolates
    /// between the out point and the in point.
    fn periods(frames: &[f64]) -> Vec<usize> {
        let seams: Vec<usize> = frames
            .windows(3)
            .enumerate()
            .filter(|(_, three)| three[1] >= three[0] && three[2] < three[1])
            .map(|(at, _)| at + 2)
            .collect();
        seams.windows(2).map(|pair| pair[1] - pair[0]).collect()
    }

    /// At the file's own speed a loop of any length plays exactly its own
    /// frames, in order, over and over: `from..to` and nothing else.
    #[test]
    fn a_loop_shorter_than_a_block_repeats_on_its_own_frames_at_unity() {
        for length in [BEAT_64, BEAT_32] {
            let mut rig = Rig::new(1.0, false);
            rig.set_loop(FROM, FROM + length);
            let render = rig.render(40 * BLOCK_FRAMES);
            let expected: Vec<f64> =
                (0..render.frames.len() as u64).map(|i| (FROM + i % length) as f64).collect();
            assert_eq!(render.frames, expected, "a {length}-frame loop did not play its own frames");
            assert!(
                render.blocks.iter().all(|&(at, frames)| at >= FROM && at + frames as u64 <= FROM + length),
                "a block ran past the out point: {:?}",
                render.blocks
            );
        }
    }

    /// Off the file's own speed the loop goes round every `length / tempo`
    /// output frames — 316.67 for 1/64 of a beat at +2 % and 633.33 for 1/32 —
    /// so each period is one of the two whole numbers either side. Before the
    /// input stopped on the out point these came out at 512 and 1,024.
    #[test]
    fn a_loop_shorter_than_a_block_repeats_every_length_over_tempo_when_stretched() {
        for tempo in [1.02_f32, 0.94] {
            for length in [BEAT_64, BEAT_32] {
                let mut rig = Rig::new(tempo, false);
                rig.set_loop(FROM, FROM + length);
                let render = rig.render(200 * BLOCK_FRAMES);
                // Past the tempo control's smoothing, which starts the
                // stretcher at the file's own speed and eases it over.
                let settled = render.frames.get(16 * BLOCK_FRAMES..).unwrap();
                let periods = periods(settled);
                let ideal = length as f64 / f64::from(tempo);
                assert!(periods.len() > 100, "too few passes to judge: {periods:?}");
                assert!(
                    periods.iter().all(|&p| (p as f64 - ideal).abs() < 1.0),
                    "a {length}-frame loop at {tempo} went round at {periods:?}, not every {ideal:.2}"
                );
                let mean = periods.iter().sum::<usize>() as f64 / periods.len() as f64;
                assert!((mean - ideal).abs() < 0.05, "a {length}-frame loop at {tempo} averaged {mean:.3}, not {ideal:.3}");
                // And nothing from outside the loop reaches the stretcher.
                let (lo, hi) = settled.iter().fold((f64::MAX, f64::MIN), |(lo, hi), &f| (lo.min(f), hi.max(f)));
                assert!(
                    lo >= FROM as f64 - 0.5 && hi <= (FROM + length) as f64,
                    "a {length}-frame loop at {tempo} played frames {lo}..{hi}"
                );
            }
        }
    }

    /// The playhead goes round with the audio, a block at a time: no block
    /// runs across the out point, and over many passes the head reports one
    /// pass per `length / tempo` output frames, as the audio makes.
    #[test]
    fn a_stretched_head_goes_round_with_its_loop() {
        for master_tempo in [false, true] {
            let tempo = 1.02_f32;
            let length = BEAT_64;
            let mut rig = Rig::new(tempo, master_tempo);
            rig.set_loop(FROM, FROM + length);
            let render = rig.render(400 * BLOCK_FRAMES);
            let mut passes = 0;
            let mut last = FROM as f64;
            for &(at, frames) in &render.blocks {
                assert!(at >= FROM && at < FROM + length, "a block started at {at}, outside the loop");
                let reach = at as f64 + frames as f64 * f64::from(tempo);
                assert!(reach < (FROM + length) as f64 + f64::from(tempo), "a block ran to {reach}, past the out point");
                // Started behind where the last one reached: gone round.
                if (at as f64) < last - 1.0 {
                    passes += 1;
                }
                last = reach;
            }
            let ideal = render.frames.len() as f64 * f64::from(tempo) / length as f64;
            assert!(
                (f64::from(passes) - ideal).abs() <= 2.0,
                "master tempo {master_tempo}: the head went round {passes} times in {} frames, not {ideal:.1}",
                render.frames.len()
            );
        }
    }

    /// A loop too long to keep in memory goes round by seeking the streamer,
    /// and lands on its in point just the same: the frame after the out
    /// point's is the in point's.
    #[test]
    fn a_loop_too_long_to_keep_goes_round_on_the_frame() {
        let length = LOOP_AUDIO_FRAMES + 1_000;
        let mut rig = Rig::with_length(1.0, false, length + 2 * FROM);
        rig.set_loop(FROM, FROM + length);
        let render = rig.render(length as usize + 20 * BLOCK_FRAMES);
        let expected: Vec<f64> =
            (0..render.frames.len() as u64).map(|i| ((FROM + i % length) % STEPS) as f64).collect();
        assert!(render.frames == expected, "a {length}-frame loop did not go round on its in point");
    }

    /// EXIT with passes of the loop already in the stretcher: those still
    /// play, the head goes round for each, and then both carry on past the out
    /// point together rather than the playhead running a loop ahead.
    #[test]
    fn exit_lets_the_queued_passes_play_and_keeps_the_head_with_the_audio() {
        for tempo in [1.0_f32, 1.02] {
            let length = BEAT_64;
            let mut rig = Rig::new(tempo, false);
            rig.set_loop(FROM, FROM + length);
            rig.render(40 * BLOCK_FRAMES);
            rig.clock.set_looping(false);
            let render = rig.render(40 * BLOCK_FRAMES);
            let &(at, frames) = render.blocks.last().unwrap();
            assert!(at > FROM + length, "at {tempo} the head stayed in the loop after EXIT: {at}");
            // The last block's audio is where its position says it is, to
            // within the tempo smoothing's head start and one interpolation.
            let heard = render.frames[render.frames.len() - frames];
            assert!(
                (heard - at as f64).abs() < 16.0,
                "at {tempo} the head said {at} while the audio was at {heard}"
            );
            // And the audio ran on from the out point without a gap or a jump.
            let after: Vec<f64> = render.frames.iter().copied().skip_while(|&f| f < (FROM + length) as f64 - 1.0).collect();
            assert!(after.windows(2).all(|pair| pair[1] > pair[0] && pair[1] - pair[0] < 2.0), "at {tempo} the audio jumped after EXIT");
        }
    }
}
