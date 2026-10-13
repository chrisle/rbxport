//! Writing the key into an audio file's tag (#204), on throwaway files only.
//!
//! What each format gets is what rekordbox 7.2.14 wrote on chris-win11 with
//! "Write the value to the ID3 tag" on [OBS, scratch copies]: `TKEY` in an
//! MP3's or AIFF's ID3v2, `INITIALKEY` in a FLAC's Vorbis comment, nothing in
//! an M4A or a WAV. The classic name under Classic (`Dbm`), the Camelot code
//! under Alphanumeric (`12A`).

#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::AudioFile;
use lofty::id3::v2::{Frame, FrameId, Id3v2Tag, TextInformationFrame};
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::TagExt;
use lofty::TextEncoding;
use rbl_db::key_tag::{read_key, write_key, KeyTagOutcome};

/// Bytes that are not silence, so a shifted or truncated copy shows.
fn noise(len: usize, seed: u32) -> Vec<u8> {
    let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state.to_le_bytes()[0]
        })
        .collect()
}

/// Forty MPEG-1 Layer III frames (128 kbps, 44.1 kHz) with noise for a body.
fn mp3_audio() -> Vec<u8> {
    let mut out = Vec::new();
    for i in 0..40 {
        out.extend_from_slice(&[0xFF, 0xFB, 0x90, 0x64]);
        out.extend_from_slice(&noise(413, i));
    }
    out
}

/// A tag like a DJ's: a key, a title, artwork, a comment, other apps' data.
fn busy_tag(key: Option<&str>) -> Id3v2Tag {
    let mut tag = Id3v2Tag::new();
    tag.insert(Frame::Text(TextInformationFrame::new(FrameId::Valid("TIT2".into()), TextEncoding::UTF8, "Transmission".into())));
    tag.insert(Frame::Text(TextInformationFrame::new(FrameId::Valid("TPE1".into()), TextEncoding::UTF16, "Olly James".into())));
    if let Some(key) = key {
        tag.insert(Frame::Text(TextInformationFrame::new(FrameId::Valid("TKEY".into()), TextEncoding::Latin1, key.into())));
    }
    tag.insert_user_text("SERATO_PLAYCOUNT".into(), "3".into());
    tag.insert_picture(Picture::new_unchecked(PictureType::CoverFront, Some(MimeType::Jpeg), None, noise(2048, 7)));
    tag.insert(Frame::Private(lofty::id3::v2::PrivateFrame::new(String::from("www.example.com"), noise(64, 9))));
    tag.insert(Frame::Binary(lofty::id3::v2::BinaryFrame::new(FrameId::Valid("GEOB".into()), {
        // GEOB: encoding, MIME, filename, description, data.
        let mut body = vec![0];
        body.extend_from_slice(b"application/octet-stream\0\0Serato Markers2\0");
        body.extend_from_slice(&noise(300, 11));
        body
    })));
    tag
}

fn write_mp3(path: &Path, tag: Option<&Id3v2Tag>, id3v23: bool) {
    std::fs::write(path, mp3_audio()).unwrap();
    if let Some(tag) = tag {
        tag.save_to_path(path, WriteOptions::default().use_id3v23(id3v23)).unwrap();
    }
}

fn write_aiff(path: &Path, tag: Option<&Id3v2Tag>) {
    let frames = 4_410_u32;
    let mut comm = Vec::new();
    comm.extend_from_slice(&1_u16.to_be_bytes());
    comm.extend_from_slice(&frames.to_be_bytes());
    comm.extend_from_slice(&16_u16.to_be_bytes());
    comm.extend_from_slice(&[0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0]);
    let mut ssnd = vec![0; 8];
    ssnd.extend_from_slice(&noise(frames as usize * 2, 3));
    let mut body = b"AIFF".to_vec();
    for (id, chunk) in [(b"COMM", &comm), (b"SSND", &ssnd)] {
        body.extend_from_slice(id);
        body.extend_from_slice(&u32::try_from(chunk.len()).unwrap().to_be_bytes());
        body.extend_from_slice(chunk);
    }
    let mut out = b"FORM".to_vec();
    out.extend_from_slice(&u32::try_from(body.len()).unwrap().to_be_bytes());
    out.extend_from_slice(&body);
    std::fs::write(path, out).unwrap();
    if let Some(tag) = tag {
        tag.save_to_path(path, WriteOptions::default()).unwrap();
    }
}

