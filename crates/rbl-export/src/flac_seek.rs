//! `PVB2`: the FLAC seek index rekordbox ends a FLAC track's `.EXT` with.
//! The #828 contributor reported slow loads and player-written partial
//! indexes on CDJ-2000NXS2 firmware 1.83. This module validates file structure;
//! it does not establish a physical loading-speed or playback result.
//!
//! The #828 contributor reported this layout matched entry for entry
//! against four rekordbox-written files (not independently reproduced here): a
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
/// frame is structurally parsed through its subframes and CRC16 footer;
/// sample numbers must be contiguous. Incomplete/corrupt streams are
/// rejected rather than publishing a partial index.
fn index(b: &[u8]) -> Option<(u64, Vec<Frame>)> {
    if !b.starts_with(b"fLaC") {
        return None;
    }
    let mut at = 4;
    let mut stream = None;
    loop {
        let head = *b.get(at)?;
        let (last, kind) = (head & 0x80 != 0, head & 0x7f);
        let len = usize::from(*b.get(at + 1)?) << 16
            | usize::from(*b.get(at + 2)?) << 8
            | usize::from(*b.get(at + 3)?);
        if (at == 4 && kind != 0) || (kind == 0 && stream.is_some()) {
            return None;
        }
        if kind == 0 {
            let info = b.get(at + 4..at + 4 + len).filter(|i| i.len() == 34)?;
            let max_block = u64::from(u16::from_be_bytes([info[2], info[3]]));
            let total = u64::from_be_bytes(info[10..18].try_into().ok()?) & ((1 << 36) - 1);
            let packed = u64::from_be_bytes(info[10..18].try_into().ok()?);
            let bits = ((packed >> 36) & 31) as usize + 1;
            stream = Some((max_block, total, bits));
        }
        at += 4 + len;
        if last {
            break;
        }
    }
    let (max_block, total, bits) =
        stream.filter(|(max_block, total, _)| *max_block > 0 && *total > 0)?;
    let start = at;
    let mut frames = Vec::new();
    let mut next = 0;
    while next < total {
        let (sample, block, header_end) = frame_header(b, at, max_block)?;
        if sample != next || sample.checked_add(u64::from(block))? > total {
            return None;
        }
        next = sample + u64::from(block);
        let boundary = frame_end(b, at, header_end, block as usize, bits)?;
        frames.push(Frame {
            sample,
            offset: (at - start) as u64,
            block,
        });
        at = boundary;
    }
    (!frames.is_empty() && at == b.len()).then_some((total, frames))
}

/// (first sample, block size, end of header) of a frame header at `at`.
fn frame_header(b: &[u8], at: usize, max_block: u64) -> Option<(u64, u32, usize)> {
    let h = b.get(at..at + 4)?;
    if h[0] != 0xff
        || h[1] & 0xfe != 0xf8
        || h[3] & 1 != 0
        || h[3] >> 4 > 10
        || (h[3] >> 1) & 7 == 3
    {
        return None;
    }
    let mut end = at + 4;
    // The frame or sample number, UTF-8 coded.
    let lead = *b.get(end)?;
    let extra = lead.leading_ones() as usize;
    if extra == 1 || extra > 7 {
        return None;
    }
    let mut number = u64::from(if extra == 0 {
        lead
    } else {
        lead & (0x7f >> extra)
    });
    for k in 1..extra {
        let c = *b.get(end + k)?;
        if c & 0xc0 != 0x80 {
            return None;
        }
        number = number << 6 | u64::from(c & 0x3f);
    }
    if extra >= 2 {
        let minimum = match extra {
            2 => 0x80,
            3 => 0x800,
            4 => 0x10000,
            5 => 0x0020_0000,
            6 => 0x0400_0000,
            _ => 0x8000_0000,
        };
        if number < minimum {
            return None;
        }
    }
    if (h[1] & 1 == 0 && number > 0x7fff_ffff) || number >= (1u64 << 36) {
        return None;
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
    let sample = if h[1] & 1 == 1 {
        number
    } else {
        number.checked_mul(max_block)?
    };
    Some((sample, block, end + 1))
}

/// A bounded bit cursor: all lengths are checked before advancing.
struct Bits<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl Bits<'_> {
    fn read(&mut self, count: usize) -> Option<u32> {
        if count > 32 {
            return None;
        }
        let mut value = 0;
        for _ in 0..count {
            let byte = *self.bytes.get(self.at / 8)?;
            value = (value << 1) | u32::from((byte >> (7 - self.at % 8)) & 1);
            self.at += 1;
        }
        Some(value)
    }
    fn skip(&mut self, count: usize) -> Option<()> {
        let end = self.at.checked_add(count)?;
        if end > self.bytes.len().checked_mul(8)? {
            return None;
        }
        self.at = end;
        Some(())
    }
    fn unary(&mut self) -> Option<usize> {
        let mut zeros = 0usize;
        while self.read(1)? == 0 {
            zeros = zeros.checked_add(1)?;
        }
        Some(zeros)
    }
}

