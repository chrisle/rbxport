//! Opening audio files for symphonia, with AIFF-C files whose compression
//! type names plain PCM by a code symphonia 0.5 does not accept.
//!
//! symphonia 0.5.5 (`symphonia-format-riff`, `aiff/chunks.rs`
//! `parse_aifc`) accepts `NONE`/`none`, `twos`, `sowt`, `fl32`, `fl64`,
//! `alaw` and `ulaw` and refuses every other code with "aifc: Compression
//! type not implemented" (issue #527). Several writers label the same
//! uncompressed samples with other codes: libsndfile, and so Audacity, writes
//! 32-bit float AIFF as `FL32` and 64-bit as `FL64`; Apple's audio software
//! labels 24- and 32-bit big-endian integer PCM `in24` and `in32`. The samples
//! are laid out exactly as under the code symphonia knows, so the file is
//! read through a view that shows the known code in place of the alias. The
//! file itself is never written.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use symphonia::core::io::MediaSource;

/// Chunks walked before giving up on finding `COMM`. A real file has a
/// handful ahead of it (`FVER`, sometimes `ID3 ` or `APPL`).
const MAX_CHUNKS: usize = 64;

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

/// Where an AIFF-C file's compression type is and what it says, or `None`
/// for anything else. Leaves `file` wherever it stopped.
fn find_compression(file: &mut File) -> io::Result<Option<(u64, [u8; 4], u16)>> {
    let mut header = [0_u8; 12];
    if file.read_exact(&mut header).is_err() || &header[0..4] != b"FORM" || &header[8..12] != b"AIFC" {
        return Ok(None);
    }
    let mut at = 12_u64;
    for _ in 0..MAX_CHUNKS {
        let mut chunk = [0_u8; 8];
        file.seek(SeekFrom::Start(at))?;
        if file.read_exact(&mut chunk).is_err() {
            return Ok(None);
        }
        let size = u64::from(u32::from_be_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]));
        let data = at + 8;
        if &chunk[0..4] == b"COMM" {
            // channels (2), frames (4), sample size (2), rate (10), then
            // the compression type.
            if size < 22 {
                return Ok(None);
            }
            let mut sample_size = [0_u8; 2];
            file.seek(SeekFrom::Start(data + 6))?;
            file.read_exact(&mut sample_size)?;
            let mut code = [0_u8; 4];
            file.seek(SeekFrom::Start(data + 18))?;
            file.read_exact(&mut code)?;
            return Ok(Some((data + 18, code, u16::from_be_bytes(sample_size))));
        }
        // Chunks are padded to an even length.
        at = data + size + (size & 1);
    }
    Ok(None)
}

/// A file read through a view with four bytes replaced.
struct Patched {
    file: File,
    position: u64,
    at: u64,
    bytes: [u8; 4],
    len: Option<u64>,
}

