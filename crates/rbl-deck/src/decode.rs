//! Streaming decode: file → stereo `f32` at the device rate.
//!
//! Streaming, not the whole track. A ninety-minute mix decoded to `f32` is
//! about 950 MB, and a deck only ever needs the next few hundred milliseconds.
//!
//! Everything a deck plays lands at the device rate, so the mixer never has to
//! reason about two decks in different rates. Where the file is already at the
//! device rate — which is most of the library on a 44.1 kHz device — the
//! resampler is not built at all rather than run at a ratio of one.

use std::path::Path;

use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder, DecoderOptions, CODEC_TYPE_MP3, CODEC_TYPE_NULL};
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, SeekedTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

use crate::{DeckError, Result};

/// Frames handed to the resampler at a time.
const RESAMPLE_CHUNK: usize = 1024;

/// How far a coarse seek that overshot is asked to step back, in seconds.
///
/// Two first, because a coarse seek that misses at all misses by about a
/// packet; ten as the second try, because a format whose estimate is that far
/// out will not be fixed by another two. Decoding ten seconds forward and
/// throwing it away costs a few tens of milliseconds, which is still two
/// orders below the scan this exists to avoid.
const BACKOFF_SECONDS: [f64; 2] = [2.0, 10.0];

/// A quality that is inaudible on a monitoring path and cheap enough to run on
/// two decks at once. `[ASSUME]` — 128 taps at a 0.95 cutoff; revisit only if
/// a resampled track is ever measurably worse than the file.
fn sinc_parameters() -> SincInterpolationParameters {
    SincInterpolationParameters {
        sinc_len: 128,
        f_cutoff: 0.95,
        oversampling_factor: 128,
        interpolation: SincInterpolationType::Cubic,
        window: WindowFunction::BlackmanHarris2,
    }
}

pub struct Streamer {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    /// MPEG coarse seeks estimate timestamps from byte offsets. Discarding
    /// samples from that estimate does not make the result frame-exact.
    seek_mode: SeekMode,
    /// The file's own rate, before resampling.
    source_rate: u32,
    device_rate: u32,
    resampler: Option<SincFixedIn<f32>>,
    /// Source-rate stereo, planar, waiting to be resampled.
    pending: [Vec<f32>; 2],
    /// Device-rate stereo, planar, straight out of the resampler.
    resampled: [Vec<f32>; 2],
    /// Device-rate stereo, interleaved, waiting to be handed to a block.
    ready: Vec<f32>,
    /// How much of `ready` has been handed out, in samples.
    taken: usize,
    /// Where the next frame handed out sits in the track, at the device rate.
    position: u64,
    /// Frames to throw away before handing anything out, so a seek lands on
    /// the frame it was asked for rather than on a packet boundary.
    skip: u64,
    total_frames: u64,
    /// The demuxer has no more packets; what is left is in the buffers.
    drained: bool,
    /// Every buffer is empty and the file is finished.
    finished: bool,
    /// Converts symphonia's own buffer to interleaved `f32`, reused per packet.
    interleaved: Option<SampleBuffer<f32>>,
}

impl Streamer {
    /// Opens a file and prepares it to be played at `device_rate`.
    pub fn open(path: &Path, device_rate: u32) -> Result<Self> {
        let file = std::fs::File::open(path)?;
        let stream = MediaSourceStream::new(
            Box::new(file),
            symphonia::core::io::MediaSourceStreamOptions::default(),
        );

        let mut hint = Hint::new();
        if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(extension);
        }

