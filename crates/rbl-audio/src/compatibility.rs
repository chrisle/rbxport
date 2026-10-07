//! Streaming export conversion. No source file is opened for writing.
use crate::{AudioError, Result};
use rubato::{FftFixedIn, Resampler};
use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{
        DecoderOptions, CODEC_TYPE_FLAC, CODEC_TYPE_MP3, CODEC_TYPE_PCM_F32LE,
        CODEC_TYPE_PCM_S16BE, CODEC_TYPE_PCM_S16LE, CODEC_TYPE_PCM_S24BE, CODEC_TYPE_PCM_S24LE,
        CODEC_TYPE_PCM_S32LE,
    },
    formats::FormatOptions,
    io::{MediaSourceStream, MediaSourceStreamOptions},
    meta::MetadataOptions,
    probe::Hint,
};

pub const RATE: u32 = 44_100;
const CHUNK: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Wav,
    Aiff,
    Mp3,
}
impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Aiff => "aiff",
            Self::Mp3 => "mp3",
        }
    }
    pub fn profile(self) -> &'static str {
        match self {
            Self::Wav => "pcm16-44100-stereo-v1",
            Self::Aiff => "pcm16be-44100-stereo-v1",
            Self::Mp3 => "mp3-320-44100-stereo-v1",
        }
    }
    pub fn bitrate(self) -> u32 {
        match self {
            Self::Wav | Self::Aiff => 1411,
            Self::Mp3 => 320,
        }
    }
}
fn error(e: impl std::fmt::Display) -> AudioError {
    AudioError::Unsupported(e.to_string())
}
fn open(path: &Path) -> Result<Box<dyn symphonia::core::formats::FormatReader>> {
    let stream = MediaSourceStream::new(
        Box::new(File::open(path)?),
        MediaSourceStreamOptions::default(),
    );
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
        hint.with_extension(ext);
    }
    Ok(symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions {
                enable_gapless: true,
                ..Default::default()
            },
            &MetadataOptions::default(),
        )
        .map_err(error)?
        .format)
}

/// Conservative common denominator for USB-capable CDJs: MPEG-1 MP3 or
/// integer PCM WAV/AIFF, mono/stereo at 44.1/48 kHz. Probe the codec, not only
/// its extension (a WAV can contain float PCM or ADPCM).
pub fn needs_conversion(path: &Path) -> Result<bool> {
    let format = open(path)?;
    let track = format.default_track().ok_or(AudioError::NoTrack)?;
    let p = &track.codec_params;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let codec_ok = match ext.as_str() {
        "mp3" => p.codec == CODEC_TYPE_MP3,
        "wav" => p.codec == CODEC_TYPE_PCM_S16LE || p.codec == CODEC_TYPE_PCM_S24LE,
        "aif" | "aiff" => p.codec == CODEC_TYPE_PCM_S16BE || p.codec == CODEC_TYPE_PCM_S24BE,
        _ => false,
    };
    Ok(!(codec_ok
        && matches!(p.sample_rate, Some(44_100 | 48_000))
        && p.channels.is_some_and(|c| matches!(c.count(), 1 | 2))))
}

