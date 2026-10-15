//! Actual exported-file properties for the statically mapped XDJ-R1 writer.
use crate::{AudioError, Result};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
#[derive(Debug, Clone)]
pub struct Info {
    pub bytes: u32,
    pub offset: u32,
    pub frames75: u32,
    pub rate: u16,
    pub format: u8,
    pub emphasis: bool,
}
fn error(s: impl std::fmt::Display) -> AudioError {
    AudioError::Unsupported(format!("XDJ-R1 audio: {s}"))
}
fn bytes<const N: usize>(f: &mut File, at: u64) -> Result<[u8; N]> {
    let mut b = [0; N];
    f.seek(SeekFrom::Start(at))?;
    f.read_exact(&mut b)?;
    Ok(b)
}
fn be32(f: &mut File, at: u64) -> Result<u32> {
    Ok(u32::from_be_bytes(bytes(f, at)?))
}
fn chunks(f: &mut File, len: u64, aiff: bool) -> Result<u32> {
    let mut at = 12_u64;
    while at.checked_add(8).is_some_and(|end| end <= len) {
        let tag = bytes::<4>(f, at)?;
        let raw = bytes::<4>(f, at + 4)?;
        let size = u64::from(if aiff {
            u32::from_be_bytes(raw)
        } else {
            u32::from_le_bytes(raw)
        });
        let end = at
            .checked_add(8)
            .and_then(|p| p.checked_add(size))
            .ok_or_else(|| error("chunk overflow"))?;
        if end > len {
            return Err(error("chunk extends beyond file"));
        }
        if (!aiff && &tag == b"data") || (aiff && &tag == b"SSND") {
            let offset = if aiff {
                if size < 8 {
                    return Err(error("short SSND"));
                }
                at + 16 + u64::from(be32(f, at + 8)?)
            } else {
                at + 8
            };
            if offset >= end {
                return Err(error("empty audio data"));
            }
            return u32::try_from(offset).map_err(error);
        }
        at = end
            .checked_add(size & 1)
            .ok_or_else(|| error("chunk padding overflow"))?;
    }
    Err(error("missing PCM data chunk"))
}
fn mp4_offsets(f: &mut File, start: u64, end: u64, depth: u8, out: &mut Vec<u64>) -> Result<()> {
    if depth > 8 {
        return Err(error("MP4 nesting exceeds limit"));
    }
    let mut at = start;
    while at + 8 <= end {
        let mut size = u64::from(be32(f, at)?);
        let tag = bytes::<4>(f, at + 4)?;
        let mut head = 8;
        if size == 1 {
            size = u64::from_be_bytes(bytes(f, at + 8)?);
            head = 16;
        } else if size == 0 {
            size = end - at;
        }
        let next = at
            .checked_add(size)
            .ok_or_else(|| error("MP4 box overflow"))?;
        if size < head || next > end {
            return Err(error("MP4 box outside file"));
        }
        if [*b"moov", *b"trak", *b"mdia", *b"minf", *b"stbl"].contains(&tag) {
            mp4_offsets(f, at + head, next, depth + 1, out)?;
        }
        if &tag == b"stco" || &tag == b"co64" {
            if size < head + 8 {
                return Err(error("short MP4 chunk table"));
            }
            let count = u64::from(be32(f, at + head + 4)?);
            let width = if &tag == b"stco" { 4 } else { 8 };
            if count == 0 || count > (size - head - 8) / width {
                return Err(error("invalid MP4 chunk count"));
            }
            let offset = if width == 4 {
                u64::from(be32(f, at + head + 8)?)
            } else {
                u64::from_be_bytes(bytes(f, at + head + 8)?)
            };
            out.push(offset);
        }
        at = next;
    }
    Ok(())
}
/// Unsupported formats are excluded from Cbm only. Invalid admitted files fail safely.
#[allow(
    clippy::too_many_lines,
    reason = "ordered format construction and validation; preserve evidence traceability"
)]
pub fn inspect(path: &Path, cancelled: &dyn Fn() -> bool) -> Result<Option<Info>> {
    use symphonia::core::{
        codecs::{CODEC_TYPE_AAC, CODEC_TYPE_MP3},
        errors::Error,
    };
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !["wav", "aif", "aiff", "mp3", "m4a", "aac"].contains(&ext.as_str()) {
        return Ok(None);
    }
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let byte_count = u32::try_from(len).map_err(|_| error("file exceeds safe 32-bit Cbm size"))?;
    let properties = crate::compatibility::inspect(path)?;
    let rate = properties
        .sample_rate
        .filter(|rate| *rate > 0)
        .ok_or_else(|| error("missing or zero sample rate"))?;
    let mut reader = crate::compatibility::open(path)?;
    let track = reader.default_track().ok_or(AudioError::NoTrack)?;
    let id = track.id;
    let codec = track.codec_params.codec;
    let mut frames = track.codec_params.n_frames.unwrap_or(0);
    let mut last = 0;
    let mut payload = 0_u64;
    if !matches!(properties.channels, Some(1 | 2)) {
        return Ok(None);
    }
    let (format, offset, emphasis, bitrate) = match ext.as_str() {
        "wav" | "aif" | "aiff" => {
            if !properties.compatible {
                return Ok(None);
            }
            let depth = properties.bit_depth.unwrap_or(0);
            // [OBS] getBitrateIndex 0x10170cf35..0x10170cfc1:
            // the low bit selects 48 kHz; 16/24-bit admission is separate.
            let code = if ext == "wav" { 0x20 } else { 0x30 } + u8::from(rate == 48_000);
            let offset = chunks(&mut file, len, ext != "wav")?;
            (
                code,
                offset,
                false,
                rate.checked_mul(depth)
                    .and_then(|v| v.checked_mul(properties.channels.unwrap_or(0)))
                    .ok_or_else(|| error("PCM rate overflow"))?
                    / 100,
            )
        }
        "mp3" => {
            if codec != CODEC_TYPE_MP3 {
                return Ok(None);
            }
            let mut offset = 0_u64;
            if len >= 10 && &bytes::<3>(&mut file, 0)? == b"ID3" {
                let h = bytes::<10>(&mut file, 0)?;
                if h[6..10].iter().any(|b| b & 0x80 != 0) {
                    return Err(error("invalid ID3 size"));
                }
                offset = 10 + h[6..10].iter().fold(0_u64, |n, b| (n << 7) | u64::from(*b));
                if h[5] & 0x10 != 0 {
                    offset += 10;
                }
            }
            let h = bytes::<4>(&mut file, offset)?;
            if h[0] != 0xff || h[1] & 0xe0 != 0xe0 || h[1] & 6 != 2 || h[1] & 0x18 == 8 {
                return Ok(None);
            }
            let index = usize::from(h[2] >> 4);
            let kbps = if h[1] & 0x18 == 0x18 {
                [
                    0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
                ][index]
            } else {
                [
                    0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0,
                ][index]
            };
            let mut prefix = vec![0; usize::try_from((len - offset).min(256)).map_err(error)?];
            file.seek(SeekFrom::Start(offset))?;
            file.read_exact(&mut prefix)?;
            let vbr = prefix.windows(4).any(|w| w == b"Xing" || w == b"VBRI");
            let code = if vbr {
                0x1f
            } else {
                match kbps {
                    32 => 0x11,
                    40 => 0x12,
                    48 => 0x13,
                    56 => 0x14,
                    64 => 0x15,
                    80 => 0x16,
                    96 => 0x17,
                    112 => 0x18,
                    128 => 0x19,
                    144 => 0x0d,
                    160 => 0x1a,
                    192 => 0x1b,
                    224 => 0x1c,
                    256 => 0x1d,
                    320 => 0x1e,
                    _ => return Ok(None),
                }
            };
            (
                code,
                u32::try_from(offset).map_err(error)?,
                h[3] & 3 == 1,
                if vbr { 0 } else { kbps * 10 },
            )
        }
        _ => {
            if codec != CODEC_TYPE_AAC || ![32000, 44100, 48000].contains(&rate) {
                return Ok(None);
            }
            let offset = if ext == "aac" {
                0
            } else {
                let mut offsets = Vec::new();
                mp4_offsets(&mut file, 0, len, 0, &mut offsets)?;
                if offsets.len() != 1 {
                    return Err(error("AAC needs one unambiguous audio chunk table"));
                }
                u32::try_from(offsets[0]).map_err(error)?
            };
            (0x41, offset, false, 0)
        }
    };
    // Count decoder timestamps when no declared frame count is available; AAC rate
    // is measured from compressed payload rather than stale Collection metadata.
    if frames == 0 || codec == CODEC_TYPE_AAC {
        loop {
            if cancelled() {
                return Err(error("cancelled"));
            }
            match reader.next_packet() {
                Ok(packet) if packet.track_id() == id => {
                    last = last.max(packet.ts.saturating_add(packet.dur));
                    payload = payload.saturating_add(packet.data.len() as u64);
                }
                Ok(_) => {}
                Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(error(e)),
            }
        }
        if frames == 0 {
            frames = last;
        }
    }
    if frames == 0 || offset >= byte_count {
        return Err(error("missing duration or invalid data offset"));
    }
    let frames75 = u32::try_from(
        frames
            .checked_mul(75)
            .ok_or_else(|| error("duration overflow"))?
            / u64::from(rate),
    )
    .map_err(error)?;
    let bitrate = if codec == CODEC_TYPE_AAC {
        u32::try_from(payload.saturating_mul(8).saturating_mul(u64::from(rate)) / frames / 100)
            .map_err(error)?
    } else {
        bitrate
    };
    if codec == CODEC_TYPE_AAC && !(160..=3200).contains(&bitrate) {
        return Ok(None);
    }
    Ok(Some(Info {
        bytes: byte_count,
        offset,
        frames75,
        rate: u16::try_from(bitrate).map_err(error)?,
        format,
        emphasis,
    }))
}
