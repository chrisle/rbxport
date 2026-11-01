//! Where the mixed audio goes.
//!
//! Behind a trait so the engine can be tested without an audio device: the
//! null sink pulls the same callback the real one does, on the calling thread,
//! so a test can ask for exactly 512 frames and look at them.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use crate::health::{AudioHealth, HealthMeter, SignalProbe, SignalReport};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::{DeckError, Result};

/// Fills a stereo interleaved buffer. Called on the audio thread, so it obeys
/// the realtime rules: no allocation, no lock, no syscall, no panic.
pub type Render = Box<dyn FnMut(&mut [f32]) + Send>;

pub trait Sink: Send + Sync {
    fn health(&self) -> AudioHealth { AudioHealth::default() }
    /// The rate everything downstream of the resampler runs at.
    fn sample_rate(&self) -> u32;
    /// Starts pulling. Called when a deck starts playing.
    fn start(&self) -> Result<()>;
    /// Stops pulling, so an idle app costs nothing.
    fn stop(&self) -> Result<()>;
}

/// How long the stream runs on after the last deck has stopped.
///
/// A stop is a fade, and a fade needs callbacks to happen in: pausing the
/// stream the instant the transport asks cuts the last two milliseconds off,
/// which is the click the fade exists to prevent. Sixty milliseconds is
/// several callbacks at any buffer size a device is likely to choose, and a
/// stream that lives that much longer costs nothing anyone can measure.
pub const LINGER: Duration = Duration::from_millis(60);

/// The same wait, in frames, for a sink that is pulled by hand.
pub const fn linger_frames(sample_rate: u32) -> usize {
    (sample_rate as usize * LINGER.as_millis() as usize) / 1000
}

/// What the thread that owns the cpal stream is asked to do.
enum Ask {
    Start,
    Stop,
    Quit,
}

/// The real device.
///
/// The stream lives on its own thread because `cpal::Stream` is not `Send` on
/// every platform — Core Audio's is not — and the engine has to be `Send` and
/// `Sync` to sit in Tauri's state. The thread owns it and takes instructions.
pub struct CpalSink {
    health: Arc<HealthMeter>,
    ask: Sender<Ask>,
    sample_rate: u32,
    running: AtomicBool,
}

impl CpalSink {
    /// Opens the default output device.
    pub fn open(render: Render) -> Result<Self> {
        Self::open_named(render, None)
    }

    /// Opens one output by id, or the default when `wanted` is `None`.
    ///
    /// By id rather than by position: a device list renumbers whenever
    /// something is plugged in, and a stored index would pick a different box
    /// after a reboot. An id that is no longer there falls back to the default
    /// rather than refusing to play — a missing interface should not be a
    /// silent app.
    pub fn open_named(render: Render, wanted: Option<String>) -> Result<Self> {
        Self::open_with(render, wanted, StreamWish::default())
    }

    /// The same, asking the device for a rate and a buffer size.
    ///
    /// Asked, not demanded: a device that does not offer the rate is opened
    /// at its default, and a buffer size outside what it supports is left to
    /// it, with a warning either way. The rate actually opened is what
    /// `sample_rate` reports and what the decks resample to.
    pub fn open_with(render: Render, wanted: Option<String>, wish: StreamWish) -> Result<Self> {
        let (ask_tx, ask_rx) = std::sync::mpsc::channel();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();

        let health = Arc::new(HealthMeter::default());
        let callback_health = Arc::clone(&health);
        std::thread::Builder::new()
            .name("rbl-deck-device".to_owned())
            .spawn(move || {
                device_thread(render, wanted.as_deref(), wish, &ask_rx, &ready_tx, callback_health);
            })
            .map_err(DeckError::Io)?;

        // The rate decides what the decoders resample to, so opening is not
        // finished until the device has said what it is.
        let sample_rate = ready_rx
            .recv()
            .map_err(|_| DeckError::Device("the audio thread stopped while starting".to_owned()))??;

        Ok(Self { health, ask: ask_tx, sample_rate, running: AtomicBool::new(false) })
    }
}

