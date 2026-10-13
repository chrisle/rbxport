//! Writing a track's key into its audio file's own tag, as rekordbox's
//! Preferences > Analysis > Key Analysis > "Write the value to the ID3 tag"
//! does.
//!
//! What rekordbox writes, and where:
//!
//! - **MP3**: the `ID3v2` tag's `TKEY` frame, the tag saved as `ID3v2.4`. The
//!   first `TKEY` is replaced; a file with none gets one, UTF-8. [OBS
//!   chris-win11, rekordbox 7.2.14, scratch copies; static 7.2.19
//!   `TagLib::WriteTag::setKey` @0x100e47a14 → `updateTextualFrame("TKEY")`,
//!   `WriteTag::save` → `MPEG::File::save(ID3v2, false, 4)`]
//! - **AIFF**: the same `TKEY` frame, in the file's `ID3 ` chunk. [OBS, same
//!   path: `WriteTag::open` takes `RIFF::AIFF::File::tag()`]
//! - **FLAC**: the Vorbis comment `INITIALKEY`, first value replaced. [OBS;
//!   static `updateVorbisCommentFirstField("INITIALKEY")` @0x100e46924]
//! - **M4A/MP4 and WAV**: nothing. `setKey` returns without writing when the
//!   file is MP4, and a WAV has no `ID3v2` tag open to write to. [OBS: both
//!   files came back from rekordbox without a key; static @0x100e47a8c]
//!
//! A file rekordbox cannot write to is passed over without a word
//! (`Metadata::update` @0x1009e3630 checks `File::hasWriteAccess` and
//! returns). This passes it over too, judged by opening it for writing rather
//! than by its permission bits, and says so to the caller.
//!
//! rekordbox rewrites the file in place through `TagLib`. This never does: the
//! new file is built beside the original, read back and compared with it —
//! the audio byte for byte, every other tag item as it was — and only then
//! renamed over it, with the original's owner, group and creation date. A
//! file whose tag cannot be read strictly is left alone, since rewriting a
//! tag that was only partly understood is how other frames get lost; so is
//! an ID3 tag that repeats a frame lofty cannot keep twice (two `TKEY`s, two
//! `COMM`s with one description), a file with other hard links (a
//! replacement would split them off, where the in-place write keeps them),
//! and a file another user owns that cannot be handed back to them.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use lofty::config::{ParseOptions, ParsingMode, WriteOptions};
use lofty::file::{AudioFile, FileType};
use lofty::id3::v2::{Frame, FrameId, Id3v2Tag, TextInformationFrame};
use lofty::prelude::TagExt;
use lofty::TextEncoding;

/// What happened to the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyTagOutcome {
    /// The key is in the file's tag.
    Written,
    /// A format rekordbox writes no key into (M4A, WAV, anything else).
    Unsupported,
    /// The file is read-only; rekordbox passes such a file over too.
    ReadOnly,
}

/// Why a key could not be written. The file is unchanged in every case.
#[derive(Debug, thiserror::Error)]
pub enum KeyTagError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("the tag could not be read safely: {0}")]
    Tag(String),
    #[error("the rewritten file did not match the original, so it was not kept: {0}")]
    Verify(String),
}

impl From<lofty::error::LoftyError> for KeyTagError {
    fn from(e: lofty::error::LoftyError) -> Self {
        Self::Tag(e.to_string())
    }
}

type Result<T> = std::result::Result<T, KeyTagError>;

/// The formats rekordbox writes a key into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Mp3,
    Aiff,
    Flac,
}

/// One file at a time: an analysis and a key edit of the same track must
/// not both build a replacement from the same original.
static WRITING: Mutex<()> = Mutex::new(());

const TKEY: &str = "TKEY";
const INITIALKEY: &str = "INITIALKEY";

