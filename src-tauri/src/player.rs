//! The playback engine, and the tick the interface reads it through.
//!
//! The engine itself is `rbl-deck` and knows nothing about Tauri. This is the
//! adapter: it opens the audio device the first time a deck is given something
//! to do, rather than at launch. A library manager that holds an output device
//! for a deck nobody has touched is a library manager that shows up in the
//! system's audio list for no reason; the stream is paused again as soon as
//! neither deck is playing.
//!
//! Position does not come through a command. One event, ten times a second,
//! carries both decks, and the interface extrapolates between ticks from the
//! frame counter. Sixty ticks a second per deck would be pure IPC churn, and
//! the interface would still have to interpolate to draw a smooth playhead.

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use rbl_deck::{Deck, DeckEvent, Engine, Render, Sink, StreamWish};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::dto::LimiterDto;
use crate::error::{AppError, AppResult, ErrorKind};

/// How often the meters go out. A tenth of a second is a meter that steps
/// rather than moves, and the payload is three numbers.
const METER_TICK: Duration = Duration::from_millis(33);

/// Meter ticks to a deck tick, so both come off one thread.
const TICKS_PER_DECK_TICK: u32 = 3;

/// One deck in a tick.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools, reason = "the deck's flags, sent together")]
pub struct DeckTickDto {
    pub frames: i64,
    pub total_frames: u64,
    /// Bumped on every load and seek, so the interface snaps its playhead
    /// rather than easing it towards a position it did not expect.
    pub generation: u32,
    pub playing: bool,
    pub loaded: bool,
    /// Identifies the load whose audio is installed. Zero means no track.
    pub load_id: u64,
    /// How fast the deck is playing, as a multiple of the file's own speed.
    pub tempo: f32,
    /// Whether the pitch is held while that speed changes.
    pub master_tempo: bool,
    /// Semitones from the track's own key.
    pub key_shift: i8,
    /// Output frames until a started deck sounds: a play held for the beat.
    pub start_in_frames: u64,
    /// The loop's in and out points, device-rate frames; both 0 for none.
    pub loop_in_frames: u64,
    pub loop_out_frames: u64,
    /// Whether the deck is inside the loop.
    pub looping: bool,
}

/// Both decks, which is what one tick carries: about 200 bytes, well inside
/// the 1 KB event cap.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TickDto {
    pub a: DeckTickDto,
    pub b: DeckTickDto,
    pub sample_rate: u32,
    /// The loudest sample the device was given last callback, per channel, so
    /// the meter reads what can be heard rather than what is in the file.
    pub peak_left: f32,
    pub peak_right: f32,
    /// The master level, 0 to +2 dB.
    pub master: f32,
    /// How far the limiter turned the sum down since the last tick, in dB.
    pub reduction: f32,
    /// Whether this build can shift a key: the Rubber Band backend is in.
    pub shifts_key: bool,
}

impl TickDto {
    /// What to report before the engine exists: two empty decks. A deck that
    /// has never been opened is stopped at zero, which is the truth.
    pub fn silent() -> Self {
        let empty = DeckTickDto {
            frames: 0,
            total_frames: 0,
            generation: 0,
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
        };
        Self {
            a: empty,
            b: empty,
            sample_rate: 0,
            peak_left: 0.0,
            peak_right: 0.0,
            master: 1.0,
            reduction: 0.0,
            shifts_key: Engine::shifts_key(),
        }
    }
}

/// The master's meters, on their own faster beat.
///
/// Separate from the deck tick because it is wanted three times as often and
/// is a twentieth of the size: sending the decks at meter rate would be pure
/// IPC churn, and sending the meters at deck rate is a meter that steps.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeterDto {
    pub rms_left: f32,
    pub rms_right: f32,
    pub peak_left: f32,
    pub peak_right: f32,
    pub master: f32,
    /// How far the limiter turned the sum down since the last tick, in dB;
    /// 0 when it did nothing.
    pub reduction: f32,
}

/// What a deck reports outside the tick.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckEventDto {
    pub deck: String,
    pub load_id: u64,
    pub total_frames: u64,
    pub sample_rate: u32,
    pub message: Option<String>,
}

