//! The browser's preview player: a click on a row's waveform plays the track
//! from there without loading it onto a deck.
//!
//! What rekordbox does, from its own binary (rekordbox 7, macOS arm64):
//!
//! - [OBS] `browse::ListViewer::cellClickedWithLeftButton` sends a plain left
//!   click on the Preview column (column id 68) to
//!   `PreviewComponent::clickWave`, unless the click is the second of a double
//!   click, is being dragged, or carries Shift or Command.
//! - [OBS] `clickWave` takes the click's x across the waveform, clamped to it,
//!   as a fraction of the track's length, and calls
//!   `ListViewer::startPreviewPlayer` with that time.
//! - [OBS] `ListViewer::preparePreviewPlayer` refuses a track whose file is
//!   not there (`db::isValidMediaPath`), stops a preview of another track,
//!   and loads the track into `PreviewPlayer` — its own player, not a deck.
//!   A click on the track already previewing only moves it.
//! - [OBS] `ListViewer::startPreviewPlayer` pauses any deck that is playing
//!   unless the app is in PERFORMANCE mode, then starts the preview.
//!   rbxport's player is EXPORT mode's, so the decks are paused here.
//! - [OBS] `PreviewComponent::cueRegionMouseDown` starts the preview from a
//!   hot cue's own time when its badge is clicked.
//!
//! And what stops it again (rekordbox 7.2.11, macOS arm64), besides its stop
//! button (`PreviewComponent::buttonClicked`):
//!
//! - [OBS] A deck that plays. `PreviewComponent::startPreviewPlayer` runs a
//!   500 ms timer while the preview plays; `PreviewComponent::timerCallback`
//!   then calls `stopPreviewPlayer` when `djengine::DjEngineIF::isPlaying` is
//!   true for any of the four channels, unless the app is in PERFORMANCE mode
//!   (the same mode flag `startPreviewPlayer` checks before pausing decks).
//!   rbxport stops it as the deck is told to play, rather than on a timer.
//! - [OBS] A track loaded onto a deck from the list:
//!   `ListViewer::loadTrack(ePlayerID, int, RowDataTrack*, bool, bool)` stops
//!   and unloads the list's `PreviewPlayer` if it is playing, whatever track
//!   it holds.
//! - [OBS] Another playlist chosen in the tree:
//!   `BrowseListViewer::currentBrowseChanged` calls `stopPreviewPlayer` when
//!   `TreeViewer::isChangeSelectedItem()` is set, which
//!   `TreeViewer::changedSelectedItem` sets when the newly selected item's
//!   browse source differs from the old one. That one is the interface's
//!   (`src/app/App.tsx`).
//!
//! [ASSUME] rekordbox can route its preview to a separate output channel
//! (`OutputChannel_Preview`). rbxport has one output, so the preview plays on
//! the same device the decks use.
//!
//! The preview is a second engine of its own, playing on its deck A. It shares
//! nothing with the decks but the device it opens.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rbl_deck::Deck;
use serde::Serialize;
use tauri::{AppHandle, Runtime};

use crate::error::{AppError, AppResult, ErrorKind};
use crate::player::{Player, SinkOpener};

/// How long a preview waits for its file to open before giving up.
const LOAD_TIMEOUT: Duration = Duration::from_secs(10);

/// The one deck of the preview engine that plays.
const PREVIEW_DECK: Deck = Deck::A;

/// The preview as the interface reads it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewStateDto {
    /// The track being previewed, or `None` before anything was.
    pub track: Option<String>,
    pub playing: bool,
    pub position_ms: f64,
    pub duration_ms: f64,
}

impl PreviewStateDto {
    fn idle() -> Self {
        Self { track: None, playing: false, position_ms: 0.0, duration_ms: 0.0 }
    }
}

/// The preview player: its own engine, and which track it holds.
pub struct Preview {
    player: Player,
    track: Mutex<Option<String>>,
    /// The newest request. An older one still waiting for its file to open
    /// gives way rather than starting a track nobody is asking for now.
    request: AtomicU64,
    /// Held by a start from its last look at `request` to the play, and by a
    /// stop while it pauses, so a stop cannot land between the two and be
    /// followed by a play that nobody can see to stop (#242).
    starting: Mutex<()>,
}

impl Default for Preview {
    fn default() -> Self {
        Self::from_player(Player::default())
    }
}

impl Preview {
    /// A preview whose engine renders into whatever `open_sink` opens.
    pub fn with_sink(open_sink: SinkOpener) -> Self {
        Self::from_player(Player::with_sink(open_sink))
    }

    fn from_player(player: Player) -> Self {
        Self {
            player: player.quiet(),
            track: Mutex::new(None),
            request: AtomicU64::new(0),
            starting: Mutex::new(()),
        }
    }