/// STREAMINFO, then the comment (if any), a PICTURE, PADDING, and noise as audio.
fn write_flac(path: &Path, comments: Option<&[&str]>, padding: usize) {
    let mut blocks: Vec<(u8, Vec<u8>)> = Vec::new();
    let mut info = Vec::new();
    info.extend_from_slice(&4096_u16.to_be_bytes());
    info.extend_from_slice(&4096_u16.to_be_bytes());
    info.extend_from_slice(&[0; 6]);
    let packed: u64 = (0xAC44 << 44) | (0xF << 36) | 0xAC44;
    info.extend_from_slice(&packed.to_be_bytes());
    info.extend_from_slice(&[0; 16]);
    blocks.push((0, info));
    if let Some(comments) = comments {
        let mut block = Vec::new();
        block.extend_from_slice(&12_u32.to_le_bytes());
        block.extend_from_slice(b"Lavf61.1.100");
        block.extend_from_slice(&u32::try_from(comments.len()).unwrap().to_le_bytes());
        for comment in comments {
            block.extend_from_slice(&u32::try_from(comment.len()).unwrap().to_le_bytes());
            block.extend_from_slice(comment.as_bytes());
        }
        blocks.push((4, block));
    }
    // A PICTURE block: type 3, MIME, empty description, 1x1, 24-bit, data.
    let mut picture = Vec::new();
    picture.extend_from_slice(&3_u32.to_be_bytes());
    picture.extend_from_slice(&10_u32.to_be_bytes());
    picture.extend_from_slice(b"image/jpeg");
    picture.extend_from_slice(&0_u32.to_be_bytes());
    for v in [1_u32, 1, 24, 0] {
        picture.extend_from_slice(&v.to_be_bytes());
    }
    let art = noise(512, 5);
    picture.extend_from_slice(&u32::try_from(art.len()).unwrap().to_be_bytes());
    picture.extend_from_slice(&art);
    blocks.push((6, picture));
    if padding > 0 {
        blocks.push((1, vec![0; padding]));
    }
    let mut out = b"fLaC".to_vec();
    let last = blocks.len() - 1;
    for (i, (kind, data)) in blocks.iter().enumerate() {
        let len = u32::try_from(data.len()).unwrap().to_be_bytes();
        out.extend_from_slice(&[if i == last { 0x80 } else { 0 } | kind, len[1], len[2], len[3]]);
        out.extend_from_slice(data);
    }
    // Audio frames: a frame sync then noise, which nothing here decodes.
    out.extend_from_slice(&[0xFF, 0xF8]);
    out.extend_from_slice(&noise(20_000, 13));
    std::fs::write(path, out).unwrap();
}

fn read_id3(path: &Path) -> Id3v2Tag {
    let mut file = std::fs::File::open(path).unwrap();
    let options = ParseOptions::new().read_properties(false);
    let ext = path.extension().unwrap().to_string_lossy().to_string();
    if ext == "mp3" {
        lofty::mpeg::MpegFile::read_from(&mut file, options).unwrap().id3v2().cloned().unwrap()
    } else {
        lofty::iff::aiff::AiffFile::read_from(&mut file, options).unwrap().id3v2().cloned().unwrap()
    }
}

/// Every frame but TKEY, as text that includes the content.
fn others(tag: &Id3v2Tag) -> Vec<String> {
    let mut out: Vec<String> = tag.into_iter().filter(|f| f.id_str() != "TKEY").map(|f| format!("{f:?}")).collect();
    out.sort();
    out
}

fn tkeys(tag: &Id3v2Tag) -> Vec<(String, TextEncoding)> {
    tag.into_iter()
        .filter_map(|f| match f {
            Frame::Text(t) if t.id().as_str() == "TKEY" => Some((t.value.clone(), t.encoding)),
            _ => None,
        })
        .collect()
}

