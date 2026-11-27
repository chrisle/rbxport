//! Opening audio files for symphonia, with two AIFF header fields shown as
//! symphonia 0.5 needs them. The file itself is never written.
//!
//! An AIFF-C file's compression type may name plain PCM by a code symphonia
//! does not accept. symphonia 0.5.5 (`symphonia-format-riff`,
//! `aiff/chunks.rs` `parse_aifc`) accepts `NONE`/`none`, `twos`, `sowt`,
//! `fl32`, `fl64`, `alaw` and `ulaw` and refuses every other code with
//! "aifc: Compression type not implemented" (issue #527). Several writers
//! label the same uncompressed samples with other codes: libsndfile, and so
//! Audacity, writes 32-bit float AIFF as `FL32` and 64-bit as `FL64`;
//! Apple's audio software labels 24- and 32-bit big-endian integer PCM
//! `in24` and `in32`. The samples are laid out exactly as under the code
//! symphonia knows, so the view shows the known code in place of the alias.
//!
//! The sound chunk (`SSND`) of every AIFF and AIFF-C file opens with an
//! offset and a block size, eight bytes ahead of the samples. symphonia
//! 0.5.5 (`aiff/chunks.rs` `SoundChunk::parse`) reads them but leaves them
//! in the length it hands the demuxer, which counts them as samples: it
//! declares frames the file does not have (four for 16-bit mono, one for
//! 24-bit stereo) and decodes up to eight bytes past the chunk, the header
//! of an `ID3 ` or marker chunk after it, as PCM. The view shows the declared
//! sound length without those eight bytes, so the demuxer ends where
//! the samples do. symphonia 0.6 subtracts them itself: this correction goes
//! when the dependency moves, or the length would be shortened twice.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use symphonia::core::io::MediaSource;

/// The offset and block size that open a sound chunk, ahead of its samples.
const SOUND_PREFIX: u32 = 8;

/// Where a FORM's chunks begin: after `FORM`, its length, and the form type.
const CHUNKS_START: u64 = 12;

/// What the header said about an AIFF-C file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Aifc {
    /// The file's own compression type, when it is an AIFF-C file with a
    /// `COMM` chunk that names one.
    pub compression: Option<[u8; 4]>,
    /// The compression type is an alias that is shown to symphonia as the
    /// code it knows. Such a file decodes, but a CDJ reading the same file
    /// sees the alias, so export converts it rather than copying it.
    pub aliased: bool,
}

impl Aifc {
    /// A decoding error, with the compression type when the file is an
    /// AIFF-C file symphonia still refuses, so a report names the code.
    pub fn describe(&self, error: impl std::fmt::Display) -> String {
        match self.compression {
            Some(code) if !self.aliased => {
                format!("{error} (AIFF-C compression type '{}')", code.escape_ascii())
            }
            _ => error.to_string(),
        }
    }
}

/// The code symphonia knows for the same sample layout, if `code` is an
/// alias for uncompressed PCM.
fn canonical(code: [u8; 4]) -> Option<([u8; 4], u16)> {
    match &code {
        b"FL32" => Some((*b"fl32", 32)),
        b"FL64" => Some((*b"fl64", 64)),
        // Big-endian signed integers, the layout AIFF itself uses; the
        // sample width must agree with the alias before showing `NONE`.
        b"in24" => Some((*b"NONE", 24)),
        b"in32" => Some((*b"NONE", 32)),
        _ => None,
    }
}

/// Where the header fields the view shows differently are, and what the
/// file says there.
#[derive(Default)]
struct Header {
    /// An AIFF-C file's compression type: where it is, what it says, and
    /// the `COMM` chunk's sample width.
    compression: Option<(u64, [u8; 4], u16)>,
    /// The sound chunk length field's location and the length it declares
    /// for samples after excluding the offset and block-size fields.
    sound: Option<(u64, u32)>,
}