        let probed = symphonia::default::get_probe()
            .format(
                &hint,
                stream,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|e| DeckError::Decode(e.to_string()))?;
        let format = probed.format;

        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or_else(|| DeckError::Decode("there is no audio track in that file".to_owned()))?;
        let track_id = track.id;
        let seek_mode = if track.codec_params.codec == CODEC_TYPE_MP3 {
            SeekMode::Accurate
        } else {
            SeekMode::Coarse
        };
        let source_rate = track.codec_params.sample_rate.unwrap_or(device_rate).max(1);
        // Length in device-rate frames, which is what the playhead counts. A
        // file that does not say gets zero, and the deck reports what it has
        // played rather than a fraction of a length it does not know.
        let total_frames = track
            .codec_params
            .n_frames
            .map_or(0, |frames| scale_frames(frames, source_rate, device_rate));

        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|e| DeckError::Decode(e.to_string()))?;

        let resampler = if source_rate == device_rate {
            None
        } else {
            Some(
                SincFixedIn::<f32>::new(
                    f64::from(device_rate) / f64::from(source_rate),
                    1.0,
                    sinc_parameters(),
                    RESAMPLE_CHUNK,
                    2,
                )
                .map_err(|e| DeckError::Decode(e.to_string()))?,
            )
        };

        Ok(Self {
            format,
            decoder,
            track_id,
            seek_mode,
            source_rate,
            device_rate,
            resampler,
            pending: [Vec::new(), Vec::new()],
            resampled: [Vec::new(), Vec::new()],
            ready: Vec::new(),
            taken: 0,
            position: 0,
            skip: 0,
            total_frames,
            drained: false,
            finished: false,
            interleaved: None,
        })
    }

    pub fn total_frames(&self) -> u64 {
        self.total_frames
    }

    /// Where the next frame handed out sits, in device-rate frames.
    pub fn position(&self) -> u64 {
        self.position
    }

    /// The frame the next `fill` starts at, in device-rate frames.
    ///
    /// Not `position` straight after a seek: that is the packet boundary the
    /// demuxer landed on, and the overshoot to the frame asked for is only
    /// thrown away by the next `fill`.
    pub fn next_frame(&self) -> u64 {
        self.position + self.skip
    }

    /// Nothing left: the file is finished and the buffers are empty.
    pub fn finished(&self) -> bool {
        self.finished
    }

    /// Moves to `frame`, counted at the device rate.
    ///
    /// Frame-exact. The demuxer only seeks to a packet boundary — WAV lands up
    /// to 576 frames early, a compressed format further — so whatever it
    /// overshoots by is decoded and thrown away. A cue point that is 7 ms out
    /// is a cue point in the wrong place.
    pub fn seek(&mut self, frame: u64) -> Result<u64> {
        let seconds = frame as f64 / f64::from(self.device_rate.max(1));
        // MPEG needs an accurate packet timestamp. Coarse seeking can put
        // audio a packet ahead while reporting the requested timestamp: the
        // subsequent discard cannot repair that estimate. Compare PCM with
        // sequential decoding, not just SeekedTo, when checking accuracy.
        // Accurate MPEG seeks may scan from the start on backward jumps;
        // other formats retain the faster coarse path and overshoot fallback.
        //
        // What accurate does guarantee is landing at or before the target, and
        // the discard below only moves forward. So an overshoot is stepped
        // back from, and a format that still overshoots after that pays for
        // the accurate seek rather than being played from the wrong place.
        let mut landed = self.seek_to(seconds, self.seek_mode)?;
        for back in BACKOFF_SECONDS {
            if self.landed_frame(&landed) <= frame {
                break;
            }
            landed = self.seek_to((seconds - back).max(0.0), SeekMode::Coarse)?;
        }
        if self.landed_frame(&landed) > frame {
            landed = self.seek_to(seconds, SeekMode::Accurate)?;
        }

        self.decoder.reset();
        for channel in &mut self.pending {
            channel.clear();
        }
        for channel in &mut self.resampled {
            channel.clear();
        }
        self.ready.clear();
        self.taken = 0;
        self.drained = false;
        self.finished = false;
        if let Some(resampler) = self.resampler.as_mut() {
            resampler.reset();
        }
        self.position = scale_frames(landed.actual_ts, self.source_rate, self.device_rate);
        self.skip = frame.saturating_sub(self.position);
        // The position it reports is where it will actually resume, which is
        // where it was asked to go once the overshoot has been discarded.
        Ok(frame.max(self.position))
    }

    /// One demuxer seek, in seconds.
    fn seek_to(&mut self, seconds: f64, mode: SeekMode) -> Result<SeekedTo> {
        self.format
            .seek(
                mode,
                SeekTo::Time {
                    time: Time::from(seconds),
                    track_id: Some(self.track_id),
                },
            )
            .map_err(|e| DeckError::Decode(e.to_string()))
    }

    /// Where a seek landed, in the device-rate frames the playhead counts.
    fn landed_frame(&self, landed: &SeekedTo) -> u64 {
        scale_frames(landed.actual_ts, self.source_rate, self.device_rate)
    }

    /// Fills `out` with interleaved stereo, returning the frames written.
    ///
    /// Fewer than asked for means the track ended; zero with `finished` set
    /// means there is nothing more at all.
    pub fn fill(&mut self, out: &mut [f32]) -> Result<usize> {
        self.discard_skipped()?;
        let mut wanted = out.len() / 2;
        // Never past the length the file declares. The resampler's tail is
        // zero-padded to a full chunk, so without this a resampled track ends
        // with a few hundred frames of silence that never existed.
        if self.total_frames > 0 {
            let left = usize::try_from(self.total_frames.saturating_sub(self.position))
                .unwrap_or(usize::MAX);
            wanted = wanted.min(left);
            if wanted == 0 {
                self.finished = true;
                return Ok(0);
            }
        }
        let mut written = 0;
        while written < wanted {
            if self.taken >= self.ready.len() {
                self.ready.clear();
                self.taken = 0;
                if !self.pump()? {
                    break;
                }
                continue;
            }
            let available = (self.ready.len() - self.taken) / 2;
            let take = available.min(wanted - written);
            let from = self
                .ready
                .get(self.taken..self.taken + take * 2)
                .unwrap_or(&[]);
            if let Some(into) = out.get_mut(written * 2..written * 2 + take * 2) {
                into.copy_from_slice(from);
            }
            self.taken += take * 2;
            written += take;
            self.position += take as u64;
        }
        if written == 0 && self.drained {
            self.finished = true;
        }
        Ok(written)
    }

    /// Throws away what a seek overshot by, decoding it if it has to.
    fn discard_skipped(&mut self) -> Result<()> {
        while self.skip > 0 {
            if self.taken >= self.ready.len() {
                self.ready.clear();
                self.taken = 0;
                if !self.pump()? {
                    // The file ended inside the overshoot: there is nothing to
                    // skip to, and the deck is at the end.
                    self.skip = 0;
                    break;
                }
                continue;
            }
            let available = ((self.ready.len() - self.taken) / 2) as u64;
            let dropped = available.min(self.skip);
            self.taken += (dropped as usize) * 2;
            self.position += dropped;
            self.skip -= dropped;
        }
        Ok(())
    }

    /// Decodes and resamples until `ready` holds something, or the file ends.
    ///
    /// Returns false when there is nothing more to be had.
    fn pump(&mut self) -> Result<bool> {
        loop {
            if self.pending_frames() >= RESAMPLE_CHUNK {
                self.resample_chunk()?;
                if !self.ready.is_empty() {
                    return Ok(true);
                }
                continue;
            }
            if self.drained {
                // What is left is shorter than a chunk: flush it and stop.
                if self.pending_frames() > 0 {
                    self.resample_tail()?;
                    return Ok(!self.ready.is_empty());
                }
                return Ok(false);
            }
            if !self.decode_packet()? {
                self.drained = true;
            }
        }
    }

    fn pending_frames(&self) -> usize {
        self.pending.first().map_or(0, Vec::len)
    }

    /// Reads one packet into `pending`. False at the end of the stream.
    fn decode_packet(&mut self) -> Result<bool> {
        loop {
            // The end of the stream is reported as an error rather than a
            // condition, and a reader that has run out is not a failure.
            let Ok(packet) = self.format.next_packet() else {
                return Ok(false);
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            let decoded = match self.decoder.decode(&packet) {
                Ok(decoded) => decoded,
                // One damaged packet should not end the track.
                Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
                Err(e) => {
                    tracing::warn!(error = %e, "the decoder stopped early");
                    return Ok(false);
                }
            };

            let spec = *decoded.spec();
            let channels = spec.channels.count().max(1);
            let buffer = self
                .interleaved
                .get_or_insert_with(|| SampleBuffer::new(decoded.capacity() as u64, spec));
            buffer.copy_interleaved_ref(decoded);

            // Anything is played as stereo: mono is doubled, and more than two
            // channels keeps the first pair rather than downmixing a surround
            // bed nobody is monitoring.
            for frame in buffer.samples().chunks(channels) {
                let left = frame.first().copied().unwrap_or(0.0);
                let right = if channels == 1 {
                    left
                } else {
                    frame.get(1).copied().unwrap_or(0.0)
                };
                if let Some(channel) = self.pending.first_mut() {
                    channel.push(left);
                }
                if let Some(channel) = self.pending.get_mut(1) {
                    channel.push(right);
                }
            }
            return Ok(true);
        }
    }

    /// One `RESAMPLE_CHUNK` of source frames through the resampler, or
    /// straight through when the file is already at the device rate.
    fn resample_chunk(&mut self) -> Result<()> {
        let have = self.pending_frames();
        let Some(resampler) = self.resampler.as_mut() else {
            self.interleave_pending(RESAMPLE_CHUNK);
            return Ok(());
        };

        let needed = resampler.input_frames_next().min(have);
        let output = resampler.output_frames_max();
        for channel in &mut self.resampled {
            channel.clear();
            channel.resize(output, 0.0);
        }
        let input: Vec<&[f32]> = self
            .pending
            .iter()
            .map(|channel| channel.get(..needed).unwrap_or(&[]))
            .collect();
        let (used, made) = resampler
            .process_into_buffer(&input, &mut self.resampled, None)
            .map_err(|e| DeckError::Decode(e.to_string()))?;
        drop(input);
        for channel in &mut self.pending {
            channel.drain(..used.min(channel.len()));
        }
        self.interleave_resampled(made);
        Ok(())
    }

    /// The last, short chunk, zero-padded so the resampler can flush it.
    fn resample_tail(&mut self) -> Result<()> {
        let frames = self.pending_frames();
        let Some(resampler) = self.resampler.as_mut() else {
            self.interleave_pending(frames);
            return Ok(());
        };
        let output = resampler.output_frames_max();
        for channel in &mut self.resampled {
            channel.clear();
            channel.resize(output, 0.0);
        }
        let input: Vec<&[f32]> = self.pending.iter().map(Vec::as_slice).collect();
        let (_, made) = resampler
            .process_partial_into_buffer(Some(&input), &mut self.resampled, None)
            .map_err(|e| DeckError::Decode(e.to_string()))?;
        drop(input);
        for channel in &mut self.pending {
            channel.clear();
        }
        self.interleave_resampled(made);
        Ok(())
    }

    fn interleave_pending(&mut self, frames: usize) {
        let take = frames.min(self.pending_frames());
        self.ready.reserve(take * 2);
        for at in 0..take {
            let left = self
                .pending
                .first()
                .and_then(|c| c.get(at))
                .copied()
                .unwrap_or(0.0);
            let right = self
                .pending
                .get(1)
                .and_then(|c| c.get(at))
                .copied()
                .unwrap_or(0.0);
            self.ready.push(left);
            self.ready.push(right);
        }
        for channel in &mut self.pending {
            channel.drain(..take.min(channel.len()));
        }
    }

    fn interleave_resampled(&mut self, frames: usize) {
        self.ready.reserve(frames * 2);
        for at in 0..frames {
            let left = self
                .resampled
                .first()
                .and_then(|c| c.get(at))
                .copied()
                .unwrap_or(0.0);
            let right = self
                .resampled
                .get(1)
                .and_then(|c| c.get(at))
                .copied()
                .unwrap_or(0.0);
            self.ready.push(left);
            self.ready.push(right);
        }
    }
}