/// Opens the output the engine renders into, given the render callback, the
/// device the interface chose (`None` is the system default), and the rate
/// and buffer size it asked for.
pub type SinkOpener =
    Box<dyn Fn(Render, Option<String>, StreamWish) -> rbl_deck::Result<Arc<dyn Sink>> + Send + Sync>;

/// Holds the engine, which is not built until something is played.
pub struct Player {
    pub(crate) loaded_tracks: Mutex<std::collections::HashMap<Deck, String>>,
    engine: Mutex<Option<Arc<Engine>>>,
    /// How the engine's output is opened: the audio device in the app, and a
    /// sink the test pulls by hand in its tests.
    open_sink: SinkOpener,
    /// Whether a ticker is already running, so play does not start a second.
    ticking: std::sync::atomic::AtomicBool,
    /// The output the engine should open, as an id from `rbl_deck`.
    ///
    /// `None` is the system default. Changing it drops the engine so the next
    /// thing played opens the new device: a stream cannot be moved between
    /// devices, and rebuilding costs nothing anyone hears — the decks are
    /// reloaded from where they were.
    device: Mutex<Option<String>>,
    /// The sample rate and buffer size the interface asked for. Like the
    /// device, a change drops the engine and the next play opens with it.
    wish: Mutex<StreamWish>,
    /// The metronome's click and volume as the interface last set them,
    /// held for the same reason the limiter is.
    metronome: Mutex<(rbl_deck::ClickSound, rbl_deck::ClickVolume)>,
    /// The master limiter as the interface last set it.
    ///
    /// Held here as well as in the engine because the engine is built late
    /// and rebuilt on a device change, and a setting that only lived in it
    /// would go back to the default every time.
    limiter: Mutex<LimiterDto>,
    master_level: Mutex<f32>,
    /// Whether the engine's load events go to the interface as `deck:*`.
    /// The browser's preview player is a second engine and keeps quiet: its
    /// deck A is not the player's deck A.
    emits_deck_events: bool,
}

impl Default for Player {
    /// A player on the system's audio output.
    fn default() -> Self {
        Self::with_sink(Box::new(|render, device, wish| {
            Ok(Arc::new(rbl_deck::CpalSink::open_with(render, device, wish)?) as Arc<dyn Sink>)
        }))
    }
}

impl Player {
    /// A player whose engine renders into whatever `open_sink` opens.
    pub fn with_sink(open_sink: SinkOpener) -> Self {
        Self {
            engine: Mutex::new(None),
            master_level: Mutex::new(1.0),
            open_sink,
            ticking: std::sync::atomic::AtomicBool::new(false),
            loaded_tracks: Mutex::new(std::collections::HashMap::new()),
            device: Mutex::new(None),
            wish: Mutex::new(StreamWish::default()),
            metronome: Mutex::new((rbl_deck::ClickSound::Two, rbl_deck::ClickVolume::Large)),
            limiter: Mutex::new(LimiterDto {
                input_gain_db: rbl_deck::DEFAULT_INPUT_GAIN_DB,
                enabled: false,
                ceiling_db: rbl_deck::DEFAULT_CEILING_DB,
                release_ms: rbl_deck::DEFAULT_RELEASE_MS,
            }),
            emits_deck_events: true,
        }
    }

    /// The same player, with its load events kept to itself.
    #[must_use]
    pub fn quiet(mut self) -> Self {
        self.emits_deck_events = false;
        self
    }
}

/// The engine's limiter as the interface sees it, read back after the engine
/// clamped it.
fn limiter_of(settings: &rbl_deck::LimiterSettings) -> LimiterDto {
    LimiterDto {
        input_gain_db: settings.input_gain_db(),
        enabled: settings.enabled(),
        ceiling_db: settings.ceiling_db(),
        release_ms: settings.release_ms(),
    }
}

