//! Audio callback deadline usage. The callback writes atomics; readers never
//! take a lock on the audio thread or allocate in it.
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

#[derive(Debug, Default, Clone, Copy)]
pub struct AudioHealth {
    /// Smoothed fraction of the buffer deadline consumed, clamped to 0..=1.
    pub load: f32,
    /// Callbacks whose processing exceeded the duration of their audio.
    pub xruns: u64,
}

#[derive(Default)]
pub(crate) struct HealthMeter {
    load: AtomicU32,
    xruns: AtomicU64,
}

impl HealthMeter {
    /// One writer: the device callback. Elapsed covers rendering and conversion.
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    pub fn record(&self, elapsed: Duration, frames: usize, rate: u32) {
        if frames == 0 || rate == 0 { return; }
        let block = frames as f64 / f64::from(rate);
        let instant = elapsed.as_secs_f64() / block;
        let previous = f32::from_bits(self.load.load(Ordering::Relaxed));
        let load = (f64::from(previous) + 0.2 * (instant - f64::from(previous))).clamp(0.0, 1.0);
        self.load.store((load as f32).to_bits(), Ordering::Relaxed);
        if instant > 1.0 { self.xruns.fetch_add(1, Ordering::Relaxed); }
    }

    pub fn snapshot(&self, running: bool) -> AudioHealth {
        AudioHealth {
            load: if running { f32::from_bits(self.load.load(Ordering::Relaxed)) } else { 0.0 },
            xruns: self.xruns.load(Ordering::Relaxed),
        }
    }
}

/// What the device callback has handed the device since it was last read.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(crate) struct SignalReport {
    /// Callbacks that ran.
    pub callbacks: u64,
    /// Largest absolute sample rendered, before conversion to the device format.
    pub peak: f32,
}

impl SignalReport {
    /// Whether anything but silence was rendered. A stream that ran with this
    /// false was fed silence by the app; one that ran with it true and was
    /// still not heard was lost after the callback, in the device or the OS.
    pub fn audible(&self) -> bool { self.peak > 0.0 }
}

/// Counts callbacks and remembers the loudest sample rendered, so a report of
/// "the stream runs but nothing is heard" can say which side the silence is on.
/// Written by the device callback; read and reset from the device thread.
#[derive(Default)]
pub(crate) struct SignalProbe {
    callbacks: AtomicU64,
    peak: AtomicU32,
}

impl SignalProbe {
    /// One writer: the device callback.
    pub fn record(&self, samples: &[f32]) {
        let loudest = peak_of(samples);
        self.callbacks.fetch_add(1, Ordering::Relaxed);
        if loudest > f32::from_bits(self.peak.load(Ordering::Relaxed)) {
            self.peak.store(loudest.to_bits(), Ordering::Relaxed);
        }
    }

    /// Reads and resets.
    pub fn take(&self) -> SignalReport {
        SignalReport {
            callbacks: self.callbacks.swap(0, Ordering::Relaxed),
            peak: f32::from_bits(self.peak.swap(0, Ordering::Relaxed)),
        }
    }
}

/// The largest absolute sample; NaN counts as silence rather than poisoning the peak.
pub(crate) fn peak_of(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0_f32, |peak, sample| {
        let magnitude = sample.abs();
        if magnitude > peak { magnitude } else { peak }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::float_cmp, reason = "peaks are copied samples, not computed")]
    fn the_probe_tells_silence_from_signal_and_resets_when_read() {
        let probe = SignalProbe::default();
        probe.record(&[0.0; 8]);
        probe.record(&[0.0; 8]);
        let silent = probe.take();
        assert_eq!(silent.callbacks, 2);
        assert!(!silent.audible());

        probe.record(&[0.0, -0.5, 0.25, f32::NAN]);
        probe.record(&[0.1, 0.0]);
        let heard = probe.take();
        assert_eq!(heard.callbacks, 2);
        assert_eq!(heard.peak, 0.5);
        assert!(heard.audible());
        assert_eq!(probe.take(), SignalReport::default());
    }

    #[test]
    #[allow(clippy::float_cmp, reason = "load is clamped to exactly 1.0 and reset to exactly 0.0")]
    fn measures_the_deadline_smooths_and_counts_only_overruns() {
        let meter = HealthMeter::default();
        meter.record(Duration::from_millis(5), 480, 48_000);
        assert!((meter.snapshot(true).load - 0.1).abs() < 0.0001);
        meter.record(Duration::from_millis(20), 480, 48_000);
        assert!((meter.snapshot(true).load - 0.48).abs() < 0.0001);
        assert_eq!(meter.snapshot(true).xruns, 1);
        meter.record(Duration::from_secs(1), 480, 48_000);
        assert_eq!(meter.snapshot(true).load, 1.0);
        assert_eq!(meter.snapshot(false).load, 0.0);
        assert_eq!(meter.snapshot(false).xruns, 2);
        meter.record(Duration::from_secs(1), 0, 0);
        assert_eq!(meter.snapshot(false).xruns, 2);
    }
}
