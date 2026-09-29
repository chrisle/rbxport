//! One track's full record, for the information panel and the deck's INFO tab.
//!
//! A point read by id, on demand. The columnar index in `rbl-index` carries
//! what every row of the track list needs and nothing else; the twenty-odd
//! columns behind the Summary and Info tabs are wanted for one track at a
//! time, and widening the index for them would cost every row in a 38,681
//! track library for the sake of the one being looked at.
//!
//! # What the columns hold
//!
//! Read off the user's own library with
//! `cargo run -p rbl-db --example track_details`, read-only, over all 38,681
//! live rows rather than one:
//!
//! - **`FileType`** follows the extension: 1 is `.mp3` (33,849), 4 `.m4a`
//!   (2,727), 5 `.flac` (56), 11 `.wav` (1,165), 12 `.aiff`/`.aif` (867).
//!   One `.m4a` carries 6 — ALAC or AAC, undetermined.
//! - **`OrgArtistID`, `ComposerID`, `RemixerID`** point into `djmdArtist`,
//!   the same table as `ArtistID`: every one of the 47, 3,118 and 634 set
//!   values resolves there.
//! - **The album artist lives on the album**, `djmdAlbum.AlbumArtistID`, and
//!   that too points into `djmdArtist` (1,591 of 9,785 albums, all resolve).
//! - **`Lyricist`** is a plain text column on the track, not a reference.
//! - **`Subtitle`** holds what the ID3 subtitle frame holds — "extended mix",
//!   "Original Mix", "remastered" on the 479 rows that have one — which is
//!   the field rekordbox labels Mix Name. Read as that; not written, because
//!   no capture shows a non-empty Mix Name to confirm it.
//! - **`HotCueAutoLoad`** is the text `"on"` on all 38,681 rows. What the
//!   unticked box is spelled as has never been seen, so it is not written.
//! - **`DeliveryControl`** is `"on"` on 220 rows, `""` on 825 and NULL on the
//!   rest; **`DeliveryComment`** is never non-empty. These read as the
//!   "Publish track information" box and the "Message" field — the KUVO
//!   delivery pair — and are shown but not written: KUVO publishing is not
//!   a thing this app does, by Chris's word (2026-09-18).
//! - **`DateCreated`** is `YYYY-MM-DD` on every row (length 10, all 38,681).
//! - **`SearchStr`** is NULL on every track and every artist, so an edit that
//!   leaves it alone stales nothing.

use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;

/// Everything the panel shows for one track.
///
/// Numbers that the library leaves NULL come back as 0, and text as empty, so
/// a consumer never has to distinguish "absent" from "zero" — rekordbox's own
/// Info tab prints 0 in an empty Year or Track number box.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrackDetails {
    pub id: String,
    /// The ids of the My Tags on the track (`djmdSongMyTag`). Empty on a
    /// library without the table.
    pub my_tags: Vec<String>,
    pub title: String,
    pub artist_id: u32,
    pub artist: String,
    pub album_id: u32,
    pub album: String,
    pub album_artist: String,
    pub original_artist_id: u32,
    pub original_artist: String,
    pub composer: String,
    pub remixer_id: u32,
    pub remixer: String,
    pub lyricist: String,
    pub genre_id: u32,
    pub genre: String,
    pub label_id: u32,
    pub label: String,
    pub key_id: u32,
    pub key: String,
    pub comment: String,
    /// `Subtitle` — see the module docs.
    pub mix_name: String,
    /// `DeliveryComment` — see the module docs.
    pub message: String,
    /// `ColorID` as stored: `"0"` or NULL for none, `"1"` to `"8"` otherwise.
    pub color: String,
    pub rating: u8,
    pub bpm_x100: u32,
    pub duration_sec: u32,
    pub year: u32,
    pub track_number: u32,
    pub disc_number: u32,
    pub play_count: u32,
    /// rekordbox's own code — see the module docs for what each is.
    pub file_type: u32,
    pub file_size: u64,
    pub bitrate: u32,
    pub sample_rate: u32,
    pub bit_depth: u32,
    pub date_created: String,
    pub release_date: String,
    /// `FolderPath`: the absolute path of the audio file.
    pub path: String,
    /// `HotCueAutoLoad == "on"`.
    pub hot_cue_auto_load: bool,
    /// `DeliveryControl == "on"`.
    pub publish: bool,
}

