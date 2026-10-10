//! Where rekordbox's timeline starts in an MP3, against symphonia's.
//!
//! Every cue, beat and waveform column rekordbox stores for an MP3 is a time
//! on rekordbox's own timeline, and that timeline does not always start at
//! the first sample symphonia decodes. rekordbox 7 builds a frame map of the
//! file before it decodes anything (`rbxfrm::FileReadMp3::FrameAnalyzer::
//! analyze`, arm64 @0x102819158 in 7.2.19 for macOS) and gives every frame in
//! it a slot of samples, the Xing/Info/VBRI frame at the head of a VBR or
//! LAME file included: that frame plays as one frame of silence. The map is
//! thrown away and started again at the next frame when one of the first four
//! frames disagrees with the frame before it on MPEG version, layer, sample
//! rate or channel mode (the comparison at @0x102819588, the restart at
//! @0x1028195f4). symphonia, by contrast, always drops a leading
//! Xing/Info/VBRI frame and never restarts.
//!
//! So on a LAME file whose Info frame says joint stereo like the audio after
//! it, rekordbox's time 0 is one frame (1,152 samples, 26.1 ms at 44.1 kHz)
//! before symphonia's, and a cue set in rekordbox plays 26 ms late from
//! symphonia's audio. On an ffmpeg (`Lavc`) file the Info frame says stereo
//! over joint-stereo audio, the map restarts at the audio, and the two agree.
//! [OBS] 200 MP3s on a rekordbox export stick, each decoded with symphonia
//! and lined up against the `PWV3` waveform rekordbox wrote for it: 86 sat
//! 1,152 samples late and 113 at 0, split exactly by this rule; the one
//! other was symphonia dropping two damaged packets, which the decoders now
//! play as silence instead. rekordbox's own `libmpg123`, opened with its
//! flags (`MPG123_FLAGS` 0x300, no gapless), decodes the same samples as
//! symphonia, so the difference is the frame map alone.

use std::io::{Read, Seek, SeekFrom};

/// How much of the file after the ID3 tag is read to find the first frames.
const SCAN_BYTES: usize = 64 * 1024;

/// rekordbox keeps a change of frame parameters once its map holds this many
/// frames, and starts the map again before that.
const MAP_SETTLED: usize = 4;

/// The frames looked at. Past [`MAP_SETTLED`] nothing can move the origin.
const FRAMES_LOOKED_AT: usize = MAP_SETTLED + 2;

/// MPEG-1 Layer III bitrates in kbit/s by index; `0` is free format.
const BITRATES_V1_L3: [u32; 15] = [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320];
/// MPEG-2 and 2.5 Layer III bitrates in kbit/s by index.
const BITRATES_V2_L3: [u32; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];

/// One MPEG audio frame header, as far as the map cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Header {
    /// The two version bits: 3 MPEG-1, 2 MPEG-2, 0 MPEG-2.5.
    version: u8,
    /// The two layer bits: 1 is Layer III.
    layer: u8,
    sample_rate_index: u8,
    /// 0 stereo, 1 joint stereo, 2 dual channel, 3 mono.
    channel_mode: u8,
    length: usize,
    samples: u32,
}

impl Header {
    fn parse(bytes: &[u8]) -> Option<Self> {
        let [a, b, c, d] = *bytes.get(..4)? else { return None };
        if a != 0xFF || b & 0xE0 != 0xE0 {
            return None;
        }
        let version = (b >> 3) & 0b11;
        let layer = (b >> 1) & 0b11;
        let bitrate_index = usize::from(c >> 4);
        let sample_rate_index = (c >> 2) & 0b11;
        // Layer III only: that is the MP3 rekordbox's frame map is built for,
        // and the only layer symphonia is built here to decode.
        if version == 1 || layer != 1 || sample_rate_index == 3 {
            return None;
        }
        let kbps = if version == 3 { BITRATES_V1_L3 } else { BITRATES_V2_L3 };
        let bitrate = *kbps.get(bitrate_index)?;
        if bitrate == 0 {
            // Free format: no length in the header to walk the frames with.
            return None;
        }
        let base_rate = [44_100u32, 48_000, 32_000][usize::from(sample_rate_index)];
        let sample_rate = match version {
            3 => base_rate,
            2 => base_rate / 2,
            _ => base_rate / 4,
        };
        let samples = if version == 3 { 1152 } else { 576 };
        let padding = usize::from((c >> 1) & 1);
        let length = usize::try_from(u64::from(samples / 8) * u64::from(bitrate) * 1000 / u64::from(sample_rate)).ok()? + padding;
        Some(Self { version, layer, sample_rate_index, channel_mode: d >> 6, length, samples })
    }