struct Output {
    file: BufWriter<File>,
    format: Format,
    encoder: Option<mp3lame_encoder::Encoder>,
    bytes: Vec<u8>,
    frames: u64,
}
impl Output {
    fn new(path: &Path, format: Format) -> Result<Self> {
        // create_new also prevents accidentally overwriting a source path.
        let mut file = BufWriter::new(File::options().write(true).create_new(true).open(path)?);
        let encoder = if format == Format::Mp3 {
            use mp3lame_encoder::{Bitrate, Builder, Quality, VbrMode};
            Some(
                Builder::new()
                    .ok_or_else(|| error("cannot initialize MP3 encoder"))?
                    .with_num_channels(2)
                    .map_err(error)?
                    .with_sample_rate(RATE)
                    .map_err(error)?
                    .with_brate(Bitrate::Kbps320)
                    .map_err(error)?
                    .with_vbr_mode(VbrMode::Off)
                    .map_err(error)?
                    .with_quality(Quality::Best)
                    .map_err(error)?
                    .with_to_write_vbr_tag(true)
                    .map_err(error)?
                    .build()
                    .map_err(error)?,
            )
        } else {
            let header = match format {
                Format::Wav => 44,
                Format::Aiff => 54,
                Format::Mp3 => 0,
            };
            file.write_all(&vec![0; header])?;
            None
        };
        Ok(Self {
            file,
            format,
            encoder,
            bytes: Vec::with_capacity(16384),
            frames: 0,
        })
    }
    fn write(&mut self, left: &[f32], right: &[f32]) -> Result<()> {
        self.bytes.clear();
        if let Some(encoder) = &mut self.encoder {
            self.bytes
                .reserve(mp3lame_encoder::max_required_buffer_size(left.len()));
            encoder
                .encode_to_vec(mp3lame_encoder::DualPcm { left, right }, &mut self.bytes)
                .map_err(error)?;
        } else {
            let (overhead, message) = match self.format {
                Format::Wav => (36, "converted WAV exceeds the 4 GB RIFF limit"),
                Format::Aiff => (46, "converted AIFF exceeds the 4 GB FORM limit"),
                Format::Mp3 => (0, "converted PCM exceeds its container limit"),
            };
            if (self.frames + left.len() as u64) * 4 > u64::from(u32::MAX - overhead) {
                return Err(error(message));
            }
            for (&l, &r) in left.iter().zip(right) {
                for sample in [l, r] {
                    let pcm = (sample.clamp(-1.0, 1.0) * 32768.0)
                        .round()
                        .clamp(-32768.0, 32767.0) as i16;
                    let bytes = match self.format {
                        Format::Aiff => pcm.to_be_bytes(),
                        Format::Wav | Format::Mp3 => pcm.to_le_bytes(),
                    };
                    self.bytes.extend_from_slice(&bytes);
                }
            }
        }
        self.file.write_all(&self.bytes)?;
        self.frames += left.len() as u64;
        Ok(())
    }
    fn finish(mut self) -> Result<u64> {
        if self.frames == 0 {
            return Err(error("decoded no audio samples"));
        }
        if let Some(encoder) = &mut self.encoder {
            self.bytes.clear();
            self.bytes.reserve(7200);
            encoder
                .flush_to_vec::<mp3lame_encoder::FlushGap>(&mut self.bytes)
                .map_err(error)?;
            self.file.write_all(&self.bytes)?;
            // Replace the reserved Info frame with duration/delay/padding.
            self.bytes.clear();
            self.bytes.reserve(encoder.lame_tag_size());
            if encoder.lame_tag_encode_to_vec(&mut self.bytes).is_some() {
                self.file
                    .seek(SeekFrom::Start(encoder.id3v2_tag_size() as u64))?;
                self.file.write_all(&self.bytes)?;
            }
        } else {
            let size = u32::try_from(self.frames * 4).map_err(error)?;
            self.file.seek(SeekFrom::Start(0))?;
            match self.format {
                Format::Wav => {
                    self.file.write_all(b"RIFF")?;
                    self.file.write_all(&(size + 36).to_le_bytes())?;
                    self.file.write_all(b"WAVEfmt ")?;
                    self.file.write_all(&16_u32.to_le_bytes())?;
                    self.file.write_all(&1_u16.to_le_bytes())?;
                    self.file.write_all(&2_u16.to_le_bytes())?;
                    self.file.write_all(&RATE.to_le_bytes())?;
                    self.file.write_all(&(RATE * 4).to_le_bytes())?;
                    self.file.write_all(&4_u16.to_le_bytes())?;
                    self.file.write_all(&16_u16.to_le_bytes())?;
                    self.file.write_all(b"data")?;
                    self.file.write_all(&size.to_le_bytes())?;
                }
                Format::Aiff => {
                    let frames = u32::try_from(self.frames).map_err(error)?;
                    self.file.write_all(b"FORM")?;
                    self.file.write_all(&(size + 46).to_be_bytes())?;
                    self.file.write_all(b"AIFFCOMM")?;
                    self.file.write_all(&18_u32.to_be_bytes())?;
                    self.file.write_all(&2_u16.to_be_bytes())?;
                    self.file.write_all(&frames.to_be_bytes())?;
                    self.file.write_all(&16_u16.to_be_bytes())?;
                    // 44,100 as an IEEE 754 80-bit extended float.
                    self.file.write_all(&[0x40, 0x0e, 0xac, 0x44, 0, 0, 0, 0, 0, 0])?;
                    self.file.write_all(b"SSND")?;
                    self.file.write_all(&(size + 8).to_be_bytes())?;
                    self.file.write_all(&0_u32.to_be_bytes())?;
                    self.file.write_all(&0_u32.to_be_bytes())?;
                }
                Format::Mp3 => return Err(error("MP3 encoder did not initialize")),
            }
        }
        self.file.flush()?;
        self.file.get_ref().sync_all()?;
        Ok(self.file.get_ref().metadata()?.len())
    }
}