fn residual(bits: &mut Bits<'_>, block: usize, order: usize) -> Option<()> {
    let parameter_bits = match bits.read(2)? {
        0 => 4,
        1 => 5,
        _ => return None,
    };
    let partitions = 1usize << bits.read(4)?;
    if block % partitions != 0 {
        return None;
    }
    let partition_samples = block / partitions;
    if partition_samples < order {
        return None;
    }
    for partition in 0..partitions {
        let samples = partition_samples - if partition == 0 { order } else { 0 };
        let parameter = bits.read(parameter_bits)? as usize;
        if parameter == (1 << parameter_bits) - 1 {
            let width = bits.read(5)? as usize;
            bits.skip(samples.checked_mul(width)?)?;
        } else {
            for _ in 0..samples {
                bits.unary()?;
                bits.skip(parameter)?;
            }
        }
    }
    Some(())
}

fn subframe(bits: &mut Bits<'_>, block: usize, depth: usize) -> Option<()> {
    if bits.read(1)? != 0 {
        return None;
    }
    let kind = bits.read(6)?;
    let wasted = if bits.read(1)? != 0 {
        bits.unary()?.checked_add(1)?
    } else {
        0
    };
    let depth = depth.checked_sub(wasted).filter(|n| *n > 0)?;
    match kind {
        0 => bits.skip(depth),
        1 => bits.skip(block.checked_mul(depth)?),
        8..=12 | 32..=63 => {
            let order = if kind < 32 {
                kind as usize - 8
            } else {
                kind as usize - 31
            };
            if order > block {
                return None;
            }
            bits.skip(order.checked_mul(depth)?)?;
            if kind >= 32 {
                let precision = bits.read(4)? as usize;
                if precision == 15 {
                    return None;
                }
                bits.skip(5)?; // signed predictor shift
                bits.skip(order.checked_mul(precision + 1)?)?;
            }
            residual(bits, block, order)
        }
        _ => None,
    }
}

/// Walk the actual subframe coding lengths; a CRC-valid header in verbatim
/// PCM is still PCM, never a frame boundary. No audio samples are allocated.
fn frame_end(
    b: &[u8],
    start: usize,
    header_end: usize,
    block: usize,
    stream_bits: usize,
) -> Option<usize> {
    let config = *b.get(start + 3)?;
    let depth = match (config >> 1) & 7 {
        0 => stream_bits,
        1 => 8,
        2 => 12,
        4 => 16,
        5 => 20,
        6 => 24,
        7 => 32,
        _ => return None,
    };
    let assignment = config >> 4;
    let channels = if assignment <= 7 {
        assignment as usize + 1
    } else {
        2
    };
    let mut bits = Bits {
        bytes: b,
        at: header_end.checked_mul(8)?,
    };
    for channel in 0..channels {
        let side = matches!((assignment, channel), (8 | 10, 1) | (9, 0));
        subframe(&mut bits, block, depth + usize::from(side))?;
    }
    let padding = (8 - bits.at % 8) % 8;
    if bits.read(padding)? != 0 {
        return None;
    }
    let end = (bits.at / 8).checked_add(2)?;
    (crc16(b.get(start..end)?) == 0).then_some(end)
}

