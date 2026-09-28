//! rekordbox's XML collection: reading one into the library.
//!
//! `File › Export Collection in xml format` in rekordbox writes a
//! `DJ_PLAYLISTS` document: a `COLLECTION` of `TRACK` elements, each with
//! its tags as attributes and its `POSITION_MARK` cues as children, and a
//! `PLAYLISTS` tree of `NODE`s — `Type="0"` a folder, `Type="1"` a playlist
//! whose `TRACK Key="…"` children name tracks by their `TrackID`. Locations
//! are `file://localhost` URLs, percent-encoded.
//!
//! Importing goes through the writer's own paths — [`Writer::import_file`]
//! for the files, [`Writer::create_folder`], [`Writer::create_playlist`] and
//! [`Writer::add_tracks`] for the tree, [`Writer::add_cue`] and
//! [`Writer::add_loop`] for the marks — so every row lands in the shape the
//! writer already makes. A track whose file is already in the library is
//! reused rather than doubled, and keeps its own cues.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rbl_core::xml::{attribute, tags, Tag};
use rusqlite::params;

use crate::write::{Writer, ROOT};
use crate::Result;

/// One `POSITION_MARK`.
#[derive(Debug, Clone, PartialEq)]
pub struct XmlCue {
    /// `Num`: -1 a memory cue, 0 and up a hot cue A, B, C …
    pub num: i32,
    pub start_secs: f64,
    /// `End`, for a loop.
    pub end_secs: Option<f64>,
}

/// One `TRACK` of the collection.
#[derive(Debug, Clone, PartialEq)]
pub struct XmlTrack {
    pub id: String,
    pub title: String,
    pub artist: String,
    /// The file, decoded from `Location`; `None` when the URL is not a file.
    pub path: Option<PathBuf>,
    /// Stars, 0 to 5, from the 0/51/…/255 `Rating`.
    pub rating: u8,
    pub comment: String,
    pub cues: Vec<XmlCue>,
}

/// One `NODE` of the playlist tree, flattened with its depth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlNode {
    pub name: String,
    pub folder: bool,
    pub depth: usize,
    /// `TrackID`s, for a playlist.
    pub track_ids: Vec<String>,
}

/// A parsed document.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct XmlLibrary {
    pub tracks: Vec<XmlTrack>,
    /// The tree in document order, the `ROOT` node left out.
    pub nodes: Vec<XmlNode>,
}