/// Decode in bounded packets, resample with an anti-aliasing filter, and
/// encode to a new staging file. Any decode failure aborts publication.
/// Mono is duplicated to stereo; unsupported surround layouts are refused.
#[allow(
    clippy::too_many_lines,
    reason = "linear streaming decode and resample loop"
)]
pub fn convert(source: &Path, destination: &Path, target: Format) -> Result<u64> {
    let mut format = open(source)?;
    let track = format.default_track().ok_or(AudioError::NoTrack)?;
    let track_id = track.id;
    let params = &track.codec_params;
    let expected_frames = if matches!(
        params.codec,
        CODEC_TYPE_FLAC
            | CODEC_TYPE_PCM_S16LE
            | CODEC_TYPE_PCM_S24LE
            | CODEC_TYPE_PCM_S32LE
            | CODEC_TYPE_PCM_F32LE
    ) {
        params.n_frames
    } else {
        None
    };
    let rate = params
        .sample_rate
        .ok_or_else(|| error("unknown sample rate"))?;
    let channels = params
        .channels
        .ok_or_else(|| error("unknown channel layout"))?
        .count();
    if rate == 0 || !matches!(channels, 1 | 2) {
        return Err(error(
            "compatibility conversion requires mono or stereo audio",
        ));
    }
    let mut decoder = symphonia::default::get_codecs()
        .make(params, &DecoderOptions::default())
        .map_err(error)?;
    let mut output = Output::new(destination, target)?;
    let mut resampler = if rate == RATE {
        None
    } else {
        Some(FftFixedIn::<f32>::new(rate as usize, RATE as usize, CHUNK, 2, 2).map_err(error)?)
    };
    let mut delay = resampler.as_ref().map_or(0, Resampler::output_delay);
    let mut pending = [Vec::<f32>::new(), Vec::<f32>::new()];
    let mut input_frames = 0_u64;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(e) => return Err(error(e)),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let audio = decoder.decode(&packet).map_err(error)?;
        if audio.spec().rate != rate || audio.spec().channels.count() != channels {
            return Err(error("audio format changed mid-track"));
        }
        let mut samples = SampleBuffer::<f32>::new(audio.capacity() as u64, *audio.spec());
        samples.copy_interleaved_ref(audio);
        input_frames += (samples.samples().len() / channels) as u64;
        for frame in samples.samples().chunks_exact(channels) {
            pending[0].push(frame[0]);
            pending[1].push(frame[channels - 1]);
        }
        if let Some(resampler) = &mut resampler {
            while pending[0].len() >= CHUNK {
                let input = [&pending[0][..CHUNK], &pending[1][..CHUNK]];
                let converted = resampler.process(&input, None).map_err(error)?;
                emit(
                    &mut output,
                    &converted,
                    &mut delay,
                    input_frames * u64::from(RATE) / u64::from(rate),
                )?;
                for channel in &mut pending {
                    channel.drain(..CHUNK);
                }
            }
        } else {
            output.write(&pending[0], &pending[1])?;
            for channel in &mut pending {
                channel.clear();
            }
        }
    }
    if expected_frames.is_some_and(|expected| input_frames != expected) {
        return Err(error("audio ended before its declared length"));
    }
    if let Some(resampler) = &mut resampler {
        let target_frames = input_frames * u64::from(RATE) / u64::from(rate);
        while output.frames < target_frames {
            for channel in &mut pending {
                channel.resize(CHUNK, 0.0);
            }
            let converted = resampler.process(&pending, None).map_err(error)?;
            emit(&mut output, &converted, &mut delay, target_frames)?;
            for channel in &mut pending {
                channel.clear();
            }
        }
    }
    output.finish()
}
fn emit(
    output: &mut Output,
    samples: &[Vec<f32>],
    delay: &mut usize,
    target_frames: u64,
) -> Result<()> {
    let skip = (*delay).min(samples[0].len());
    *delay -= skip;
    let count = (samples[0].len() - skip).min(target_frames.saturating_sub(output.frames) as usize);
    output.write(
        &samples[0][skip..skip + count],
        &samples[1][skip..skip + count],
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    fn fixture() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stereo-96k.flac")
    }

    #[test]
    fn flac_to_wav_preserves_stereo_and_duration_at_cd_rate() {
        let dir = tempfile::tempdir().unwrap();
        let source = fixture();
        let before = std::fs::read(&source).unwrap();
        assert!(needs_conversion(&source).unwrap());
        let dest = dir.path().join("converted.wav");
        let size = convert(&source, &dest, Format::Wav).unwrap();
        assert_eq!(size, 44 + 11025 * 4);
        assert!(!needs_conversion(&dest).unwrap());
        let bytes = std::fs::read(&dest).unwrap();
        assert_eq!(&bytes[20..24], &[1, 0, 2, 0]);
        assert_eq!(&bytes[34..36], &[16, 0]);
        let frames: Vec<_> = bytes[44..]
            .chunks_exact(4)
            .map(|b| {
                (
                    f32::from(i16::from_le_bytes([b[0], b[1]])) / 32768.0,
                    f32::from(i16::from_le_bytes([b[2], b[3]])) / 32768.0,
                )
            })
            .collect();
        // Test the signal, not just its header: no pitch change, channel swap,
        // mono downmix, or resampler delay moving cues away from their audio.
        let error = frames
            .iter()
            .enumerate()
            .skip(100)
            .take(10000)
            .map(|(i, (l, r))| {
                let t = i as f32 / RATE as f32;
                (l - 0.5 * (std::f32::consts::TAU * 440.0 * t).sin()).abs()
                    + (r - 0.25 * (std::f32::consts::TAU * 880.0 * t).sin()).abs()
            })
            .sum::<f32>()
            / 10000.0;
        assert!(error < 0.002, "resampled stereo error: {error}");
        assert_eq!(std::fs::read(source).unwrap(), before);
    }

    #[test]
    fn flac_to_mp3_is_320_kbps_and_decodes_at_cd_rate() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("converted.mp3");
        convert(&fixture(), &dest, Format::Mp3).unwrap();
        assert!(!needs_conversion(&dest).unwrap());
        let audio = crate::decode_mono(&dest, None).unwrap();
        assert_eq!(audio.sample_rate, RATE);
        assert_eq!(audio.source_channels, 2);
        assert!(audio.samples.iter().any(|s| s.abs() > 0.1));
        let bytes = std::fs::read(dest).unwrap();
        // MPEG-1 Layer III bitrate index 14 is 320 kbps (first Info frame).
        assert_eq!(bytes[2] >> 4, 14);
        assert!(bytes.windows(4).any(|w| w == b"Info"));
        assert!(audio.duration_secs() >= 0.25 && audio.duration_secs() < 0.35);
        let restored = dir.path().join("decoded.wav");
        convert(&dir.path().join("converted.mp3"), &restored, Format::Wav).unwrap();
        assert_eq!(
            std::fs::metadata(restored).unwrap().len(),
            44 + 11025 * 4,
            "LAME delay and padding must be recoverable"
        );
    }

    #[test]
    fn flac_to_aiff_is_big_endian_pcm_at_cd_rate() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("converted.aiff");
        let size = convert(&fixture(), &dest, Format::Aiff).unwrap();
        assert_eq!(size, 54 + 11025 * 4);
        assert!(!needs_conversion(&dest).unwrap());
        let bytes = std::fs::read(&dest).unwrap();
        assert_eq!(&bytes[..12], b"FORM\0\0\xacrAIFF");
        assert_eq!(&bytes[12..20], b"COMM\0\0\0\x12");
        assert_eq!(&bytes[20..28], &[0, 2, 0, 0, 43, 17, 0, 16]);
        assert_eq!(&bytes[28..38], &[0x40, 0x0e, 0xac, 0x44, 0, 0, 0, 0, 0, 0]);
        assert_eq!(&bytes[38..54], b"SSND\0\0\xacL\0\0\0\0\0\0\0\0");
        let audio = crate::decode_mono(&dest, None).unwrap();
        assert_eq!(audio.sample_rate, RATE);
        assert_eq!(audio.source_channels, 2);
        assert!(audio.samples.iter().any(|sample| sample.abs() > 0.1));
    }

    #[test]
    fn refuses_to_overwrite_source_or_accept_invalid_audio() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.flac");
        let bytes = std::fs::read(fixture()).unwrap();
        std::fs::write(&source, &bytes).unwrap();
        assert!(convert(&source, &source, Format::Wav).is_err());
        assert_eq!(std::fs::read(&source).unwrap(), bytes);
        std::fs::write(&source, &bytes[..bytes.len() / 2]).unwrap();
        assert!(convert(&source, &dir.path().join("truncated.wav"), Format::Wav).is_err());
        std::fs::write(&source, b"invalid audio").unwrap();
        assert!(convert(&source, &dir.path().join("out.wav"), Format::Wav).is_err());
    }
}