fn crc8(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0, |crc, byte| {
        (0..8).fold(crc ^ byte, |c, _| {
            if c & 0x80 == 0 {
                c << 1
            } else {
                c << 1 ^ 0x07
            }
        })
    })
}

fn crc16_byte(mut crc: u16, byte: u8) -> u16 {
    crc ^= u16::from(byte) << 8;
    for _ in 0..8 {
        crc = if crc & 0x8000 != 0 {
            (crc << 1) ^ 0x8005
        } else {
            crc << 1
        };
    }
    crc
}

fn crc16(bytes: &[u8]) -> u16 {
    bytes.iter().fold(0, |crc, byte| crc16_byte(crc, *byte))
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
        let k = frames
            .partition_point(|f| f.sample <= i * step)
            .saturating_sub(1);
        let f = frames[k];
        payload.extend_from_slice(&f.sample.to_be_bytes());
        payload.extend_from_slice(&f.offset.to_be_bytes());
        payload.extend_from_slice(&f.block.to_be_bytes());
    }
    rbl_anlz::Section::new(b"PVB2", header, payload)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
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
            h.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // two constant 16-bit channels
            let crc = crc16(&h);
            h.extend_from_slice(&crc.to_be_bytes());
            b.extend_from_slice(&h);
        }
        b
    }

    #[test]
    fn indexes_every_frame_and_skips_sync_like_data() {
        let b = stream(500, 500 * 4096);
        let (total, frames) = index(&b).unwrap_or_default();
        assert_eq!(total, 500 * 4096);
        assert_eq!(frames.len(), 500);
        assert!(frames
            .iter()
            .enumerate()
            .all(|(i, f)| f.sample == i as u64 * 4096 && f.block == 4096));
        assert_eq!(
            frames[1].offset - frames[0].offset,
            frames[2].offset - frames[1].offset
        );
    }

    #[test]
    fn entry_holds_the_frame_with_the_floored_step() {
        let total = 501 * 4096;
        let b = stream(501, total);
        let (_, frames) = index(&b).unwrap_or_default();
        let s = build(total, &frames);
        assert_eq!((s.len_header(), s.len_tag()), (32, 32 + 400 * 20));
        assert_eq!(&s.header[4..12], &total.to_be_bytes());
        let p = &s.payload;
        let entry =
            |i: usize| u64::from_be_bytes(p[i * 20..8 + i * 20].try_into().unwrap_or_default());
        let step = total / 400;
        for i in [0, 1, 206, 399] {
            assert_eq!(entry(i), (i as u64 * step) / 4096 * 4096);
        }
    }

    #[test]
    fn indexes_valid_constant_subframes() {
        let mut b = stream(0, 8192);
        b[20] &= !0x0e; // STREAMINFO mono
        for number in 0..2 {
            // Mono 16-bit constant subframe, 4096 samples at 44.1 kHz.
            let mut frame = vec![0xff, 0xf8, 0xc9, 0x08, number];
            frame.push(crc8(&frame));
            frame.extend_from_slice(&[0, 0x12, 0x34]);
            frame.extend_from_slice(&crc16(&frame).to_be_bytes());
            b.extend_from_slice(&frame);
        }
        let (_, frames) = index(&b).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[1].offset, 11);
    }

    #[test]
    fn variable_blocks_and_short_final_frame() {
        let mut b = stream(0, 24);
        b[20] &= !0x0e; // STREAMINFO mono
        b[8..12].copy_from_slice(&[0, 16, 0, 16]);
        for (sample, block) in [(0, 16), (16, 8)] {
            let mut frame = vec![0xff, 0xf9, 0x69, 0x08, sample, block - 1];
            frame.push(crc8(&frame));
            frame.extend_from_slice(&[0, 0, 0]);
            frame.extend_from_slice(&crc16(&frame).to_be_bytes());
            b.extend_from_slice(&frame);
        }
        let (_, frames) = index(&b).unwrap();
        assert_eq!(frames[1].sample, 16);
        assert_eq!(frames[1].block, 8);
    }

    #[test]
    fn rejects_partial_and_corrupt_streams() {
        let b = stream(3, 3 * 4096);
        assert!(index(&b).is_some());
        assert!(index(&b[..b.len() - 2]).is_none());
        assert!(index(&stream(2, 3 * 4096)).is_none());
        let mut corrupt = b;
        corrupt[50] ^= 1;
        assert!(index(&corrupt).is_none());
    }

    #[test]
    fn crc_valid_header_and_prefix_inside_verbatim_pcm_are_not_a_frame() {
        let mut b = stream(0, 8192);
        b[20] &= !0x0e; // STREAMINFO mono
        let header = |number| {
            let mut h = vec![0xff, 0xf8, 0xc9, 0x08, number];
            h.push(crc8(&h));
            h
        };
        let mut first = header(0);
        first.push(2); // mono verbatim subframe; all following 8192 bytes are PCM
        first.extend_from_slice(&[0; 12]);
        first.extend_from_slice(&crc16(&first).to_be_bytes());
        let fake = first.len();
        first.extend_from_slice(&header(1));
        first.resize(6 + 1 + 8192, 0);
        first.extend_from_slice(&crc16(&first).to_be_bytes());
        let real = first.len();
        let mut second = header(1);
        second.push(2);
        second.extend_from_slice(&[0; 8192]);
        second.extend_from_slice(&crc16(&second).to_be_bytes());
        assert_eq!(crc16(&first[..fake]), 0);
        let mut both = first;
        both.extend_from_slice(&second);
        assert_eq!(crc16(&both[fake..]), 0); // the checksum-only scanner was fooled
        b.extend_from_slice(&both);
        let (_, frames) = index(&b).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[1].offset, real as u64);
        assert_ne!(frames[1].offset, fake as u64);
    }

    #[test]
    fn parses_predictors_residuals_and_wasted_bits() {
        fn check(fields: &[(u32, usize)], block: usize, depth: usize, valid: bool) {
            let mut raw = Vec::new();
            let mut length = 0;
            for &(value, width) in fields {
                for shift in (0..width).rev() {
                    if length % 8 == 0 {
                        raw.push(0u8);
                    }
                    let last = raw.len() - 1;
                    raw[last] |= (((value >> shift) & 1) as u8) << (7 - length % 8);
                    length += 1;
                }
            }
            let mut bits = Bits { bytes: &raw, at: 0 };
            assert_eq!(subframe(&mut bits, block, depth).is_some(), valid);
            if valid {
                assert_eq!(bits.at, length);
            }
        }
        // Fixed order 0, Rice parameter 0: sixteen zero residuals (unary 1).
        check(
            &[(0x10, 8), (0, 2), (0, 4), (0, 4), (0xffff, 16)],
            16,
            16,
            true,
        );
        // Fixed order 1, one warmup sample, Rice2 escape with zero-width residuals.
        check(
            &[(0x12, 8), (0, 16), (1, 2), (0, 4), (31, 5), (0, 5)],
            16,
            16,
            true,
        );
        // LPC order 1, precision 1, signed shift, coefficient, escaped residuals.
        check(
            &[
                (0x40, 8),
                (0, 16),
                (0, 4),
                (0, 5),
                (0, 1),
                (0, 2),
                (0, 4),
                (15, 4),
                (0, 5),
            ],
            16,
            16,
            true,
        );
        // Constant with 2 wasted low bits, leaving a 14-bit signed sample.
        check(&[(1, 8), (1, 2), (0, 14)], 16, 16, true);
        check(&[(0x10, 8), (0, 2), (0, 4), (0, 4), (0, 16)], 16, 16, false); // unterminated unary
        check(&[(0x04, 8)], 16, 16, false); // reserved subframe
        check(&[(0x10, 8), (2, 2)], 16, 16, false); // reserved residual method
        check(&[(0x18, 8)], 2, 16, false); // predictor exceeds block
        check(&[(0x40, 8), (0, 16), (15, 4)], 16, 16, false); // reserved LPC precision
    }

    #[test]
    fn parses_side_channel_bit_depth_and_zero_padding() {
        for assignment in [8u8, 9, 10] {
            let mut b = stream(0, 4096);
            let mut frame = vec![0xff, 0xf8, 0xc9, (assignment << 4) | 8, 0];
            frame.push(crc8(&frame));
            let mut payload = Vec::<u8>::new();
            let mut length = 0;
            for channel in 0..2 {
                let side = matches!((assignment, channel), (8 | 10, 1) | (9, 0));
                let depth = 16 + usize::from(side);
                for (value, width) in [(0u32, 8usize), (1, depth)] {
                    for shift in (0..width).rev() {
                        if length % 8 == 0 {
                            payload.push(0);
                        }
                        let last = payload.len() - 1;
                        payload[last] |= (((value >> shift) & 1) as u8) << (7 - length % 8);
                        length += 1;
                    }
                }
            }
            frame.extend_from_slice(&payload);
            frame.extend_from_slice(&crc16(&frame).to_be_bytes());
            b.extend_from_slice(&frame);
            assert!(index(&b).is_some(), "assignment {assignment}");
            // Nonzero byte-alignment padding is invalid even with a valid CRC.
            let footer = b.len() - 2;
            b[footer - 1] |= 1;
            let crc = crc16(&b[42..footer]);
            b[footer..].copy_from_slice(&crc.to_be_bytes());
            assert!(index(&b).is_none());
        }
    }

    #[test]
    fn compressed_flac_fixture_has_exact_frame_offsets_and_pvb2_bytes() {
        let bytes = include_bytes!("../../rbl-audio/tests/fixtures/stereo-96k.flac");
        let (total, frames) = index(bytes).unwrap();
        assert_eq!(total, 24_000);
        assert_eq!(frames, [
            Frame { sample: 0, offset: 0, block: 8_192 },
            Frame { sample: 8_192, offset: 18_776, block: 8_192 },
            Frame { sample: 16_384, offset: 37_899, block: 7_616 },
        ]);
        let section = build(total, &frames);
        let mut header = vec![0; 4];
        header.extend_from_slice(&24_000u64.to_be_bytes());
        header.extend_from_slice(&400u32.to_be_bytes());
        header.extend_from_slice(&20u32.to_be_bytes());
        assert_eq!(section.header, header);
        // A 60-sample step selects the first frame for entries 0..137,
        // the second for 137..274, and the short final frame thereafter.
        for (range, sample, offset, block) in [
            (0..137, 0u64, 0u64, 8_192u32),
            (137..274, 8_192, 18_776, 8_192),
            (274..400, 16_384, 37_899, 7_616),
        ] {
            let mut expected = sample.to_be_bytes().to_vec();
            expected.extend_from_slice(&offset.to_be_bytes());
            expected.extend_from_slice(&block.to_be_bytes());
            for entry in range {
                assert_eq!(&section.payload[entry * 20..(entry + 1) * 20], expected);
            }
        }
    }

    #[test]
    fn not_flac_is_none() {
        assert!(index(b"RIFF....WAVE").is_none());
    }
}
