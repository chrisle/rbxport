//! The collection as rekordbox's XML, for `File › Export Collection in xml
//! format`: what another rekordbox, or anything that reads the format,
//! takes in.
//!
//! A `DJ_PLAYLISTS` document: every track of the library as a `TRACK` with
//! its tags as attributes and its cues as `POSITION_MARK`s, and the playlist
//! tree as `NODE`s. An intelligent playlist is written as the tracks its
//! rule admits now, which is what rekordbox writes for one too. No `TEMPO`
//! entries: the grids live in the analysis files, and reading 38,681 of
//! them for one export is not what this is for.

use std::fmt::Write as _;

use rbl_core::xml::escape;

use crate::{Cue, Library, Row, TrackSource};

/// rekordbox's `Colour` values for `ColorID` 1 to 8, in the palette's own
/// order (see `rbl_db::COLOR_NAMES`) [DOC: pyrekordbox's account of the format].
const COLOURS: [&str; 8] =
    ["0xFF007F", "0xFF0000", "0xFFA500", "0xFFFF00", "0x00FF00", "0x25FDE9", "0x0000FF", "0x660099"];

/// The whole library as a document.
#[must_use]
pub fn export_xml(lib: &Library) -> String {
    let mut out = String::with_capacity(lib.len() * 400);
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<DJ_PLAYLISTS Version=\"1.0.0\">\n");
    out.push_str("  <PRODUCT Name=\"rbxport\" Version=\"");
    out.push_str(env!("CARGO_PKG_VERSION"));
    out.push_str("\" Company=\"rbxport\"/>\n");
    let _ = writeln!(out, "  <COLLECTION Entries=\"{}\">", lib.len());
    for row in 0..u32::try_from(lib.len()).unwrap_or(u32::MAX) {
        push_track(&mut out, lib, row);
    }
    out.push_str("  </COLLECTION>\n  <PLAYLISTS>\n");
    push_tree(&mut out, lib);
    out.push_str("  </PLAYLISTS>\n</DJ_PLAYLISTS>\n");
    out
}

fn push_track(out: &mut String, lib: &Library, row: Row) {
    let i = row as usize;
    let id = lib.ids.get(i).copied().unwrap_or(0);
    let bpm = f64::from(lib.bpm_x100.get(i).copied().unwrap_or(0)) / 100.0;
    let colour = lib.color.get(i).copied().unwrap_or(0) as usize;
    let _ = write!(
        out,
        "    <TRACK TrackID=\"{id}\" Name=\"{}\" Artist=\"{}\" Album=\"{}\" Genre=\"{}\" Label=\"{}\" \
         TotalTime=\"{}\" Year=\"{}\" AverageBpm=\"{bpm:.2}\" DateAdded=\"{}\" Comments=\"{}\" \
         PlayCount=\"{}\" Rating=\"{}\" Tonality=\"{}\" Location=\"{}\"",
        escape(lib.title.get(i)),
        escape(lib.artist_name(row)),
        escape(lib.album_name(row)),
        escape(lib.genre_name(row)),
        escape(lib.label_name(row)),
        lib.length_sec.get(i).copied().unwrap_or(0),
        lib.year.get(i).copied().unwrap_or(0),
        escape(lib.date_added.get(i)),
        escape(lib.comment.get(i)),
        lib.play_count.get(i).copied().unwrap_or(0),
        u32::from(lib.rating.get(i).copied().unwrap_or(0)) * 51,
        escape(lib.key_name(row)),
        escape(&location(lib.folder_path.get(i))),
    );
    if let Some(hex) = colour.checked_sub(1).and_then(|c| COLOURS.get(c)) {
        let _ = write!(out, " Colour=\"{hex}\"");
    }
    let cues = lib.cues_of(row);
    if cues.is_empty() {
        out.push_str("/>\n");
        return;
    }
    out.push_str(">\n");
    for cue in &cues {
        push_cue(out, cue);
    }
    out.push_str("    </TRACK>\n");
}