impl XmlLibrary {
    /// Parses a document. Anything that is not a `DJ_PLAYLISTS` document
    /// parses as empty.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut library = Self::default();
        let mut in_collection = false;
        let mut in_playlists = false;
        let mut track: Option<XmlTrack> = None;
        // The tree's depth below ROOT, and which node is open.
        let mut depth: usize = 0;
        let mut open_node: Option<usize> = None;
        let mut node_stack: Vec<Option<usize>> = Vec::new();
        for tag in tags(text) {
            match tag {
                Tag::Open { name, attributes, closed } => match name.as_str() {
                    "COLLECTION" => in_collection = !closed,
                    "PLAYLISTS" => in_playlists = !closed,
                    "TRACK" if in_collection => {
                        let parsed = XmlTrack {
                            id: attribute(&attributes, "TrackID"),
                            title: attribute(&attributes, "Name"),
                            artist: attribute(&attributes, "Artist"),
                            path: file_path(&attribute(&attributes, "Location")),
                            rating: stars(&attribute(&attributes, "Rating")),
                            comment: attribute(&attributes, "Comments"),
                            cues: Vec::new(),
                        };
                        if closed {
                            library.tracks.push(parsed);
                        } else {
                            track = Some(parsed);
                        }
                    }
                    "POSITION_MARK" => {
                        if let Some(current) = track.as_mut() {
                            let end = attribute(&attributes, "End");
                            current.cues.push(XmlCue {
                                num: attribute(&attributes, "Num").trim().parse().unwrap_or(-1),
                                start_secs: attribute(&attributes, "Start").trim().parse().unwrap_or(0.0),
                                end_secs: (!end.trim().is_empty()).then(|| end.trim().parse().unwrap_or(0.0)),
                            });
                        }
                    }
                    "NODE" if in_playlists => {
                        let is_root = depth == 0;
                        let index = if is_root {
                            None
                        } else {
                            library.nodes.push(XmlNode {
                                name: attribute(&attributes, "Name"),
                                folder: attribute(&attributes, "Type").trim() != "1",
                                // Below ROOT, which is depth 0 here.
                                depth: depth - 1,
                                track_ids: Vec::new(),
                            });
                            Some(library.nodes.len() - 1)
                        };
                        if closed {
                            continue;
                        }
                        node_stack.push(open_node);
                        open_node = index;
                        depth += 1;
                    }
                    "TRACK" if in_playlists => {
                        if let Some(node) = open_node.and_then(|i| library.nodes.get_mut(i)) {
                            node.track_ids.push(attribute(&attributes, "Key"));
                        }
                    }
                    _ => {}
                },
                Tag::Close { name } => match name.as_str() {
                    "COLLECTION" => in_collection = false,
                    "PLAYLISTS" => in_playlists = false,
                    "TRACK" if in_collection => {
                        if let Some(done) = track.take() {
                            library.tracks.push(done);
                        }
                    }
                    "NODE" if in_playlists => {
                        depth = depth.saturating_sub(1);
                        open_node = node_stack.pop().flatten();
                    }
                    _ => {}
                },
            }
        }
        library
    }
}

/// The path a `Location` names: `file://localhost/Users/…`, percent-encoded.
/// A URL to anything but a file is not a path.
/// The file a `file://` URL names, percent-decoded; `None` for another
/// scheme or nothing.
pub(crate) fn file_path(location: &str) -> Option<PathBuf> {
    let rest = location.strip_prefix("file://")?;
    // `file://localhost/x` and `file:///x` both mean `/x`.
    let path = rest.strip_prefix("localhost").unwrap_or(rest);
    let decoded = percent_decode(path);
    if decoded.is_empty() {
        return None;
    }
    // A Windows path arrives as `/C:/Users/…`.
    let decoded = if decoded.len() > 2 && decoded.as_bytes()[0] == b'/' && decoded.as_bytes()[2] == b':' {
        decoded.get(1..).unwrap_or("").to_owned()
    } else {
        decoded
    };
    Some(PathBuf::from(decoded))
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        if byte == b'%' && i + 2 < bytes.len() {
            if let Ok(value) = u8::from_str_radix(text.get(i + 1..i + 3).unwrap_or(""), 16) {
                out.push(value);
                i += 3;
                continue;
            }
        }
        out.push(byte);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Stars from rekordbox's `Rating`: 0, 51, 102, 153, 204, 255.
fn stars(rating: &str) -> u8 {
    let value: u32 = rating.trim().parse().unwrap_or(0);
    u8::try_from((value + 25) / 51).unwrap_or(5).min(5)
}

/// Stars from iTunes's 0 to 100, twenty a star.
pub(crate) fn stars_of_hundred(rating: u32) -> u8 {
    u8::try_from((rating + 10) / 20).unwrap_or(5).min(5)
}

/// What importing a document did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct XmlImportReport {
    /// Tracks added to the library.
    pub imported: usize,
    /// Tracks whose file was already in the library, reused as they are.
    pub existing: usize,
    /// One line per track that could not be added, saying why.
    pub skipped: Vec<String>,
    /// Playlists and folders made.
    pub playlists: usize,
    /// Cues and loops added to the tracks that were imported.
    pub cues: usize,
    /// The tracks that landed, `(id, title)`, so they can be analysed.
    pub tracks: Vec<(String, String)>,
}

