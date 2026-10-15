//! XDJ-R1 Cbm DBH, mapped from stock rekordbox 6.8.7 and firmware 1.13.
//! [OBS] Static writer/reader evidence; generated output is not a stock capture.
//! See docs/reference/xdj-r1.md for evidence boundaries and safe deviations.
use crate::{ExportError, Result};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

pub const HEADER: usize = 0x9c5c;
pub const RECORD: usize = 0x9f8;
pub const DIRECTORY: usize = 0x1a0;
pub const TOC: usize = 0x18;
pub const FILES: [&str; 6] = [
    "CbmDB.DBH",
    "CbmTT.DBH",
    "CbmAR.DBH",
    "CbmAB.DBH",
    "CbmBP.DBH",
    "CbmPL.DBH",
];
pub const MAX_TRACKS: usize = 10_000;
pub const MAX_DIRECTORIES: usize = 10_000;

fn invalid(message: impl Into<String>) -> ExportError {
    ExportError::Conflict(format!("XDJ-R1: {}", message.into()))
}
fn number(n: usize) -> Result<u32> {
    u32::try_from(n).map_err(|_| invalid("database offset exceeds 32 bits"))
}
fn put32(b: &mut [u8], at: usize, n: u32) {
    b[at..at + 4].copy_from_slice(&n.to_le_bytes());
}
fn put16(b: &mut [u8], at: usize, n: u16) {
    b[at..at + 2].copy_from_slice(&n.to_le_bytes());
}
fn word(b: &[u8], at: usize) -> Result<u32> {
    let data = b
        .get(
            at..at
                .checked_add(4)
                .ok_or_else(|| invalid("offset overflow"))?,
        )
        .ok_or_else(|| invalid(format!("short u32 at {at:#x}")))?;
    Ok(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
}
fn short(b: &[u8], at: usize) -> Result<u16> {
    let data = b
        .get(
            at..at
                .checked_add(2)
                .ok_or_else(|| invalid("offset overflow"))?,
        )
        .ok_or_else(|| invalid(format!("short u16 at {at:#x}")))?;
    Ok(u16::from_le_bytes([data[0], data[1]]))
}
fn units(s: &str, limit: usize) -> Vec<u16> {
    let mut u: Vec<_> = s
        .encode_utf16()
        .take_while(|u| *u != 0)
        .take(limit)
        .collect();
    if u.last().is_some_and(|c| (0xd800..=0xdbff).contains(c)) {
        u.pop();
    }
    u
}
fn utf16(b: &mut [u8], at: usize, u: &[u16]) {
    for (i, c) in u.iter().enumerate() {
        put16(b, at + 2 * i, *c);
    }
}
/// [OBS] The compact helper deliberately excludes U+DFFF as a pair endpoint.
fn compact(u: &[u16]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < u.len() {
        let c = u[i];
        out.push(u8::try_from(c).unwrap_or(b'?'));
        if (0xd800..=0xdbff).contains(&c)
            && u.get(i + 1).is_some_and(|v| (0xdc00..0xdfff).contains(v))
        {
            i += 1;
        }
        i += 1;
    }
    out
}
fn name(b: &mut [u8], ascii: Option<usize>, wide: usize, value: &[u16]) -> Result<()> {
    b[wide] =
        u8::try_from(value.len() * 2).map_err(|_| invalid("display text exceeds 63 units"))?;
    utf16(b, wide + 1, value);
    if let Some(at) = ascii {
        let text = compact(value);
        b[at] = u8::try_from(text.len()).map_err(|_| invalid("compact text too long"))?;
        b[at + 1..at + 1 + text.len()].copy_from_slice(&text);
    }
    Ok(())
}
fn filename(s: &str) -> Vec<u16> {
    if let Some((stem, ext)) = s.rsplit_once('.') {
        let suffix = units(&format!(".{ext}"), 5);
        let mut result = units(stem, 63 - suffix.len());
        result.extend(suffix);
        result
    } else {
        units(s, 63)
    }
}
fn fold(u: u16) -> u16 {
    let u = match u {
        0xff01..=0xff5e => u - 0xfee0,
        0x3000 => 0x20,
        _ => u,
    };
    if (0x61..=0x7a).contains(&u) {
        u - 0x20
    } else {
        u
    }
}
fn weight(u: u16) -> u16 {
    match fold(u) {
        u @ 0x20..=0x2f => u + 0x3d,
        u @ 0x30..=0x39 => u + 0x23,
        u @ 0x3a..=0x40 => u + 0x33,
        u @ 0x41..=0x5a => u - 0x21,
        u @ 0x5b..=0x60 => u + 0x19,
        u => u,
    }
}
/// [OBS] Width/ASCII folding, UTF-16 code-unit weights, empty sorts last.
pub fn compare(a: &str, b: &str) -> Ordering {
    match (a.is_empty(), b.is_empty()) {
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        _ => a
            .encode_utf16()
            .map(weight)
            .cmp(b.encode_utf16().map(weight)),
    }
}
fn header(timestamp: u32) -> Vec<u8> {
    let mut b = vec![0; HEADER];
    put32(&mut b, 12, 0x2001);
    put32(&mut b, 16, timestamp);
    b
}

#[derive(Debug, Clone)]
pub struct Track {
    pub id: u32,
    pub audio: String,
    pub filename: String,
    pub title: String,
    pub artist: String,
    pub artist_id: u32,
    pub album: String,
    pub album_id: u32,
    pub genre: String,
    pub key: String,
    pub comment: String,
    pub artwork: String,
    pub track_number: u32,
    pub bpm: u32,
    pub hot_cue_auto_load: bool,
    pub bytes: u32,
    pub data_offset: u32,
    pub frames75: u32,
    pub rate: u16,
    pub format: u8,
    pub emphasis: bool,
}
#[derive(Debug, Clone)]
pub struct Playlist {
    pub id: u32,
    pub name: String,
    pub tracks: Vec<u32>,
}

/// No physical FAT addresses are written. Units are device blocks; 512 is default.
#[allow(
    clippy::too_many_lines,
    reason = "ordered format construction and validation; preserve evidence traceability"
)]
pub fn build(
    tracks: &[Track],
    playlists: &[Playlist],
    source_playlist_rows: usize,
    block: u32,
    timestamp: u32,
) -> Result<BTreeMap<String, Vec<u8>>> {
    if tracks.len() > MAX_TRACKS || block < 512 || block % 512 != 0 {
        return Err(invalid("track count or block size outside mapped bounds"));
    }
    let mut ids = BTreeSet::new();
    for t in tracks {
        if t.id == 0 || !ids.insert(t.id) {
            return Err(invalid("zero/duplicate content ID"));
        }
        if !matches!(t.format, 0x0d | 0x11..=0x21 | 0x30 | 0x31 | 0x41) {
            return Err(invalid("unsupported Cbm format code"));
        }
        if t.bytes == 0 || t.data_offset >= t.bytes {
            return Err(invalid(format!("invalid audio extent for {}", t.audio)));
        }
        if t.audio.contains('\0')
            || t.audio.encode_utf16().count() > 256
            || !t.audio.starts_with('/')
        {
            return Err(invalid(format!(
                "audio path exceeds Cbm bounds: {}",
                t.audio
            )));
        }
        if t.artwork.encode_utf16().count() > 260 || t.artwork.contains('\0') {
            return Err(invalid("artwork path exceeds Cbm bounds"));
        }
    }
    let mut ordered: Vec<_> = tracks.iter().collect();
    ordered.sort_by(|a, b| {
        compare(&a.audio, &b.audio).then_with(|| compare(&a.filename, &b.filename))
    });
    let mut contents = header(timestamp);
    let mut locations = BTreeMap::new();
    let mut directory = "";
    let mut position = 150_u32;
    for t in &ordered {
        let parent = t.audio.rsplit_once('/').map_or("", |(p, _)| p);
        if parent != directory {
            position = 150;
            directory = parent;
        }
        let blocks = t.bytes.div_ceil(block);
        locations.insert(t.id, (number(contents.len())?, *t, blocks));
        let mut r = vec![0; RECORD];
        put32(&mut r, 0, number(contents.len() + RECORD)?);
        put32(
            &mut r,
            4,
            position
                .checked_add(t.data_offset / block)
                .ok_or_else(|| invalid("logical position overflow"))?,
        );
        put32(&mut r, 8, position);
        put32(&mut r, 16, blocks);
        put32(&mut r, 20, 150);
        put32(
            &mut r,
            24,
            t.frames75
                .checked_add(150)
                .ok_or_else(|| invalid("duration overflow"))?,
        );
        put16(&mut r, 32, t.rate);
        r[34] = if t.emphasis { 0x51 } else { 0x41 };
        r[35] = t.format;
        r[0x132] = u8::from(t.hot_cue_auto_load);
        put32(&mut r, 0x134, t.data_offset % block);
        put32(&mut r, 0x138, t.bpm.saturating_add(5) / 10);
        name(&mut r, Some(0x32), 0x148, &filename(&t.filename))?;
        for (ascii, wide, s, limit) in [
            (Some(0x72), 0x1c7, t.title.as_str(), 63),
            (Some(0xb2), 0x246, &t.artist, 63),
            (Some(0xf2), 0x2c5, &t.album, 63),
            (None, 0x344, &t.genre, 63),
            (None, 0x3c3, &t.key, 6),
        ] {
            name(&mut r, ascii, wide, &units(s, limit))?;
        }
        // Safe deviation: do not reproduce stock's comment write into audio-path unit 252.
        for (length, data, s, limit, terminator) in [
            (0x3d0, 0x3d2, t.comment.as_str(), 255, false),
            (0x5d0, 0x5d2, t.audio.as_str(), 256, true),
            (0x7da, 0x7dc, t.artwork.as_str(), 260, false),
        ] {
            let u = units(s, limit);
            put16(
                &mut r,
                length,
                u16::try_from(u.len() + usize::from(terminator))
                    .map_err(|_| invalid("text length overflow"))?,
            );
            utf16(&mut r, data, &u);
        }
        put32(&mut r, 0x9e4, t.id);
        put32(&mut r, 0x9e8, t.frames75);
        put32(&mut r, 0x9ec, t.data_offset);
        put32(&mut r, 0x9f4, t.bytes);
        contents.extend(r);
        position = position
            .checked_add(blocks)
            .ok_or_else(|| invalid("block position overflow"))?;
    }
    let end = number(contents.len())?;
    put32(&mut contents, 4, end);
    contents.resize(contents.len() + 0x1000, 0);
    let mut title: Vec<_> = tracks.iter().collect();
    // Deterministic tie order is a safe deviation from stock's unstable heap sort.
    title.sort_by(|a, b| {
        compare(&a.title, &b.title).then_with(|| compare(&a.filename, &b.filename))
    });
    let mut groups: Vec<(String, u32, Vec<u32>)> = Vec::new();
    if !title.is_empty() {
        groups.push(("[ALL]".into(), 0, title.iter().map(|t| t.id).collect()));
        let mut last = None;
        for t in &title {
            let c = fold(t.title.encode_utf16().next().unwrap_or(0));
            let c = if (65..=90).contains(&c) { c } else { 48 };
            if last != Some(c) {
                groups.push((
                    if c == 48 {
                        "123".into()
                    } else {
                        char::from_u32(u32::from(c)).unwrap_or('?').to_string()
                    },
                    0,
                    Vec::new(),
                ));
                last = Some(c);
            }
            if let Some(g) = groups.last_mut() {
                g.2.push(t.id);
            }
        }
    }
    let mut out = BTreeMap::new();
    out.insert(FILES[0].into(), contents);
    out.insert(
        FILES[1].into(),
        category(&groups, &locations, block, timestamp, 0)?,
    );
    for (file, album) in [(FILES[2], false), (FILES[3], true)] {
        let mut names: Vec<_> = tracks
            .iter()
            .map(|t| {
                if album {
                    (t.album_id, t.album.as_str())
                } else {
                    (t.artist_id, t.artist.as_str())
                }
            })
            .filter(|(id, _)| *id != 0)
            .collect();
        names.sort_by(|a, b| compare(a.1, b.1).then(a.0.cmp(&b.0)));
        names.dedup();
        let mut groups: Vec<(String, u32, Vec<u32>)> = Vec::new();
        for (id, text) in names {
            if groups.last().is_none_or(|g| g.0 != text) {
                groups.push((text.into(), id, Vec::new()));
            }
            let mut members: Vec<_> = title
                .iter()
                .copied()
                .filter(|t| {
                    if album {
                        t.album_id == id
                    } else {
                        t.artist_id == id
                    }
                })
                .collect();
            if album {
                members.sort_by(|a, b| {
                    a.track_number
                        .cmp(&b.track_number)
                        .then_with(|| compare(&a.title, &b.title))
                        .then(a.id.cmp(&b.id))
                });
            }
            if let Some(g) = groups.last_mut() {
                g.2.extend(members.iter().map(|t| t.id));
            }
        }
        let unknown: Vec<_> = title
            .iter()
            .filter(|t| {
                if album {
                    t.album_id == 0
                } else {
                    t.artist_id == 0
                }
            })
            .map(|t| t.id)
            .collect();
        if !unknown.is_empty() {
            groups.push(("Unknown".into(), 0, unknown));
        }
        out.insert(
            file.into(),
            category(&groups, &locations, block, timestamp, 0)?,
        );
    }
    let mut bpm = BTreeMap::<u32, Vec<u32>>::new();
    for t in &title {
        if (50..=49_949).contains(&t.bpm) {
            bpm.entry((t.bpm + 50) / 100).or_default().push(t.id);
        }
    }
    let groups: Vec<_> = bpm
        .into_iter()
        .map(|(b, ids)| (format!("{b}bpm"), b * 100, ids))
        .collect();
    out.insert(
        FILES[4].into(),
        category(&groups, &locations, block, timestamp, 0)?,
    );
    let groups: Vec<_> = playlists
        .iter()
        .filter(|p| !p.name.is_empty())
        .map(|p| {
            (
                p.name.clone(),
                p.id,
                p.tracks
                    .iter()
                    .copied()
                    .filter(|id| ids.contains(id))
                    .collect(),
            )
        })
        .collect();
    out.insert(
        FILES[5].into(),
        category(
            &groups,
            &locations,
            block,
            timestamp,
            1000_u32.saturating_sub(number(source_playlist_rows)?),
        )?,
    );
    validate(&out)?;
    Ok(out)
}