fn flac_comments(path: &Path) -> (Vec<String>, usize) {
    let bytes = std::fs::read(path).unwrap();
    let mut at = 4;
    let mut comments = Vec::new();
    loop {
        let header = &bytes[at..at + 4];
        let len = (usize::from(header[1]) << 16) | (usize::from(header[2]) << 8) | usize::from(header[3]);
        let body = &bytes[at + 4..at + 4 + len];
        if header[0] & 0x7f == 4 {
            let vendor = u32::from_le_bytes(body[0..4].try_into().unwrap()) as usize;
            let count = u32::from_le_bytes(body[4 + vendor..8 + vendor].try_into().unwrap());
            let mut j = 8 + vendor;
            for _ in 0..count {
                let n = u32::from_le_bytes(body[j..j + 4].try_into().unwrap()) as usize;
                comments.push(String::from_utf8(body[j + 4..j + 4 + n].to_vec()).unwrap());
                j += 4 + n;
            }
        }
        at += 4 + len;
        if header[0] & 0x80 != 0 {
            return (comments, at);
        }
    }
}

#[test]
fn an_mp3_gets_its_tkey_replaced_and_keeps_every_other_frame_and_its_audio() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("issue-204-busy.mp3");
    write_mp3(&path, Some(&busy_tag(Some("Am"))), false);
    let before = read_id3(&path);
    let audio = mp3_audio();

    assert_eq!(write_key(&path, "Dbm").unwrap(), KeyTagOutcome::Written);

    let after = read_id3(&path);
    // The old frame's encoding is kept, as TagLib's `setText` keeps it.
    assert_eq!(tkeys(&after), vec![("Dbm".to_owned(), TextEncoding::Latin1)]);
    assert_eq!(others(&after), others(&before));
    assert!(std::fs::read(&path).unwrap().ends_with(&audio), "the audio frames are untouched");
    assert_eq!(read_key(&path).unwrap().as_deref(), Some("Dbm"));
    // Nothing left beside it.
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn an_mp3_without_a_tag_gets_a_utf8_tkey_in_an_id3v2_4_tag() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("issue-204-clean.mp3");
    write_mp3(&path, None, false);

    assert_eq!(write_key(&path, "Dbm").unwrap(), KeyTagOutcome::Written);

    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..4], b"ID3\x04", "saved as ID3v2.4, as rekordbox saves it");
    assert_eq!(tkeys(&read_id3(&path)), vec![("Dbm".to_owned(), TextEncoding::UTF8)]);
    assert!(bytes.ends_with(&mp3_audio()));
}

#[test]
fn the_alphanumeric_notation_is_written_as_rekordbox_stores_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("issue-204-alnum.mp3");
    write_mp3(&path, Some(&busy_tag(Some("Dbm"))), false);

    assert_eq!(write_key(&path, "12A").unwrap(), KeyTagOutcome::Written);

    assert_eq!(read_key(&path).unwrap().as_deref(), Some("12A"));
}

