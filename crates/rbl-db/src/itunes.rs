//! iTunes / Music.app's `Library.xml`, read into the same shape as a
//! rekordbox XML collection, so one importer serves both.
//!
//! Music.app writes the file when "Share iTunes Library XML with other
//! applications" is on (Music › Settings › Files); rekordbox reads the same
//! file for its iTunes section. It is an Apple property list: one `dict`
//! with `Tracks` (a dict of track id to a dict of fields) and `Playlists`
//! (an array of dicts). The fields read here are `Name`, `Artist`,
//! `Location` (a `file://` URL), `Rating` (0 to 100, twenty a star) and
//! `Comments` on a track; `Name`, `Folder`, `Playlist Persistent ID`,
//! `Parent Persistent ID`, `Master`, `Distinguished Kind` and
//! `Playlist Items` on a playlist. The master library list and the
//! built-in ones (Music, Downloaded, Genius…) are left out, as rekordbox
//! leaves them out of its iTunes tree.
//!
//! The plist grammar this needs is small — `dict`, `array`, `key`,
//! `string`, `integer`, `true`, `false`, and a few it skips — so it is
//! read here rather than through a crate.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rbl_core::xml::unescape;

use crate::xml::{file_path, stars_of_hundred, XmlLibrary, XmlNode, XmlTrack};

/// The files Music.app and iTunes write their shared library XML to, under the
/// user's music folder, the newer layout first. Music.app writes
/// `Music/Library.xml` when "Share Library XML with other applications" is on;
/// iTunes wrote `iTunes/iTunes Music Library.xml`.
///
/// The paths are candidates, not a promise the files are there: the caller
/// reads the first one that exists and parses.
#[must_use]
pub fn candidate_library_paths(music_dir: &Path) -> Vec<PathBuf> {
    [["Music", "Library.xml"], ["iTunes", "iTunes Music Library.xml"]]
        .into_iter()
        .map(|parts| music_dir.join(parts[0]).join(parts[1]))
        .collect()
}

/// A property list value, as much of it as the library needs.
#[derive(Debug, Clone, PartialEq)]
enum Value {
    Dict(Vec<(String, Value)>),
    Array(Vec<Value>),
    Text(String),
    Bool(bool),
    Other,
}

impl Value {
    fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Self::Dict(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn text(&self, key: &str) -> String {
        match self.get(key) {
            Some(Self::Text(text)) => text.clone(),
            _ => String::new(),
        }
    }

    fn flag(&self, key: &str) -> bool {
        matches!(self.get(key), Some(Self::Bool(true)))
    }
}

/// Reads the document. `None` when it is not a property list holding a
/// `Tracks` dictionary.
#[must_use]
pub fn parse(text: &str) -> Option<XmlLibrary> {
    let mut scanner = Scanner { rest: text };
    // Past the declaration, the doctype and `<plist>`, to the root dict.
    let root = loop {
        match scanner.next_token()? {
            Token::Open(name) if name == "dict" => break scanner.dict()?,
            Token::Open(name) if name == "plist" => {}
            Token::Open(_) | Token::Close(_) | Token::Empty(_) | Token::Text(_) => {}
        }
    };
    let tracks = root.get("Tracks")?;
    let Value::Dict(track_entries) = tracks else { return None };

    let mut library = XmlLibrary::default();
    for (id, track) in track_entries {
        let title = track.text("Name");
        let artist = track.text("Artist");
        let location = track.text("Location");
        let rating = track.text("Rating").parse::<u32>().unwrap_or(0);
        library.tracks.push(XmlTrack {
            id: id.clone(),
            title,
            artist,
            path: if location.is_empty() { None } else { file_path(&location) },
            rating: stars_of_hundred(rating),
            comment: track.text("Comments"),
            cues: Vec::new(),
        });
    }

    // The playlists, as a tree by persistent id, then flattened depth
    // first in the file's order, top-level at depth 0 — the same shape and
    // base [`crate::xml::import`] takes from a rekordbox document.
    let mut lists: Vec<(String, String, XmlNode)> = Vec::new(); // (id, parent, node)
    if let Some(Value::Array(playlists)) = root.get("Playlists") {
        for playlist in playlists {
            if playlist.flag("Master") || playlist.get("Distinguished Kind").is_some() {
                continue;
            }
            let name = playlist.text("Name");
            if name.is_empty() {
                continue;
            }
            let folder = playlist.flag("Folder");
            let track_ids: Vec<String> = match playlist.get("Playlist Items") {
                Some(Value::Array(items)) if !folder => {
                    items.iter().map(|item| item.text("Track ID")).filter(|id| !id.is_empty()).collect()
                }
                _ => Vec::new(),
            };
            lists.push((
                playlist.text("Playlist Persistent ID"),
                playlist.text("Parent Persistent ID"),
                XmlNode { name, folder, depth: 0, track_ids },
            ));
        }
    }
    let known: HashMap<&str, usize> = lists.iter().enumerate().map(|(i, (id, _, _))| (id.as_str(), i)).collect();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); lists.len()];
    let mut roots: Vec<usize> = Vec::new();
    for (i, (_, parent, _)) in lists.iter().enumerate() {
        match known.get(parent.as_str()) {
            Some(&at) if at != i && lists[at].2.folder => children[at].push(i),
            _ => roots.push(i),
        }
    }
    let mut stack: Vec<(usize, usize)> = roots.iter().rev().map(|&i| (i, 0)).collect();
    let mut placed = vec![false; lists.len()];
    while let Some((at, depth)) = stack.pop() {
        if placed[at] {
            continue;
        }
        placed[at] = true;
        let mut node = lists[at].2.clone();
        node.depth = depth;
        library.nodes.push(node);
        for &child in children[at].iter().rev() {
            stack.push((child, depth + 1));
        }
    }
    Some(library)
}