/// Walks AIFF chunk headers within the limits accepted by Symphonia 0.5.5.
/// Looks for the AIFF-C compression code and the first sound header without
/// reading samples. A missing or unsupported sound prefix leaves its length
/// field unchanged. Leaves `file` wherever it stopped.
fn scan(file: &mut File) -> io::Result<Header> {
    let mut found = Header::default();
    let mut header = [0_u8; 12];
    if file.read_exact(&mut header).is_err() || &header[0..4] != b"FORM" {
        return Ok(found);
    }
    let aifc = match &header[8..12] {
        b"AIFC" => true,
        b"AIFF" => false,
        _ => return Ok(found),
    };
    // The FORM length counts from byte 8. Symphonia's ChunksReader skips the
    // form type before counting, so it accepts chunks ending up to four
    // bytes beyond the declared boundary.
    let form_len = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);
    let end = CHUNKS_START + u64::from(form_len);
    // channels (2), frames (4), sample size (2), rate (10), then in AIFF-C
    // the compression type (4).
    let least_comm = if aifc { 22 } else { 18 };
    let mut sound_seen = false;
    let mut at = CHUNKS_START;
    // A header that does not fit is where symphonia reports no sound chunk.
    while at + 8 <= end {
        let mut chunk = [0_u8; 8];
        file.seek(SeekFrom::Start(at))?;
        if file.read_exact(&mut chunk).is_err() {
            // The file ends inside the header, where symphonia fails too.
            break;
        }
        let declared = u32::from_be_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
        let size = u64::from(declared);
        let data = at + 8;
        if data + size > end {
            // symphonia refuses the file at this chunk, or (the FORM and the
            // chunk both at `u32::MAX`) reads it to the end of the file:
            // nothing after it is looked at, and a sound chunk is left as is.
            break;
        }
        match &chunk[0..4] {
            b"COMM" => {
                if size < least_comm {
                    return Ok(Header::default());
                }
                if aifc && found.compression.is_none() {
                    let mut sample_size = [0_u8; 2];
                    file.seek(SeekFrom::Start(data + 6))?;
                    file.read_exact(&mut sample_size)?;
                    let mut code = [0_u8; 4];
                    file.seek(SeekFrom::Start(data + 18))?;
                    file.read_exact(&mut code)?;
                    found.compression = Some((data + 18, code, u16::from_be_bytes(sample_size)));
                }
            }
            b"SSND" if !sound_seen => {
                sound_seen = true;
                // Only a chunk symphonia takes as it is: long enough to hold
                // the prefix (it is inside the FORM, checked above) and with
                // the zero offset and block size it accepts. Anything else
                // is shown as it is.
                let mut prefix = [0_u8; 8];
                if declared >= SOUND_PREFIX && file.read_exact(&mut prefix).is_ok() && prefix == [0; 8] {
                    found.sound = Some((at + 4, declared - SOUND_PREFIX));
                }
            }
            _ => {}
        }
        if sound_seen && (found.compression.is_some() || !aifc) {
            break;
        }
        // Chunks are padded to an even length.
        at = data + size + (size & 1);
    }
    Ok(found)
}

/// Four bytes of the file shown as something else.
struct Patch {
    at: u64,
    bytes: [u8; 4],
}

/// A file read through a view with a few four-byte fields replaced.
struct Patched {
    file: File,
    position: u64,
    patches: Vec<Patch>,
    len: Option<u64>,
}

impl Read for Patched {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.file.read(buf)?;
        let start = self.position;
        let end = start + read as u64;
        for patch in &self.patches {
            for (offset, byte) in (patch.at..).zip(patch.bytes) {
                if (start..end).contains(&offset) {
                    if let Some(slot) = usize::try_from(offset - start).ok().and_then(|i| buf.get_mut(i)) {
                        *slot = byte;
                    }
                }
            }
        }
        self.position = end;
        Ok(read)
    }
}

impl Seek for Patched {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        self.position = self.file.seek(from)?;
        Ok(self.position)
    }
}

impl MediaSource for Patched {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        self.len
    }
}