impl Sink for CpalSink {
    fn health(&self) -> AudioHealth { self.health.snapshot(self.running.load(Ordering::Relaxed)) }
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn start(&self) -> Result<()> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        self.ask
            .send(Ask::Start)
            .map_err(|_| DeckError::Device("the audio thread has stopped".to_owned()))
    }

    fn stop(&self) -> Result<()> {
        if !self.running.swap(false, Ordering::SeqCst) {
            return Ok(());
        }
        self.ask
            .send(Ask::Stop)
            .map_err(|_| DeckError::Device("the audio thread has stopped".to_owned()))
    }
}

impl Drop for CpalSink {
    fn drop(&mut self) {
        let _ = self.ask.send(Ask::Quit);
    }
}

/// Owns the stream and does as it is told.
fn device_thread(
    render: Render,
    wanted: Option<&str>,
    wish: StreamWish,
    ask: &Receiver<Ask>,
    ready: &Sender<Result<u32>>,
    health: Arc<HealthMeter>,
) {
    let probe = Arc::new(SignalProbe::default());
    let stream = match build_stream(render, wanted, wish, health, Arc::clone(&probe)) {
        Ok((stream, rate)) => {
            if ready.send(Ok(rate)).is_err() {
                return;
            }
            stream
        }
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };

    // A stop is not taken at once: see `LINGER`. Until it is due, the thread
    // waits on the channel rather than on the clock, so a start that arrives
    // in the meantime simply cancels it.
    let mut due: Option<Instant> = None;
    loop {
        let next = match due {
            Some(at) => match ask.recv_timeout(at.saturating_duration_since(Instant::now())) {
                Ok(next) => next,
                Err(RecvTimeoutError::Timeout) => {
                    due = None;
                    if let Err(e) = stream.pause() {
                        tracing::error!(error = %e, "the audio device would not stop");
                    }
                    log_signal("audio output stopped", probe.take());
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => match ask.recv() {
                Ok(next) => next,
                Err(_) => break,
            },
        };
        match next {
            Ask::Start => {
                due = None;
                probe.take();
                if let Err(e) = stream.play() {
                    tracing::error!(error = %e, "the audio device would not start");
                }
            }
            Ask::Stop => due = Some(Instant::now() + LINGER),
            Ask::Quit => break,
        }
    }
    // Dropping the stream here, on the thread that built it.
    drop(stream);
}

/// What the callback rendered while the stream ran, for a log a person can read
/// when a report says the stream opened and ran but nothing was heard:
/// `audible=false` means the app fed the device silence; `audible=true` means
/// the signal left the callback and was lost after it.
fn log_signal(what: &str, report: SignalReport) {
    tracing::info!(callbacks = report.callbacks, peak = report.peak, audible = report.audible(), "{what}");
}

/// One output the audio can go to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDevice {
    /// What to store and what to open by: cpal's own id, which it documents as
    /// stable across runs, disconnections and reboots. A position in a list is
    /// not — plugging an interface in renumbers everything after it.
    pub id: String,
    /// What to show. The device's own name, which is what a person recognises.
    pub name: String,
}

/// Every output the default host offers.
///
/// A device that will not give an id is skipped: it cannot be stored, so
/// offering it would be offering a choice that does not survive a restart.
#[must_use]
pub fn output_devices() -> Vec<AudioDevice> {
    let host = cpal::default_host();
    let Ok(devices) = host.output_devices() else { return Vec::new() };
    devices
        .filter_map(|device| {
            let id = device.id().ok()?;
            Some(AudioDevice { id: id.to_string(), name: device.to_string() })
        })
        .collect()
}

/// The one the engine opens when nothing has been chosen.
#[must_use]
pub fn default_output_device() -> Option<AudioDevice> {
    let device = cpal::default_host().default_output_device()?;
    let id = device.id().ok()?;
    Some(AudioDevice { id: id.to_string(), name: device.to_string() })
}

/// What Preferences › Audio asks of the device: a sample rate and a buffer
/// size, either of which may be left to the device.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StreamWish {
    pub sample_rate: Option<u32>,
    /// Frames per callback.
    pub buffer_frames: Option<u32>,
}