enum Token {
    Open(String),
    Close(String),
    /// `<true/>`, `<false/>`, `<dict/>`, `<array/>`.
    Empty(String),
    Text(String),
}

struct Scanner<'a> {
    rest: &'a str,
}

impl Scanner<'_> {
    fn next_token(&mut self) -> Option<Token> {
        loop {
            let rest = self.rest;
            if rest.is_empty() {
                return None;
            }
            if let Some(after) = rest.strip_prefix('<') {
                if after.starts_with('?') || after.starts_with('!') {
                    let end = after.find('>')?;
                    self.rest = after.get(end + 1..)?;
                    continue;
                }
                let end = after.find('>')?;
                let inside = after.get(..end)?.trim();
                self.rest = after.get(end + 1..)?;
                if let Some(name) = inside.strip_prefix('/') {
                    return Some(Token::Close(name.trim().to_owned()));
                }
                if let Some(name) = inside.strip_suffix('/') {
                    return Some(Token::Empty(name.split_whitespace().next().unwrap_or("").to_owned()));
                }
                return Some(Token::Open(inside.split_whitespace().next().unwrap_or("").to_owned()));
            }
            let end = rest.find('<').unwrap_or(rest.len());
            let text = rest.get(..end)?;
            self.rest = rest.get(end..)?;
            if !text.trim().is_empty() {
                return Some(Token::Text(unescape(text)));
            }
        }
    }

    /// The value an opening tag introduces, consumed up to its close.
    fn value(&mut self, open: &str) -> Option<Value> {
        match open {
            "dict" => self.dict(),
            "array" => self.array(),
            "string" | "integer" | "real" | "date" | "data" => {
                let mut text = String::new();
                loop {
                    match self.next_token()? {
                        Token::Text(t) => text.push_str(&t),
                        Token::Close(_) => break,
                        Token::Open(_) | Token::Empty(_) => {}
                    }
                }
                Some(if open == "string" || open == "integer" { Value::Text(text.trim().to_owned()) } else { Value::Other })
            }
            _ => {
                self.skip_to_close(open)?;
                Some(Value::Other)
            }
        }
    }

    fn empty_value(name: &str) -> Value {
        match name {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            "dict" => Value::Dict(Vec::new()),
            "array" => Value::Array(Vec::new()),
            _ => Value::Other,
        }
    }

    /// After `<dict>`: pairs of `<key>` and a value, to `</dict>`.
    fn dict(&mut self) -> Option<Value> {
        let mut entries = Vec::new();
        let mut key: Option<String> = None;
        loop {
            match self.next_token()? {
                Token::Close(name) if name == "dict" => return Some(Value::Dict(entries)),
                Token::Open(name) if name == "key" => {
                    let mut text = String::new();
                    loop {
                        match self.next_token()? {
                            Token::Text(t) => text.push_str(&t),
                            Token::Close(_) => break,
                            Token::Open(_) | Token::Empty(_) => {}
                        }
                    }
                    key = Some(text);
                }
                Token::Open(name) => {
                    let value = self.value(&name)?;
                    if let Some(key) = key.take() {
                        entries.push((key, value));
                    }
                }
                Token::Empty(name) => {
                    if let Some(key) = key.take() {
                        entries.push((key, Self::empty_value(&name)));
                    }
                }
                Token::Close(_) | Token::Text(_) => {}
            }
        }
    }

    fn array(&mut self) -> Option<Value> {
        let mut items = Vec::new();
        loop {
            match self.next_token()? {
                Token::Close(name) if name == "array" => return Some(Value::Array(items)),
                Token::Open(name) => items.push(self.value(&name)?),
                Token::Empty(name) => items.push(Self::empty_value(&name)),
                Token::Close(_) | Token::Text(_) => {}
            }
        }
    }

    fn skip_to_close(&mut self, name: &str) -> Option<()> {
        let mut depth = 1;
        loop {
            match self.next_token()? {
                Token::Open(n) if n == name => depth += 1,
                Token::Close(n) if n == name => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(());
                    }
                }
                Token::Open(_) | Token::Close(_) | Token::Empty(_) | Token::Text(_) => {}
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Major Version</key><integer>1</integer>
	<key>Application Version</key><string>1.4.3</string>
	<key>Tracks</key>
	<dict>
		<key>101</key>
		<dict>
			<key>Track ID</key><integer>101</integer>
			<key>Name</key><string>Tech &amp; House</string>
			<key>Artist</key><string>Someone</string>
			<key>Rating</key><integer>80</integer>
			<key>Comments</key><string>a note</string>
			<key>Location</key><string>file:///Users/me/Music/Tech%20%26%20House.mp3</string>
		</dict>
		<key>102</key>
		<dict>
			<key>Track ID</key><integer>102</integer>
			<key>Name</key><string>Stream</string>
			<key>Track Type</key><string>Remote</string>
		</dict>
	</dict>
	<key>Playlists</key>
	<array>
		<dict>
			<key>Name</key><string>Library</string>
			<key>Master</key><true/>
			<key>Playlist Items</key><array><dict><key>Track ID</key><integer>101</integer></dict></array>
		</dict>
		<dict>
			<key>Name</key><string>Music</string>
			<key>Distinguished Kind</key><integer>4</integer>
		</dict>
		<dict>
			<key>Name</key><string>Sets</string>
			<key>Playlist Persistent ID</key><string>F1</string>
			<key>Folder</key><true/>
		</dict>
		<dict>
			<key>Name</key><string>Warm Up</string>
			<key>Playlist Persistent ID</key><string>P1</string>
			<key>Parent Persistent ID</key><string>F1</string>
			<key>Playlist Items</key>
			<array>
				<dict><key>Track ID</key><integer>101</integer></dict>
				<dict><key>Track ID</key><integer>102</integer></dict>
			</array>
		</dict>
		<dict>
			<key>Name</key><string>Loose</string>
			<key>Playlist Persistent ID</key><string>P2</string>
			<key>Playlist Items</key><array/>
		</dict>
	</array>
</dict>
</plist>
"#;

    #[test]
    fn tracks_and_the_playlist_tree_read_into_the_collection_shape() {
        let library = parse(SAMPLE).expect("a library");
        assert_eq!(library.tracks.len(), 2);
        let first = &library.tracks[0];
        assert_eq!((first.id.as_str(), first.title.as_str(), first.artist.as_str()), ("101", "Tech & House", "Someone"));
        assert_eq!(first.path.as_deref().map(|p| p.display().to_string()), Some("/Users/me/Music/Tech & House.mp3".to_owned()));
        assert_eq!((first.rating, first.comment.as_str()), (4, "a note"));
        assert_eq!(library.tracks[1].path, None, "a stream has no file");

        // Top-level at depth 0, as a rekordbox document flattens — so the
        // sibling `Loose` lands beside `Sets`, not inside it, when imported.
        let names: Vec<(&str, bool, usize)> = library.nodes.iter().map(|n| (n.name.as_str(), n.folder, n.depth)).collect();
        assert_eq!(names, vec![("Sets", true, 0), ("Warm Up", false, 1), ("Loose", false, 0)]);
        assert_eq!(library.nodes[1].track_ids, vec!["101", "102"]);
        assert!(parse("<html/>").is_none());
    }

    #[test]
    fn candidate_paths_cover_music_app_and_itunes() {
        let paths = candidate_library_paths(Path::new("/Users/me/Music"));
        assert_eq!(
            paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            vec![
                "/Users/me/Music/Music/Library.xml".to_owned(),
                "/Users/me/Music/iTunes/iTunes Music Library.xml".to_owned(),
            ]
        );
    }
}