type Locations<'a> = BTreeMap<u32, (u32, &'a Track, u32)>;
fn category(
    groups: &[(String, u32, Vec<u32>)],
    locations: &Locations<'_>,
    block: u32,
    time: u32,
    capacity: u32,
) -> Result<Vec<u8>> {
    if groups.len() > MAX_DIRECTORIES {
        return Err(invalid(
            "more than 10,000 directories would overwrite the zero sentinel",
        ));
    }
    let mut b = header(time);
    put32(&mut b, 8, capacity);
    for (i, (text, key, ids)) in groups.iter().enumerate() {
        if ids.len() > MAX_TRACKS {
            return Err(invalid("category has more than 10,000 entries"));
        }
        let first = if ids.is_empty() { 0 } else { number(b.len())? };
        let mut position = 150_u32;
        for id in ids {
            let (offset, t, blocks) = locations
                .get(id)
                .ok_or_else(|| invalid("category references missing content"))?;
            let mut toc = vec![0; TOC];
            put32(&mut toc, 0, *offset);
            put32(
                &mut toc,
                4,
                position
                    .checked_add(t.data_offset / block)
                    .ok_or_else(|| invalid("TOC position overflow"))?,
            );
            put32(&mut toc, 8, position);
            put32(&mut toc, 16, *blocks);
            put32(&mut toc, 20, *id);
            b.extend(toc);
            position = position
                .checked_add(*blocks)
                .ok_or_else(|| invalid("TOC block overflow"))?;
        }
        let offset = number(b.len())?;
        put32(&mut b, 20 + i * 4, offset);
        let mut d = vec![0; DIRECTORY];
        put32(&mut d, 0, first);
        put16(
            &mut d,
            12,
            u16::try_from(i + 1).map_err(|_| invalid("directory ID overflow"))?,
        );
        put16(&mut d, 14, u16::from(!ids.is_empty()));
        put16(
            &mut d,
            16,
            u16::try_from(ids.len()).map_err(|_| invalid("directory count overflow"))?,
        );
        d[0xba] = 1;
        name(&mut d, Some(0xc8), 0x108, &units(text, 63))?;
        put32(&mut d, 0x188, *key);
        b.extend(d);
    }
    Ok(b)
}

