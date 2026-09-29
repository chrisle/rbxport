//! Builders for tests and fixtures. Never used against the real library.

use crate::{strings::fold, Cue, Cues, Library, Playlists, Row};

/// Minimal track description for constructing an index without a database.
#[derive(Debug, Clone, Default)]
pub struct TestTrack {
    pub id: u64,
    pub title: &'static str,
    pub artist: &'static str,
    pub album: &'static str,
    pub label: &'static str,
    pub comment: &'static str,
    pub bpm_x100: u32,
    pub length_sec: u32,
    pub rating: u8,
    pub date_added: &'static str,
    pub key: &'static str,
    /// `ColorID`, 1 to 8; 0 is none.
    pub color: u8,
    /// In any order; the index sorts them by position as the loader does.
    pub cues: Vec<Cue>,
    /// The file's absolute path, as `djmdContent.FolderPath` holds it.
    pub path: &'static str,
    pub genre: &'static str,
    pub year: u16,
    pub play_count: u16,
}

/// Builds an index directly, bypassing SQL.
pub fn library_from(tracks: &[TestTrack]) -> Library {
    let mut lib = Library::default();
    for t in tracks {
        lib.ids.push(t.id);
        lib.title.push(t.title);
        lib.title_folded.push(&fold(t.title));
        lib.comment.push(t.comment);
        lib.folder_path.push(t.path);
        lib.file_name.push(std::path::Path::new(t.path).file_name().and_then(|n| n.to_str()).unwrap_or(""));
        lib.analysis_path.push("");
        lib.date_added.push(t.date_added);
        lib.release_date.push("");
        lib.artist.push(lib.artists.push(t.artist));
        lib.album.push(lib.albums.push(t.album));
        lib.genre.push(lib.genres.push(t.genre));
        lib.label.push(lib.labels.push(t.label));
        // One interner entry per track, names repeating, which is what
        // `djmdKey` does on the reference library (`A` under two ids).
        lib.key.push(lib.keys.push(t.key));
        lib.bpm_x100.push(t.bpm_x100);
        lib.length_sec.push(t.length_sec);
        lib.rating.push(t.rating);
        lib.color.push(t.color);
        lib.play_count.push(t.play_count);
        lib.analysed.push(u8::from(t.bpm_x100 > 0));
        lib.year.push(t.year);
        lib.bitrate.push(0);
        lib.sample_rate.push(0);
        lib.file_size.push(0);
    }
    lib.count = tracks.len();
    lib.set_cues(Cues::from_per_track(tracks.iter().map(|t| t.cues.clone()).collect()));
    lib.set_playlists(Playlists::default());
    lib.build_ranks();
    lib.build_search();
    lib
}

/// Adds a playlist over the given row indices, returning its index.
pub fn add_playlist(lib: &mut Library, name: &str, rows: &[Row]) -> usize {
    add_list(lib, name, rows, false)
}

/// Adds an empty folder at the top of the tree, returning its index.
pub fn add_folder(lib: &mut Library, name: &str) -> usize {
    add_list(lib, name, &[], true)
}

/// Adds a history session over the given row indices, returning its index.
pub fn add_history(lib: &mut Library, name: &str, rows: &[Row]) -> usize {
    let mut histories = (*lib.histories()).clone();
    let index = histories.ids.len();
    histories.ids.push(2000 + u64::try_from(index).unwrap_or(0));
    histories.names.push(name);
    histories.parent.push(crate::NO_ID);
    histories.seq.push(u32::try_from(index).unwrap_or(0));
    histories.attribute.push(0);
    histories.smart.push("");
    histories.members.push(rows.to_vec());
    lib.set_histories(histories);
    index
}

/// Adds an intelligent playlist with the given rule XML, returning its index.
pub fn add_smart_playlist(lib: &mut Library, name: &str, rule: &str) -> usize {
    add_list_with(lib, name, &[], crate::ATTRIBUTE_SMART, rule)
}

fn add_list(lib: &mut Library, name: &str, rows: &[Row], folder: bool) -> usize {
    add_list_with(lib, name, rows, if folder { crate::ATTRIBUTE_FOLDER } else { 0 }, "")
}

fn add_list_with(lib: &mut Library, name: &str, rows: &[Row], attribute: u8, rule: &str) -> usize {
    let mut playlists = (*lib.playlists()).clone();
    let index = playlists.ids.len();
    playlists.ids.push(1000 + u64::try_from(index).unwrap_or(0));
    playlists.names.push(name);
    playlists.parent.push(crate::NO_ID);
    playlists.seq.push(u32::try_from(index).unwrap_or(0));
    playlists.attribute.push(attribute);
    playlists.smart.push(rule);
    playlists.members.push(rows.to_vec());
    lib.set_playlists(playlists);
    index
}

/// Sets the My Tag categories, which a fixture without a database cannot read.
pub fn set_my_tags(lib: &mut Library, categories: Vec<crate::TagCategory>) {
    lib.set_my_tags(categories);
}