/// Writes `key` into the tag of the audio file at `path`, the way rekordbox
/// does with "Write the value to the ID3 tag" on. An empty `key` empties the
/// key, as clearing it in rekordbox's Info panel does [OBS: an empty `TKEY`].
///
/// # Errors
///
/// The file could not be read or replaced, its tag could not be read
/// strictly, or the rewritten copy did not check out. The original is
/// untouched in every case.
pub fn write_key(path: &Path, key: &str) -> Result<KeyTagOutcome> {
    let _one = WRITING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    // The file itself: a symlink is followed, so the link stays a link.
    let path = fs::canonicalize(path)?;
    let meta = fs::metadata(&path)?;
    if !meta.is_file() {
        return Err(KeyTagError::Io(io::Error::new(io::ErrorKind::InvalidInput, "not a file")));
    }
    if !can_write(&path, &meta)? {
        return Ok(KeyTagOutcome::ReadOnly);
    }
    let Some(format) = format_of(&path)? else {
        return Ok(KeyTagOutcome::Unsupported);
    };
    if links(&meta) > 1 {
        // rekordbox writes in place, so every name of the file sees the key.
        // A replacement would leave the other names on the old file.
        return Err(KeyTagError::Tag("the file has other hard links, which a replacement would split off".into()));
    }
    remove_stale_temps(&path);
    let temp = temp_beside(&path);
    let result = replace_via(&path, &temp, format, key, &meta);
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.map(|()| KeyTagOutcome::Written)
}

/// The key as the file now holds it: `TKEY` or `INITIALKEY`, or `None` for a
/// format with no key tag or a file without one.
///
/// # Errors
///
/// The file could not be read.
pub fn read_key(path: &Path) -> Result<Option<String>> {
    let Some(format) = format_of(path)? else { return Ok(None) };
    match format {
        Format::Mp3 | Format::Aiff => {
            let tag = read_id3(path, format)?;
            Ok(tag.as_ref().and_then(tkey).map(|(text, _)| text))
        }
        Format::Flac => {
            let flac = read_flac(&mut BufReader::new(File::open(path)?))?;
            Ok(flac.comments().and_then(|c| c.first_value(INITIALKEY)))
        }
    }
}

fn format_of(path: &Path) -> Result<Option<Format>> {
    let probe = lofty::probe::Probe::open(path)?.guess_file_type()?;
    Ok(match probe.file_type() {
        Some(FileType::Mpeg) => Some(Format::Mp3),
        Some(FileType::Aiff) => Some(Format::Aiff),
        Some(FileType::Flac) => Some(Format::Flac),
        _ => None,
    })
}

/// Whether this user may write the file, as rekordbox's `File::hasWriteAccess`
/// (`access(W_OK)`) asks. The permission bits alone do not say: a `0644` file
/// owned by someone else has write bits and still cannot be written, and a
/// rename over it would replace it anyway, since that needs only the folder.
/// So the file is opened for writing, without truncating or writing to it.
fn can_write(path: &Path, meta: &fs::Metadata) -> Result<bool> {
    if meta.permissions().readonly() {
        return Ok(false);
    }
    match OpenOptions::new().write(true).open(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// How many names the file has.
fn links(meta: &fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.nlink()
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        1
    }
}

const TEMP_TAG: &str = ".rbx-key-";

/// A name in the same directory, so the rename is a rename and not a copy.
fn temp_beside(path: &Path) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    path.with_file_name(format!(".{name}{TEMP_TAG}{}-{stamp}.tmp", std::process::id()))
}