/// Imports a parsed document through the writer.
///
/// `progress` is told how many tracks are done of how many, after each.
pub fn import(writer: &mut Writer, library: &XmlLibrary, progress: &mut dyn FnMut(usize, usize)) -> Result<XmlImportReport> {
    let mut report = XmlImportReport::default();
    // `TrackID` → the library's content id, for the playlists.
    let mut ids: HashMap<&str, String> = HashMap::with_capacity(library.tracks.len());
    let total = library.tracks.len();
    for (done, track) in library.tracks.iter().enumerate() {
        let label = if track.artist.is_empty() { track.title.clone() } else { format!("{} — {}", track.artist, track.title) };
        let Some(path) = track.path.as_deref() else {
            report.skipped.push(format!("{label}: not a file"));
            progress(done + 1, total);
            continue;
        };
        if !path.is_file() {
            report.skipped.push(format!("{label}: {} is not there", path.display()));
            progress(done + 1, total);
            continue;
        }
        match writer.import_file(path) {
            Ok(id) => {
                report.imported += 1;
                // What the file's tags do not carry and the document does.
                if track.rating > 0 {
                    writer.set_rating(&id, track.rating)?;
                }
                if !track.comment.trim().is_empty() {
                    writer.set_comment(&id, track.comment.trim())?;
                }
                report.cues += add_cues(writer, &id, &track.cues)?;
                report.tracks.push((id.clone(), track.title.clone()));
                ids.insert(track.id.as_str(), id);
            }
            Err(crate::DbError::WriteRefused(reason)) => {
                // Already in the library: reused, with its own cues kept.
                match existing_id(writer, path) {
                    Some(id) => {
                        report.existing += 1;
                        ids.insert(track.id.as_str(), id);
                    }
                    None => report.skipped.push(format!("{label}: {reason}")),
                }
            }
            Err(other) => return Err(other),
        }
        progress(done + 1, total);
    }

    // The tree, in document order: a node's parent is the nearest open node
    // one level up, so a stack of the ids made so far finds it.
    let mut parents: Vec<String> = vec![ROOT.to_owned()];
    for node in &library.nodes {
        parents.truncate(node.depth + 1);
        let parent = parents.last().cloned().unwrap_or_else(|| ROOT.to_owned());
        if node.folder {
            let id = writer.create_folder(&node.name, &parent)?;
            parents.push(id);
        } else {
            let id = writer.create_playlist(&node.name, &parent)?;
            let members: Vec<String> = node.track_ids.iter().filter_map(|key| ids.get(key.as_str()).cloned()).collect();
            if !members.is_empty() {
                writer.add_tracks(&id, &members)?;
            }
            parents.push(id);
        }
        report.playlists += 1;
    }
    Ok(report)
}

/// The cues of a document track onto a library track. `Num` -1 is a memory
/// cue; 0 and up is a hot cue by letter from A. Seconds become
/// milliseconds; a mark with an `End` after its `Start` is a loop.
fn add_cues(writer: &mut Writer, content: &str, cues: &[XmlCue]) -> Result<usize> {
    let mut added = 0;
    for cue in cues {
        let kind = if cue.num < 0 {
            0
        } else {
            // The slot as `djmdCue.Kind`: A to C are 1 to 3, kind 4 is
            // unused, so D and everything after sit one higher; past P
            // there is no slot.
            let slot = u8::try_from(cue.num).unwrap_or(u8::MAX);
            match slot {
                0..=2 => slot + 1,
                3..=15 => slot + 2,
                _ => continue,
            }
        };
        let in_ms = millis(cue.start_secs);
        match cue.end_secs.map(millis).filter(|&out| out > in_ms) {
            Some(out_ms) => {
                writer.add_loop(content, kind, in_ms, out_ms, 0)?;
            }
            None => {
                writer.add_cue(content, kind, in_ms)?;
            }
        }
        added += 1;
    }
    Ok(added)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "clamped to u32's range first")]