/// A Hot Cue Bank row from rekordbox's master database.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HotCueBank {
    pub id: u32,
    pub name: String,
    pub folder: bool,
}

/// One of the three saved cue points belonging to a Hot Cue Bank.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HotCueBankCue {
    pub slot: u8,
    pub content: u32,
    pub in_ms: u32,
    pub out_ms: Option<u32>,
    pub color: u32,
    pub color_table_index: u32,
    pub active_loop: bool,
    pub beat_loop_size: u32,
    pub cue_microsec: u32,
}

/// Lists live Hot Cue Banks below `parent`; `None` selects rekordbox's root.
pub fn hot_cue_banks(conn: &Connection, parent: Option<u32>) -> Result<Vec<HotCueBank>> {
    let parent = parent.map_or_else(|| "root".to_owned(), |id| id.to_string());
    let mut statement = conn.prepare(
        "SELECT ID, COALESCE(Name, ''), COALESCE(Attribute, 0)
         FROM djmdHotCueBanklist
         WHERE ParentID = ?1 AND rb_local_deleted = 0
         ORDER BY Seq, ID",
    )?;
    let rows = statement.query_map(params![parent], |row| {
        Ok(HotCueBank {
            id: small(number(row, 0)),
            name: text(row, 1),
            // rekordbox uses Attribute=1 for a folder, as it does for its
            // playlist tree; leaf banks have Attribute=0.
            folder: number(row, 2) == 1,
        })
    })?;
    Ok(rows.filter_map(std::result::Result::ok).collect())
}

/// Reads the up-to-three cue points assigned to a Hot Cue Bank.
pub fn hot_cue_bank_cues(conn: &Connection, bank: u32) -> Result<Vec<HotCueBankCue>> {
    let mut statement = conn.prepare(
        "SELECT TrackNo, ContentID, InMsec, OutMsec, Color, ColorTableIndex,
                ActiveLoop, BeatLoopSize, CueMicrosec
         FROM djmdSongHotCueBanklist
         WHERE HotCueBanklistID = ?1 AND rb_local_deleted = 0
         ORDER BY TrackNo, ID",
    )?;
    let rows = statement.query_map(params![bank.to_string()], |row| {
        let out: Option<i64> = row.get(3)?;
        Ok(HotCueBankCue {
            slot: u8::try_from(number(row, 0)).unwrap_or(0),
            content: small(number(row, 1)),
            in_ms: small(number(row, 2)),
            out_ms: out.filter(|value| *value >= 0).map(small),
            color: small(number(row, 4)),
            color_table_index: small(number(row, 5)),
            active_loop: number(row, 6) != 0,
            beat_loop_size: small(number(row, 7)),
            cue_microsec: small(number(row, 8)),
        })
    })?;
    Ok(rows.filter_map(std::result::Result::ok).filter(|cue| (1..=3).contains(&cue.slot)).collect())
}

/// One of the artist references rekordbox exposes as a browse category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtistRole {
    Original,
    Remixer,
}

impl ArtistRole {
    fn column(self) -> &'static str {
        match self {
            Self::Original => "OrgArtistID",
            Self::Remixer => "RemixerID",
        }
    }
}

