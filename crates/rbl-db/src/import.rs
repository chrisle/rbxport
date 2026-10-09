//! Reading a file's tags, so a track can be added to the library.
//!
//! Separate from the writer: what a file says about itself is a different
//! question from what the database should record, and keeping them apart means
//! the tag reading is testable without a database at all.

use std::path::Path;

use lofty::config::ParseOptions;
use lofty::file::{AudioFile, FileType, TaggedFile, TaggedFileExt};
use lofty::prelude::{ItemKey, TagExt};
use lofty::probe::Probe;
use lofty::tag::{Tag, TagType};

/// Extensions rekordbox will play, and so the only ones worth importing.
pub const AUDIO_EXTENSIONS: &[&str] =
    &["mp3", "m4a", "aac", "flac", "wav", "aiff", "aif", "ogg", "opus"];

/// What a file says about itself.
///
/// Every field is optional because tags routinely are: an untagged file is
/// still importable, it just arrives with its filename as its title.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackTags {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub genre: String,
    pub label: String,
    pub comment: String,
    /// Seconds, rounded.
    pub duration_sec: u32,
    pub bitrate: u32,
    pub sample_rate: u32,
    pub year: u16,
    pub track_no: u16,
    pub file_size: u64,
    /// Bits per sample; 16 when the format does not say (an MP3), which is
    /// what rekordbox records for one.
    pub bit_depth: u8,
    /// The musical key the tag names, as written there (`Am`, `8A`); empty
    /// when it names none. See [`tag_key`].
    pub key: String,
}

/// The MP4 atom rekordbox takes a key from. Not lofty's `InitialKey`
/// mapping (`----:com.apple.iTunes:initialkey`): rekordbox's `parseKey`
/// names this one and no other [OBS: see [`tag_key`]].
const MP4_KEY: &str = "----:com.apple.iTunes:KEY";

/// The tag rekordbox treats as a file's `ID3v2` tag, if it has one.
///
/// rekordbox 7.2.19 (macOS arm64) `TagLib::ParseTag::getID3v2Tag`
/// @0x100e4bafc [OBS static]: an MP3's `ID3v2` tag when the file has one
/// (`MPEG::File::hasID3v2Tag`), an AIFF's `ID3v2` tag, and nothing for any
/// other format. A WAV's `id3 ` chunk is not one of them: the cast to
/// `RIFF::WAV::File` there has its result thrown away.
fn id3v2_tag(tagged: &TaggedFile) -> Option<&Tag> {
    matches!(tagged.file_type(), FileType::Mpeg | FileType::Aiff)
        .then(|| tagged.tag(TagType::Id3v2))
        .flatten()
}

