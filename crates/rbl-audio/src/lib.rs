//! Decoding for analysis.
//!
//! Analysis wants one mono signal at a known rate, not the file's own layout.
//! Decoding is the expensive step, so it happens once and every later stage
//! reads the same buffer.

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "sample counts and durations convert between usize and f64 throughout decoding"
)]

pub mod compatibility;

use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("unsupported or unreadable audio: {0}")]
    Unsupported(String),
    #[error("no audio track in the file")]
    NoTrack,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, AudioError>;

/// Mono audio at a known sample rate.
#[derive(Debug, Clone)]
pub struct Audio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    /// Channels in the source, before the downmix.
    pub source_channels: u16,
}

impl Audio {
    pub fn duration_secs(&self) -> f64 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.samples.len() as f64 / f64::from(self.sample_rate)
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

/// Decodes a file to mono `f32`.
///
/// `max_secs` bounds the work: tempo and key are stable well before a whole
/// long mix is decoded, and an unbounded decode is how a 2-hour file becomes a
/// memory problem. A caller that needs the rest of the file as well, without
/// holding it, reads it from a [`MonoStream`] instead.
pub fn decode_mono(path: &Path, max_secs: Option<f64>) -> Result<Audio> {
    let mut stream = MonoStream::open(path)?;
    let cap = max_secs.map(|s| (s * f64::from(stream.sample_rate())) as usize);
    let mut samples: Vec<f32> = Vec::with_capacity(cap.unwrap_or(0).min(1 << 24));
    while let Some(chunk) = stream.next_chunk() {
        samples.extend_from_slice(chunk);
        if let Some(cap) = cap {
            if samples.len() >= cap {
                samples.truncate(cap);
                break;
            }
        }
    }

    if samples.is_empty() {
        return Err(AudioError::Unsupported("decoded no samples".into()));
    }

    Ok(Audio { samples, sample_rate: stream.sample_rate(), source_channels: stream.source_channels() })
}

/// A file decoded to mono `f32` one packet at a time.
///
/// What [`decode_mono`] collects into one buffer, for a caller that wants
/// to keep only part of it: the analysis keeps the first half hour for the
/// tempo and key and only folds the rest into the waveform, so a two-hour
/// mix is drawn to its end without its samples all being held at once.
pub struct MonoStream {
    format: Box<dyn symphonia::core::formats::FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    sample_rate: u32,
    source_channels: u16,
    buffer: Option<SampleBuffer<f32>>,
    mono: Vec<f32>,
    /// What is left of the file's lead on rekordbox's timeline: frames of
    /// silence still to hand out when positive, decoded frames still to drop
    /// when negative. See [`rekordbox_lead`].
    lead: i64,
}

/// Source-rate frames rekordbox's timeline for a file runs ahead of what
/// symphonia decodes from it, zero for anything but an MP3 that rekordbox
/// maps differently. Every time analysis writes (beats, cues, waveform
/// columns) and every time read back for audio is on rekordbox's timeline,
/// so decoding for either starts this much later. See [`rbl_core::mpeg`].
fn rekordbox_lead(path: &Path, codec: symphonia::core::codecs::CodecType) -> i64 {
    if codec != symphonia::core::codecs::CODEC_TYPE_MP3 {
        return 0;
    }
    std::fs::File::open(path).map_or(0, |mut file| rbl_core::mpeg::rekordbox_lead_frames(&mut file))
}

impl MonoStream {
    /// Opens a file and its first audio track.
    pub fn open(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path)?;
        let stream = MediaSourceStream::new(Box::new(file), symphonia::core::io::MediaSourceStreamOptions::default());

        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        let probed = symphonia::default::get_probe()
            .format(&hint, stream, &FormatOptions::default(), &MetadataOptions::default())
            .map_err(|e| AudioError::Unsupported(e.to_string()))?;
        let format = probed.format;

        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
            .ok_or(AudioError::NoTrack)?;
        let track_id = track.id;
        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|e| AudioError::Unsupported(e.to_string()))?;
        let sample_rate = track.codec_params.sample_rate.unwrap_or(44_100);
        let source_channels = track
            .codec_params
            .channels
            .map_or(2, |c| u16::try_from(c.count()).unwrap_or(2));
        let lead = rekordbox_lead(path, track.codec_params.codec);
        Ok(Self { format, decoder, track_id, sample_rate, source_channels, buffer: None, mono: Vec::new(), lead })
    }

    /// The rate of the samples handed out so far, or the one the container
    /// declares before the first packet.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Channels in the source, before the downmix.
    pub fn source_channels(&self) -> u16 {
        self.source_channels
    }

    /// The next packet's samples, downmixed; `None` at the end of the file.
    ///
    /// `next_packet` reports the end of the stream as an error, so any error
    /// there ends it. A damaged packet mid-file is skipped rather than
    /// discarding what came before it.
    pub fn next_chunk(&mut self) -> Option<&[f32]> {
        if self.lead > 0 {
            self.mono.clear();
            self.mono.resize(usize::try_from(self.lead).unwrap_or(0), 0.0);
            self.lead = 0;
            return Some(&self.mono);
        }
        loop {
            let packet = self.format.next_packet().ok()?;
            if packet.track_id() != self.track_id {
                continue;
            }
            let frames = match self.decoder.decode(&packet) {
                Ok(d) => d,
                // A damaged packet lasts as long as it would have, in silence,
                // as it does in rekordbox: dropping it would move everything
                // after it earlier.
                Err(symphonia::core::errors::Error::DecodeError(_)) => {
                    self.mono.clear();
                    self.mono.resize(usize::try_from(packet.dur).unwrap_or(0), 0.0);
                    if self.drop_lead() {
                        return Some(&self.mono);
                    }
                    continue;
                }
                Err(_) => return None,
            };

            let spec = *frames.spec();
            self.sample_rate = spec.rate;
            self.source_channels = u16::try_from(spec.channels.count()).unwrap_or(2);

            let interleaved =
                self.buffer.get_or_insert_with(|| SampleBuffer::new(frames.capacity() as u64, spec));
            interleaved.copy_interleaved_ref(frames);

            let channels = spec.channels.count().max(1);
            self.mono.clear();
            self.mono.extend(interleaved.samples().chunks(channels).map(|frame| frame.iter().sum::<f32>() / channels as f32));
            if self.drop_lead() {
                return Some(&self.mono);
            }
        }
    }

    /// Drops what a negative lead still owes from the front of `mono`, and
    /// says whether anything is left to hand out.
    fn drop_lead(&mut self) -> bool {
        if self.lead < 0 {
            let owed = usize::try_from(self.lead.unsigned_abs()).unwrap_or(usize::MAX);
            let dropped = owed.min(self.mono.len());
            self.mono.drain(..dropped);
            self.lead += dropped as i64;
        }
        !self.mono.is_empty()
    }
}