/// Opens `path` for symphonia: the file as it is, or an AIFF or AIFF-C file
/// through a view with an aliased compression type shown under the code
/// symphonia knows and the sound chunk's length shown without its prefix.
pub fn open(path: &Path) -> io::Result<(Box<dyn MediaSource>, Aifc)> {
    let file = File::open(path).inspect_err(|error| log_source_failure("file-open", error))?;
    prepare(file).inspect_err(|error| log_source_failure("source-header", error))
}

fn log_source_failure(operation: &str, error: &io::Error) {
    // Keep diagnostics useful without recording the path or arbitrary error text.
    tracing::warn!(operation, error_kind = ?error.kind(), os_error = ?error.raw_os_error(),
        "audio source preparation failed");
}

fn prepare(mut file: File) -> io::Result<(Box<dyn MediaSource>, Aifc)> {
    let header = scan(&mut file)?;
    file.seek(SeekFrom::Start(0))?;
    let mut info = Aifc::default();
    let mut patches = Vec::new();
    if let Some((at, code, bits)) = header.compression {
        info.compression = Some(code);
        if let Some((bytes, expected_bits)) = canonical(code) {
            if bits != expected_bits {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "AIFF-C compression type '{}' requires {expected_bits}-bit samples, COMM sample width is {bits} bits",
                        code.escape_ascii()
                    ),
                ));
            }
            info.aliased = true;
            patches.push(Patch { at, bytes });
        }
    }
    if let Some((at, len)) = header.sound {
        patches.push(Patch { at, bytes: len.to_be_bytes() });
    }
    if patches.is_empty() {
        return Ok((Box::new(file), info));
    }
    let len = file.metadata().ok().map(|m| m.len());
    Ok((Box::new(Patched { file, position: 0, patches, len }), info))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn capture(action: impl FnOnce()) -> String {
        let log = tempfile::NamedTempFile::new().unwrap();
        let writer = log.reopen().unwrap();
        let subscriber = tracing_subscriber::fmt().without_time().with_ansi(false)
            .with_max_level(tracing::Level::WARN)
            .with_writer(move || writer.try_clone().unwrap()).finish();
        tracing::subscriber::with_default(subscriber, action);
        std::fs::read_to_string(log.path()).unwrap()
    }

    #[test]
    fn source_failure_stages_do_not_log_paths_or_change_errors() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("private-name-missing.aif");
        let original = File::open(&missing).err().unwrap();
        let logged = capture(|| {
            let error = open(&missing).err().unwrap();
            assert_eq!(error.kind(), original.kind());
            assert_eq!(error.raw_os_error(), original.raw_os_error());
            assert_eq!(error.to_string(), original.to_string());
        });
        assert!(logged.contains("operation=\"file-open\""), "{logged}");
        assert!(logged.contains("error_kind=NotFound"), "{logged}");
        assert!(logged.contains(&format!("os_error={:?}", original.raw_os_error())), "{logged}");
        assert_eq!(logged.lines().count(), 1);
        assert!(!logged.contains("private-name"));
        assert!(!logged.contains(&*dir.path().to_string_lossy()));

        let invalid = dir.path().join("private-name-invalid.aif");
        std::fs::write(&invalid, aifc(*b"in24", 16, &[0; 4])).unwrap();
        let logged = capture(|| {
            let error = open(&invalid).err().unwrap();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert_eq!(error.raw_os_error(), None);
            assert_eq!(error.to_string(), "AIFF-C compression type 'in24' requires 24-bit samples, COMM sample width is 16 bits");
        });
        assert!(logged.contains("operation=\"source-header\""), "{logged}");
        assert!(logged.contains("error_kind=InvalidData"), "{logged}");
        assert_eq!(logged.lines().count(), 1);
        assert!(!logged.contains("private-name"));
        assert!(!logged.contains(&*dir.path().to_string_lossy()));
        assert!(!logged.contains("in24"), "no arbitrary error text is logged");

        let valid = dir.path().join("private-name-valid.aif");
        std::fs::write(&valid, aifc(*b"NONE", 16, &[0; 4])).unwrap();
        assert_eq!(capture(|| { assert!(open(&valid).is_ok()); }), "");
    }

    /// A chunk with its header, padded to an even length.
    fn chunk(name: [u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = name.to_vec();
        out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
        out.extend_from_slice(data);
        if data.len() % 2 == 1 {
            out.push(0);
        }
        out
    }

    /// A FORM of `kind` holding `chunks`, with the length it has.
    fn form(kind: [u8; 4], chunks: &[Vec<u8>]) -> Vec<u8> {
        let mut body = kind.to_vec();
        for chunk in chunks {
            body.extend_from_slice(chunk);
        }
        let mut file = b"FORM".to_vec();
        file.extend_from_slice(&u32::try_from(body.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&body);
        file
    }

    /// The `FVER` chunk an AIFF-C file opens with.
    fn fver() -> Vec<u8> {
        chunk(*b"FVER", &0xA280_5140_u32.to_be_bytes())
    }

    /// A 44.1 kHz `COMM` chunk of `frames` frames of `channels` `bits`-bit
    /// samples, with `code` as the compression type for an AIFF-C file.
    fn comm(channels: u16, bits: u16, frames: usize, code: Option<[u8; 4]>) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&channels.to_be_bytes());
        data.extend_from_slice(&u32::try_from(frames).unwrap().to_be_bytes());
        data.extend_from_slice(&bits.to_be_bytes());
        data.extend_from_slice(&[0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0]);
        if let Some(code) = code {
            data.extend_from_slice(&code);
            data.extend_from_slice(&[0, 0]);
        }
        chunk(*b"COMM", &data)
    }

    /// An `SSND` chunk: a zero offset and block size, then `data`.
    fn ssnd(data: &[u8]) -> Vec<u8> {
        let mut sound = vec![0; 8];
        sound.extend_from_slice(data);
        chunk(*b"SSND", &sound)
    }

    /// A minimal mono AIFF-C file: FVER, COMM with `code`, SSND with `data`.
    fn aifc(code: [u8; 4], bits: u16, data: &[u8]) -> Vec<u8> {
        let frames = data.len() / usize::from(bits / 8);
        form(*b"AIFC", &[fver(), comm(1, bits, frames, Some(code)), ssnd(data)])
    }

    /// A minimal mono AIFF file: COMM, SSND with `data`, then a chunk of
    /// metadata, the first eight bytes of which symphonia took for samples.
    fn aiff(bits: u16, data: &[u8]) -> Vec<u8> {
        let frames = data.len() / usize::from(bits / 8);
        form(*b"AIFF", &[comm(1, bits, frames, None), ssnd(data), chunk(*b"ID3 ", &[0x11, 0x22, 0x33, 0x44])])
    }

    /// Where `needle` first appears in `bytes`.
    fn find(bytes: &[u8], needle: &[u8]) -> usize {
        bytes.windows(needle.len()).position(|w| w == needle).unwrap()
    }

    /// `original` as the view shows it: the compression type `alias` as
    /// `known` when given, and the sound chunk's length without its prefix.
    fn expected_view(original: &[u8], code: Option<([u8; 4], [u8; 4])>) -> Vec<u8> {
        let mut expected = original.to_vec();
        if let Some((alias, known)) = code {
            let at = find(original, &alias);
            expected[at..at + 4].copy_from_slice(&known);
        }
        let len = find(original, b"SSND") + 4;
        let declared = u32::from_be_bytes(original[len..len + 4].try_into().unwrap());
        expected[len..len + 4].copy_from_slice(&(declared - 8).to_be_bytes());
        expected
    }

    fn read_all(path: &Path) -> (Vec<u8>, Aifc) {
        let (mut source, info) = open(path).unwrap();
        let mut bytes = Vec::new();
        // Odd-sized reads so the patches straddle read boundaries.
        let mut piece = [0_u8; 7];
        loop {
            let n = source.read(&mut piece).unwrap();
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&piece[..n]);
        }
        (bytes, info)
    }

    #[test]
    fn an_alias_is_shown_as_the_known_code_and_the_sound_length_without_its_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("issue-527-in24.aif");
        let original = aifc(*b"in24", 24, &[1, 2, 3, 4, 5, 6]);
        std::fs::write(&path, &original).unwrap();

        let (seen, info) = read_all(&path);
        assert_eq!(info, Aifc { compression: Some(*b"in24"), aliased: true });
        let expected = expected_view(&original, Some((*b"in24", *b"NONE")));
        assert_eq!(seen, expected);
        // Two fields and nothing else: the code, and a length of 14 shown as 6.
        let differences: Vec<usize> = (0..original.len()).filter(|&i| original[i] != expected[i]).collect();
        let code = find(&original, b"in24");
        let len = find(&original, b"SSND") + 4;
        assert_eq!(differences, [code, code + 1, code + 2, code + 3, len + 3]);
        assert_eq!(seen[len..len + 4], 6_u32.to_be_bytes());
        assert_eq!(std::fs::read(&path).unwrap(), original, "the file is never written");
    }

    #[test]
    fn a_seek_into_a_patched_field_still_sees_the_view() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("issue-527-fl32.aif");
        let original = aifc(*b"FL32", 32, &[0; 8]);
        std::fs::write(&path, &original).unwrap();
        let code = find(&original, b"FL32") as u64;
        let len = find(&original, b"SSND") as u64 + 4;

        let (mut source, _) = open(&path).unwrap();
        source.seek(SeekFrom::Start(code + 2)).unwrap();
        let mut two = [0_u8; 2];
        source.read_exact(&mut two).unwrap();
        assert_eq!(&two, b"32");
        source.seek(SeekFrom::Start(code)).unwrap();
        let mut four = [0_u8; 4];
        source.read_exact(&mut four).unwrap();
        assert_eq!(&four, b"fl32");
        // Into the sound chunk's length: 16 in the file, 8 in the view.
        source.seek(SeekFrom::Start(len + 3)).unwrap();
        let mut one = [0_u8; 1];
        source.read_exact(&mut one).unwrap();
        assert_eq!(one, [8]);
        source.seek(SeekFrom::Start(len)).unwrap();
        source.read_exact(&mut four).unwrap();
        assert_eq!(four, 8_u32.to_be_bytes());
        // And back over both fields from the start, in large reads.
        source.seek(SeekFrom::Start(0)).unwrap();
        let mut all = Vec::new();
        source.read_to_end(&mut all).unwrap();
        assert_eq!(all, expected_view(&original, Some((*b"FL32", *b"fl32"))));
    }

    #[test]
    fn plain_aiff_shows_the_sound_length_without_its_prefix() {
        let dir = tempfile::tempdir().unwrap();
        // One frame and five, 16-bit: the eight bytes were four frames more.
        let short: &[u8] = &[0x20, 0];
        let normal: &[u8] = &[0x20, 0, 0xC0, 0, 0, 0, 0x40, 0, 0xE0, 0];
        for (name, data) in [("short", short), ("normal", normal)] {
            let path = dir.path().join(format!("{name}.aif"));
            let original = aiff(16, data);
            std::fs::write(&path, &original).unwrap();
            let (seen, info) = read_all(&path);
            assert_eq!(info, Aifc::default(), "{name}");
            assert_eq!(seen, expected_view(&original, None), "{name}");
            let len = find(&seen, b"SSND") + 4;
            assert_eq!(seen[len..len + 4], u32::try_from(data.len()).unwrap().to_be_bytes(), "{name}");
            assert_eq!(std::fs::read(&path).unwrap(), original, "{name}: the file is never written");
        }
    }

    #[test]
    fn known_codes_are_not_aliased_and_only_the_sound_length_changes() {
        let dir = tempfile::tempdir().unwrap();
        let sowt = dir.path().join("issue-527-sowt.aif");
        let original = aifc(*b"sowt", 16, &[0; 4]);
        std::fs::write(&sowt, &original).unwrap();
        let (seen, info) = read_all(&sowt);
        assert_eq!(info, Aifc { compression: Some(*b"sowt"), aliased: false });
        assert_eq!(seen, expected_view(&original, None));

        // A code symphonia refuses is refused at the COMM chunk, before the
        // sound chunk's length is read, so the file fails as it did.
        let ima4 = dir.path().join("issue-527-ima4.aif");
        let original = aifc(*b"ima4", 16, &[0; 4]);
        std::fs::write(&ima4, &original).unwrap();
        let (seen, info) = read_all(&ima4);
        assert_eq!(info, Aifc { compression: Some(*b"ima4"), aliased: false });
        assert_eq!(seen, expected_view(&original, None));
        assert_eq!(
            info.describe("unsupported feature: aifc: Compression type not implemented"),
            "unsupported feature: aifc: Compression type not implemented (AIFF-C compression type 'ima4')"
        );

        let other = dir.path().join("issue-527.txt");
        std::fs::write(&other, b"not audio").unwrap();
        let (seen, info) = read_all(&other);
        assert_eq!(info, Aifc::default());
        assert_eq!(seen, b"not audio");
    }

    #[test]
    fn odd_chunks_are_padded_around_and_in_the_sound_chunk() {
        let dir = tempfile::tempdir().unwrap();
        // Odd-sized chunks before COMM and before SSND, 8-bit samples that
        // leave the sound chunk itself odd, and a chunk after it.
        let path = dir.path().join("padded.aif");
        let original = form(*b"AIFF", &[
            chunk(*b"ANNO", b"odd"),
            comm(1, 8, 5, None),
            chunk(*b"NAME", b"x"),
            ssnd(&[0x20, 0xC0, 0, 0x40, 0xE0]),
            chunk(*b"ID3 ", &[0x11, 0x22, 0x33, 0x44]),
        ]);
        std::fs::write(&path, &original).unwrap();
        let (seen, info) = read_all(&path);
        assert_eq!(info, Aifc::default());
        assert_eq!(seen, expected_view(&original, None));
        let len = find(&seen, b"SSND") + 4;
        assert_eq!(seen[len..len + 4], 5_u32.to_be_bytes());

        // The same in AIFF-C beside the alias, with the last chunk's pad
        // byte left off by the writer, as some leave it.
        let path = dir.path().join("padded.aifc");
        let mut original = form(*b"AIFC", &[fver(), chunk(*b"ANNO", b"odd"), comm(1, 24, 1, Some(*b"in24")), ssnd(&[1, 2, 3])]);
        assert_eq!(original.pop(), Some(0), "the pad byte");
        let form_len = u32::try_from(original.len() - 8).unwrap();
        original[4..8].copy_from_slice(&form_len.to_be_bytes());
        std::fs::write(&path, &original).unwrap();
        let (seen, info) = read_all(&path);
        assert_eq!(info, Aifc { compression: Some(*b"in24"), aliased: true });
        assert_eq!(seen, expected_view(&original, Some((*b"in24", *b"NONE"))));
        let len = find(&seen, b"SSND") + 4;
        assert_eq!(seen[len..len + 4], 3_u32.to_be_bytes());
    }

    #[test]
    fn a_sound_chunk_that_is_not_whole_is_shown_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let whole = aiff(16, &[0x20, 0, 0xC0, 0]);
        let ssnd = find(&whole, b"SSND");
        let mut cases = Vec::new();
        // Too short to hold its own offset and block size.
        let mut short = whole.clone();
        short[ssnd + 4..ssnd + 8].copy_from_slice(&4_u32.to_be_bytes());
        cases.push(("short", short));
        // Running five bytes past the end the FORM's length declares: past
        // the four symphonia reads anyway, so it refuses the file.
        let mut over = whole.clone();
        let chunk_end = ssnd + 8 + 12;
        over[4..8].copy_from_slice(&u32::try_from(chunk_end - 8 - 5).unwrap().to_be_bytes());
        cases.push(("over", over));
        // The FORM's length ends at the sound chunk's header: symphonia finds
        // no sound chunk, and the header is not read.
        let mut outside = whole.clone();
        outside[4..8].copy_from_slice(&u32::try_from(ssnd - 8).unwrap().to_be_bytes());
        cases.push(("outside", outside));
        // The file ends inside the chunk header, or inside the prefix.
        cases.push(("cut-header", whole[..ssnd + 6].to_vec()));
        cases.push(("cut-prefix", whole[..ssnd + 12].to_vec()));
        // An offset, or a block size, symphonia refuses.
        let mut offset = whole.clone();
        offset[ssnd + 11] = 2;
        cases.push(("offset", offset));
        let mut block = whole.clone();
        block[ssnd + 15] = 2;
        cases.push(("block", block));
        // The lengths a writer that could not seek back left at their
        // maximum, which symphonia reads to the end of the file.
        let mut streamed = whole.clone();
        streamed[4..8].copy_from_slice(&u32::MAX.to_be_bytes());
        streamed[ssnd + 4..ssnd + 8].copy_from_slice(&u32::MAX.to_be_bytes());
        cases.push(("streamed", streamed));
        for (name, bytes) in cases {
            let path = dir.path().join(format!("{name}.aif"));
            std::fs::write(&path, &bytes).unwrap();
            let (seen, info) = read_all(&path);
            assert_eq!(info, Aifc::default(), "{name}");
            assert_eq!(seen, bytes, "{name}: shown as it is");
        }

        // An alias is still shown as the known code when its sound chunk is
        // not whole, as it was before the length was looked at.
        let mut aliased = aifc(*b"in24", 24, &[1, 2, 3, 4, 5, 6]);
        let ssnd = find(&aliased, b"SSND");
        aliased[ssnd + 11] = 2;
        let path = dir.path().join("aliased-offset.aif");
        std::fs::write(&path, &aliased).unwrap();
        let (seen, info) = read_all(&path);
        assert_eq!(info, Aifc { compression: Some(*b"in24"), aliased: true });
        let code = find(&aliased, b"in24");
        let mut expected = aliased.clone();
        expected[code..code + 4].copy_from_slice(b"NONE");
        assert_eq!(seen, expected);
    }

    /// symphonia 0.5.5 reads a chunk ending as much as four bytes past the
    /// end the FORM's length declares, as its chunk reader does not count
    /// the form type: a file whose FORM length is short by that much decodes,
    /// so its sound chunk's length is corrected and it ends with its samples.
    /// Short by five, the file is refused as it was, so it is shown as it is.
    #[test]
    fn a_form_length_short_by_up_to_four_bytes_still_decodes_so_is_still_corrected() {
        let dir = tempfile::tempdir().unwrap();
        let whole = aiff(16, &[0x20, 0, 0xC0, 0]);
        let ssnd = find(&whole, b"SSND");
        // A FORM length covering the chunks up to the end of the sound chunk
        // exactly, the chunk after it outside.
        let exact = u32::try_from(ssnd + 8 + 12 - 8).unwrap();
        for short_by in 0..=5_u32 {
            let mut bytes = whole.clone();
            bytes[4..8].copy_from_slice(&(exact - short_by).to_be_bytes());
            let path = dir.path().join(format!("short-by-{short_by}.aif"));
            std::fs::write(&path, &bytes).unwrap();
            let (seen, info) = read_all(&path);
            assert_eq!(info, Aifc::default(), "short by {short_by}");
            if short_by <= 4 {
                assert_eq!(seen, expected_view(&bytes, None), "short by {short_by}");
                let audio = crate::decode_mono(&path, None).unwrap_or_else(|e| panic!("short by {short_by}: {e}"));
                assert_eq!(audio.samples, [0.25, -0.5], "short by {short_by}: two frames, not six");
            } else {
                assert_eq!(seen, bytes, "short by {short_by}: shown as it is");
                let error = crate::decode_mono(&path, None).unwrap_err().to_string();
                assert!(error.contains("riff: chunk length exceeds parent (list) chunk length"), "{error}");
            }
        }
    }

    #[test]
    fn a_file_without_a_whole_common_chunk_or_a_sound_chunk_is_shown_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        // A FORM whose length ends inside its COMM chunk: symphonia refuses
        // it there ("riff: chunk length exceeds parent (list) chunk length"),
        // so the walk stops there too, short of the sound chunk after it.
        let mut comm_past_form = form(*b"AIFF", &[comm(1, 16, 2, None), ssnd(&[0; 4])]);
        comm_past_form[4..8].copy_from_slice(&20_u32.to_be_bytes());
        let cases = [
            ("no-sound", form(*b"AIFF", &[comm(1, 16, 0, None)])),
            ("short-comm", form(*b"AIFF", &[chunk(*b"COMM", &[0; 10]), ssnd(&[0; 4])])),
            ("short-aifc-comm", form(*b"AIFC", &[fver(), chunk(*b"COMM", &[0; 20]), ssnd(&[0; 4])])),
            ("comm-past-form", comm_past_form),
            ("empty-form", form(*b"AIFF", &[])),
        ];
        for (name, bytes) in cases {
            let path = dir.path().join(format!("{name}.aif"));
            std::fs::write(&path, &bytes).unwrap();
            let (seen, info) = read_all(&path);
            assert_eq!(info, Aifc::default(), "{name}");
            assert_eq!(seen, bytes, "{name}: shown as it is");
        }
    }

    /// The demuxer ends where the samples do whatever the frame size: a
    /// stereo 16-bit file (two frames too many before) and a mono 24-bit one
    /// (also two, of three bytes), each with a chunk after the sound.
    #[test]
    fn decoding_ends_with_the_samples_not_with_the_chunk_after_them() {
        let dir = tempfile::tempdir().unwrap();
        let frames = 1_000_usize;
        // Left a ramp, right its inverse: the mono mix is silence, and the
        // chunk header that followed the samples is not.
        let mut stereo = Vec::new();
        for i in 0..frames {
            let value = i16::try_from(i * 20).unwrap();
            stereo.extend_from_slice(&value.to_be_bytes());
            stereo.extend_from_slice(&(-value).to_be_bytes());
        }
        let path = dir.path().join("stereo.aif");
        let file = form(*b"AIFF", &[comm(2, 16, frames, None), ssnd(&stereo), chunk(*b"ID3 ", &[0x7F; 8])]);
        std::fs::write(&path, file).unwrap();
        let mut stream = crate::MonoStream::open(&path).unwrap();
        assert_eq!(stream.expected_frames(), Some(frames as u64));
        let mut mono = Vec::new();
        while let Some(packet) = stream.next_chunk() {
            mono.extend_from_slice(packet);
        }
        assert_eq!(mono.len(), frames);
        assert!(mono.iter().all(|s| s.abs() < 1e-9));

        let mut wide = Vec::new();
        for i in 0..frames {
            wide.extend_from_slice(&(i32::try_from(i).unwrap() << 12).to_be_bytes()[1..]);
        }
        let path = dir.path().join("wide.aif");
        let file = form(*b"AIFF", &[comm(1, 24, frames, None), ssnd(&wide), chunk(*b"ID3 ", &[0x7F; 8])]);
        std::fs::write(&path, file).unwrap();
        let audio = crate::decode_mono(&path, None).unwrap();
        assert_eq!(audio.samples.len(), frames);
        let last = audio.samples[frames - 1];
        let expected = (999_i32 << 12) as f32 / 8_388_608.0;
        assert!((last - expected).abs() < 1e-6, "{last}");
    }
}