/// An ID3v2.3 tag as iTunes and many taggers write it: UTF-16 with a byte
/// order mark, the year and date in TYER and TDAT, a TXXX and a GEOB.
fn v23_tag() -> Vec<u8> {
    fn frame(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        out.extend_from_slice(&u32::try_from(body.len()).unwrap().to_be_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(body);
        out
    }
    fn utf16(text: &str) -> Vec<u8> {
        let mut out = vec![1, 0xFF, 0xFE];
        for unit in text.encode_utf16() {
            out.extend_from_slice(&unit.to_le_bytes());
        }
        out
    }
    let mut frames = Vec::new();
    frames.extend(frame(b"TIT2", &utf16("Transmission")));
    frames.extend(frame(b"TPE1", &utf16("Olly James")));
    frames.extend(frame(b"TYER", b"\x002024"));
    frames.extend(frame(b"TKEY", b"\x00Am"));
    let mut txxx = vec![0];
    txxx.extend_from_slice(b"SERATO_PLAYCOUNT\x003");
    frames.extend(frame(b"TXXX", &txxx));
    let mut geob = vec![0];
    geob.extend_from_slice(b"application/octet-stream\0\0Serato Markers2\0");
    geob.extend_from_slice(&noise(300, 11));
    frames.extend(frame(b"GEOB", &geob));
    frames.extend(std::iter::repeat_n(0, 256)); // padding
    let size = frames.len();
    let mut tag = b"ID3\x03\x00\x00".to_vec();
    tag.extend_from_slice(&[(size >> 21) as u8 & 0x7f, (size >> 14) as u8 & 0x7f, (size >> 7) as u8 & 0x7f, size as u8 & 0x7f]);
    tag.extend_from_slice(&frames);
    tag
}

#[test]
fn an_id3v2_3_tag_keeps_its_frames_when_it_is_saved_as_v2_4() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("issue-204-v23.mp3");
    let mut bytes = v23_tag();
    bytes.extend_from_slice(&mp3_audio());
    std::fs::write(&path, &bytes).unwrap();
    let before = read_id3(&path);

    assert_eq!(write_key(&path, "F#m").unwrap(), KeyTagOutcome::Written);

    let written = std::fs::read(&path).unwrap();
    assert_eq!(&written[..4], b"ID3\x04", "saved as ID3v2.4, as TagLib saves it for rekordbox");
    let after = read_id3(&path);
    assert_eq!(tkeys(&after), vec![("F#m".to_owned(), TextEncoding::Latin1)]);
    assert_eq!(others(&after), others(&before));
    assert!(others(&after).iter().any(|f| f.contains("GEOB")), "the Serato frame survives");
    assert!(written.ends_with(&mp3_audio()));
}

#[test]
fn an_aiff_gets_tkey_in_its_id3_chunk_and_keeps_its_sound() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("issue-204.aiff");
    write_aiff(&path, Some(&busy_tag(None)));
    let before = read_id3(&path);

    assert_eq!(write_key(&path, "Dbm").unwrap(), KeyTagOutcome::Written);

    let after = read_id3(&path);
    assert_eq!(tkeys(&after), vec![("Dbm".to_owned(), TextEncoding::UTF8)]);
    assert_eq!(others(&after), others(&before));
    let bytes = std::fs::read(&path).unwrap();
    let ssnd = bytes.windows(4).position(|w| w == b"SSND").unwrap();
    assert_eq!(&bytes[ssnd + 16..ssnd + 16 + 8820], &noise(8820, 3)[..]);
    assert_eq!(read_key(&path).unwrap().as_deref(), Some("Dbm"));
}

#[test]
fn an_aiff_with_no_tag_gets_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("issue-204-clean.aiff");
    write_aiff(&path, None);

    assert_eq!(write_key(&path, "12A").unwrap(), KeyTagOutcome::Written);

    assert_eq!(read_key(&path).unwrap().as_deref(), Some("12A"));
}

#[test]
fn a_flac_gets_its_first_initialkey_replaced_and_keeps_the_rest_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("issue-204.flac");
    write_flac(&path, Some(&["title=t204 flac", "initialkey=Am", "encoder=Lavf61.1.100", "INITIALKEY=C"]), 8192);
    let (_, audio_before) = flac_comments(&path);
    let size_before = std::fs::metadata(&path).unwrap().len();

    assert_eq!(write_key(&path, "Dbm").unwrap(), KeyTagOutcome::Written);

    let (comments, audio_after) = flac_comments(&path);
    // TagLib replaces the first value and keeps any others.
    assert_eq!(comments, ["title=t204 flac", "INITIALKEY=Dbm", "encoder=Lavf61.1.100", "INITIALKEY=C"]);
    // The padding took up the difference: the audio did not move.
    assert_eq!(audio_after, audio_before);
    assert_eq!(std::fs::metadata(&path).unwrap().len(), size_before);
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.ends_with(&noise(20_000, 13)));
    // The picture is still there.
    assert!(bytes.windows(10).any(|w| w == b"image/jpeg"));
    assert_eq!(read_key(&path).unwrap().as_deref(), Some("Dbm"));
}