/// The device's configuration for a wish: its default, at the wished rate
/// if it offers that rate, with the wished buffer if that is in its range.
fn configure(device: &cpal::Device, wish: StreamWish) -> Result<(cpal::StreamConfig, cpal::SampleFormat)> {
    let supported = device
        .default_output_config()
        .map_err(|e| DeckError::Device(e.to_string()))?;
    let format = supported.sample_format();
    let buffer_range = *supported.buffer_size();
    let mut config: cpal::StreamConfig = supported.into();

    if let Some(rate) = wish.sample_rate {
        // Among the device's ranges, one at the default format that covers
        // the rate; a device that cannot run there keeps its default.
        let offers = device.supported_output_configs().is_ok_and(|mut ranges| {
            ranges.any(|range| {
                range.sample_format() == format
                    && range.channels() == config.channels
                    && range.min_sample_rate() <= rate
                    && rate <= range.max_sample_rate()
            })
        });
        if offers {
            config.sample_rate = rate;
        } else {
            tracing::warn!(rate, opened = config.sample_rate, "the audio device does not offer that sample rate");
        }
    }
    if let Some(frames) = wish.buffer_frames {
        match buffer_range {
            cpal::SupportedBufferSize::Range { min, max } if (min..=max).contains(&frames) => {
                config.buffer_size = cpal::BufferSize::Fixed(frames);
            }
            cpal::SupportedBufferSize::Range { min, max } => {
                tracing::warn!(frames, min, max, "the audio device does not offer that buffer size");
            }
            cpal::SupportedBufferSize::Unknown => {
                // The device will not say; asking is harmless, and it may take it.
                config.buffer_size = cpal::BufferSize::Fixed(frames);
            }
        }
    }
    Ok((config, format))
}

fn build_stream(
    render: Render,
    wanted: Option<&str>,
    wish: StreamWish,
    health: Arc<HealthMeter>,
    probe: Arc<SignalProbe>,
) -> Result<(cpal::Stream, u32)> {
    let host = cpal::default_host();
    // The named one if it is there, and the default if it is not: a device
    // that has been unplugged since it was chosen should not stop the app
    // making a sound.
    let device = wanted
        .and_then(|id| {
            host.output_devices().ok().and_then(|mut devices| {
                devices.find(|device| device.id().is_ok_and(|found| found.to_string() == id))
            })
        })
        .or_else(|| host.default_output_device())
        .ok_or(DeckError::NoDevice)?;
    if let (Some(wanted), Ok(opened)) = (wanted, device.id()) {
        if wanted != opened.to_string() {
            tracing::warn!(wanted, opened = %device, "that audio device is not here; using another");
        }
    }
    let (config, format) = configure(&device, wish)?;
    let rate = config.sample_rate;
    let channels = config.channels;
    tracing::debug!(device = %device, rate, buffer = ?config.buffer_size, "audio output opened");
    // What the device agreed to, beyond the rate: a Bluetooth output can offer
    // a format or channel count the callback has to be told about.
    tracing::info!(
        device = %device,
        rate,
        channels,
        format = %format,
        buffer = ?config.buffer_size,
        "audio output negotiated"
    );

    let error = |e: cpal::Error| match e.kind() {
        cpal::ErrorKind::Xrun => tracing::warn!(error = %e, "audio buffer underrun or overrun"),
        cpal::ErrorKind::DeviceChanged => tracing::info!(error = %e, "audio output changed"),
        _ => tracing::error!(error = %e, "audio device error"),
    };
    let stream = match format {
        cpal::SampleFormat::F32 => build::<f32>(&device, config, channels, render, error, health, probe),
        cpal::SampleFormat::I16 => build::<i16>(&device, config, channels, render, error, health, probe),
        cpal::SampleFormat::U16 => build::<u16>(&device, config, channels, render, error, health, probe),
        cpal::SampleFormat::I32 => build::<i32>(&device, config, channels, render, error, health, probe),
        other => Err(DeckError::Device(format!("this device wants {other} samples, which we do not write"))),
    }?;
    Ok((stream, rate))
}