/// The tag a non-empty `ID3v2`, else a non-empty MP4 tag, else the Vorbis
/// comments: the order rekordbox's `parseKey` and `parseLabel` look in.
enum Keyed<'a> {
    Id3v2(&'a Tag),
    Mp4(&'a Tag),
    Vorbis(Option<&'a Tag>),
}

fn keyed(tagged: &TaggedFile) -> Keyed<'_> {
    if let Some(id3) = id3v2_tag(tagged).filter(|t| !t.is_empty()) {
        Keyed::Id3v2(id3)
    } else if let Some(mp4) = tagged.tag(TagType::Mp4Ilst).filter(|t| !t.is_empty()) {
        Keyed::Mp4(mp4)
    } else {
        Keyed::Vorbis(tagged.tag(TagType::VorbisComments).filter(|_| tagged.file_type() == FileType::Flac))
    }
}

/// The key a file's tags name, read where rekordbox reads it.
///
/// rekordbox 7.2.19 `TagLib::ParseTag::parseKey` @0x100e4d424 [OBS static]:
/// the `ID3v2` tag's `TKEY` when the file has a non-empty `ID3v2` tag (see
/// [`id3v2_tag`]); otherwise the MP4 atom [`MP4_KEY`]; otherwise the Vorbis
/// comment `INITIALKEY`. A non-empty `ID3v2` tag without a `TKEY` gives no
/// key; the other tags are not consulted, and a WAV gives none at all. The
/// text is kept as written: neither `parseKey`, `GetFirstTextFrame` nor
/// `analyze_tag` trims it. Import (`DatabaseMediator::addTrack` @0x101a1577c)
/// and Reload Tag (`DatabaseMediator::readTag` @0x100c459ec) both store it
/// through `convertTagData` @0x100c463e0, which copies the key when it is not
/// empty and never reads the tag's BPM, so a BPM tag is not read here either.
fn tag_key(tagged: &TaggedFile) -> String {
    let found = match keyed(tagged) {
        Keyed::Id3v2(id3) => id3.get_string(&ItemKey::InitialKey),
        Keyed::Mp4(mp4) => mp4.get_string(&ItemKey::Unknown(MP4_KEY.to_owned())),
        Keyed::Vorbis(vorbis) => vorbis.and_then(|t| t.get_string(&ItemKey::InitialKey)),
    };
    found.unwrap_or_default().to_owned()
}

/// The label, read where rekordbox reads it: `ParseTag::parseLabel`
/// @0x100e4cf7c [OBS static] takes `TPUB` from a non-empty `ID3v2` tag, else
/// `----:com.apple.iTunes:LABEL` from a non-empty MP4 tag, else the Vorbis
/// comment `LABEL` or `ORGANIZATION`. An APE tag, an `ID3v1` and RIFF INFO
/// give none.
fn tag_label(tagged: &TaggedFile) -> String {
    let found = match keyed(tagged) {
        Keyed::Id3v2(tag) | Keyed::Mp4(tag) => tag.get_string(&ItemKey::Label),
        Keyed::Vorbis(vorbis) => vorbis.and_then(|t| t.get_string(&ItemKey::Label)),
    };
    found.unwrap_or_default().to_owned()
}

/// The tags rekordbox reads a file's title, artist, album, genre and comment
/// from (first), and its year and track number from (second), in order: a
/// field comes from the first of them that has it.
///
/// rekordbox 7.2.19 `TagLib::ParseTag::initialize` @0x100e4a9cc [OBS static]:
/// - a WAV reads RIFF INFO alone (`WAV::File::InfoTag`, which `TagLib` makes
///   empty when the file has none), never its `id3 ` chunk;
/// - a FLAC reads its Vorbis comments alone (`FLAC::File::xiphComment`);
/// - an MP3 or AIFF with an `ID3v2` tag (see [`id3v2_tag`]) reads the text
///   fields from that tag alone (`GetFirstTextFrame` on `TIT2`, `TPE1`,
///   `TALB`, `parseID3v2Comments`, `GetID3V2GenreTextFrame`), so an `ID3v1`
///   does not fill a field the `ID3v2` lacks; the year and track number come
///   from `File::tag()`, which for an MP3 is `TagLib`'s union of its `ID3v2`,
///   APE and `ID3v1` tags, first non-empty value wins;
/// - anything else reads everything from `File::tag()`: an M4A its MP4 tag,
///   an MP3 without an `ID3v2` the union of its APE and `ID3v1` tags.
fn field_sources(tagged: &TaggedFile) -> (Vec<&Tag>, Vec<&Tag>) {
    let only = |kind: TagType| tagged.tag(kind).into_iter().collect::<Vec<_>>();
    let union = |kinds: &[TagType]| kinds.iter().filter_map(|k| tagged.tag(*k)).collect::<Vec<_>>();
    match tagged.file_type() {
        FileType::Wav => (only(TagType::RiffInfo), only(TagType::RiffInfo)),
        FileType::Flac => (only(TagType::VorbisComments), only(TagType::VorbisComments)),
        FileType::Mp4 => (only(TagType::Mp4Ilst), only(TagType::Mp4Ilst)),
        FileType::Aiff => (only(TagType::Id3v2), only(TagType::Id3v2)),
        FileType::Mpeg => {
            let numbers = union(&[TagType::Id3v2, TagType::Ape, TagType::Id3v1]);
            match id3v2_tag(tagged) {
                Some(id3) => (vec![id3], numbers),
                None => (union(&[TagType::Ape, TagType::Id3v1]), numbers),
            }
        }
        // Formats rekordbox's ParseTag has no branch for: the primary tag, or
        // the first there is, as before.
        _ => {
            let one: Vec<&Tag> = tagged.primary_tag().or_else(|| tagged.first_tag()).into_iter().collect();
            (one.clone(), one)
        }
    }
}

/// `djmdContent.FileType` for a file, by extension: what rekordbox writes on
/// 38,681 reference rows [OBS] — 1 on every `.mp3`, 4 on `.m4a`, 5 on `.flac`,
/// 11 on `.wav`, 12 on `.aiff`/`.aif`. Anything else is unseen and left unset.
#[must_use]
pub fn file_type(path: &Path) -> Option<i64> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "mp3" => Some(1),
        "m4a" => Some(4),
        "flac" => Some(5),
        "wav" => Some(11),
        "aiff" | "aif" => Some(12),
        _ => None,
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("{0} is not a file")]
    NotAFile(String),
    #[error("{extension:?} is not a format rekordbox plays")]
    UnsupportedFormat { extension: String },
    #[error("could not read {path}: {reason}")]
    Unreadable { path: String, reason: String },
}