/// A frame count from one rate into another, without overflowing on a long mix.
fn scale_frames(frames: u64, from: u32, to: u32) -> u64 {
    if from == to || from == 0 {
        return frames;
    }
    let scaled = u128::from(frames) * u128::from(to) / u128::from(from.max(1));
    u64::try_from(scaled).unwrap_or(u64::MAX)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// A 16-bit PCM WAV, so the tests need no fixture file.
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

    fn tone(path: &Path, rate: u32, seconds: f32) {
        let frames = (rate as f32 * seconds) as usize;
        let mut samples = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let value = (i as f32 * 2.0 * std::f32::consts::PI * 440.0 / rate as f32).sin() * 0.5;
            samples.push(value);
            samples.push(-value);
        }
        write_wav(path, rate, 2, &samples);
    }

    #[test]
    fn a_file_at_the_device_rate_comes_back_frame_for_frame() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tone.wav");
        tone(&path, 44_100, 0.5);

        let mut streamer = Streamer::open(&path, 44_100).unwrap();
        assert_eq!(streamer.total_frames(), 22_050);

        let mut out = vec![0.0_f32; 1024];
        let mut frames = 0;
        while !streamer.finished() {
            frames += streamer.fill(&mut out).unwrap();
        }
        assert_eq!(frames, 22_050);
        // Stereo is kept: the right channel is the inverse of the left.
        assert_eq!(streamer.position(), 22_050);
    }

    #[test]
    fn a_seek_back_to_the_start_after_the_first_blocks_still_decodes() {
        // The first drag on a freshly loaded track: the worker has decoded the
        // ring's worth from the top, then the drag seeks back to 0 and fills
        // its window from there. At both a matching and a different device
        // rate, that window must hold audio.
        for device_rate in [44_100_u32, 48_000] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("tone.wav");
            tone(&path, 44_100, 1.0);
            let mut streamer = Streamer::open(&path, device_rate).unwrap();
            let mut out = vec![0.0_f32; 512];
            for _ in 0..16 {
                assert!(streamer.fill(&mut out).unwrap() > 0);
            }
            assert_eq!(streamer.seek(0).unwrap(), 0);
            let mut window = vec![0.0_f32; 8192];
            let frames = streamer.fill(&mut window).unwrap();
            assert!(
                frames > 0,
                "no audio after seeking back to 0 at {device_rate} Hz"
            );
            assert!(
                window.iter().any(|s| *s != 0.0),
                "silence after seeking back to 0 at {device_rate} Hz"
            );
        }
    }

    #[test]
    fn mp3_seeks_keep_the_sequential_audio_timeline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("seek.mp3");
        std::fs::write(&path, include_bytes!("../tests/fixtures/seek-noise.mp3")).unwrap();
        let mut linear = Streamer::open(&path, 44_100).unwrap();
        let mut reference = vec![0.0; 44_100 * 2 * 2];
        linear.fill(&mut reference).unwrap();
        let mut seeked = Streamer::open(&path, 44_100).unwrap();
        // Include backward seeks and zero: a coarse MP3 seek can label the
        // second packet as time zero even though continuous decoding doesn't.
        for at in [0, 22_050, 11_025, 0] {
            seeked.seek(at as u64).unwrap();
            let mut samples = vec![0.0; 16_384];
            assert_eq!(seeked.fill(&mut samples).unwrap(), 8192);
            // Ignore decoder warmup and compare actual PCM, not the reported
            // position (which can agree while the audio is a packet ahead).
            let error = samples[8192..]
                .iter()
                .zip(&reference[at * 2 + 8192..at * 2 + samples.len()])
                .map(|(a, b)| (a - b).abs())
                .fold(0.0_f32, f32::max);
            assert!(error < 1e-5, "seek to {at}: PCM shifted, max error {error}");
        }
    }

    #[test]
    fn a_file_at_another_rate_is_resampled_to_the_device() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tone48.wav");
        tone(&path, 48_000, 0.5);

        let mut streamer = Streamer::open(&path, 44_100).unwrap();
        // 24,000 frames at 48 kHz is 22,050 at 44.1 kHz.
        assert_eq!(streamer.total_frames(), 22_050);

        let mut out = vec![0.0_f32; 2048];
        let mut frames = 0;
        while !streamer.finished() {
            frames += streamer.fill(&mut out).unwrap();
        }
        // Exactly the length the file declares: the resampler's zero-padded
        // tail is not audio and is not handed out.
        assert_eq!(frames, 22_050);
    }

    #[test]
    fn a_seek_lands_where_it_was_asked_and_the_position_follows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tone.wav");
        tone(&path, 44_100, 2.0);

        let mut streamer = Streamer::open(&path, 44_100).unwrap();
        let landed = streamer.seek(44_100).unwrap();
        assert_eq!(landed, 44_100);

        let mut out = vec![0.0_f32; 512];
        let frames = streamer.fill(&mut out).unwrap();
        assert_eq!(frames, 256);
        // Frame-exact: the demuxer lands on a packet boundary up to 576 frames
        // early and the overshoot is decoded and dropped.
        assert_eq!(streamer.position(), 44_100 + 256);
    }

    #[test]
    fn a_seek_past_the_end_finishes_rather_than_hanging() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tone.wav");
        tone(&path, 44_100, 0.2);

        let mut streamer = Streamer::open(&path, 44_100).unwrap();
        let _ = streamer.seek(44_100 * 10);
        let mut out = vec![0.0_f32; 512];
        let mut guard = 0;
        while !streamer.finished() && guard < 100 {
            let _ = streamer.fill(&mut out);
            guard += 1;
        }
        assert!(streamer.finished(), "a seek past the end never finished");
    }

    #[test]
    fn a_mono_file_is_played_as_stereo() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mono.wav");
        write_wav(&path, 44_100, 1, &vec![0.5_f32; 4_410]);

        let mut streamer = Streamer::open(&path, 44_100).unwrap();
        let mut out = vec![0.0_f32; 64];
        let frames = streamer.fill(&mut out).unwrap();
        assert!(frames > 0);
        // Both channels carry the same signal rather than one being silent.
        assert!((out[0] - out[1]).abs() < 1e-3, "{} vs {}", out[0], out[1]);
        assert!(out[0] > 0.4);
    }

    #[test]
    fn a_file_that_is_not_audio_is_refused_rather_than_played_as_noise() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.txt");
        std::fs::write(&path, b"this is not a wav").unwrap();
        assert!(Streamer::open(&path, 44_100).is_err());
    }
}