impl Read for Patched {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.file.read(buf)?;
        let start = self.position;
        let end = start + read as u64;
        for (offset, byte) in (self.at..).zip(self.bytes) {
            if (start..end).contains(&offset) {
                if let Some(slot) = usize::try_from(offset - start).ok().and_then(|i| buf.get_mut(i)) {
                    *slot = byte;
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

/// Opens `path` for symphonia: the file as it is, or an AIFF-C file with an
/// aliased compression type shown under the code symphonia knows.
pub fn open(path: &Path) -> io::Result<(Box<dyn MediaSource>, Aifc)> {
    let mut file = File::open(path)?;
    let found = find_compression(&mut file)?;
    file.seek(SeekFrom::Start(0))?;
    let Some((at, code, bits)) = found else {
        return Ok((Box::new(file), Aifc::default()));
    };
    let Some((bytes, expected_bits)) = canonical(code) else {
        return Ok((Box::new(file), Aifc { compression: Some(code), aliased: false }));
    };
    if bits != expected_bits {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "AIFF-C compression type '{}' requires {expected_bits}-bit samples, COMM sample width is {bits} bits",
                code.escape_ascii()
            ),
        ));
    }
    let len = file.metadata().ok().map(|m| m.len());
    let source = Patched { file, position: 0, at, bytes, len };
    Ok((Box::new(source), Aifc { compression: Some(code), aliased: true }))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// A minimal AIFF-C file: FVER, COMM with `code`, SSND with `data`.
    fn aifc(code: [u8; 4], bits: u16, data: &[u8]) -> Vec<u8> {
        let mut comm = Vec::new();
        comm.extend_from_slice(&1_u16.to_be_bytes());
        comm.extend_from_slice(&u32::try_from(data.len() / usize::from(bits / 8)).unwrap().to_be_bytes());
        comm.extend_from_slice(&bits.to_be_bytes());
        comm.extend_from_slice(&[0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0]);
        comm.extend_from_slice(&code);
        comm.extend_from_slice(&[0, 0]);
        let mut body = b"AIFC".to_vec();
        body.extend_from_slice(b"FVER");
        body.extend_from_slice(&4_u32.to_be_bytes());
        body.extend_from_slice(&0xA280_5140_u32.to_be_bytes());
        body.extend_from_slice(b"COMM");
        body.extend_from_slice(&u32::try_from(comm.len()).unwrap().to_be_bytes());
        body.extend_from_slice(&comm);
        body.extend_from_slice(b"SSND");
        body.extend_from_slice(&u32::try_from(data.len() + 8).unwrap().to_be_bytes());
        body.extend_from_slice(&[0; 8]);
        body.extend_from_slice(data);
        let mut file = b"FORM".to_vec();
        file.extend_from_slice(&u32::try_from(body.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&body);
        file
    }

    fn read_all(path: &Path) -> (Vec<u8>, Aifc) {
        let (mut source, info) = open(path).unwrap();
        let mut bytes = Vec::new();
        // Odd-sized reads so the patch straddles read boundaries.
        let mut chunk = [0_u8; 7];
        loop {
            let n = source.read(&mut chunk).unwrap();
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..n]);
        }
        (bytes, info)
    }

    #[test]
    fn an_alias_is_shown_as_the_known_code_and_nothing_else_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("issue-527-in24.aif");
        let original = aifc(*b"in24", 24, &[1, 2, 3, 4, 5, 6]);
        std::fs::write(&path, &original).unwrap();

        let (seen, info) = read_all(&path);
        assert_eq!(info, Aifc { compression: Some(*b"in24"), aliased: true });
        let at = original.windows(4).position(|w| w == b"in24").unwrap();
        let mut expected = original.clone();
        expected[at..at + 4].copy_from_slice(b"NONE");
        assert_eq!(seen, expected);
        assert_eq!(std::fs::read(&path).unwrap(), original, "the file is never written");
    }

    #[test]
    fn a_seek_into_the_code_still_sees_the_known_code() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("issue-527-fl32.aif");
        let original = aifc(*b"FL32", 32, &[0; 8]);
        std::fs::write(&path, &original).unwrap();
        let at = original.windows(4).position(|w| w == b"FL32").unwrap();

        let (mut source, _) = open(&path).unwrap();
        source.seek(SeekFrom::Start(at as u64 + 2)).unwrap();
        let mut two = [0_u8; 2];
        source.read_exact(&mut two).unwrap();
        assert_eq!(&two, b"32");
        source.seek(SeekFrom::Start(at as u64)).unwrap();
        let mut four = [0_u8; 4];
        source.read_exact(&mut four).unwrap();
        assert_eq!(&four, b"fl32");
    }

    #[test]
    fn known_codes_and_plain_aiff_are_read_as_they_are() {
        let dir = tempfile::tempdir().unwrap();
        let sowt = dir.path().join("issue-527-sowt.aif");
        std::fs::write(&sowt, aifc(*b"sowt", 16, &[0; 4])).unwrap();
        assert_eq!(open(&sowt).unwrap().1, Aifc { compression: Some(*b"sowt"), aliased: false });

        let ima4 = dir.path().join("issue-527-ima4.aif");
        std::fs::write(&ima4, aifc(*b"ima4", 16, &[0; 4])).unwrap();
        let info = open(&ima4).unwrap().1;
        assert_eq!(info, Aifc { compression: Some(*b"ima4"), aliased: false });
        assert_eq!(
            info.describe("unsupported feature: aifc: Compression type not implemented"),
            "unsupported feature: aifc: Compression type not implemented (AIFF-C compression type 'ima4')"
        );

        let other = dir.path().join("issue-527.txt");
        std::fs::write(&other, b"not audio").unwrap();
        assert_eq!(open(&other).unwrap().1, Aifc::default());
    }
}