    /// What rekordbox compares between neighbouring frames.
    fn shape(&self) -> (u8, u8, u8, u8) {
        (self.version, self.layer, self.sample_rate_index, self.channel_mode)
    }

    /// Bytes of side information after the four header bytes.
    fn side_info(&self) -> usize {
        match (self.version == 3, self.channel_mode == 3) {
            (true, false) => 32,
            (true, true) | (false, false) => 17,
            (false, true) => 9,
        }
    }
}

/// Whether a frame is symphonia's idea of a tag frame: `Xing` or `Info`
/// straight after zeroed side information, or `VBRI` 32 bytes in.
fn is_tag_frame(frame: &[u8], header: &Header) -> bool {
    let at = 4 + header.side_info();
    let tag = frame.get(at..at + 4);
    if matches!(tag, Some(b"Xing" | b"Info")) {
        return frame.get(4..at).is_some_and(|side| side.iter().all(|&b| b == 0));
    }
    frame.get(36..40) == Some(b"VBRI")
}

/// The source-rate sample frames rekordbox's timeline runs ahead of
/// symphonia's for this file: rekordbox's time `t` is symphonia's `t - lead`.
///
/// Positive when rekordbox plays a leading tag frame as silence that
/// symphonia drops; negative when rekordbox starts its map after audio that
/// symphonia plays. Zero for anything that is not an MPEG Layer III stream,
/// or that this cannot read: the timelines are then taken to agree.
pub fn rekordbox_lead_frames<R: Read + Seek>(reader: &mut R) -> i64 {
    lead_frames(reader).unwrap_or(0)
}

fn lead_frames<R: Read + Seek>(reader: &mut R) -> std::io::Result<i64> {
    let mut at = 0u64;
    // ID3v2 tags, one after another, as rekordbox's analyzer skips them.
    loop {
        reader.seek(SeekFrom::Start(at))?;
        let mut tag = [0u8; 10];
        if reader.read(&mut tag)? < 10 || &tag[..3] != b"ID3" {
            break;
        }
        let size = tag[6..10].iter().fold(0u64, |size, &b| (size << 7) | u64::from(b & 0x7F));
        let footer = if tag[5] & 0x10 == 0 { 0 } else { 10 };
        at += 10 + size + footer;
    }
    reader.seek(SeekFrom::Start(at))?;
    let mut bytes = Vec::with_capacity(SCAN_BYTES);
    reader.take(SCAN_BYTES as u64).read_to_end(&mut bytes)?;
    Ok(lead_in(&bytes))
}

/// [`rekordbox_lead_frames`] over the bytes that follow the ID3 tag.
fn lead_in(bytes: &[u8]) -> i64 {
    let frames = first_frames(bytes);
    let Some((_, first)) = frames.first() else { return 0 };
    // symphonia's first sample is the first frame's, or the second's when
    // the first is a tag frame.
    let symphonia = usize::from(frames.first().is_some_and(|(at, header)| {
        bytes.get(*at..*at + header.length).is_some_and(|frame| is_tag_frame(frame, header))
    }));
    // rekordbox's is wherever its map last started again.
    let mut rekordbox = 0;
    for (index, pair) in frames.windows(2).enumerate() {
        let [(_, before), (_, after)] = pair else { continue };
        let held = index + 1 - rekordbox;
        if before.shape() != after.shape() && held < MAP_SETTLED {
            rekordbox = index + 1;
        }
    }
    let frames_ahead = i64::try_from(symphonia).unwrap_or(0) - i64::try_from(rekordbox).unwrap_or(0);
    frames_ahead * i64::from(first.samples)
}