/// Removes this file's replacements left behind by a crash between the copy
/// and the rename: names of exactly `temp_beside`'s shape for this file, last
/// changed over an hour ago, so a write still in progress elsewhere is safe.
fn remove_stale_temps(path: &Path) {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else { return };
    let prefix = format!(".{}{TEMP_TAG}", name.to_string_lossy());
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let entry_name = entry.file_name();
        let entry_name = entry_name.to_string_lossy();
        let Some(rest) = entry_name.strip_prefix(&prefix).and_then(|r| r.strip_suffix(".tmp")) else { continue };
        let ours = rest.split_once('-').is_some_and(|(pid, stamp)| {
            !pid.is_empty()
                && !stamp.is_empty()
                && pid.bytes().all(|b| b.is_ascii_digit())
                && stamp.bytes().all(|b| b.is_ascii_digit())
        });
        let stale = entry
            .metadata()
            .ok()
            .filter(fs::Metadata::is_file)
            .and_then(|m| m.modified().ok())
            .and_then(|m| m.elapsed().ok())
            .is_some_and(|age| age > std::time::Duration::from_secs(3600));
        if ours && stale {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn replace_via(path: &Path, temp: &Path, format: Format, key: &str, meta: &fs::Metadata) -> Result<()> {
    // A copy first, whatever the format: it carries the file's permissions
    // and, where the platform copies them, its attributes and extended
    // attributes, which a file created from nothing would not have.
    fs::copy(path, temp)?;
    match format {
        Format::Mp3 | Format::Aiff => write_id3(path, temp, format, key)?,
        Format::Flac => write_flac(path, temp, key)?,
    }
    verify(path, temp, format, key)?;
    let file = OpenOptions::new().write(true).open(temp)?;
    keep_created(&file, meta);
    keep_owner(&file, meta)?;
    file.sync_all()?;
    drop(file);
    fs::rename(temp, path)?;
    sync_parent(path);
    Ok(())
}

/// The creation date is the original file's: a key is not a new file, and
/// rekordbox's in-place write does not change it.
fn keep_created(file: &File, meta: &fs::Metadata) {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    if let Ok(created) = meta.created() {
        #[cfg(target_os = "macos")]
        use std::os::macos::fs::FileTimesExt;
        #[cfg(target_os = "windows")]
        use std::os::windows::fs::FileTimesExt;
        let _ = file.set_times(fs::FileTimes::new().set_created(created));
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = (file, meta);
}

/// The replacement belongs to whoever the original did, as it does after
/// rekordbox's in-place write. The group is set where the user may (they
/// belong to it); a file owned by another user that this user may still
/// write (mode `0666`, say) cannot be given back to them without root, so it
/// is not replaced. A group the user is not in cannot be kept either; that is
/// logged, and the file is still replaced, since its owner keeps it.
fn keep_owner(file: &File, meta: &fs::Metadata) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{fchown, MetadataExt};
        let mine = file.metadata()?;
        if mine.uid() != meta.uid() {
            let _ = fchown(file, Some(meta.uid()), Some(meta.gid()));
        } else if mine.gid() != meta.gid() {
            let _ = fchown(file, None, Some(meta.gid()));
        }
        let now = file.metadata()?;
        if now.uid() != meta.uid() {
            return Err(KeyTagError::Tag("the file belongs to another user, and a replacement would take it from them".into()));
        }
        if now.gid() != meta.gid() {
            // Not refused: the owner may still read and write it, as before.
            tracing::warn!(from = meta.gid(), to = now.gid(), "the key tag write could not keep the file's group");
        }
    }
    #[cfg(not(unix))]
    let _ = (file, meta);
    Ok(())
}

fn sync_parent(path: &Path) {
    #[cfg(unix)]
    if let Some(dir) = path.parent() {
        if let Ok(dir) = File::open(dir) {
            let _ = dir.sync_all();
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

// --- ID3 (MP3, AIFF) ------------------------------------------------------

fn parse_options() -> ParseOptions {
    // Strict: a frame lofty cannot read is an error rather than a frame
    // quietly left out of the rewritten tag.
    ParseOptions::new().parsing_mode(ParsingMode::Strict).read_properties(false)
}

fn read_id3(path: &Path, format: Format) -> Result<Option<Id3v2Tag>> {
    let mut file = BufReader::new(File::open(path)?);
    Ok(match format {
        Format::Mp3 => lofty::mpeg::MpegFile::read_from(&mut file, parse_options())?.id3v2().cloned(),
        Format::Aiff => lofty::iff::aiff::AiffFile::read_from(&mut file, parse_options())?.id3v2().cloned(),
        Format::Flac => None,
    })
}

/// The first `TKEY` frame's text and encoding.
fn tkey(tag: &Id3v2Tag) -> Option<(String, TextEncoding)> {
    tag.into_iter().find_map(|frame| match frame {
        Frame::Text(text) if text.id().as_str() == TKEY => Some((text.value.clone(), text.encoding)),
        _ => None,
    })
}

/// The raw `ID3v2` tag of the file, header included: at the start of an MP3,
/// in the `ID3 ` chunk of an AIFF.
fn raw_id3(path: &Path, format: Format) -> Result<Option<Vec<u8>>> {
    let mut file = BufReader::new(File::open(path)?);
    match format {
        Format::Mp3 => {
            let mut head = [0u8; 10];
            if file.read_exact(&mut head).is_err() || !head.starts_with(b"ID3") {
                return Ok(None);
            }
            let size = synchsafe(&head[6..10]);
            let mut tag = head.to_vec();
            tag.extend_from_slice(&read_exact_vec(&mut file, size)?);
            Ok(Some(tag))
        }
        Format::Aiff => {
            let len = file.get_ref().metadata()?.len();
            let mut at = 12u64;
            file.seek(SeekFrom::Start(at))?;
            while at + 8 <= len {
                let mut header = [0u8; 8];
                file.read_exact(&mut header)?;
                let size = u64::from(u32::from_be_bytes([header[4], header[5], header[6], header[7]]));
                if header[..4].eq_ignore_ascii_case(b"ID3 ") {
                    let n = usize::try_from(size).map_err(|_| KeyTagError::Tag("ID3 chunk too large".into()))?;
                    return Ok(Some(read_exact_vec(&mut file, n)?));
                }
                let padded = size + (size & 1);
                file.seek(SeekFrom::Current(i64::try_from(padded).map_err(|_| KeyTagError::Tag("chunk too large".into()))?))?;
                at += 8 + padded;
            }
            Ok(None)
        }
        Format::Flac => Ok(None),
    }
}

fn synchsafe(bytes: &[u8]) -> usize {
    bytes.iter().fold(0usize, |acc, b| (acc << 7) | usize::from(b & 0x7f))
}

/// The frame ids in a raw `ID3v2` tag, walked by hand. An error when the walk
/// does not come out clean: a frame running past the tag, an id that is not
/// one. Frames with no content are left out; there is nothing in them to lose.
fn raw_frame_ids(tag: &[u8]) -> Result<Vec<String>> {
    let bad = |what: &str| KeyTagError::Tag(format!("ID3v2: {what}"));
    let header = tag.get(..10).ok_or_else(|| bad("short header"))?;
    let version = header[3];
    let flags = header[5];
    let mut body = tag.get(10..).unwrap_or_default().to_vec();
    if version < 4 && flags & 0x80 != 0 {
        // Whole-tag unsynchronisation: FF 00 stands for FF.
        let mut undone = Vec::with_capacity(body.len());
        let mut previous = 0u8;
        for b in body {
            if !(previous == 0xFF && b == 0) {
                undone.push(b);
            }
            previous = b;
        }
        body = undone;
    }
    let mut at = 0usize;
    if flags & 0x40 != 0 {
        match version {
            3 => at = 4 + body.get(..4).map_or(0, |s| s.iter().fold(0usize, |acc, b| (acc << 8) | usize::from(*b))),
            4 => at = synchsafe(body.get(..4).ok_or_else(|| bad("short extended header"))?),
            _ => return Err(bad("compressed ID3v2.2 tag")),
        }
    }
    let (id_len, header_len) = if version == 2 { (3, 6) } else { (4, 10) };
    let mut ids = Vec::new();
    while at + header_len <= body.len() {
        let frame = body.get(at..at + header_len).ok_or_else(|| bad("short frame header"))?;
        if frame[0] == 0 {
            break; // padding
        }
        let id = frame.get(..id_len).ok_or_else(|| bad("short frame id"))?;
        if !id.iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) {
            return Err(bad("a frame id that is not one"));
        }
        let size_bytes = frame.get(id_len..id_len + if version == 2 { 3 } else { 4 }).unwrap_or_default();
        let size = if version == 4 {
            synchsafe(size_bytes)
        } else {
            size_bytes.iter().fold(0usize, |acc, b| (acc << 8) | usize::from(*b))
        };
        if at + header_len + size > body.len() {
            return Err(bad("a frame runs past the end of the tag"));
        }
        if size > 1 {
            ids.push(String::from_utf8_lossy(id).into_owned());
        }
        at += header_len + size;
    }
    Ok(ids)
}

/// Whether lofty's reading of the tag holds every frame the raw walk found,
/// by id or by the id lofty upgrades it to, as many times as the raw tag
/// has it.
///
/// lofty keeps one frame per key as it reads (the id for text and URL
/// frames, the description for `TXXX`/`WXXX`, the language and description
/// for `COMM`/`USLT`, ...): a later frame with the same key replaces the
/// earlier one. A tag with two `COMM eng ""` frames, or two `TKEY`s, would
/// lose one in the rewrite. `TagLib` keeps them all, and rekordbox replaces
/// only the first `TKEY`; this cannot, so it leaves such a file alone.
fn lofty_read_everything(raw: &[u8], tag: Option<&Id3v2Tag>) -> Result<()> {
    use std::collections::HashMap;
    let version = raw.get(3).copied().unwrap_or(4);
    let mut kept: HashMap<&str, usize> = HashMap::new();
    for frame in tag.into_iter().flatten() {
        *kept.entry(frame.id_str()).or_default() += 1;
    }
    let mut found: HashMap<String, usize> = HashMap::new();
    for id in raw_frame_ids(raw)? {
        // lofty folds an ID3v2.3 date and time into the v2.4 TDRC (or keeps
        // one it cannot fold as it is).
        if matches!(id.as_str(), "TDAT" | "TIME" | "TRDA") {
            if !kept.contains_key(id.as_str()) && !kept.contains_key("TDRC") {
                return Err(KeyTagError::Tag(format!("the {id} frame could not be read")));
            }
            *found.entry(id).or_default() += 1;
            continue;
        }
        let upgraded = match version {
            2 => lofty::id3::v2::upgrade_v2(&id),
            3 => lofty::id3::v2::upgrade_v3(&id),
            _ => None,
        };
        // An id lofty keeps as it is, or the one it upgrades it to.
        let as_kept = if kept.contains_key(id.as_str()) { id.clone() } else { upgraded.map_or(id.clone(), str::to_owned) };
        if !kept.contains_key(as_kept.as_str()) {
            return Err(KeyTagError::Tag(format!("the {id} frame could not be read")));
        }
        *found.entry(as_kept).or_default() += 1;
    }
    for (id, count) in &found {
        let folded = matches!(id.as_str(), "TDAT" | "TIME" | "TRDA");
        let has = kept.get(id.as_str()).copied().unwrap_or(0);
        if (folded && *count > 1) || (!folded && *count > has) {
            return Err(KeyTagError::Tag(format!(
                "the tag repeats a {id} frame ({count} in the file, {} told apart), and rewriting it would lose the repeat",
                if folded { 1 } else { has }
            )));
        }
    }
    Ok(())
}

fn write_id3(original: &Path, temp: &Path, format: Format, key: &str) -> Result<()> {
    let tag = read_id3(original, format)?;
    if let Some(raw) = raw_id3(original, format)? {
        lofty_read_everything(&raw, tag.as_ref())?;
    }
    let mut tag = tag.unwrap_or_default();
    // TagLib's `setText` keeps the replaced frame's encoding; a new frame
    // is UTF-8, as `Frame::createTextualFrame` makes it [OBS: UTF-8 TKEY].
    let encoding = tkey(&tag).map_or(TextEncoding::UTF8, |(_, encoding)| encoding);
    // `insert` replaces the first frame with the same id and keeps the rest.
    tag.insert(Frame::Text(TextInformationFrame::new(FrameId::Valid(TKEY.into()), encoding, key.to_owned())));
    let mut file = OpenOptions::new().read(true).write(true).open(temp)?;
    file.seek(SeekFrom::Start(0))?;
    tag.save_to(&mut file, WriteOptions::default())?;
    file.flush()?;
    Ok(())
}

/// Each frame as something comparable: its id and everything in it.
fn frame_prints(tag: Option<&Id3v2Tag>) -> Vec<String> {
    let mut prints: Vec<String> = tag
        .into_iter()
        .flatten()
        .map(|frame| format!("{frame:?}"))
        .collect();
    prints.sort();
    prints
}

// --- FLAC -----------------------------------------------------------------

const FLAC_STREAMINFO: u8 = 0;
const FLAC_PADDING: u8 = 1;
const FLAC_VORBIS_COMMENT: u8 = 4;
const FLAC_MAX_BLOCK: usize = 0x00FF_FFFF;

struct FlacBlock {
    kind: u8,
    data: Vec<u8>,
}

struct Flac {
    /// Anything before `fLaC`: an `ID3v2` tag some tools put there, kept as it is.
    prefix: Vec<u8>,
    blocks: Vec<FlacBlock>,
    /// Where the audio frames begin.
    audio_at: u64,
}

struct Comments {
    vendor: Vec<u8>,
    fields: Vec<Vec<u8>>,
}

impl Flac {
    fn comments(&self) -> Option<Comments> {
        self.blocks.iter().find(|b| b.kind == FLAC_VORBIS_COMMENT).and_then(|b| Comments::parse(&b.data))
    }
}

impl Comments {
    fn parse(data: &[u8]) -> Option<Self> {
        let mut at = 0usize;
        let mut take = |n: usize| -> Option<&[u8]> {
            let slice = data.get(at..at.checked_add(n)?)?;
            at += n;
            Some(slice)
        };
        let len = |b: &[u8]| -> Option<usize> { usize::try_from(u32::from_le_bytes(b.try_into().ok()?)).ok() };
        let vendor_len = len(take(4)?)?;
        let vendor = take(vendor_len)?.to_vec();
        let count = len(take(4)?)?;
        let mut fields = Vec::with_capacity(count.min(4096));
        for _ in 0..count {
            let n = len(take(4)?)?;
            fields.push(take(n)?.to_vec());
        }
        // Trailing bytes (an Ogg framing bit some writers leave) are not ours
        // to keep or drop silently.
        (at == data.len()).then_some(Self { vendor, fields })
    }

    fn render(&self) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        let n = |len: usize| u32::try_from(len).ok().map(u32::to_le_bytes);
        out.extend_from_slice(&n(self.vendor.len())?);
        out.extend_from_slice(&self.vendor);
        out.extend_from_slice(&n(self.fields.len())?);
        for field in &self.fields {
            out.extend_from_slice(&n(field.len())?);
            out.extend_from_slice(field);
        }
        Some(out)
    }

    fn is_named(field: &[u8], name: &str) -> bool {
        field.len() > name.len()
            && field.get(name.len()) == Some(&b'=')
            && field.get(..name.len()).is_some_and(|n| n.eq_ignore_ascii_case(name.as_bytes()))
    }

    fn first_value(&self, name: &str) -> Option<String> {
        self.fields
            .iter()
            .find(|f| Self::is_named(f, name))
            .and_then(|f| f.get(name.len() + 1..))
            .map(|v| String::from_utf8_lossy(v).into_owned())
    }

    /// The first `name` field takes `value`; a comment with none gets one at
    /// the end. Every other field stays where it was.
    fn set_first(&mut self, name: &str, value: &str) {
        let field = format!("{name}={value}").into_bytes();
        match self.fields.iter_mut().find(|f| Self::is_named(f, name)) {
            Some(existing) => *existing = field,
            None => self.fields.push(field),
        }
    }
}

fn read_exact_vec<R: Read>(reader: &mut R, n: usize) -> io::Result<Vec<u8>> {
    let mut buf = vec![0u8; n];
    reader.read_exact(&mut buf)?;
    Ok(buf)
}

fn read_flac<R: Read + Seek>(reader: &mut R) -> Result<Flac> {
    let bad = |what: &str| KeyTagError::Tag(format!("FLAC: {what}"));
    let mut prefix = Vec::new();
    let mut magic = read_exact_vec(reader, 4)?;
    if magic.starts_with(b"ID3") {
        let mut rest = read_exact_vec(reader, 6)?;
        magic.append(&mut rest);
        let size = magic.get(6..10).ok_or_else(|| bad("short ID3 header"))?;
        let synchsafe = size.iter().fold(0usize, |acc, b| (acc << 7) | usize::from(b & 0x7f));
        let footer = if magic.get(5).is_some_and(|flags| flags & 0x10 != 0) { 10 } else { 0 };
        let mut body = read_exact_vec(reader, synchsafe + footer)?;
        prefix.append(&mut magic);
        prefix.append(&mut body);
        magic = read_exact_vec(reader, 4)?;
    }
    if magic != b"fLaC" {
        return Err(bad("no fLaC marker"));
    }
    let mut blocks = Vec::new();
    loop {
        let header = read_exact_vec(reader, 4)?;
        let (first, size) = header.split_first().ok_or_else(|| bad("short block header"))?;
        let len = size.iter().fold(0usize, |acc, b| (acc << 8) | usize::from(*b));
        let data = read_exact_vec(reader, len)?;
        blocks.push(FlacBlock { kind: first & 0x7f, data });
        if first & 0x80 != 0 {
            break;
        }
        if blocks.len() > 1024 {
            return Err(bad("too many metadata blocks"));
        }
    }
    if blocks.first().map(|b| b.kind) != Some(FLAC_STREAMINFO) {
        return Err(bad("STREAMINFO is not the first block"));
    }
    let audio_at = reader.stream_position()?;
    Ok(Flac { prefix, blocks, audio_at })
}

fn write_flac(original: &Path, temp: &Path, key: &str) -> Result<()> {
    let mut source = BufReader::new(File::open(original)?);
    let mut flac = read_flac(&mut source)?;
    let mut comments = match flac.comments() {
        Some(comments) => comments,
        None if flac.blocks.iter().any(|b| b.kind == FLAC_VORBIS_COMMENT) => {
            return Err(KeyTagError::Tag("FLAC: the Vorbis comment block could not be read".into()));
        }
        None => Comments { vendor: b"rbxport".to_vec(), fields: Vec::new() },
    };
    comments.set_first(INITIALKEY, key);
    let rendered = comments.render().ok_or_else(|| KeyTagError::Tag("FLAC: comment too large".into()))?;
    let old_len = flac.blocks.iter().find(|b| b.kind == FLAC_VORBIS_COMMENT).map(|b| b.data.len());
    match flac.blocks.iter_mut().find(|b| b.kind == FLAC_VORBIS_COMMENT) {
        Some(block) => block.data = rendered,
        // A new comment goes straight after STREAMINFO, where TagLib puts it.
        None => flac.blocks.insert(1, FlacBlock { kind: FLAC_VORBIS_COMMENT, data: rendered }),
    }
    // Like TagLib, take a change in size out of the padding when there is
    // enough of it, so the audio stays where it was.
    // Sizes with the 4-byte block header, a missing block being 0.
    let old_total = old_len.map_or(0, |len| len + 4);
    let new_total = flac.blocks.iter().find(|b| b.kind == FLAC_VORBIS_COMMENT).map_or(0, |b| b.data.len() + 4);
    if let Some(padding) = flac.blocks.iter_mut().find(|b| b.kind == FLAC_PADDING) {
        if new_total > old_total {
            let need = new_total - old_total;
            if padding.data.len() >= need {
                padding.data.truncate(padding.data.len() - need);
            }
        } else {
            padding.data.resize(padding.data.len() + (old_total - new_total), 0);
        }
    }
    let mut out = OpenOptions::new().write(true).truncate(true).open(temp)?;
    {
        let mut writer = io::BufWriter::new(&mut out);
        writer.write_all(&flac.prefix)?;
        writer.write_all(b"fLaC")?;
        let last = flac.blocks.len().saturating_sub(1);
        for (i, block) in flac.blocks.iter().enumerate() {
            if block.data.len() > FLAC_MAX_BLOCK {
                return Err(KeyTagError::Tag("FLAC: metadata block too large".into()));
            }
            let last_bit = if i == last { 0x80 } else { 0 };
            let len = u32::try_from(block.data.len()).unwrap_or(0).to_be_bytes();
            writer.write_all(&[last_bit | block.kind, len[1], len[2], len[3]])?;
            writer.write_all(&block.data)?;
        }
        source.seek(SeekFrom::Start(flac.audio_at))?;
        io::copy(&mut source, &mut writer)?;
        writer.flush()?;
    }
    out.sync_all()?;
    Ok(())
}

// --- Checking the copy ------------------------------------------------------

fn verify(original: &Path, temp: &Path, format: Format, key: &str) -> Result<()> {
    let fail = |what: String| Err(KeyTagError::Verify(what));
    match format {
        Format::Mp3 | Format::Aiff => {
            let before = read_id3(original, format)?;
            let after = read_id3(temp, format)?;
            let Some(after) = after else { return fail("no tag".into()) };
            match tkey(&after) {
                Some((text, _)) if text == key => {}
                other => return fail(format!("TKEY reads back as {other:?}")),
            }
            // Every frame but the first TKEY as it was.
            let strip_first_tkey = |tag: Option<&Id3v2Tag>| {
                let mut prints = frame_prints(tag);
                if let Some(at) = tag.and_then(|t| t.into_iter().find(|f| f.id_str() == TKEY)).map(|f| format!("{f:?}"))
                    .and_then(|print| prints.iter().position(|p| *p == print))
                {
                    prints.remove(at);
                }
                prints
            };
            if strip_first_tkey(before.as_ref()) != strip_first_tkey(Some(&after)) {
                return fail("another frame changed".into());
            }
            match format {
                Format::Mp3 => same_mp3_audio(original, temp),
                _ => same_aiff_chunks(original, temp),
            }
        }
        Format::Flac => {
            let mut a = BufReader::new(File::open(original)?);
            let mut b = BufReader::new(File::open(temp)?);
            let before = read_flac(&mut a)?;
            let after = read_flac(&mut b)?;
            if before.prefix != after.prefix {
                return fail("the bytes before fLaC changed".into());
            }
            let others = |f: &Flac| -> Vec<(u8, Vec<u8>)> {
                f.blocks.iter()
                    .filter(|b| b.kind != FLAC_VORBIS_COMMENT && b.kind != FLAC_PADDING)
                    .map(|b| (b.kind, b.data.clone()))
                    .collect()
            };
            if others(&before) != others(&after) {
                return fail("a metadata block other than the comment changed".into());
            }
            let Some(comments) = after.comments() else { return fail("no comment".into()) };
            if comments.first_value(INITIALKEY).as_deref() != Some(key) {
                return fail("INITIALKEY reads back wrong".into());
            }
            let mut expected = before.comments().map(|c| c.fields).unwrap_or_default();
            if let Some(first) = expected.iter().position(|f| Comments::is_named(f, INITIALKEY)) {
                expected.remove(first);
            }
            let mut got = comments.fields;
            if let Some(first) = got.iter().position(|f| Comments::is_named(f, INITIALKEY)) {
                got.remove(first);
            }
            if expected != got {
                return fail("another comment changed".into());
            }
            same_tail(original, before.audio_at, temp, after.audio_at)
        }
    }
}

/// The length of the `ID3v2` tag at the start of an MP3, or 0.
fn id3_len(path: &Path) -> Result<u64> {
    let mut head = [0u8; 10];
    let mut file = File::open(path)?;
    if file.read_exact(&mut head).is_err() || !head.starts_with(b"ID3") {
        return Ok(0);
    }
    let size = head[6..10].iter().fold(0u64, |acc, b| (acc << 7) | u64::from(b & 0x7f));
    let footer = if head[5] & 0x10 != 0 { 10 } else { 0 };
    Ok(10 + size + footer)
}

fn same_mp3_audio(original: &Path, temp: &Path) -> Result<()> {
    same_tail(original, id3_len(original)?, temp, id3_len(temp)?)
}

/// Whether everything from `a_at` in `a` equals everything from `b_at` in `b`.
fn same_tail(first: &Path, first_at: u64, second: &Path, second_at: u64) -> Result<()> {
    let mut left = BufReader::with_capacity(1 << 16, File::open(first)?);
    let mut right = BufReader::with_capacity(1 << 16, File::open(second)?);
    left.seek(SeekFrom::Start(first_at))?;
    right.seek(SeekFrom::Start(second_at))?;
    let (mut left_buf, mut right_buf) = (vec![0u8; 1 << 16], vec![0u8; 1 << 16]);
    loop {
        let read_left = read_full(&mut left, &mut left_buf)?;
        let read_right = read_full(&mut right, &mut right_buf)?;
        if read_left != read_right || left_buf.get(..read_left) != right_buf.get(..read_right) {
            return Err(KeyTagError::Verify("the audio changed".into()));
        }
        if read_left == 0 {
            return Ok(());
        }
    }
}

fn read_full<R: Read>(reader: &mut R, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(buf.get_mut(filled..).unwrap_or_default())? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

/// An AIFF's chunks other than its ID3 chunk, in order.
fn aiff_chunks(path: &Path) -> Result<Vec<([u8; 4], Vec<u8>)>> {
    let bad = |what: &str| KeyTagError::Verify(format!("AIFF: {what}"));
    let mut file = BufReader::new(File::open(path)?);
    let len = file.get_ref().metadata()?.len();
    let mut head = [0u8; 12];
    file.read_exact(&mut head)?;
    if &head[0..4] != b"FORM" {
        return Err(bad("no FORM"));
    }
    let form = u64::from(u32::from_be_bytes([head[4], head[5], head[6], head[7]]));
    if form + 8 != len {
        return Err(bad("FORM size does not match the file"));
    }
    let mut chunks = Vec::new();
    let mut at = 12u64;
    while at + 8 <= len {
        let mut header = [0u8; 8];
        file.read_exact(&mut header)?;
        let id = [header[0], header[1], header[2], header[3]];
        let size = u64::from(u32::from_be_bytes([header[4], header[5], header[6], header[7]]));
        let padded = size + (size & 1);
        if at + 8 + padded > len {
            return Err(bad("a chunk runs past the end"));
        }
        if id.eq_ignore_ascii_case(b"ID3 ") {
            file.seek(SeekFrom::Current(i64::try_from(padded).map_err(|_| bad("chunk too large"))?))?;
        } else {
            let data = read_exact_vec(&mut file, usize::try_from(padded).map_err(|_| bad("chunk too large"))?)?;
            chunks.push((id, data));
        }
        at += 8 + padded;
    }
    Ok(chunks)
}

fn same_aiff_chunks(original: &Path, temp: &Path) -> Result<()> {
    if aiff_chunks(original)? != aiff_chunks(temp)? {
        return Err(KeyTagError::Verify("an AIFF chunk other than ID3 changed".into()));
    }
    Ok(())
}