impl Player {
    /// The engine, opening the audio device on first use.
    ///
    /// Opening it is what makes noise possible, so it happens when a deck is
    /// asked to do something and not before. The stream itself stays paused
    /// until something plays.
    pub fn engine<R: Runtime>(&self, app: &AppHandle<R>) -> AppResult<Arc<Engine>> {
        let mut held = self.engine.lock();
        if let Some(engine) = held.as_ref() {
            return Ok(Arc::clone(engine));
        }
        let handle = app.clone();
        let emits = self.emits_deck_events;
        let events: rbl_deck::EventSink = Arc::new(move |event: DeckEvent| {
            if emits {
                emit_deck_event(&handle, &event);
            }
        });
        let device = self.device.lock().clone();
        let wish = *self.wish.lock();
        let engine = Engine::with_sink(|render| (self.open_sink)(render, device, wish), &events).map_err(|e| {
            AppError::new(ErrorKind::Internal, "The audio device could not be opened.")
                .with_detail(e.to_string())
        })?;
        // What the interface asked for, before the first callback runs.
        Self::apply_limiter(engine.limiter(), *self.limiter.lock());
        engine.master().set_gain(*self.master_level.lock());
        let (sound, volume) = *self.metronome.lock();
        engine.metronome().set_sound(sound);
        engine.metronome().set_volume(volume);
        let engine = Arc::new(engine);
        *held = Some(Arc::clone(&engine));
        Ok(engine)
    }

    /// Retain the level when an output-device change rebuilds the engine.
    pub fn set_master_level(&self, level: f32) {
        let engine = self.engine.lock();
        let safe = if level.is_finite() { level.clamp(0.0, rbl_deck::MAX_MASTER_GAIN) } else { 1.0 };
        *self.master_level.lock() = safe;
        if let Some(engine) = engine.as_ref() {
            engine.master().set_gain(safe);
        }
    }

    fn apply_limiter(settings: &rbl_deck::LimiterSettings, wanted: LimiterDto) {
        settings.set_input_gain_db(wanted.input_gain_db);
        settings.set_enabled(wanted.enabled);
        settings.set_ceiling_db(wanted.ceiling_db);
        settings.set_release_ms(wanted.release_ms);
    }

    /// Sets the master limiter, now if the engine is up and at its build if
    /// not, and returns what was actually set — the engine clamps.
    pub fn set_limiter(&self, wanted: LimiterDto) -> LimiterDto {
        // Through a throwaway settings so the clamping is the engine's own,
        // whether or not there is an engine yet.
        let clamped = rbl_deck::LimiterSettings::default();
        Self::apply_limiter(&clamped, wanted);
        let safe = limiter_of(&clamped);
        *self.limiter.lock() = safe;
        if let Some(engine) = self.opened() {
            Self::apply_limiter(engine.limiter(), safe);
        }
        safe
    }

    pub fn limiter(&self) -> LimiterDto {
        *self.limiter.lock()
    }

    /// Which output to open. `None` is the system default.
    ///
    /// Takes effect on the next thing played: the engine is dropped here and
    /// rebuilt then, because a running stream belongs to the device it was
    /// opened on.
    pub fn set_device(&self, device: Option<String>) -> bool {
        let mut current = self.device.lock();
        if *current == device {
            return false;
        }
        *current = device;
        drop(current);
        // Dropped rather than replaced: whatever is playing is on the old
        // device, and the next play opens the new one.
        self.engine.lock().take().is_some()
    }

    pub fn device(&self) -> Option<String> {
        self.device.lock().clone()
    }

    /// The rate and buffer size to open the device with. A change drops the
    /// engine, as a device change does: a stream has the rate it was opened at.
    pub fn set_wish(&self, wish: StreamWish) -> bool {
        let mut current = self.wish.lock();
        if *current == wish {
            return false;
        }
        *current = wish;
        drop(current);
        self.engine.lock().take().is_some()
    }

    pub fn wish(&self) -> StreamWish {
        *self.wish.lock()
    }

    /// The metronome's click and volume, now if the engine is up and at its
    /// build if not.
    pub fn set_metronome(&self, sound: rbl_deck::ClickSound, volume: rbl_deck::ClickVolume) {
        *self.metronome.lock() = (sound, volume);
        if let Some(engine) = self.opened() {
            engine.metronome().set_sound(sound);
            engine.metronome().set_volume(volume);
        }
    }

    /// Already-built engine only — for a tick, which must not open a device.
    pub fn opened(&self) -> Option<Arc<Engine>> {
        self.engine.lock().clone()
    }

    pub fn ticking(&self) -> &std::sync::atomic::AtomicBool {
        &self.ticking
    }
}