#[test]
fn a_flac_without_a_comment_or_padding_gets_a_comment_after_streaminfo() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("issue-204-bare.flac");
    write_flac(&path, None, 0);

    assert_eq!(write_key(&path, "12A").unwrap(), KeyTagOutcome::Written);

    let (comments, _) = flac_comments(&path);
    assert_eq!(comments, ["INITIALKEY=12A"]);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(bytes[4 + 4 + 34] & 0x7f, 4, "the comment is the second block");
    assert!(bytes.ends_with(&noise(20_000, 13)));
    assert!(bytes.windows(10).any(|w| w == b"image/jpeg"));
}

#[test]
fn m4a_and_wav_are_left_alone_as_rekordbox_leaves_them() {
    let dir = tempfile::tempdir().unwrap();
    let m4a = dir.path().join("issue-204.m4a");
    std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/silent.m4a"), &m4a).unwrap();
    let wav = dir.path().join("issue-204.wav");
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&(36_u32 + 200).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    for v in [1_u16, 1] {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    bytes.extend_from_slice(&44_100_u32.to_le_bytes());
    bytes.extend_from_slice(&88_200_u32.to_le_bytes());
    for v in [2_u16, 16] {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&200_u32.to_le_bytes());
    bytes.extend_from_slice(&noise(200, 1));
    std::fs::write(&wav, &bytes).unwrap();
    let m4a_before = std::fs::read(&m4a).unwrap();

    assert_eq!(write_key(&m4a, "Dbm").unwrap(), KeyTagOutcome::Unsupported);
    assert_eq!(write_key(&wav, "Dbm").unwrap(), KeyTagOutcome::Unsupported);

    assert_eq!(std::fs::read(&m4a).unwrap(), m4a_before);
    assert_eq!(std::fs::read(&wav).unwrap(), bytes);
}

#[test]
fn a_read_only_file_is_passed_over_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("issue-204-locked.mp3");
    write_mp3(&path, Some(&busy_tag(Some("Am"))), false);
    let before = std::fs::read(&path).unwrap();
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions.clone()).unwrap();

    assert_eq!(write_key(&path, "Dbm").unwrap(), KeyTagOutcome::ReadOnly);

    assert_eq!(std::fs::read(&path).unwrap(), before);
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    std::fs::set_permissions(&path, permissions).unwrap();
}

#[test]
fn a_tag_that_cannot_be_read_strictly_is_not_rewritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("issue-204-broken.mp3");
    // An ID3v2.4 tag whose one frame claims more bytes than the tag holds.
    let mut bytes = b"ID3\x04\x00\x00\x00\x00\x00\x40".to_vec();
    bytes.extend_from_slice(b"TIT2\x00\x00\x00\x7f\x00\x00\x03Lost title");
    bytes.extend_from_slice(&[0; 0x40 - 21]);
    bytes.extend_from_slice(&mp3_audio());
    std::fs::write(&path, &bytes).unwrap();

    assert!(write_key(&path, "Dbm").is_err());

    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "no temporary file is left");
}

#[test]
fn a_symlink_stays_a_link_and_its_target_gets_the_key() {
    #[cfg(unix)]
    {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("issue-204-target.mp3");
        write_mp3(&target, None, false);
        let link = dir.path().join("issue-204-link.mp3");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        assert_eq!(write_key(&link, "Dbm").unwrap(), KeyTagOutcome::Written);

        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(read_key(&target).unwrap().as_deref(), Some("Dbm"));
    }
}

#[test]
fn clearing_the_key_empties_it() {
    let dir = tempfile::tempdir().unwrap();
    let mp3 = dir.path().join("issue-204-clear.mp3");
    write_mp3(&mp3, Some(&busy_tag(Some("Am"))), false);
    let flac = dir.path().join("issue-204-clear.flac");
    write_flac(&flac, Some(&["INITIALKEY=Am"]), 64);

    assert_eq!(write_key(&mp3, "").unwrap(), KeyTagOutcome::Written);
    assert_eq!(write_key(&flac, "").unwrap(), KeyTagOutcome::Written);

    assert_eq!(read_key(&mp3).unwrap().as_deref().unwrap_or(""), "");
    assert_eq!(flac_comments(&flac).0, ["INITIALKEY="]);
}