/// Writes the stretch of a file between `from_secs` and `to_secs` as a
/// 16-bit PCM WAV at the file's own rate and channel count: rekordbox's
/// Export Loop As WAV. Returns the frames written. Nothing is written when
/// the range holds no audio.
pub fn write_range_wav(source: &Path, from_secs: f64, to_secs: f64, out: &Path) -> Result<u64> {
    if to_secs <= from_secs || from_secs < 0.0 || !to_secs.is_finite() {
        return Err(AudioError::Unsupported("the range is empty".into()));
    }
    let file = std::fs::File::open(source)?;
    let stream = MediaSourceStream::new(Box::new(file), symphonia::core::io::MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    if let Some(ext) = source.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, stream, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| AudioError::Unsupported(e.to_string()))?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .ok_or(AudioError::NoTrack)?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| AudioError::Unsupported(e.to_string()))?;

    let mut sample_rate = track.codec_params.sample_rate.unwrap_or(44_100);
    let mut channels = track.codec_params.channels.map_or(2, |c| c.count().max(1));
    // Interleaved frames of the range, as `i16`.
    let mut pcm: Vec<i16> = Vec::new();
    let mut buffer: Option<SampleBuffer<f32>> = None;
    // The range is on rekordbox's timeline, where the first decoded frame is
    // at the file's lead; before it is silence. See [`rekordbox_lead`].
    let lead = rekordbox_lead(source, track.codec_params.codec);
    let mut frame_at = lead;
    let mut first = (from_secs * f64::from(sample_rate)) as u64;
    let mut last = (to_secs * f64::from(sample_rate)) as u64;
    while let Ok(packet) = format.next_packet() {
        if packet.track_id() != track_id {
            continue;
        }
        if frame_at > first as i64 && pcm.is_empty() {
            // The range opens inside a positive lead: its silence first.
            let quiet = (frame_at.min(last as i64) - first as i64) as usize;
            pcm.resize(quiet * channels, 0);
        }
        let silent: Vec<f32>;
        let samples = match decoder.decode(&packet) {
            Ok(frames) => {
                let spec = *frames.spec();
                if spec.rate != sample_rate {
                    sample_rate = spec.rate;
                    first = (from_secs * f64::from(sample_rate)) as u64;
                    last = (to_secs * f64::from(sample_rate)) as u64;
                }
                channels = spec.channels.count().max(1);
                let interleaved = buffer.get_or_insert_with(|| SampleBuffer::new(frames.capacity() as u64, spec));
                interleaved.copy_interleaved_ref(frames);
                interleaved.samples()
            }
            // A damaged packet keeps its place, in silence, as in rekordbox.
            Err(symphonia::core::errors::Error::DecodeError(_)) => {
                silent = vec![0.0; usize::try_from(packet.dur).unwrap_or(0) * channels];
                &silent
            }
            Err(_) => break,
        };
        let count = (samples.len() / channels) as i64;
        let packet_start = frame_at;
        frame_at += count;
        if frame_at <= first as i64 {
            continue;
        }
        let take_from = (first as i64 - packet_start).max(0) as usize;
        let take_to = ((last as i64 - packet_start).max(0) as usize).min(count as usize);
        for frame in samples.chunks(channels).take(take_to).skip(take_from) {
            for &sample in frame {
                pcm.push((sample.clamp(-1.0, 1.0) * 32767.0) as i16);
            }
        }
        if frame_at >= last as i64 {
            break;
        }
    }
    if pcm.is_empty() {
        return Err(AudioError::Unsupported("no audio in the range".into()));
    }
    let channels_u16 = u16::try_from(channels).unwrap_or(2);
    let block_align = channels_u16 * 2;
    let data_len = u32::try_from(pcm.len() * 2).unwrap_or(u32::MAX);
    let mut bytes = Vec::with_capacity(44 + pcm.len() * 2);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&channels_u16.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * u32::from(block_align)).to_le_bytes());
    bytes.extend_from_slice(&block_align.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for sample in &pcm {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(out, bytes)?;
    Ok((pcm.len() / channels) as u64)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
mod tests {
    use super::*;

    /// Writes a 16-bit PCM WAV, so the test needs no fixture file.
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
        for s in samples {
            out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
        }
        std::fs::write(path, out).unwrap();
    }

    #[test]
    fn a_range_is_written_as_a_wav_of_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("two.wav");
        // Stereo, 1000 Hz, 2 seconds: the left channel a ramp, the right zero.
        let mut samples = Vec::new();
        for i in 0..2000 {
            samples.push(i as f32 / 2000.0);
            samples.push(0.0);
        }
        write_wav(&source, 1000, 2, &samples);
        let out = dir.path().join("loop.wav");
        let frames = write_range_wav(&source, 0.5, 1.0, &out).unwrap();
        assert_eq!(frames, 500);
        let back = decode_mono(&out, None).unwrap();
        assert_eq!((back.sample_rate, back.source_channels, back.samples.len()), (1000, 2, 500));
        // The mono mix of the ramp from 0.5 s: half of 0.25.
        assert!((back.samples[0] - 0.125).abs() < 0.01, "{}", back.samples[0]);
        assert!(write_range_wav(&source, 1.0, 0.5, &out).is_err());
    }

    #[test]
    fn decodes_a_mono_wav() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tone.wav");
        let samples: Vec<f32> = (0..44_100)
            .map(|i| (i as f32 * 2.0 * std::f32::consts::PI * 440.0 / 44_100.0).sin() * 0.5)
            .collect();
        write_wav(&path, 44_100, 1, &samples);

        let audio = decode_mono(&path, None).unwrap();
        assert_eq!(audio.sample_rate, 44_100);
        assert_eq!(audio.source_channels, 1);
        assert!((audio.duration_secs() - 1.0).abs() < 0.01);
        // The tone should survive round-tripping through 16-bit.
        let peak = audio.samples.iter().fold(0.0_f32, |a, s| a.max(s.abs()));
        assert!((peak - 0.5).abs() < 0.01, "peak was {peak}");
    }

    #[test]
    fn downmixes_stereo_to_mono() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stereo.wav");
        // Left +0.5, right -0.5 must cancel to silence.
        let mut samples = Vec::new();
        for _ in 0..1000 {
            samples.push(0.5);
            samples.push(-0.5);
        }
        write_wav(&path, 44_100, 2, &samples);

        let audio = decode_mono(&path, None).unwrap();
        assert_eq!(audio.source_channels, 2);
        assert_eq!(audio.samples.len(), 1000);
        let peak = audio.samples.iter().fold(0.0_f32, |a, s| a.max(s.abs()));
        assert!(peak < 0.01, "opposite channels should cancel, peak was {peak}");
    }

    #[test]
    fn respects_the_duration_cap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("long.wav");
        let samples: Vec<f32> = (0..44_100 * 10).map(|i| (i as f32 / 1000.0).sin()).collect();
        write_wav(&path, 44_100, 1, &samples);

        let audio = decode_mono(&path, Some(2.0)).unwrap();
        assert!(audio.duration_secs() <= 2.05, "got {}", audio.duration_secs());
        assert!(audio.duration_secs() >= 1.95);
    }

    #[test]
    fn a_missing_file_is_an_error() {
        assert!(decode_mono(Path::new("/definitely/not/here.wav"), None).is_err());
    }

    #[test]
    fn a_non_audio_file_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("not-audio.wav");
        std::fs::write(&path, b"this is not a wav file at all").unwrap();
        assert!(decode_mono(&path, None).is_err());
    }

    /// A LAME MP3 of a second of stereo tone, joint stereo with a LAME Info
    /// frame in the same mode: the kind of file rekordbox's timeline starts a
    /// frame earlier on (#277).
    pub(crate) fn lame_mp3(dir: &Path) -> std::path::PathBuf {
        let wav = dir.join("tone.wav");
        let samples: Vec<f32> = (0..44_100)
            .flat_map(|i| {
                let v = (i as f32 * std::f32::consts::TAU * 440.0 / 44_100.0).sin() * 0.5;
                [v, v * 0.5]
            })
            .collect();
        write_wav(&wav, 44_100, 2, &samples);
        let mp3 = dir.join("tone.mp3");
        crate::compatibility::convert(&wav, &mp3, crate::compatibility::Format::Mp3).unwrap();
        mp3
    }

    /// The file as symphonia decodes it, mono, with nothing moved.
    fn symphonia_mono(path: &Path) -> Vec<f32> {
        let file = std::fs::File::open(path).unwrap();
        let stream = MediaSourceStream::new(Box::new(file), Default::default());
        let mut format = symphonia::default::get_probe()
            .format(&Hint::new(), stream, &FormatOptions::default(), &MetadataOptions::default())
            .unwrap()
            .format;
        let track = format.default_track().unwrap().clone();
        let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default()).unwrap();
        let mut mono = Vec::new();
        while let Ok(packet) = format.next_packet() {
            let Ok(decoded) = decoder.decode(&packet) else { continue };
            let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
            buffer.copy_interleaved_ref(decoded);
            mono.extend(buffer.samples().chunks(2).map(|f| f.iter().sum::<f32>() / 2.0));
        }
        mono
    }

    #[test]
    fn an_mp3_is_decoded_on_rekordbox_s_timeline() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = lame_mp3(dir.path());
        assert_eq!(rbl_core::mpeg::rekordbox_lead_frames(&mut std::fs::File::open(&mp3).unwrap()), 1152);
        let raw = symphonia_mono(&mp3);
        let audio = decode_mono(&mp3, None).unwrap();
        // rekordbox plays the Info frame as a frame of silence; then the same
        // samples, a frame later.
        assert_eq!(audio.samples.len(), raw.len() + 1152);
        assert!(audio.samples[..1152].iter().all(|&s| s == 0.0));
        assert_eq!(&audio.samples[1152..], &raw[..]);
    }

    #[test]
    fn a_damaged_mp3_packet_keeps_its_place_in_silence() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = lame_mp3(dir.path());
        let whole = decode_mono(&mp3, None).unwrap().samples;
        // The fourth frame claims 511 big values in its first granule, more
        // than a granule holds: symphonia refuses to decode it, as it does
        // the broken frames some files open with.
        let mut bytes = std::fs::read(&mp3).unwrap();
        let mut at = bytes.windows(2).position(|w| w == [0xFF, 0xFB]).unwrap();
        for _ in 0..3 {
            at += 1044 + usize::from((bytes[at + 2] >> 1) & 1);
        }
        assert_eq!(&bytes[at..at + 2], &[0xFF, 0xFB]);
        // Side information: main_data_begin (9 bits), private bits (3),
        // scfsi (8), part2_3_length (12), then big_values (9).
        bytes[at + 8] = 0xFF;
        bytes[at + 9] |= 0x80;
        let damaged = dir.path().join("damaged.mp3");
        std::fs::write(&damaged, bytes).unwrap();
        let audio = decode_mono(&damaged, None).unwrap().samples;
        assert_eq!(audio.len(), whole.len(), "a dropped packet moves everything after it");
        // What follows the damage is where it was.
        let tail = whole.len() - 22_050;
        let drift = audio[tail..].iter().zip(&whole[tail..]).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(drift < 1e-6, "{drift}");
    }

    #[test]
    fn a_range_of_an_mp3_is_cut_on_rekordbox_s_timeline() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = lame_mp3(dir.path());
        let raw = symphonia_mono(&mp3);
        let out = dir.path().join("range.wav");
        // From 10 ms, inside the frame rekordbox plays as silence, to 50 ms.
        let frames = write_range_wav(&mp3, 0.01, 0.05, &out).unwrap();
        assert_eq!(frames, 2205 - 441);
        let cut = decode_mono(&out, None).unwrap().samples;
        let quiet = 1152 - 441;
        assert!(cut[..quiet].iter().all(|&s| s == 0.0));
        let drift = cut[quiet..].iter().zip(&raw).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(drift < 1e-3, "{drift}");
    }
}