fn emit_deck_event<R: Runtime>(app: &AppHandle<R>, event: &DeckEvent) {
    let (name, payload) = match event {
        DeckEvent::Loaded { deck, load_id, total_frames, sample_rate } => (
            "deck:loaded",
            DeckEventDto {
                deck: deck.name().to_owned(),
                load_id: *load_id,
                total_frames: *total_frames,
                sample_rate: *sample_rate,
                message: None,
            },
        ),
        DeckEvent::Error { deck, load_id, message } => (
            "deck:error",
            DeckEventDto {
                deck: deck.name().to_owned(),
                load_id: *load_id,
                total_frames: 0,
                sample_rate: 0,
                message: Some(message.clone()),
            },
        ),
    };
    if let Err(e) = app.emit(name, payload) {
        tracing::warn!(error = %e, "a deck event did not reach the interface");
    }
}

/// Starts the ticker if one is not already running.
///
/// It stops after a short RMS tail when neither deck is playing nor scrubbing.
/// Playing or beginning a drag starts it again.
pub fn start_ticker<R: Runtime>(app: &AppHandle<R>) {
    let player = app.state::<Arc<Player>>();
    if player.ticking().swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let handle = app.clone();
    // Its own thread rather than the async runtime: this is a fixed 10 Hz beat
    // that ends when playback does, and it must not share a runtime worker
    // with a command that is reading the database.
    let spawned = std::thread::Builder::new().name("rbl-deck-tick".to_owned()).spawn(move || {
        let mut since_deck_tick = 0_u32;
        let mut silent_ticks = 0_u32;
        loop {
            std::thread::sleep(METER_TICK);
            let player = handle.state::<Arc<Player>>();
            let Some(engine) = player.opened() else { break };
            let master = engine.master();
            let (peak_left, peak_right) = master.peaks();
            let reduction = master.reduction_db();
            let (rms_left, rms_right) = master.rms();
            if let Err(e) = handle.emit(
                "deck:meters",
                MeterDto { peak_left, peak_right, rms_left, rms_right, master: master.gain(), reduction },
            ) {
                tracing::warn!(error = %e, "a meter tick did not reach the interface");
            }

            since_deck_tick += 1;
            if since_deck_tick < TICKS_PER_DECK_TICK {
                continue;
            }
            since_deck_tick = 0;
            let snapshot = engine.snapshot();
            // The peaks were taken and cleared above, so the deck tick carries
            // what this pass read rather than an empty meter.
            let mut tick = tick_of(&snapshot, master);
            tick.peak_left = peak_left;
            tick.peak_right = peak_right;
            tick.reduction = reduction;
            if let Err(e) = handle.emit("deck:tick", tick) {
                tracing::warn!(error = %e, "a deck tick did not reach the interface");
            }
            if engine.any_sounding() {
                silent_ticks = 0;
            } else {
                // Publish the 400 ms RMS tail after playback stops.
                silent_ticks += 1;
                if silent_ticks >= 5 { break; }
            }
        }
        let player = handle.state::<Arc<Player>>();
        player.ticking().store(false, std::sync::atomic::Ordering::SeqCst);
        // A play or scrub can start between the idle check and releasing the
        // ticker flag. Its start request saw us still running; hand it off now.
        if player.opened().is_some_and(|engine| engine.any_sounding()) {
            start_ticker(&handle);
        }
    });
    if let Err(e) = spawned {
        tracing::error!(error = %e, "the deck ticker could not be started");
        player.ticking().store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

pub fn tick_of(snapshot: &rbl_deck::Snapshot, master: &rbl_deck::Master) -> TickDto {
    let deck = |s: &rbl_deck::DeckSnapshot| DeckTickDto {
        frames: if s.pre_roll_frames > 0 {
            -i64::try_from(s.pre_roll_frames).unwrap_or(i64::MAX)
        } else {
            i64::try_from(s.position_frames).unwrap_or(i64::MAX)
        },
        total_frames: s.total_frames,
        generation: s.generation,
        playing: s.playing,
        loaded: s.loaded,
        load_id: s.load_id,
        tempo: s.tempo,
        master_tempo: s.master_tempo,
        key_shift: s.key_shift,
        start_in_frames: s.start_in_frames,
        loop_in_frames: s.loop_in_frames,
        loop_out_frames: s.loop_out_frames,
        looping: s.looping,
    };
    let (peak_left, peak_right) = master.peaks();
    TickDto {
        a: deck(&snapshot.a),
        b: deck(&snapshot.b),
        sample_rate: snapshot.sample_rate,
        peak_left,
        peak_right,
        master: master.gain(),
        reduction: master.reduction_db(),
        shifts_key: Engine::shifts_key(),
    }
}

/// Which deck a command names. Unknown names are deck A rather than an error:
/// the interface only ever sends what it was given in a tick.
pub fn deck_of(name: &str) -> Deck {
    match name {
        "b" | "B" => Deck::B,
        _ => Deck::A,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[allow(clippy::float_cmp, reason = "the clamp's bounds are exact constants, and that is the assertion")]
mod tests {
    use super::*;

    #[test]
    fn a_deck_name_maps_to_a_deck_and_never_fails() {
        assert_eq!(deck_of("a"), Deck::A);
        assert_eq!(deck_of("b"), Deck::B);
        assert_eq!(deck_of("B"), Deck::B);
        // The interface only ever sends what a tick gave it, so an unknown
        // name is a bug elsewhere rather than something to refuse a command
        // over; it plays on deck A.
        assert_eq!(deck_of("nonsense"), Deck::A);
    }

    #[test]
    fn a_tick_before_the_engine_exists_is_two_stopped_decks() {
        let tick = TickDto::silent();
        assert_eq!(tick.a.frames, 0);
        assert!(!tick.a.playing);
        assert!(!tick.b.loaded);
        assert_eq!(tick.sample_rate, 0);
    }

    #[test]
    fn a_new_player_carries_the_engine_s_limiter_defaults() {
        let limiter = Player::default().limiter();
        assert!(!limiter.enabled);
        assert_eq!(limiter.input_gain_db, rbl_deck::DEFAULT_INPUT_GAIN_DB);
        assert_eq!(limiter.ceiling_db, rbl_deck::DEFAULT_CEILING_DB);
        assert_eq!(limiter.release_ms, rbl_deck::DEFAULT_RELEASE_MS);
    }

    #[test]
    fn set_limiter_returns_what_the_engine_would_clamp_to() {
        let player = Player::default();
        let set = player.set_limiter(LimiterDto { input_gain_db: 6.0, enabled: false, ceiling_db: 3.0, release_ms: 5.0 });
        assert!(!set.enabled);
        assert_eq!(set.ceiling_db, 0.0, "a ceiling over full scale is full scale");
        assert_eq!(set.release_ms, 10.0, "a release under ten milliseconds is ten");
    }

    #[test]
    fn set_limiter_turns_a_nan_into_the_default() {
        let player = Player::default();
        let set = player.set_limiter(LimiterDto {
            input_gain_db: f32::NAN,
            enabled: true,
            ceiling_db: f32::NAN,
            release_ms: f32::NAN,
        });
        assert_eq!(set.input_gain_db, rbl_deck::DEFAULT_INPUT_GAIN_DB);
        assert_eq!(set.ceiling_db, rbl_deck::DEFAULT_CEILING_DB);
        assert_eq!(set.release_ms, rbl_deck::DEFAULT_RELEASE_MS);
    }

    #[test]
    fn the_limiter_is_remembered_before_there_is_an_engine() {
        // No engine has been built: the setting still has to be there for
        // the build, and read back as set — clamped, not as asked.
        let player = Player::default();
        player.set_limiter(LimiterDto { input_gain_db: 6.0, enabled: false, ceiling_db: -40.0, release_ms: 250.0 });
        let held = player.limiter();
        assert!(!held.enabled);
        assert_eq!(held.ceiling_db, rbl_deck::MIN_CEILING_DB);
        assert_eq!(held.release_ms, 250.0);
        assert_eq!(held.input_gain_db, 6.0);
    }

    #[test]
    fn a_tick_is_small_enough_for_the_event_cap() {
        // The cap on an event payload is 1 KB; this carries both decks.
        let json = serde_json::to_string(&TickDto::silent()).unwrap();
        assert!(json.len() < 1_024, "a tick was {} bytes", json.len());
    }
}