/// The first frames of the stream, as `(offset, header)`: from the first
/// sync that the next frame confirms, frame after frame while they chain.
fn first_frames(bytes: &[u8]) -> Vec<(usize, Header)> {
    let mut frames = Vec::with_capacity(FRAMES_LOOKED_AT);
    let Some(start) = (0..bytes.len()).find(|&at| {
        let Some(header) = bytes.get(at..).and_then(Header::parse) else { return false };
        bytes
            .get(at + header.length..)
            .and_then(Header::parse)
            .is_some_and(|next| (next.version, next.layer, next.sample_rate_index) == (header.version, header.layer, header.sample_rate_index))
    }) else {
        return frames;
    };
    let mut at = start;
    while frames.len() < FRAMES_LOOKED_AT {
        let Some(header) = bytes.get(at..).and_then(Header::parse) else { break };
        frames.push((at, header));
        at += header.length;
    }
    frames
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    /// A 320 kbit/s, 44.1 kHz MPEG-1 Layer III frame: 1,044 bytes.
    fn frame(mode: u8, body: &[u8]) -> Vec<u8> {
        let mut frame = vec![0u8; 1044];
        frame[..4].copy_from_slice(&[0xFF, 0xFB, 0xE0, mode << 6]);
        frame[4..4 + body.len()].copy_from_slice(body);
        frame
    }

    /// An Info frame: zeroed side information, then the tag.
    fn info(mode: u8) -> Vec<u8> {
        let mut body = vec![0u8; 32];
        body.extend_from_slice(b"Info\0\0\0\x0f");
        frame(mode, &body)
    }

    /// Audio: side information that is not all zeros.
    fn audio(mode: u8) -> Vec<u8> {
        frame(mode, &[0x00, 0x01, 0x23, 0x45])
    }

    const STEREO: u8 = 0;
    const JOINT: u8 = 1;

    fn stream(frames: &[Vec<u8>]) -> Vec<u8> {
        frames.concat()
    }

    #[test]
    fn a_lame_info_frame_matching_its_audio_is_a_frame_of_silence_in_rekordbox() {
        // LAME 3.99, joint stereo throughout: "02 atom bomb.mp3" on the stick.
        let bytes = stream(&[info(JOINT), audio(JOINT), audio(JOINT), audio(JOINT), audio(JOINT), audio(JOINT)]);
        assert_eq!(lead_in(&bytes), 1152);
    }

    #[test]
    fn an_info_frame_that_disagrees_with_its_audio_restarts_the_map() {
        // Lavc 57: a stereo Info frame over joint-stereo audio.
        let bytes = stream(&[info(STEREO), audio(JOINT), audio(JOINT), audio(JOINT), audio(JOINT), audio(JOINT)]);
        assert_eq!(lead_in(&bytes), 0);
    }

    #[test]
    fn a_file_without_a_tag_frame_starts_where_symphonia_does() {
        let bytes = stream(&[audio(JOINT), audio(JOINT), audio(JOINT), audio(JOINT), audio(JOINT)]);
        assert_eq!(lead_in(&bytes), 0);
    }

    #[test]
    fn audio_rekordbox_restarts_after_is_audio_symphonia_plays_and_rekordbox_does_not() {
        let bytes = stream(&[audio(STEREO), audio(JOINT), audio(JOINT), audio(JOINT), audio(JOINT), audio(JOINT)]);
        assert_eq!(lead_in(&bytes), -1152);
    }

    #[test]
    fn a_change_after_the_map_has_settled_is_kept() {
        let bytes = stream(&[audio(JOINT), audio(JOINT), audio(JOINT), audio(JOINT), audio(STEREO), audio(STEREO)]);
        assert_eq!(lead_in(&bytes), 0);
    }

    #[test]
    fn the_id3_tag_is_skipped_and_anything_unreadable_is_no_offset() {
        let mut file = b"ID3\x04\0\0\0\0\x01\x00".to_vec();
        file.extend(std::iter::repeat_n(0u8, 128));
        file.extend(stream(&[info(JOINT), audio(JOINT), audio(JOINT), audio(JOINT), audio(JOINT)]));
        assert_eq!(rekordbox_lead_frames(&mut std::io::Cursor::new(file)), 1152);
        assert_eq!(rekordbox_lead_frames(&mut std::io::Cursor::new(b"RIFF....WAVEfmt ".to_vec())), 0);
        assert_eq!(rekordbox_lead_frames(&mut std::io::Cursor::new(Vec::new())), 0);
    }
}