    /// Plays `track`, from the file at `path`, from `position_ms`.
    ///
    /// Pauses the decks first, as rekordbox does outside PERFORMANCE mode.
    /// Returns once the preview is playing, or once a newer request has taken
    /// over from this one.
    pub fn play<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        decks: &Player,
        track: &str,
        path: &std::path::Path,
        position_ms: f64,
    ) -> AppResult<()> {
        if !path.is_file() {
            return Err(AppError::new(ErrorKind::NotFound, "That track's file could not be found.")
                .with_detail(path.display().to_string()));
        }
        let request = self.request.fetch_add(1, Ordering::SeqCst).wrapping_add(1).max(1);

        if let Some(engine) = decks.opened() {
            let snapshot = engine.snapshot();
            let mut paused = false;
            for (deck, state) in [(Deck::A, snapshot.a), (Deck::B, snapshot.b)] {
                if state.playing {
                    engine.pause(deck);
                    paused = true;
                }
            }
            // The decks' tick is what tells the interface they stopped.
            if paused {
                crate::player::start_ticker(app);
            }
        }

        // The device the decks use, their level and their limiter, so a
        // preview is heard where and as loud as they are. The level is the
        // one the master knob last set, whether or not a deck has opened the
        // device yet: reading it off a running deck engine left a preview
        // started before any deck at full level.
        self.player.set_device(decks.device());
        self.player.set_wish(decks.wish());
        self.player.set_master_level(decks.master_level());
        self.player.set_limiter(decks.limiter());
        let engine = self.player.engine(app)?;

        let snapshot = engine.snapshot().a;
        let holding = self.track.lock().as_deref() == Some(track) && snapshot.loaded;
        if !holding {
            engine.load_as(PREVIEW_DECK, &PathBuf::from(path), request);
            *self.track.lock() = Some(track.to_owned());
            let deadline = Instant::now() + LOAD_TIMEOUT;
            loop {
                if self.request.load(Ordering::SeqCst) != request {
                    return Ok(());
                }
                let deck = engine.snapshot().a;
                if deck.loaded && deck.load_id == request {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err(AppError::new(ErrorKind::Internal, "The track could not be previewed.")
                        .with_detail(format!("{} did not open in time", path.display())));
                }
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        let _starting = self.starting.lock();
        if self.request.load(Ordering::SeqCst) != request {
            return Ok(());
        }
        let position_ms = if position_ms.is_finite() { position_ms.max(0.0) } else { 0.0 };
        engine.seek_ms(PREVIEW_DECK, position_ms);
        engine.play(PREVIEW_DECK);
        Ok(())
    }

    /// Follows the master knob. The preview has an engine of its own, so a
    /// level set on the decks alone left a playing preview as loud as it
    /// started, whatever the knob did after (#207).
    pub fn set_master_level(&self, level: f32) {
        self.player.set_master_level(level);
    }

    /// Follows the master limiter, for the same reason as the level.
    pub fn set_limiter(&self, limiter: crate::dto::LimiterDto) {
        self.player.set_limiter(limiter);
    }

    /// Stops the preview where it is. The track stays held, so a click on it
    /// again starts at once.
    ///
    /// Also what a deck's Play and a deck load call: in rekordbox outside
    /// PERFORMANCE mode a playing deck stops the preview, and so does loading
    /// a track onto a deck (see the top of this file).
    pub fn stop(&self) {
        let _starting = self.starting.lock();
        // Anything still waiting for its file gives way too.
        self.request.fetch_add(1, Ordering::SeqCst);
        if let Some(engine) = self.player.opened() {
            engine.pause(PREVIEW_DECK);
        }
    }

    #[allow(clippy::cast_precision_loss, reason = "a frame count is far below 2^52")]
    pub fn state(&self) -> PreviewStateDto {
        let Some(engine) = self.player.opened() else { return PreviewStateDto::idle() };
        let track = self.track.lock().clone();
        let snapshot = engine.snapshot();
        let deck = snapshot.a;
        let rate = f64::from(snapshot.sample_rate.max(1));
        if !deck.loaded {
            return PreviewStateDto { track, ..PreviewStateDto::idle() };
        }
        PreviewStateDto {
            track,
            playing: deck.playing,
            position_ms: deck.position_frames as f64 * 1000.0 / rate,
            duration_ms: deck.total_frames as f64 * 1000.0 / rate,
        }
    }

    /// The preview's own engine, once something has been previewed.
    pub fn opened(&self) -> Option<Arc<rbl_deck::Engine>> {
        self.player.opened()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_previewed_is_an_idle_state() {
        let preview = Preview::with_sink(Box::new(|_, _, _| Err(rbl_deck::DeckError::NoDevice)));
        assert_eq!(preview.state(), PreviewStateDto::idle());
        // Stopping what never started is not an error.
        preview.stop();
        assert!(preview.opened().is_none());
    }
}