/// Whether a path looks like something worth importing.
#[must_use]
pub fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.as_str()))
}

/// Reads embedded artwork separately from the fast Explorer tag probe.
/// Prefer a front cover across all tags, then the first available picture.
pub fn read_artwork(path: &Path) -> Result<Option<Vec<u8>>, ImportError> {
    let tagged = Probe::open(path)
        .map(|probe| probe.options(ParseOptions::new().read_properties(false)))
        .and_then(Probe::read)
        .map_err(|e| ImportError::Unreadable {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;
    let pictures = || tagged.tags().iter().flat_map(lofty::tag::Tag::pictures);
    Ok(pictures()
        .find(|p| p.pic_type() == lofty::picture::PictureType::CoverFront)
        .or_else(|| pictures().next())
        .map(|p| p.data().to_vec()))
}

/// Reads a file's tags.
///
/// A missing title falls back to the file's own name rather than being left
/// empty: a library row with no title is unusable, and the filename is what
/// the person actually recognises.
pub fn read_tags(path: &Path) -> Result<TrackTags, ImportError> {
    if !path.is_file() {
        return Err(ImportError::NotAFile(path.display().to_string()));
    }
    if !is_audio(path) {
        return Err(ImportError::UnsupportedFormat {
            extension: path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default()
                .to_owned(),
        });
    }

    // Without the cover art. Nothing here stores a picture, and a picture is
    // most of a tag by size: reading it made one probe cost 15 ms on a local
    // disk and 56 ms cold from an SD card against under a millisecond
    // without [OBS], which is the difference between an Explorer page that
    // lands and one that stalls.
    let tagged = Probe::open(path)
        .map(|probe| probe.options(ParseOptions::new().read_cover_art(false)))
        .and_then(lofty::probe::Probe::read)
        .map_err(|e| ImportError::Unreadable {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;

    let properties = tagged.properties();
    let mut tags = TrackTags {
        duration_sec: u32::try_from(properties.duration().as_secs()).unwrap_or(0),
        bitrate: properties.audio_bitrate().unwrap_or(0),
        sample_rate: properties.sample_rate().unwrap_or(0),
        file_size: std::fs::metadata(path).map_or(0, |m| m.len()),
        bit_depth: properties.bit_depth().unwrap_or(16),
        ..TrackTags::default()
    };

    // Each field from the tag rekordbox takes it from; see [`field_sources`].
    let (text_from, numbers_from) = field_sources(&tagged);
    let text = |key: &ItemKey| {
        text_from
            .iter()
            .find_map(|tag| tag.get_string(key).filter(|v| !v.is_empty()))
            .unwrap_or_default()
            .to_owned()
    };
    tags.title = text(&ItemKey::TrackTitle);
    tags.artist = text(&ItemKey::TrackArtist);
    tags.album = text(&ItemKey::AlbumTitle);
    tags.genre = text(&ItemKey::Genre);
    tags.comment = text(&ItemKey::Comment);
    tags.label = tag_label(&tagged);
    tags.year = numbers_from
        .iter()
        .find_map(|tag| {
            tag.get_string(&ItemKey::RecordingDate)
                .and_then(|v| v.get(..4).and_then(|y| y.parse().ok()))
                .or_else(|| tag.get_string(&ItemKey::Year).and_then(|v| v.parse().ok()))
                .filter(|&y: &u16| y != 0)
        })
        .unwrap_or(0);
    tags.track_no = numbers_from
        .iter()
        .find_map(|tag| {
            tag.get_string(&ItemKey::TrackNumber)
                // "3/12" is a legal track number; take the part before the slash.
                .and_then(|v| v.split('/').next().and_then(|n| n.trim().parse().ok()))
                .filter(|&n: &u16| n != 0)
        })
        .unwrap_or(0);
    tags.key = tag_key(&tagged);

    if tags.title.is_empty() {
        tags.title = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
    }
    Ok(tags)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn audio_files_are_recognised_by_extension() {
        for good in ["a.mp3", "a.M4A", "a.flac", "a.aiff", "a.wav"] {
            assert!(is_audio(Path::new(good)), "{good}");
        }
        for bad in ["a.txt", "a.jpg", "a", "a.mp4", "a.mp3.txt"] {
            assert!(!is_audio(Path::new(bad)), "{bad}");
        }
    }

    #[test]
    fn a_file_that_is_not_there_is_refused_before_anything_is_read() {
        let outcome = read_tags(Path::new("/no/such/file.mp3"));
        assert!(matches!(outcome, Err(ImportError::NotAFile(_))));
    }

    #[test]
    fn a_format_rekordbox_cannot_play_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.txt");
        std::fs::write(&path, b"x").unwrap();
        assert!(matches!(
            read_tags(&path),
            Err(ImportError::UnsupportedFormat { .. })
        ));
    }

    use lofty::config::WriteOptions;

    /// Forty silent MPEG-1 Layer III frames: 128 kbps, 44.1 kHz, 417 bytes
    /// each, enough for the probe to find a stream.
    fn write_mp3(path: &Path) {
        let mut out = Vec::new();
        for _ in 0..40 {
            let start = out.len();
            out.extend_from_slice(&[0xFF, 0xFB, 0x90, 0x64]);
            out.resize(start + 417, 0);
        }
        std::fs::write(path, out).unwrap();
    }

    /// One second of 16-bit mono silence.
    fn write_wav(path: &Path) {
        let rate = 44_100_u32;
        let data_len = rate * 2;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16_u32.to_le_bytes());
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2).to_le_bytes());
        out.extend_from_slice(&2_u16.to_le_bytes());
        out.extend_from_slice(&16_u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        out.resize(44 + data_len as usize, 0);
        std::fs::write(path, out).unwrap();
    }

    /// A tenth of a second of 16-bit mono silence as AIFF.
    fn write_aiff(path: &Path) {
        let frames = 4_410_u32;
        let mut comm = Vec::new();
        comm.extend_from_slice(&1_u16.to_be_bytes());
        comm.extend_from_slice(&frames.to_be_bytes());
        comm.extend_from_slice(&16_u16.to_be_bytes());
        // 44,100 as an 80-bit extended float.
        comm.extend_from_slice(&[0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0]);
        let mut ssnd = vec![0; 8];
        ssnd.resize(8 + frames as usize * 2, 0);
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
    }

    /// A tenth of a second of silent AAC in an M4A, made by `afconvert`, with
    /// no tags of its own worth reading.
    fn write_m4a(path: &Path) {
        std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/silent.m4a"), path).unwrap();
    }

    fn save_tag(path: &Path, kind: TagType, items: &[(ItemKey, &str)]) {
        let mut tag = Tag::new(kind);
        for (key, value) in items {
            tag.insert_text(key.clone(), (*value).to_owned());
        }
        tag.save_to_path(path, WriteOptions::default()).unwrap();
    }

    /// A FLAC stream with the given Vorbis comments and no audio frames:
    /// 44.1 kHz, mono, 16-bit, one second. Written by hand, since lofty's
    /// writer wants real frames to write around.
    fn write_flac(path: &Path, comments: &[&str]) {
        let mut out = b"fLaC".to_vec();
        // STREAMINFO (type 0), 34 bytes, not the last block.
        out.extend_from_slice(&[0x00, 0, 0, 34]);
        out.extend_from_slice(&4096_u16.to_be_bytes());
        out.extend_from_slice(&4096_u16.to_be_bytes());
        out.extend_from_slice(&[0; 6]);
        // 20 bits rate (44,100), 3 bits channels - 1 (0), 5 bits depth - 1
        // (15), 36 bits samples (44,100).
        let packed: u64 = (0xAC44 << 44) | (0xF << 36) | 0xAC44;
        out.extend_from_slice(&packed.to_be_bytes());
        out.extend_from_slice(&[0; 16]);
        // VORBIS_COMMENT (type 4), the last block: little-endian lengths.
        let mut block = Vec::new();
        block.extend_from_slice(&4_u32.to_le_bytes());
        block.extend_from_slice(b"test");
        block.extend_from_slice(&u32::try_from(comments.len()).unwrap().to_le_bytes());
        for comment in comments {
            block.extend_from_slice(&u32::try_from(comment.len()).unwrap().to_le_bytes());
            block.extend_from_slice(comment.as_bytes());
        }
        let len = u32::try_from(block.len()).unwrap().to_be_bytes();
        out.extend_from_slice(&[0x84, len[1], len[2], len[3]]);
        out.extend_from_slice(&block);
        std::fs::write(path, out).unwrap();
    }

    // Which tag each field comes from: rekordbox 7.2.19
    // `TagLib::ParseTag::initialize` (see `field_sources`).

    #[test]
    fn an_mp3_with_an_id3v2_takes_its_text_from_it_and_its_numbers_from_any_tag() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("two tags.mp3");
        write_mp3(&path);
        save_tag(&path, TagType::Id3v1, &[
            (ItemKey::TrackTitle, "Old Title"),
            (ItemKey::TrackArtist, "Only In V1"),
            (ItemKey::AlbumTitle, "V1 Album"),
            (ItemKey::Year, "2019"),
            (ItemKey::TrackNumber, "4"),
        ]);
        save_tag(&path, TagType::Id3v2, &[(ItemKey::TrackTitle, "New Title")]);

        let tags = read_tags(&path).unwrap();
        // TIT2/TPE1/TALB from the ID3v2 alone: the ID3v1 artist is not used.
        assert_eq!(tags.title, "New Title");
        assert_eq!((tags.artist.as_str(), tags.album.as_str()), ("", ""));
        // Year and track number from TagLib's tag union: the ID3v1 fills in.
        assert_eq!((tags.year, tags.track_no), (2019, 4));
    }

    #[test]
    fn an_mp3_without_an_id3v2_takes_each_field_from_its_ape_then_its_id3v1() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no v2.mp3");
        write_mp3(&path);
        save_tag(&path, TagType::Id3v1, &[
            (ItemKey::TrackTitle, "V1 Title"),
            (ItemKey::TrackArtist, "V1 Artist"),
        ]);
        save_tag(&path, TagType::Ape, &[(ItemKey::TrackTitle, "Ape Title"), (ItemKey::Label, "Ape Label")]);

        let tags = read_tags(&path).unwrap();
        assert_eq!((tags.title.as_str(), tags.artist.as_str()), ("Ape Title", "V1 Artist"));
        // parseLabel reads no APE tag.
        assert_eq!(tags.label, "");
    }

    #[test]
    fn a_wav_reads_its_riff_info_and_not_its_id3_chunk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("both.wav");
        write_wav(&path);
        save_tag(&path, TagType::RiffInfo, &[
            (ItemKey::TrackTitle, "Info Title"),
            (ItemKey::TrackArtist, "Info Artist"),
            (ItemKey::TrackNumber, "2"),
        ]);
        save_tag(&path, TagType::Id3v2, &[
            (ItemKey::TrackTitle, "Id3 Title"),
            (ItemKey::AlbumTitle, "Id3 Album"),
            (ItemKey::Label, "Id3 Label"),
            (ItemKey::InitialKey, "2A"),
        ]);

        let tags = read_tags(&path).unwrap();
        assert_eq!((tags.title.as_str(), tags.artist.as_str()), ("Info Title", "Info Artist"));
        assert_eq!((tags.album.as_str(), tags.label.as_str(), tags.key.as_str()), ("", "", ""));
        assert_eq!(tags.track_no, 2);
    }

    #[test]
    fn a_wav_tagged_only_in_an_id3_chunk_reads_as_untagged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Id3 Only.wav");
        write_wav(&path);
        save_tag(&path, TagType::Id3v2, &[(ItemKey::TrackTitle, "Id3 Title"), (ItemKey::TrackArtist, "A")]);
        let tags = read_tags(&path).unwrap();
        assert_eq!((tags.title.as_str(), tags.artist.as_str()), ("Id3 Only", ""));
    }

    #[test]
    fn an_aiff_reads_its_id3v2() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tagged.aiff");
        write_aiff(&path);
        save_tag(&path, TagType::Id3v2, &[
            (ItemKey::TrackTitle, "Aiff Title"),
            (ItemKey::TrackArtist, "Aiff Artist"),
            (ItemKey::Label, "Aiff Label"),
            (ItemKey::InitialKey, "11B"),
        ]);
        let tags = read_tags(&path).unwrap();
        assert_eq!((tags.title.as_str(), tags.artist.as_str()), ("Aiff Title", "Aiff Artist"));
        assert_eq!((tags.label.as_str(), tags.key.as_str()), ("Aiff Label", "11B"));
    }

    #[test]
    fn a_file_with_one_tag_reads_as_before() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plain.mp3");
        write_mp3(&path);
        save_tag(&path, TagType::Id3v2, &[
            (ItemKey::TrackTitle, "Title"),
            (ItemKey::TrackArtist, "Artist"),
            (ItemKey::Label, "Label"),
            (ItemKey::RecordingDate, "2021-05-01"),
            (ItemKey::TrackNumber, "3/12"),
        ]);
        let tags = read_tags(&path).unwrap();
        assert_eq!((tags.title.as_str(), tags.artist.as_str()), ("Title", "Artist"));
        assert_eq!(tags.label, "Label");
        assert_eq!((tags.year, tags.track_no), (2021, 3));

        let untagged = dir.path().join("No Tags.mp3");
        write_mp3(&untagged);
        let tags = read_tags(&untagged).unwrap();
        assert_eq!((tags.title.as_str(), tags.artist.as_str(), tags.key.as_str()), ("No Tags", "", ""));
    }

    // The key: rekordbox 7.2.19 `TagLib::ParseTag::parseKey` (see `tag_key`).

    #[test]
    fn the_key_comes_from_an_id3v2_tkey_as_written() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = dir.path().join("keyed.mp3");
        write_mp3(&mp3);
        save_tag(&mp3, TagType::Id3v2, &[(ItemKey::TrackTitle, "T"), (ItemKey::InitialKey, "2A")]);
        assert_eq!(read_tags(&mp3).unwrap().key, "2A");

        // Not trimmed or rewritten: parseKey copies the frame's text.
        let odd = dir.path().join("odd.mp3");
        write_mp3(&odd);
        save_tag(&odd, TagType::Id3v2, &[(ItemKey::TrackTitle, "T"), (ItemKey::InitialKey, " F#m ")]);
        assert_eq!(read_tags(&odd).unwrap().key, " F#m ");
    }

    #[test]
    fn a_flac_takes_its_key_and_label_from_its_vorbis_comments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keyed.flac");
        write_flac(&path, &["TITLE=T", "INITIALKEY=Am", "ORGANIZATION=Org"]);
        let tags = read_tags(&path).unwrap();
        assert_eq!((tags.title.as_str(), tags.key.as_str(), tags.label.as_str()), ("T", "Am", "Org"));
    }

    #[test]
    fn an_m4a_takes_its_key_from_the_itunes_key_atom_only() {
        let dir = tempfile::tempdir().unwrap();
        let keyed = dir.path().join("keyed.m4a");
        write_m4a(&keyed);
        let mut ilst = lofty::mp4::Ilst::default();
        lofty::tag::Accessor::set_title(&mut ilst, "M4A Title".to_owned());
        let freeform = |name: &'static str, value: &str| {
            lofty::mp4::Atom::new(
                lofty::mp4::AtomIdent::Freeform { mean: "com.apple.iTunes".into(), name: name.into() },
                lofty::mp4::AtomData::UTF8(value.to_owned()),
            )
        };
        ilst.insert(freeform("LABEL", "M4A Label"));
        ilst.insert(freeform("KEY", "8A"));
        ilst.save_to_path(&keyed, WriteOptions::default()).unwrap();
        let tags = read_tags(&keyed).unwrap();
        assert_eq!((tags.title.as_str(), tags.label.as_str(), tags.key.as_str()), ("M4A Title", "M4A Label", "8A"));

        // Mixed In Key's `----:com.apple.iTunes:initialkey` is not read.
        let mik = dir.path().join("mik.m4a");
        write_m4a(&mik);
        save_tag(&mik, TagType::Mp4Ilst, &[(ItemKey::TrackTitle, "T"), (ItemKey::InitialKey, "8A")]);
        assert_eq!(read_tags(&mik).unwrap().key, "");
    }

    #[test]
    fn a_bpm_tag_does_not_become_a_key_and_no_key_tag_means_no_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bpm only.mp3");
        write_mp3(&path);
        save_tag(&path, TagType::Id3v2, &[(ItemKey::TrackTitle, "T"), (ItemKey::Bpm, "128")]);
        assert_eq!(read_tags(&path).unwrap().key, "");
    }

    #[test]
    fn a_file_with_no_id3v2_does_not_take_a_key_from_its_ape_tag() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ape.mp3");
        write_mp3(&path);
        save_tag(&path, TagType::Ape, &[(ItemKey::TrackArtist, "A"), (ItemKey::InitialKey, "Am")]);
        let tags = read_tags(&path).unwrap();
        assert_eq!((tags.artist.as_str(), tags.key.as_str()), ("A", ""));
    }

    #[test]
    fn a_file_that_is_not_really_audio_fails_rather_than_panicking() {
        // The extension says mp3; the bytes do not.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lying.mp3");
        std::fs::write(&path, b"this is not an mp3").unwrap();
        assert!(matches!(read_tags(&path), Err(ImportError::Unreadable { .. })));
    }
}