fn millis(secs: f64) -> u32 {
    (secs * 1000.0).round().clamp(0.0, f64::from(u32::MAX)) as u32
}

/// The library's id for a file already in it.
fn existing_id(writer: &Writer, path: &Path) -> Option<String> {
    let folder = path.to_string_lossy().into_owned();
    writer
        .library()
        .connection()
        .query_row(
            "SELECT ID FROM djmdContent WHERE FolderPath = ?1 AND rb_local_deleted = 0",
            params![folder],
            |r| r.get::<_, String>(0),
        )
        .ok()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const DOC: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<DJ_PLAYLISTS Version="1.0.0">
  <PRODUCT Name="rekordbox" Version="7.2.11" Company="AlphaTheta"/>
  <COLLECTION Entries="2">
    <TRACK TrackID="1" Name="All U Need" Artist="TRIODE" Rating="204" Comments="peak &amp; more"
           Location="file://localhost/Users/me/Music/All%20U%20Need.mp3">
      <TEMPO Inizio="0.025" Bpm="128.00" Metro="4/4" Battito="1"/>
      <POSITION_MARK Name="" Type="0" Start="12.5" Num="-1"/>
      <POSITION_MARK Name="" Type="0" Start="30" Num="0"/>
      <POSITION_MARK Name="" Type="4" Start="60" End="63.75" Num="1"/>
    </TRACK>
    <TRACK TrackID="2" Name="Stream" Artist="Nobody" Rating="0" Location="https://example.com/x"/>
  </COLLECTION>
  <PLAYLISTS>
    <NODE Type="0" Name="ROOT" Count="2">
      <NODE Name="Sets" Type="0" Count="1">
        <NODE Name="Warm up" Type="1" KeyType="0" Entries="2">
          <TRACK Key="1"/>
          <TRACK Key="2"/>
        </NODE>
      </NODE>
      <NODE Name="Loose" Type="1" KeyType="0" Entries="0"/>
    </NODE>
  </PLAYLISTS>
</DJ_PLAYLISTS>"#;

    #[test]
    fn a_document_parses_into_tracks_cues_and_a_tree() {
        let library = XmlLibrary::parse(DOC);
        assert_eq!(library.tracks.len(), 2);
        let first = &library.tracks[0];
        assert_eq!(first.path.as_deref(), Some(Path::new("/Users/me/Music/All U Need.mp3")));
        assert_eq!((first.rating, first.comment.as_str()), (4, "peak & more"));
        assert_eq!(first.cues.len(), 3);
        assert_eq!(first.cues[2], XmlCue { num: 1, start_secs: 60.0, end_secs: Some(63.75) });
        assert_eq!(library.tracks[1].path, None);

        let names: Vec<(&str, bool, usize)> = library.nodes.iter().map(|n| (n.name.as_str(), n.folder, n.depth)).collect();
        assert_eq!(names, vec![("Sets", true, 0), ("Warm up", false, 1), ("Loose", false, 0)]);
        assert_eq!(library.nodes[1].track_ids, vec!["1", "2"]);
        assert!(XmlLibrary::parse("<html/>").tracks.is_empty());
    }

    #[test]
    fn locations_decode_as_paths() {
        assert_eq!(file_path("file:///Music/a%20b.mp3"), Some(PathBuf::from("/Music/a b.mp3")));
        assert_eq!(file_path("file://localhost/C:/Users/me/x.mp3"), Some(PathBuf::from("C:/Users/me/x.mp3")));
        assert_eq!(file_path("file://localhost/M%C3%BCsic/%E2%99%AA.mp3"), Some(PathBuf::from("/Müsic/♪.mp3")));
        assert_eq!(file_path("http://x/y"), None);
        assert_eq!(stars("255"), 5);
        assert_eq!(stars("51"), 1);
        assert_eq!(stars(""), 0);
    }
}
