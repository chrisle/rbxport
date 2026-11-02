//! `PVB2`: the FLAC seek index rekordbox ends a FLAC track's `.EXT` with.
//! Without it a CDJ-2000NXS2 (fw 1.83) scans the file on load and appends a
//! partial index of its own; five FLACs exported without one came back from a
//! gig that way, after slow loads [OBS 2026-11-02].
//!
//! Layout, matched entry for entry against four rekordbox-written files: a
//! 20-byte header (a zero word, total samples from STREAMINFO as u64, entry
//! count, entry length 20), then 400 entries of (first sample u64, byte offset
//! from the first audio frame u64, block size u32), all big-endian. Entry `i`
//! is the frame holding sample `i * (total / 400)`.

use std::path::Path;

const ENTRIES: u32 = 400;
const ENTRY_LEN: u32 = 20;

/// The `PVB2` section for the FLAC at `path`, or `None` when it does not
/// read as FLAC.
pub fn section(path: &Path) -> std::io::Result<Option<rbl_anlz::Section>> {
    let bytes = std::fs::read(path)?;
    Ok(index(&bytes).map(|(total, frames)| build(total, &frames)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Frame {
    sample: u64,
    offset: u64,
    block: u32,
}

/// STREAMINFO's total samples and every audio frame, in order. A candidate
/// sync code counts only when its header CRC holds and it starts exactly
/// where the previous frame ended, so sync-like bytes inside frame data
/// cannot be taken for a frame.
fn index(b: &[u8]) -> Option<(u64, Vec<Frame>)> {
    if !b.starts_with(b"fLaC") {
        return None;
    }
    let mut at = 4;
    let mut stream = None;
    loop {
        let head = *b.get(at)?;
        let (last, kind) = (head & 0x80 != 0, head & 0x7f);
        let len = usize::from(*b.get(at + 1)?) << 16 | usize::from(*b.get(at + 2)?) << 8 | usize::from(*b.get(at + 3)?);
        if kind == 0 {
            let info = b.get(at + 4..at + 4 + len).filter(|i| i.len() >= 18)?;
            let max_block = u64::from(u16::from_be_bytes([info[2], info[3]]));
            let total = u64::from_be_bytes(info[10..18].try_into().ok()?) & ((1 << 36) - 1);
            stream = Some((max_block, total));
        }
        at += 4 + len;
        if last {
            break;
        }
    }
    let (max_block, total) = stream.filter(|(_, total)| *total > 0)?;
    let start = at;
    let mut frames = Vec::new();
    let mut next = 0;
    while at < b.len() && next < total {
        match frame_header(b, at, max_block) {
            Some((sample, block, end)) if sample == next => {
                frames.push(Frame { sample, offset: (at - start) as u64, block });
                next = sample + u64::from(block);
                at = end;
            }
            _ => at += 1,
        }
    }
    (!frames.is_empty()).then_some((total, frames))
}

/// (first sample, block size, end of header) of a frame header at `at`.
fn frame_header(b: &[u8], at: usize, max_block: u64) -> Option<(u64, u32, usize)> {
    let h = b.get(at..at + 4)?;
    if h[0] != 0xff || h[1] & 0xfe != 0xf8 {
        return None;
    }
    let mut end = at + 4;
    // The frame or sample number, UTF-8 coded.
    let lead = *b.get(end)?;
    let extra = lead.leading_ones() as usize;
    if extra == 1 || extra > 7 {
        return None;
    }
    let mut number = u64::from(if extra == 0 { lead } else { lead & (0x7f >> extra) });
    for k in 1..extra {
        let c = *b.get(end + k)?;
        if c & 0xc0 != 0x80 {
            return None;
        }
        number = number << 6 | u64::from(c & 0x3f);
    }
    end += extra.max(1);
    let block = match h[2] >> 4 {
        1 => 192,
        n @ 2..=5 => 576 << (n - 2),
        6 => {
            end += 1;
            u32::from(*b.get(end - 1)?) + 1
        }
        7 => {
            end += 2;
            u32::from(u16::from_be_bytes([*b.get(end - 2)?, *b.get(end - 1)?])) + 1
        }
        n @ 8..=15 => 256 << (n - 8),
        _ => return None,
    };
    match h[2] & 0x0f {
        12 => end += 1,
        13 | 14 => end += 2,
        15 => return None,
        _ => {}
    }
    if crc8(b.get(at..end)?) != *b.get(end)? {
        return None;
    }
    let sample = if h[1] & 1 == 1 { number } else { number * max_block };
    Some((sample, block, end + 1))
}

fn crc8(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0, |crc, byte| {
        (0..8).fold(crc ^ byte, |c, _| if c & 0x80 == 0 { c << 1 } else { c << 1 ^ 0x07 })
    })
}

fn build(total: u64, frames: &[Frame]) -> rbl_anlz::Section {
    let mut header = Vec::with_capacity(20);
    header.extend_from_slice(&[0; 4]);
    header.extend_from_slice(&total.to_be_bytes());
    header.extend_from_slice(&ENTRIES.to_be_bytes());
    header.extend_from_slice(&ENTRY_LEN.to_be_bytes());
    let step = total / u64::from(ENTRIES);
    let mut payload = Vec::with_capacity(400 * 20);
    for i in 0..u64::from(ENTRIES) {
        let k = frames.partition_point(|f| f.sample <= i * step).saturating_sub(1);
        let f = frames[k];
        payload.extend_from_slice(&f.sample.to_be_bytes());
        payload.extend_from_slice(&f.offset.to_be_bytes());
        payload.extend_from_slice(&f.block.to_be_bytes());
    }
    rbl_anlz::Section::new(b"PVB2", header, payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed-block stream of `n` frames of 4096 samples with sync-like
    /// bytes in the frame data.
    fn stream(n: u64, total: u64) -> Vec<u8> {
        let mut b = b"fLaC".to_vec();
        let mut info = vec![0x10, 0x00, 0x10, 0x00, 0, 0, 0, 0, 0, 0];
        info.extend_from_slice(&(0x0ac4_4100_0000_0000 | total).to_be_bytes());
        info.extend_from_slice(&[0; 16]);
        b.extend_from_slice(&[0x80, 0, 0, 34]);
        b.extend_from_slice(&info);
        for number in 0..n {
            let mut h = vec![0xff, 0xf8, 0xc9, 0x18];
            let byte = |v: u64| u8::try_from(v).unwrap_or(0);
            if number < 0x80 {
                h.push(byte(number));
            } else {
                h.extend_from_slice(&[0xc0 | byte(number >> 6), 0x80 | byte(number & 0x3f)]);
            }
            h.push(crc8(&h));
            b.extend_from_slice(&h);
            b.extend_from_slice(&[0xff, 0xf8, 0xc9, 0x18, 0x00, 0x00, 0x12, 0x34]);
        }
        b
    }

    #[test]
    fn indexes_every_frame_and_skips_sync_like_data() {
        let b = stream(500, 500 * 4096 - 100);
        let (total, frames) = index(&b).unwrap_or_default();
        assert_eq!(total, 500 * 4096 - 100);
        assert_eq!(frames.len(), 500);
        assert!(frames.iter().enumerate().all(|(i, f)| f.sample == i as u64 * 4096 && f.block == 4096));
        assert_eq!(frames[1].offset - frames[0].offset, frames[2].offset - frames[1].offset);
    }

    #[test]
    fn entry_holds_the_frame_with_the_floored_step() {
        let total = 500 * 4096 - 100;
        let b = stream(500, total);
        let (_, frames) = index(&b).unwrap_or_default();
        let s = build(total, &frames);
        assert_eq!((s.len_header(), s.len_tag()), (32, 32 + 400 * 20));
        assert_eq!(&s.header[4..12], &total.to_be_bytes());
        let p = &s.payload;
        let entry = |i: usize| u64::from_be_bytes(p[i * 20..8 + i * 20].try_into().unwrap_or_default());
        let step = total / 400;
        for i in [0, 1, 206, 399] {
            assert_eq!(entry(i), (i as u64 * step) / 4096 * 4096);
        }
    }

    #[test]
    fn not_flac_is_none() {
        assert!(index(b"RIFF....WAVE").is_none());
    }
}