/// Independent structural reader. Retains complete records, including unknown bytes.
#[derive(Debug)]
pub struct Parsed {
    pub header: Vec<u8>,
    pub tocs: Vec<Vec<u8>>,
    pub records: Vec<Vec<u8>>,
    pub directories: Vec<Vec<u8>>,
    pub pending_edits: bool,
}
fn check_header(b: &[u8]) -> Result<()> {
    if b.len() < HEADER || word(b, 0)? != 0 || word(b, 12)? != 0x2001 {
        return Err(invalid("invalid Cbm header"));
    }
    Ok(())
}
fn text(b: &[u8], at: usize, count: usize) -> Result<String> {
    let bytes = b
        .get(
            at..at
                .checked_add(
                    count
                        .checked_mul(2)
                        .ok_or_else(|| invalid("text length overflow"))?,
                )
                .ok_or_else(|| invalid("text offset overflow"))?,
        )
        .ok_or_else(|| invalid("text outside record"))?;
    String::from_utf16(
        &bytes
            .chunks_exact(2)
            .map(|p| u16::from_le_bytes([p[0], p[1]]))
            .collect::<Vec<_>>(),
    )
    .map_err(|_| invalid("invalid UTF-16"))
}
pub fn audio_path(record: &[u8]) -> Result<String> {
    let n = usize::from(short(record, 0x5d0)?);
    if !(1..=257).contains(&n) || short(record, 0x5d2 + 2 * (n - 1))? != 0 {
        return Err(invalid("audio path count/terminator"));
    }
    text(record, 0x5d2, n - 1)
}
pub fn parse_contents(b: &[u8]) -> Result<Parsed> {
    check_header(b)?;
    let end = usize::try_from(word(b, 4)?).map_err(|_| invalid("contents offset"))?;
    if end < HEADER
        || (end - HEADER) % RECORD != 0
        || (end - HEADER) / RECORD > MAX_TRACKS
        || end.checked_add(4096) != Some(b.len())
        || b[end..].iter().any(|v| *v != 0)
    {
        return Err(invalid("contents size/trailer mismatch"));
    }
    let mut records = Vec::new();
    let mut ids = BTreeSet::new();
    for at in (HEADER..end).step_by(RECORD) {
        let r = &b[at..at + RECORD];
        if word(r, 0)? != number(at + RECORD)?
            || !ids.insert(word(r, 0x9e4)?)
            || word(r, 0x9e4)? == 0
        {
            return Err(invalid("contents link/identity"));
        }
        if word(r, 0x9f4)? == 0 || word(r, 0x9ec)? >= word(r, 0x9f4)? {
            return Err(invalid("audio-data offset is outside the recorded file"));
        }
        let path = audio_path(r)?;
        if !path.starts_with('/') || path.contains('\0') {
            return Err(invalid("invalid relative audio path"));
        }
        for at in [0x148, 0x1c7, 0x246, 0x2c5, 0x344, 0x3c3] {
            let count = usize::from(r[at]);
            if count & 1 != 0 || count > 126 {
                return Err(invalid("invalid display length"));
            }
            text(r, at + 1, count / 2)?;
        }
        if short(r, 0x3d0)? > 255 || short(r, 0x7da)? > 260 {
            return Err(invalid("invalid long text count"));
        }
        text(r, 0x3d2, usize::from(short(r, 0x3d0)?))?;
        text(r, 0x7dc, usize::from(short(r, 0x7da)?))?;
        records.push(r.to_vec());
    }
    Ok(Parsed {
        header: b[..HEADER].to_vec(),
        tocs: Vec::new(),
        records,
        directories: Vec::new(),
        pending_edits: false,
    })
}
pub fn parse_category(b: &[u8], contents: &[Vec<u8>]) -> Result<Parsed> {
    check_header(b)?;
    let mut directories = Vec::new();
    let mut tocs = Vec::new();
    let mut cursor = HEADER;
    let mut pending_edits = false;
    let mut terminated = false;
    for slot in 0..MAX_DIRECTORIES + 2 {
        let offset =
            usize::try_from(word(b, 20 + 4 * slot)?).map_err(|_| invalid("directory offset"))?;
        if offset == 0 {
            terminated = true;
            continue;
        }
        if terminated || slot >= MAX_DIRECTORIES {
            return Err(invalid("nonzero offsets after sentinel or table overflow"));
        }
        let d = b
            .get(
                offset
                    ..offset
                        .checked_add(DIRECTORY)
                        .ok_or_else(|| invalid("directory overflow"))?,
            )
            .ok_or_else(|| invalid("directory outside file"))?;
        let count = usize::from(short(d, 16)?);
        if count > MAX_TRACKS
            || offset != cursor + count * TOC
            || short(d, 12)? != u16::try_from(slot + 1).map_err(|_| invalid("directory ID"))?
            || usize::try_from(word(d, 0)?).map_err(|_| invalid("first TOC"))?
                != if count == 0 { 0 } else { cursor }
        {
            return Err(invalid("TOC span/directory identity mismatch"));
        }
        for at in (cursor..offset).step_by(TOC) {
            tocs.push(b[at..at + TOC].to_vec());
            let target = usize::try_from(word(b, at)?).map_err(|_| invalid("TOC reference"))?;
            if target < HEADER || (target - HEADER) % RECORD != 0 {
                return Err(invalid("unaligned content reference"));
            }
            let r = contents
                .get((target - HEADER) / RECORD)
                .ok_or_else(|| invalid("TOC content outside file"))?;
            if word(b, at + 20)? != word(r, 0x9e4)? || word(b, at + 16)? != word(r, 16)? {
                return Err(invalid("TOC content ID/block mismatch"));
            }
        }
        let length = usize::from(d[0x108]);
        if length > 126 || length % 2 != 0 {
            return Err(invalid("directory text length"));
        }
        text(d, 0x109, length / 2)?;
        pending_edits |= word(d, 0x18c)? & 0x3f != 0;
        directories.push(d.to_vec());
        cursor = offset + DIRECTORY;
    }
    if !terminated || cursor != b.len() {
        return Err(invalid("missing sentinel or unreferenced category bytes"));
    }
    Ok(Parsed {
        header: b[..HEADER].to_vec(),
        tocs,
        records: Vec::new(),
        directories,
        pending_edits,
    })
}
pub fn validate(files: &BTreeMap<String, Vec<u8>>) -> Result<()> {
    let contents = parse_contents(
        files
            .get(FILES[0])
            .ok_or_else(|| invalid("missing CbmDB"))?,
    )?;
    for file in &FILES[1..] {
        parse_category(
            files
                .get(*file)
                .ok_or_else(|| invalid(format!("missing {file}")))?,
            &contents.records,
        )?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    pub(super) fn track() -> Track {
        Track {
            id: 42,
            audio: "/Contents/A/a.wav".into(),
            filename: "a.wav".into(),
            title: "Alpha".into(),
            artist: "Artist".into(),
            artist_id: 1,
            album: "Album".into(),
            album_id: 1,
            genre: "Genre".into(),
            key: "Am".into(),
            comment: "note".into(),
            artwork: String::new(),
            track_number: 1,
            bpm: 12850,
            hot_cue_auto_load: true,
            bytes: 176_444,
            data_offset: 44,
            frames75: 75,
            rate: 14112,
            format: 0x20,
            emphasis: false,
        }
    }
    #[test]
    fn exact_offsets_links_categories_and_unknown_bytes() {
        let files = build(
            &[track()],
            &[Playlist {
                id: 7,
                name: "Set".into(),
                tracks: vec![42],
            }],
            1,
            512,
            123,
        )
        .unwrap();
        let db = &files[FILES[0]];
        assert_eq!(db.len(), 40028 + 2552 + 4096);
        assert_eq!(word(db, 4).unwrap(), 42580);
        let r = &db[40028..42580];
        assert_eq!(word(r, 0).unwrap(), 42580);
        assert_eq!(word(r, 4).unwrap(), 150);
        assert_eq!(word(r, 0x10).unwrap(), 345);
        assert_eq!(word(r, 0x18).unwrap(), 225);
        assert_eq!(word(r, 0x134).unwrap(), 44);
        assert_eq!(word(r, 0x138).unwrap(), 1285);
        assert_eq!(r[0x132], 1);
        assert_eq!(word(r, 0x9e4).unwrap(), 42);
        let title = &files[FILES[1]];
        assert_eq!(title.len(), 40028 + 48 + 832);
        assert_eq!(word(title, 20).unwrap(), 40052);
        let toc = &title[40028..40052];
        assert_eq!(word(toc, 0).unwrap(), 40028);
        assert_eq!(word(toc, 20).unwrap(), 42);
        let mut category = title.clone();
        put32(&mut category, HEADER + 12, 0x1234_5678);
        let parsed_category =
            parse_category(&category, &parse_contents(db).unwrap().records).unwrap();
        assert_eq!(word(&parsed_category.tocs[0], 12).unwrap(), 0x1234_5678);
        assert_eq!(parsed_category.header.len(), HEADER);
        let mut raw = db.clone();
        raw[40028 + 0x25] = 0xa7;
        assert_eq!(parse_contents(&raw).unwrap().records[0][0x25], 0xa7);
    }
    #[test]
    fn source_collation_and_unicode_boundaries() {
        assert_eq!(compare("a", "Ａ"), Ordering::Equal);
        assert_eq!(compare("", "A"), Ordering::Greater);
        assert_eq!(compare("A", "0"), Ordering::Less);
        assert_ne!(compare("é", "É"), Ordering::Equal);
        assert_ne!(compare("か", "カ"), Ordering::Equal);
        assert_eq!(compact(&[0xd800, 0xdffe]), b"?");
        assert_eq!(compact(&[0xd800, 0xdfff]), b"??");
        assert_eq!(compact(&[0xe9]), vec![0xe9]);
        let mut t = track();
        t.title = format!("{}😀", "a".repeat(62));
        t.filename = format!("{}.aiff", "b".repeat(100));
        t.comment = format!("{}😀", "c".repeat(254));
        let files = build(&[t], &[], 0, 512, 0).unwrap();
        let r = &parse_contents(&files[FILES[0]]).unwrap().records[0].clone();
        assert_eq!(r[0x1c7], 124);
        assert_eq!(short(r, 0x3d0).unwrap(), 254);
        assert_eq!(r[0x148], 126);
        assert_eq!(
            text(r, 0x149, 63)
                .unwrap()
                .chars()
                .rev()
                .take(5)
                .collect::<String>(),
            "ffia."
        );
    }
    #[test]
    fn bounds_offsets_and_following_zero_sentinel() {
        let leaves: Vec<_> = (0..10000)
            .map(|i| Playlist {
                id: i + 1,
                name: "Empty".into(),
                tracks: vec![],
            })
            .collect();
        let files = build(&[], &leaves, 10000, 512, 0).unwrap();
        let pl = &files[FILES[5]];
        assert_eq!(&pl[0x9c54..0x9c5c], &[0; 8]);
        let mut extra = leaves;
        extra.push(Playlist {
            id: 10001,
            name: "overflow".into(),
            tracks: vec![],
        });
        assert!(build(&[], &extra, 0, 512, 0).is_err());
        let mut corrupted = pl.clone();
        put32(&mut corrupted, 20, u32::MAX);
        assert!(parse_category(&corrupted, &[]).is_err());
        let mut t = track();
        t.audio = format!("/{}", "x".repeat(256));
        assert!(build(&[t], &[], 0, 512, 0).is_err());
        for n in 0..HEADER {
            assert!(parse_contents(&vec![0; n]).is_err());
        }
    }
    #[test]
    fn timing_clamp_boundary_is_not_an_export_rejection() {
        // [OBS] Native guarded timing load clamps spans greater than 449999.
        // Serialize true duration on both sides; do not claim a universal file limit.
        for frames in [449_998, 449_999, 450_000] {
            let mut t = track();
            t.frames75 = frames;
            let files = build(&[t], &[], 0, 512, 0).unwrap();
            let r = &parse_contents(&files[FILES[0]]).unwrap().records[0].clone();
            assert_eq!(word(r, 0x18).unwrap() - word(r, 0x14).unwrap(), frames);
            assert_eq!(word(r, 0x9e8).unwrap(), frames);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod category_tests {
    use super::*;
    #[test]
    fn full_content_ids_survive_a_stock_hash_bucket_collision() {
        let a = super::tests::track();
        let mut b = a.clone();
        b.id = 10_042;
        b.audio = "/Contents/B/b.wav".into();
        let files = build(
            &[a, b],
            &[Playlist {
                id: 1,
                name: "Order".into(),
                tracks: vec![10_042, 42],
            }],
            1,
            512,
            0,
        )
        .unwrap();
        let records = parse_contents(&files[FILES[0]]).unwrap().records;
        let playlist = parse_category(&files[FILES[5]], &records).unwrap();
        assert_eq!(word(&playlist.tocs[0], 20).unwrap(), 10_042);
        assert_eq!(word(&playlist.tocs[1], 20).unwrap(), 42);
        assert_ne!(
            word(&playlist.tocs[0], 0).unwrap(),
            word(&playlist.tocs[1], 0).unwrap()
        );
    }
    #[test]
    fn comment_does_not_corrupt_a_surrogate_in_path_unit_252() {
        let mut t = super::tests::track();
        t.audio = format!("/{}😀", "x".repeat(251));
        t.comment = "c".repeat(255);
        let set = build(&[t.clone()], &[], 0, 512, 0).unwrap();
        let parsed = parse_contents(&set[FILES[0]]).unwrap();
        assert_eq!(audio_path(&parsed.records[0]).unwrap(), t.audio);
        assert_eq!(short(&parsed.records[0], 0x3d0).unwrap(), 255);
    }
    #[test]
    fn exact_name_grouping_album_order_and_bpm_rounding_are_distinct_from_collation() {
        let mut a = super::tests::track();
        a.id = 1;
        a.title = "Beta".into();
        a.track_number = 2;
        a.bpm = 12849;
        let mut b = a.clone();
        b.id = 2;
        b.title = "Alpha".into();
        b.track_number = 1;
        b.audio = "/b.wav".into();
        b.bpm = 12850;
        let mut c = b.clone();
        c.id = 3;
        c.artist = "artist".into();
        c.artist_id = 2;
        c.audio = "/c.wav".into();
        c.bpm = 49;
        let files = build(&[a, b, c], &[], 0, 512, 0).unwrap();
        let records = parse_contents(&files[FILES[0]]).unwrap().records;
        let artists = parse_category(&files[FILES[2]], &records).unwrap();
        assert_eq!(artists.directories.len(), 2);
        let bp = parse_category(&files[FILES[4]], &records).unwrap();
        assert_eq!(bp.directories.len(), 2);
        assert_eq!(word(&bp.directories[0], 0x188).unwrap(), 12800);
        assert_eq!(word(&bp.directories[1], 0x188).unwrap(), 12900);
        let albums = &files[FILES[3]];
        assert_eq!(word(albums, HEADER + 20).unwrap(), 2);
        assert_eq!(word(albums, HEADER + TOC + 20).unwrap(), 3);
        assert_eq!(word(albums, HEADER + 2 * TOC + 20).unwrap(), 1);
    }
}
