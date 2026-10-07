//! Builders for tests and fixtures. Never used against the real library.

use crate::{strings::fold, Cue, Cues, Library, Playlists, Row};

/// Minimal track description for constructing an index without a database.
#[derive(Debug, Clone, Default)]
pub struct TestTrack {
    pub id: u64,
    pub title: &'static str,
    pub artist: &'static str,
    pub album: &'static str,
    pub album_artist: &'static str,
    pub original_artist: &'static str,
    pub composer: &'static str,
    pub remixer: &'static str,
    pub mix_name: &'static str,
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
    pub file_size: u64,
    pub sample_rate: u32,
    pub bitrate: u32,
    /// The tag's track number, `djmdContent.TrackNo`.
    pub track_number: u32,
    pub disc_no: u16,
    /// rekordbox's file type code.
    pub file_type: u8,
    pub bit_depth: u16,
    pub lyricist: &'static str,
    pub date_created: &'static str,
    /// The Publish track information box.
    pub publish: bool,
    /// `djmdContent.DeliveryComment`.
    pub message: &'static str,
}

/// Builds an index directly, bypassing SQL.
pub fn library_from(tracks: &[TestTrack]) -> Library {
    let mut lib = Library::default();
    for t in tracks {
        lib.ids.push(t.id);
        lib.title.push(t.title);
        lib.title_folded.push(&fold(t.title));
        lib.comment.push(t.comment);
        for (column, value) in lib.search_extra.iter_mut().zip([
            t.composer,
            t.album_artist,
            t.remixer,
            t.original_artist,
            t.mix_name,
        ]) {
            column.push(value);
        }
        lib.folder_path.push(t.path);
        lib.file_name.push(
            std::path::Path::new(t.path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(""),
        );
        lib.analysis_path.push("");
        lib.date_added.push(t.date_added);
        lib.release_date.push("");
        lib.date_created.push(t.date_created);
        lib.lyricist.push(t.lyricist);
        lib.message.push(t.message);
        let artist = lib.artists.push(t.artist);
        let album = lib.albums.push(t.album);
        let genre = lib.genres.push(t.genre);
        let label = lib.labels.push(t.label);
        lib.artist.push(artist);
        lib.album.push(album);
        lib.genre.push(genre);
        lib.label.push(label);
        lib.artist_ids.push(artist + 1);
        lib.album_ids.push(album + 1);
        lib.genre_ids.push(genre + 1);
        lib.label_ids.push(label + 1);
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
        lib.bitrate.push(t.bitrate);
        lib.sample_rate.push(t.sample_rate);
        lib.file_size.push(t.file_size);
        lib.track_number.push(t.track_number);
        lib.disc_no.push(t.disc_no);
        lib.file_type.push(t.file_type);
        lib.bit_depth.push(t.bit_depth);
        lib.publish.push(u8::from(t.publish));
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

/// Adds an intelligent playlist with contradictory stored membership.
/// Link Export uses this to prove that the rule remains authoritative.
pub fn add_smart_playlist_with_members(
    lib: &mut Library,
    name: &str,
    rows: &[Row],
    rule: &str,
) -> usize {
    add_list_with(lib, name, rows, crate::ATTRIBUTE_SMART, rule)
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