/// The device's own buffer never reaches this in practice; a callback asking
/// for more is served in several passes rather than by allocating.
const SCRATCH_FRAMES: usize = 4096;

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: u16,
    mut render: Render,
    error: fn(cpal::Error),
    health: Arc<HealthMeter>,
    probe: Arc<SignalProbe>,
) -> Result<cpal::Stream>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    // Allocated here, on the control thread, and only written to inside the
    // callback: the callback itself never allocates.
    let mut scratch = vec![0.0_f32; SCRATCH_FRAMES * 2];
    let lanes = channels.max(1) as usize;
    let rate = config.sample_rate;

    device
        .build_output_stream(
            config,
            move |out: &mut [T], _: &cpal::OutputCallbackInfo| {
                let started = Instant::now();
                for chunk in out.chunks_mut(SCRATCH_FRAMES * lanes) {
                    let frames = chunk.len() / lanes;
                    // The chunk is bounded by the scratch, so this cannot be
                    // short; silence is the right answer if it ever were.
                    let Some(stereo) = scratch.get_mut(..frames * 2) else {
                        for sample in chunk.iter_mut() {
                            *sample = T::from_sample(0.0_f32);
                        }
                        continue;
                    };
                    stereo.fill(0.0);
                    render(stereo);
                    probe.record(stereo);
                    // Stereo into however many lanes the device has: a third
                    // and further channels stay silent rather than repeating.
                    for (frame, lane) in stereo.chunks_exact(2).zip(chunk.chunks_mut(lanes)) {
                        for (at, sample) in lane.iter_mut().enumerate() {
                            let value = if at < 2 { frame.get(at).copied().unwrap_or(0.0) } else { 0.0 };
                            *sample = T::from_sample(value);
                        }
                    }
                }
                health.record(started.elapsed(), out.len() / lanes, rate);
            },
            error,
            None,
        )
        .map_err(|e| DeckError::Device(e.to_string()))
}

/// A sink with no device behind it: the test pulls it by hand.
pub struct NullSink {
    render: Mutex<Render>,
    sample_rate: u32,
    running: AtomicBool,
    /// Frames still to be pulled after a stop, so a fade has somewhere to
    /// happen. The device does the same thing: see `LINGER`.
    linger: AtomicUsize,
}

impl NullSink {
    pub fn new(sample_rate: u32, render: Render) -> Self {
        Self {
            render: Mutex::new(render),
            sample_rate,
            running: AtomicBool::new(false),
            linger: AtomicUsize::new(0),
        }
    }

    /// Pulls `frames` stereo frames, as a device callback would.
    ///
    /// Silence once the stream is down. A stream that was just stopped is
    /// still pulled for `LINGER`, which is where the decks fade themselves
    /// out; the real device behaves the same way.
    pub fn pull(&self, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0_f32; frames * 2];
        if !self.running.load(Ordering::SeqCst) {
            let left = self.linger.load(Ordering::SeqCst);
            if left == 0 {
                return out;
            }
            self.linger.store(left.saturating_sub(frames), Ordering::SeqCst);
        }
        if let Ok(mut render) = self.render.lock() {
            render(&mut out);
        }
        out
    }

    pub fn running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

impl Sink for NullSink {
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn start(&self) -> Result<()> {
        self.linger.store(0, Ordering::SeqCst);
        self.running.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn stop(&self) -> Result<()> {
        if self.running.swap(false, Ordering::SeqCst) {
            self.linger.store(linger_frames(self.sample_rate), Ordering::SeqCst);
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn a_stopped_null_sink_gives_silence_and_does_not_pull() {
        let sink = NullSink::new(44_100, Box::new(|out| out.fill(0.5)));
        assert_eq!(sink.pull(4), vec![0.0; 8]);
        sink.start().expect("start");
        assert_eq!(sink.pull(4), vec![0.5; 8]);
    }
}