fn push_cue(out: &mut String, cue: &Cue) {
    // `Num` -1 is a memory cue; a hot cue is its slot from 0. `Type` 0 is a
    // cue and 4 a loop, which also carries its `End`.
    let num = cue.hot_letter().map_or(-1, |letter| i32::from(letter as u8) - i32::from(b'A'));
    let start = f64::from(cue.position_ms) / 1000.0;
    if cue.out_ms > cue.position_ms {
        let end = f64::from(cue.out_ms) / 1000.0;
        let _ = writeln!(out, "      <POSITION_MARK Name=\"\" Type=\"4\" Start=\"{start:.3}\" End=\"{end:.3}\" Num=\"{num}\"/>");
    } else {
        let _ = writeln!(out, "      <POSITION_MARK Name=\"\" Type=\"0\" Start=\"{start:.3}\" Num=\"{num}\"/>");
    }
}

/// The tree, depth-first in `Seq` order, folders as `Type="0"` and
/// playlists as `Type="1"` with their tracks.
fn push_tree(out: &mut String, lib: &Library) {
    let playlists = lib.playlists();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); playlists.len()];
    let mut roots: Vec<usize> = Vec::new();
    for index in 0..playlists.len() {
        match playlists.parent.get(index).copied() {
            Some(parent) if parent != crate::NO_ID && (parent as usize) < playlists.len() => {
                if let Some(bucket) = children.get_mut(parent as usize) {
                    bucket.push(index);
                }
            }
            _ => roots.push(index),
        }
    }
    let _ = writeln!(out, "    <NODE Type=\"0\" Name=\"ROOT\" Count=\"{}\">", roots.len());
    // Iterative with a visited set, as the tree command walks it: a corrupt
    // parent cycle must not recurse for ever.
    let mut visited = vec![false; playlists.len()];
    let mut stack: Vec<(usize, usize, bool)> = roots.iter().rev().map(|&i| (i, 3, false)).collect();
    while let Some((index, depth, closing)) = stack.pop() {
        let indent = "  ".repeat(depth);
        if closing {
            let _ = writeln!(out, "{indent}</NODE>");
            continue;
        }
        if visited.get(index).copied().unwrap_or(true) {
            continue;
        }
        if let Some(slot) = visited.get_mut(index) {
            *slot = true;
        }
        let name = escape(playlists.name(index));
        if playlists.is_folder(index) {
            let below = children.get(index).cloned().unwrap_or_default();
            let _ = writeln!(out, "{indent}<NODE Name=\"{name}\" Type=\"0\" Count=\"{}\">", below.len());
            stack.push((index, depth, true));
            for &child in below.iter().rev() {
                stack.push((child, depth + 1, false));
            }
        } else {
            let source = if playlists.is_smart(index) {
                TrackSource::SmartPlaylist(index)
            } else {
                TrackSource::Playlist(index)
            };
            // Read after the guard: `source_rows` takes the playlists lock.
            let rows = lib.source_rows_unlocked(&playlists, &source);
            let _ = writeln!(out, "{indent}<NODE Name=\"{name}\" Type=\"1\" KeyType=\"0\" Entries=\"{}\">", rows.len());
            for row in rows {
                let _ = writeln!(out, "{indent}  <TRACK Key=\"{}\"/>", lib.ids.get(row as usize).copied().unwrap_or(0));
            }
            let _ = writeln!(out, "{indent}</NODE>");
        }
    }
    out.push_str("    </NODE>\n");
}

/// A path as rekordbox writes `Location`: `file://localhost` and the path
/// percent-encoded, `/` and the unreserved characters left as they are.
fn location(path: &str) -> String {
    let mut out = String::with_capacity(path.len() + 16);
    out.push_str("file://localhost");
    let path = path.replace('\\', "/");
    if !path.starts_with('/') {
        out.push('/');
    }
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => out.push(char::from(byte)),
            _ => {
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn a_location_is_a_file_url_with_the_path_percent_encoded() {
        assert_eq!(location("/Users/me/Music/All U Need.mp3"), "file://localhost/Users/me/Music/All%20U%20Need.mp3");
        assert_eq!(location("C:\\Music\\x&y.mp3"), "file://localhost/C%3A/Music/x%26y.mp3");
        assert_eq!(location("/Müsic/♪.mp3"), "file://localhost/M%C3%BCsic/%E2%99%AA.mp3");
    }
}