/// The live artists used by an advanced browse category, in rekordbox id space.
pub fn artist_role_names(conn: &Connection, role: ArtistRole) -> Result<Vec<(u32, String)>> {
    let sql = format!(
        "SELECT artist.ID, COALESCE(artist.Name, '') FROM djmdArtist artist
         WHERE artist.rb_local_deleted = 0 AND EXISTS (
           SELECT 1 FROM djmdContent content WHERE content.{} = artist.ID AND content.rb_local_deleted = 0
         ) ORDER BY artist.Name COLLATE NOCASE, artist.ID", role.column());
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map([], |row| Ok((small(number(row, 0)), text(row, 1))))?;
    Ok(rows.filter_map(std::result::Result::ok).collect())
}

/// The live tracks assigned to one original artist or remixer.
pub fn artist_role_track_ids(conn: &Connection, role: ArtistRole, artist: u32) -> Result<Vec<u32>> {
    let sql = format!(
        "SELECT ID FROM djmdContent WHERE {} = ?1 AND rb_local_deleted = 0",
        role.column()
    );
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map([artist], |row| Ok(small(number(row, 0))))?;
    Ok(rows.filter_map(std::result::Result::ok).collect())
}

/// Saved notes for the live memory and hot cues of one track.
pub fn cue_comments(
    conn: &Connection,
    id: &str,
) -> Result<std::collections::HashMap<String, String>> {
    let mut statement = conn.prepare(
        "SELECT ID, COALESCE(Comment, '') FROM djmdCue WHERE ContentID = ?1 AND rb_local_deleted = 0",
    )?;
    let rows = statement.query_map([id], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The `Color` value of each live memory cue. Rekordbox stores 0..7 for the
/// named palette and 255 for no colour.
pub fn memory_cue_colours(
    conn: &Connection,
    id: &str,
) -> Result<std::collections::HashMap<String, u8>> {
    let mut statement = conn.prepare(
        "SELECT ID, Color FROM djmdCue WHERE ContentID = ?1 AND Kind = 0 AND rb_local_deleted = 0",
    )?;
    let rows = statement.query_map([id], |row| {
        let value: i64 = row.get(1)?;
        Ok((row.get(0)?, u8::try_from(value).unwrap_or(255)))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Reads one live track, or `None` when there is no such track.
pub fn track_details(conn: &Connection, id: &str) -> Result<Option<TrackDetails>> {
    let Some(mut details) = track_row(conn, id)? else {
        return Ok(None);
    };
    details.my_tags = my_tags_of(conn, id);
    Ok(Some(details))
}

/// Browser page enrichment omits the separate My Tag id query. Its names are
/// read only when that column is visible.
pub fn browser_details(conn: &Connection, id: &str) -> Result<Option<TrackDetails>> {
    track_row(conn, id)
}

/// Live tracks connected to `seed` by a live recommendation-like relation.
///
/// Rekordbox treats `djmdRecommendLike` as bidirectional. Relation row order
/// is retained so the caller can apply its requested track sort explicitly.
pub fn matching_ids(conn: &Connection, seed: u32) -> Result<Vec<u32>> {
    let seed = seed.to_string();
    let mut statement = conn.prepare(
        "SELECT CASE WHEN relation.ContentID1 = ?1
                     THEN relation.ContentID2 ELSE relation.ContentID1 END
         FROM djmdRecommendLike relation
         JOIN djmdContent first
           ON first.ID = relation.ContentID1 AND first.rb_local_deleted = 0
         JOIN djmdContent second
           ON second.ID = relation.ContentID2 AND second.rb_local_deleted = 0
         WHERE relation.rb_local_deleted = 0
           AND (relation.ContentID1 = ?1 OR relation.ContentID2 = ?1)
         ORDER BY relation.rowid",
    )?;
    let rows = statement.query_map([seed], |row| row.get::<_, Option<String>>(0))?;
    let mut ids = Vec::new();
    for id in rows {
        let Some(id) = id? else { continue };
        if let Ok(id) = id.parse() {
            ids.push(id);
        }
    }
    Ok(ids)
}

/// The My Tags on a track, by id; none on a library without the table.
fn my_tags_of(conn: &Connection, id: &str) -> Vec<String> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT MyTagID FROM djmdSongMyTag WHERE ContentID = ?1 AND rb_local_deleted = 0 ORDER BY TrackNo, MyTagID",
    ) else {
        return Vec::new();
    };
    stmt.query_map([id], |r| r.get::<_, Option<String>>(0))
        .map(|rows| rows.filter_map(std::result::Result::ok).flatten().collect())
        .unwrap_or_default()
}

/// The browser's My Tag column uses names, not opaque tag ids.
pub fn my_tag_names(conn: &Connection, id: &str) -> Vec<String> {
    let Ok(mut stmt) = conn.prepare(
        "SELECT COALESCE(t.Name, '') FROM djmdSongMyTag s
         JOIN djmdMyTag t ON t.ID = s.MyTagID AND t.rb_local_deleted = 0
         WHERE s.ContentID = ?1 AND s.rb_local_deleted = 0
         ORDER BY s.TrackNo, s.MyTagID",
    ) else {
        return Vec::new();
    };
    stmt.query_map([id], |row| row.get::<_, String>(0))
        .map(|rows| {
            rows.filter_map(std::result::Result::ok)
                .filter(|name| !name.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn track_row(conn: &Connection, id: &str) -> Result<Option<TrackDetails>> {
    // One statement with the lookups joined, rather than a query per
    // reference: seven round trips for one row is seven times the work for
    // no reason, and the joins are on primary keys.
    conn.query_row(
        "SELECT c.ID, c.Title, c.ArtistID, artist.Name, c.AlbumID, album.Name,
                album_artist.Name, c.OrgArtistID, org.Name, composer.Name,
                c.RemixerID, remixer.Name, c.Lyricist, c.GenreID, genre.Name,
                c.LabelID, label.Name, c.KeyID, key.ScaleName, c.Commnt, c.Subtitle,
                c.DeliveryComment, c.ColorID, c.Rating, c.BPM, c.Length,
                c.ReleaseYear, c.TrackNo, c.DiscNo, c.DJPlayCount, c.FileType,
                c.FileSize, c.BitRate, c.SampleRate, c.BitDepth, c.DateCreated,
                c.ReleaseDate, c.FolderPath, c.HotCueAutoLoad, c.DeliveryControl
         FROM djmdContent c
         LEFT JOIN djmdArtist artist ON artist.ID = c.ArtistID
         LEFT JOIN djmdAlbum album ON album.ID = c.AlbumID
         LEFT JOIN djmdArtist album_artist ON album_artist.ID = album.AlbumArtistID
         LEFT JOIN djmdArtist org ON org.ID = c.OrgArtistID
         LEFT JOIN djmdArtist composer ON composer.ID = c.ComposerID
         LEFT JOIN djmdArtist remixer ON remixer.ID = c.RemixerID
         LEFT JOIN djmdGenre genre ON genre.ID = c.GenreID
         LEFT JOIN djmdLabel label ON label.ID = c.LabelID
         LEFT JOIN djmdKey key ON key.ID = c.KeyID
         WHERE c.ID = ?1 AND c.rb_local_deleted = 0",
        params![id],
        |r| {
            Ok(TrackDetails {
                id: text(r, 0),
                my_tags: Vec::new(),
                title: text(r, 1),
                artist_id: small(number(r, 2)),
                artist: text(r, 3),
                album_id: small(number(r, 4)),
                album: text(r, 5),
                album_artist: text(r, 6),
                original_artist_id: small(number(r, 7)),
                original_artist: text(r, 8),
                composer: text(r, 9),
                remixer_id: small(number(r, 10)),
                remixer: text(r, 11),
                lyricist: text(r, 12),
                genre_id: small(number(r, 13)),
                genre: text(r, 14),
                label_id: small(number(r, 15)),
                label: text(r, 16),
                key_id: small(number(r, 17)),
                key: text(r, 18),
                comment: text(r, 19),
                mix_name: text(r, 20),
                message: text(r, 21),
                color: text(r, 22),
                // Stars as a count, 0 to 5, as the index reads them.
                rating: u8::try_from(number(r, 23)).unwrap_or(5).min(5),
                bpm_x100: small(number(r, 24)),
                duration_sec: small(number(r, 25)),
                year: small(number(r, 26)),
                track_number: small(number(r, 27)),
                disc_number: small(number(r, 28)),
                play_count: small(number(r, 29)),
                file_type: small(number(r, 30)),
                file_size: u64::try_from(number(r, 31)).unwrap_or(0),
                bitrate: small(number(r, 32)),
                sample_rate: small(number(r, 33)),
                bit_depth: small(number(r, 34)),
                date_created: text(r, 35),
                release_date: text(r, 36),
                path: text(r, 37),
                hot_cue_auto_load: text(r, 38) == "on",
                publish: text(r, 39) == "on",
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

/// A text column, with NULL as empty.
fn text(r: &rusqlite::Row<'_>, index: usize) -> String {
    r.get::<_, Option<String>>(index)
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// An integer column, with NULL as zero.
///
/// Several nominally-numeric columns are TEXT in the real schema (`ColorID`,
/// `DBVersion`), so reading them as `i64` fails on some rows and not others;
/// whatever type the cell has is read and parsed, the way `rbl-index` does.
fn number(r: &rusqlite::Row<'_>, index: usize) -> i64 {
    match r.get::<_, Value>(index).unwrap_or(Value::Null) {
        Value::Integer(n) => n,
        Value::Real(f) => real_to_i64(f),
        Value::Text(s) => s.trim().parse::<f64>().map_or(0, real_to_i64),
        Value::Null | Value::Blob(_) => 0,
    }
}

/// Saturating rather than a lossy `as`: a NaN must not become an arbitrary
/// integer, and no rekordbox field is anywhere near the bound.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn real_to_i64(f: f64) -> i64 {
    if f.is_nan() {
        0
    } else if f >= i64::MAX as f64 {
        i64::MAX
    } else if f <= i64::MIN as f64 {
        i64::MIN
    } else {
        f as i64
    }
}

/// A count or size that is never negative.
fn small(n: i64) -> u32 {
    u32::try_from(n.max(0)).unwrap_or(u32::MAX)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::fixture::{self, Shape, track_id};
    use crate::{Library, OpenMode};

    fn open() -> (tempfile::TempDir, Library) {
        let dir = tempfile::tempdir().expect("tempdir");
        let location = fixture::build(dir.path(), Shape::default()).expect("fixture");
        let library = Library::open(location, OpenMode::ReadWrite).expect("open");
        (dir, library)
    }

    #[test]
    fn cue_notes_keep_saved_text_and_exclude_deleted_and_other_tracks() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE djmdCue (ID TEXT, ContentID TEXT, Comment TEXT, rb_local_deleted INTEGER);
            INSERT INTO djmdCue VALUES ('1', 'track', '136 BPM', 0),
            ('2', 'track', '136-128 BPM', 0), ('3', 'track', NULL, 0),
            ('4', 'track', 'deleted', 1), ('5', 'other', 'other track', 0);").unwrap();
        let notes = cue_comments(&conn, "track").unwrap();
        assert_eq!(notes.len(), 3);
        assert_eq!(notes["1"], "136 BPM");
        assert_eq!(notes["2"], "136-128 BPM");
        assert_eq!(notes["3"], "");
    }

    #[test]
    fn hot_cue_banks_and_slots_follow_rekordbox_order() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE djmdHotCueBanklist (ID TEXT, Seq INTEGER, Name TEXT, Attribute INTEGER, ParentID TEXT, rb_local_deleted INTEGER);
             CREATE TABLE djmdSongHotCueBanklist (
                 ID TEXT, HotCueBanklistID TEXT, TrackNo INTEGER, ContentID TEXT, InMsec INTEGER, OutMsec INTEGER,
                 Color INTEGER, ColorTableIndex INTEGER, ActiveLoop INTEGER, BeatLoopSize INTEGER, CueMicrosec INTEGER,
                 rb_local_deleted INTEGER
             );
             INSERT INTO djmdHotCueBanklist VALUES ('10', 2, 'Late', 0, 'root', 0), ('9', 1, 'Folder', 1, 'root', 0), ('11', 3, 'gone', 0, 'root', 1);
             INSERT INTO djmdSongHotCueBanklist VALUES
               ('a', '10', 2, '200', 2200, NULL, 3, 21, 0, 0, 0, 0),
               ('b', '10', 1, '100', 1100, 1800, 2, 20, 1, 262145, 7, 0),
               ('c', '10', 4, '400', 0, NULL, 0, 0, 0, 0, 0, 0);",
        ).unwrap();
        assert_eq!(hot_cue_banks(&conn, None).unwrap(), vec![
            HotCueBank { id: 9, name: "Folder".into(), folder: true },
            HotCueBank { id: 10, name: "Late".into(), folder: false },
        ]);
        assert_eq!(hot_cue_bank_cues(&conn, 10).unwrap(), vec![
            HotCueBankCue { slot: 1, content: 100, in_ms: 1100, out_ms: Some(1800), color: 2, color_table_index: 20, active_loop: true, beat_loop_size: 262145, cue_microsec: 7 },
            HotCueBankCue { slot: 2, content: 200, in_ms: 2200, out_ms: None, color: 3, color_table_index: 21, active_loop: false, beat_loop_size: 0, cue_microsec: 0 },
        ]);
    }

    #[test]
    fn matching_relations_are_bidirectional_live_and_source_ordered() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE djmdContent (ID TEXT, rb_local_deleted INTEGER);
             CREATE TABLE djmdRecommendLike (
                 ContentID1 TEXT, ContentID2 TEXT, rb_local_deleted INTEGER
             );
             INSERT INTO djmdContent VALUES
                 ('10', 0), ('20', 0), ('30', 0), ('40', 1), ('50', 0);
             INSERT INTO djmdRecommendLike VALUES
                 ('10', '30', 0), ('20', '10', 0), ('10', '40', 0),
                 ('10', '50', 1);",
        )
        .unwrap();

        assert_eq!(matching_ids(&conn, 10).unwrap(), [30, 20]);
    }

    #[test]
    fn a_bare_fixture_row_reads_with_zeros_and_blanks() {
        let (_dir, library) = open();
        let details = track_details(library.connection(), &track_id(1))
            .expect("read")
            .expect("row");
        assert_eq!(details.title, "Track 001");
        assert_eq!(details.path, "/fixture/audio/track001.mp3");
        assert_eq!(details.bpm_x100, 12_801);
        assert_eq!(details.duration_sec, 300);
        assert_eq!(details.artist, "");
        assert_eq!(details.album_artist, "");
        assert_eq!(details.year, 0);
        assert_eq!(details.file_size, 0);
        assert!(!details.hot_cue_auto_load);
        assert!(!details.publish);
    }

    #[test]
    fn my_tag_column_reads_names_in_track_order() {
        let (_dir, library) = open();
        let conn = library.connection();
        let track = track_id(1);
        let stamp = rbl_core::time::now();
        for (id, tag, order) in [
            ("song-tag-warm", fixture::MY_TAG_WARM_UP, 1),
            ("song-tag-peak", fixture::MY_TAG_PEAK, 2),
        ] {
            conn.execute(
                "INSERT INTO djmdSongMyTag
                 (ID, MyTagID, ContentID, TrackNo, rb_local_deleted, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 0, ?5, ?5)",
                params![id, tag, track, order, stamp],
            )
            .unwrap();
        }
        assert_eq!(my_tag_names(conn, &track), ["Warm-up", "Peak"]);
    }

    #[test]
    fn every_reference_resolves_through_its_table() {
        let (_dir, library) = open();
        let conn = library.connection();
        let stamp = rbl_core::time::now();
        for (table, id, name) in [
            ("djmdArtist", "a1", "Artist One"),
            ("djmdArtist", "a2", "Album Artist"),
            ("djmdArtist", "a3", "Original"),
            ("djmdArtist", "a4", "Composer"),
            ("djmdArtist", "a5", "Remixer"),
            ("djmdGenre", "g1", "House"),
            ("djmdLabel", "l1", "Anjuna"),
        ] {
            conn.execute(
                &format!(
                    "INSERT INTO {table} (ID, Name, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)"
                ),
                params![id, name, stamp],
            )
            .expect("insert");
        }
        conn.execute(
            "INSERT INTO djmdAlbum (ID, Name, AlbumArtistID, created_at, updated_at)
             VALUES ('al1', 'The Album', 'a2', ?1, ?1)",
            params![stamp],
        )
        .expect("album");
        conn.execute(
            "INSERT INTO djmdKey (ID, ScaleName, created_at, updated_at) VALUES ('k1', 'Fm', ?1, ?1)",
            params![stamp],
        )
        .expect("key");
        conn.execute(
            "UPDATE djmdContent SET ArtistID = 'a1', AlbumID = 'al1', OrgArtistID = 'a3',
                ComposerID = 'a4', RemixerID = 'a5', GenreID = 'g1', LabelID = 'l1', KeyID = 'k1',
                Lyricist = 'Words', Subtitle = 'extended mix', DeliveryComment = 'hello',
                ColorID = '2', Rating = 4, ReleaseYear = 2023, TrackNo = 4, DiscNo = 2,
                DJPlayCount = 7, FileType = 11, FileSize = 47322584, BitRate = 1411,
                SampleRate = 44100, BitDepth = 16, DateCreated = '2023-08-06',
                ReleaseDate = '2023-08-01', HotCueAutoLoad = 'on', DeliveryControl = 'on'
             WHERE ID = ?1",
            params![track_id(5)],
        )
        .expect("update");

        let d = track_details(conn, &track_id(5))
            .expect("read")
            .expect("row");
        assert_eq!(d.artist, "Artist One");
        assert_eq!(d.album, "The Album");
        assert_eq!(d.album_artist, "Album Artist");
        assert_eq!(d.original_artist, "Original");
        assert_eq!(d.composer, "Composer");
        assert_eq!(d.remixer, "Remixer");
        assert_eq!(d.lyricist, "Words");
        assert_eq!(d.genre, "House");
        assert_eq!(d.label, "Anjuna");
        assert_eq!(d.key, "Fm");
        assert_eq!(d.mix_name, "extended mix");
        assert_eq!(d.message, "hello");
        assert_eq!(d.color, "2");
        assert_eq!(d.rating, 4);
        assert_eq!(d.year, 2023);
        assert_eq!(d.track_number, 4);
        assert_eq!(d.disc_number, 2);
        assert_eq!(d.play_count, 7);
        assert_eq!(d.file_type, 11);
        assert_eq!(d.file_size, 47_322_584);
        assert_eq!(d.bitrate, 1411);
        assert_eq!(d.sample_rate, 44_100);
        assert_eq!(d.bit_depth, 16);
        assert_eq!(d.date_created, "2023-08-06");
        assert_eq!(d.release_date, "2023-08-01");
        assert!(d.hot_cue_auto_load);
        assert!(d.publish);
    }

    #[test]
    fn a_deleted_or_unknown_track_is_none_not_an_error() {
        let (_dir, library) = open();
        let conn = library.connection();
        conn.execute(
            "UPDATE djmdContent SET rb_local_deleted = 1 WHERE ID = ?1",
            params![track_id(3)],
        )
        .expect("soft delete");
        assert_eq!(track_details(conn, &track_id(3)).expect("read"), None);
        assert_eq!(track_details(conn, "no-such-id").expect("read"), None);
    }

    #[test]
    fn a_number_stored_as_text_still_reads() {
        let (_dir, library) = open();
        let conn = library.connection();
        conn.execute(
            "UPDATE djmdContent SET BPM = '12850.0', DJPlayCount = 'three' WHERE ID = ?1",
            params![track_id(2)],
        )
        .expect("update");
        let d = track_details(conn, &track_id(2))
            .expect("read")
            .expect("row");
        assert_eq!(d.bpm_x100, 12_850);
        assert_eq!(
            d.play_count, 0,
            "unparseable text reads as zero rather than failing the row"
        );
    }
}
