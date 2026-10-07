//! Loads the whole library into the columnar index in one pass.
//!
//! Only live rows and only the columns the app actually reads: on the user's
//! library that is 38,681 of 115,613 content rows, so filtering in SQL rather
//! than in Rust is most of the win.

use std::collections::HashMap;
use std::time::Instant;

use rbl_db::Library as Db;
use rusqlite::Connection;

use crate::{strings::StrColumn, Cue, Cues, Library, Playlists, Row, TagCategory, NO_ID};

/// Reads text without letting one malformed library value abort the index.
///
/// SQLite does not require bytes stored with TEXT affinity to be valid UTF-8,
/// and real rekordbox libraries can contain such values. The interface can
/// still browse the rest of the library if the invalid sequence is replaced
/// for display, which is the same boundary behavior used by the diagnostic
/// and database inspection tools.
fn text(row: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<Option<String>> {
    use rusqlite::types::ValueRef;
    Ok(match row.get_ref(idx)? {
        ValueRef::Null => None,
        ValueRef::Text(value) | ValueRef::Blob(value) => {
            Some(String::from_utf8_lossy(value).into_owned())
        }
        ValueRef::Integer(value) => Some(value.to_string()),
        ValueRef::Real(value) => Some(value.to_string()),
    })
}

fn text_or_default(row: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<String> {
    Ok(text(row, idx)?.unwrap_or_default())
}

/// Converts a REAL to an integer without a lossy cast: NaN becomes 0 and
/// out-of-range values saturate.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "value is clamped into i64 range and NaN is handled explicitly"
)]
fn real_to_i64(v: f64) -> i64 {
    if v.is_nan() {
        0
    } else {
        // `clamp` then round-trip through the integer range.
        let clamped = v.clamp(i64::MIN as f64, i64::MAX as f64);
        clamped.trunc() as i64
    }
}

/// Clamps into range before narrowing, so the conversion cannot truncate.
fn clamp_u32(v: i64) -> u32 {
    u32::try_from(v.clamp(0, i64::from(u32::MAX))).unwrap_or(0)
}
fn clamp_u16(v: i64) -> u16 {
    u16::try_from(v.clamp(0, i64::from(u16::MAX))).unwrap_or(0)
}
fn clamp_u8(v: i64, max: u8) -> u8 {
    u8::try_from(v.clamp(0, i64::from(max))).unwrap_or(0)
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LoadStats {
    pub tracks: usize,
    pub playlists: usize,
    pub memberships: usize,
    /// History sessions, and the folders they are filed under.
    pub histories: usize,
    /// Tracks played across every session.
    pub plays: usize,
    pub read_ms: u128,
    pub index_ms: u128,
    pub heap_bytes: usize,
}

/// Reads a numeric column that rekordbox may store as INTEGER, REAL or TEXT.
///
/// Several nominally-numeric fields are TEXT in the real schema (`DBVersion`,
/// `ColorID`), so reading them as `i64` fails at runtime on some rows and not
/// others. Being tolerant here turns a class of crash into a `0`.
fn num(row: &rusqlite::Row<'_>, idx: usize) -> rusqlite::Result<i64> {
    use rusqlite::types::ValueRef;
    #[allow(
        clippy::match_same_arms,
        reason = "each arm documents a distinct storage case"
    )]
    Ok(match row.get_ref(idx)? {
        ValueRef::Integer(v) => v,
        ValueRef::Null => 0,
        // Saturating rather than a lossy `as`: no rekordbox field is this large,
        // and a NaN must not become an arbitrary integer.
        ValueRef::Real(v) => real_to_i64(v),
        ValueRef::Text(t) => std::str::from_utf8(t)
            .ok()
            .and_then(|s| s.trim().parse::<f64>().ok())
            .map_or(0, real_to_i64),
        // A blob in a numeric column is meaningless; treat it as absent.
        ValueRef::Blob(_) => 0,
    })
}

/// Reads a lookup table into an interner, returning rekordbox id -> dense id.
fn load_lookup(
    conn: &Connection,
    table: &str,
    name_column: &str,
    interner: &mut crate::strings::Interner,
    wire_ids: &mut Vec<u32>,
) -> rusqlite::Result<HashMap<String, u32>> {
    // Identifiers come from the constants below, never from user input.
    let sql = format!("SELECT ID, {name_column} FROM {table}");
    let mut stmt = conn.prepare(&sql)?;
    let mut map = HashMap::new();
    let rows = stmt.query_map([], |r| Ok((text(r, 0)?, text(r, 1)?)))?;
    for row in rows {
        let (Some(id), name) = row? else { continue };
        let dense = interner.push(name.as_deref().unwrap_or(""));
        wire_ids.push(id.parse::<u32>().unwrap_or(0));
        map.insert(id, dense);
    }
    Ok(map)
}

/// A number that changes when the library's content does, and not otherwise.
///
/// `MAX(rb_local_usn)` over the tables the index reads, mixed with each one's
/// live row count so a deletion moves it too. Two queries per table against
/// indexed columns, which is milliseconds — far cheaper than the 543 ms of
/// decryption it decides whether to skip.
///
/// This exists because rekordbox rewrites the write-ahead log constantly
/// without changing a row, so a snapshot keyed to the file was refused on
/// every start rekordbox happened to be running for.
///
/// `djmdCue` is in the mix because the snapshot carries the cues, but it is
/// read differently: its `rb_local_usn` is NULL on every one of the reference
/// library's 1,041,056 cues, so rekordbox does not move it, and its live row
/// count is a 300 ms scan rather than an index hit. What is cheap — 17 µs
/// warm, both indexed — is `MAX(rowid)`, which moves when rekordbox adds a
/// cue, and `MAX(rb_local_usn)`, which moves when *this app's* writer edits
/// one. A cue rekordbox moves or recolours in place is the gap: it changes
/// neither, and the snapshot keeps the old cue until something else in the
/// library changes. `[UNKNOWN]` whether rekordbox bumps the track's own usn
/// when it edits a cue, which would close the gap for free; the hot-cue diff
/// recording answers that.
pub fn content_version(db: &Db) -> rusqlite::Result<u64> {
    let conn = db.connection();
    let mut mixed: u64 = 0;
    for table in ["djmdContent", "djmdPlaylist", "djmdSongPlaylist"] {
        let (usn, rows): (i64, i64) = conn.query_row(
            &format!(
                "SELECT COALESCE(MAX(rb_local_usn), 0), COUNT(*) FROM {table}
                 WHERE rb_local_deleted = 0"
            ),
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        // Order matters, so a row moving between tables cannot cancel out.
        mixed = mix(mixed, usn, rows);
    }
    let (usn, last_row): (i64, i64) = conn.query_row(
        "SELECT (SELECT COALESCE(MAX(rb_local_usn), 0) FROM djmdCue),
                (SELECT COALESCE(MAX(rowid), 0) FROM djmdCue)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(mix(mixed, usn, last_row))
}

fn mix(acc: u64, a: i64, b: i64) -> u64 {
    acc.rotate_left(17)
        .wrapping_add(a.unsigned_abs())
        .rotate_left(17)
        .wrapping_add(b.unsigned_abs())
}

/// Builds the index from an open (read-only is fine) library.
pub fn load(db: &Db) -> rusqlite::Result<(Library, LoadStats)> {
    load_with_cue_reader(db, None)
}

/// Startup may supply an independent read-only connection so cue reads overlap
/// playlist, history and search metadata reads. Ordinary edit reloads keep one
/// connection, preserving visibility of the writer's transaction.
pub fn load_with_cue_reader(
    db: &Db,
    cue_reader: Option<Db>,
) -> rusqlite::Result<(Library, LoadStats)> {
    let conn = db.connection();
    let t0 = Instant::now();
    let mut lib = Library::default();
    let cloud_root = rbl_db::cloud_contents_root();
    let mut stats = LoadStats::default();

    let artists = load_lookup(
        conn,
        "djmdArtist",
        "Name",
        &mut lib.artists,
        &mut lib.artist_ids,
    )?;
    let albums = load_lookup(
        conn,
        "djmdAlbum",
        "Name",
        &mut lib.albums,
        &mut lib.album_ids,
    )?;
    let genres = load_lookup(
        conn,
        "djmdGenre",
        "Name",
        &mut lib.genres,
        &mut lib.genre_ids,
    )?;
    let labels = load_lookup(
        conn,
        "djmdLabel",
        "Name",
        &mut lib.labels,
        &mut lib.label_ids,
    )?;
    let mut key_ids = Vec::new();
    let keys = load_lookup(conn, "djmdKey", "ScaleName", &mut lib.keys, &mut key_ids)?;

    let mut stmt = conn.prepare(
        "SELECT ID, Title, ArtistID, AlbumID, GenreID, LabelID, KeyID,
                BPM, Length, Rating, ColorID, FolderPath, FileNameL,
                AnalysisDataPath, DJPlayCount, StockDate, ReleaseDate, Commnt, Analysed,
                ImagePath, BitRate, SampleRate, FileSize, ReleaseYear,
                TrackNo, DiscNo, FileType, BitDepth, Lyricist, DateCreated,
                DeliveryControl, DeliveryComment
         FROM djmdContent
         WHERE rb_local_deleted = 0",
    )?;

    // Capacity guesses sized from the real library so the arenas rarely regrow.
    let expected = 40_000;
    lib.ids = Vec::with_capacity(expected);
    lib.title = StrColumn::with_capacity(expected, expected * 40);
    lib.title_folded = StrColumn::with_capacity(expected, expected * 40);
    lib.comment = StrColumn::with_capacity(expected, expected * 16);
    lib.folder_path = StrColumn::with_capacity(expected, expected * 90);
    lib.file_name = StrColumn::with_capacity(expected, expected * 40);
    lib.analysis_path = StrColumn::with_capacity(expected, expected * 60);
    lib.artwork_path = StrColumn::with_capacity(expected, expected * 60);
    lib.date_added = StrColumn::with_capacity(expected, expected * 11);
    lib.release_date = StrColumn::with_capacity(expected, expected * 11);
    lib.date_created = StrColumn::with_capacity(expected, expected * 11);
    // Both empty on almost every row of the reference library.
    lib.lyricist = StrColumn::with_capacity(expected, 0);
    lib.message = StrColumn::with_capacity(expected, 0);

    let mut content_row: HashMap<u64, Row> = HashMap::with_capacity(expected);

    let lookup = |map: &HashMap<String, u32>, id: Option<String>| -> u32 {
        id.and_then(|k| map.get(&k).copied()).unwrap_or(NO_ID)
    };

    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let Some(id_text) = text(r, 0)? else {
            continue;
        };
        let row_index = u32::try_from(lib.ids.len()).unwrap_or(u32::MAX);

        let title = text_or_default(r, 1)?;
        lib.title_folded.push(&crate::strings::fold(&title));
        lib.title.push(&title);

        lib.artist.push(lookup(&artists, text(r, 2)?));
        lib.album.push(lookup(&albums, text(r, 3)?));
        lib.genre.push(lookup(&genres, text(r, 4)?));
        lib.label.push(lookup(&labels, text(r, 5)?));
        lib.key.push(lookup(&keys, text(r, 6)?));

        // BPM is stored x100; Length is whole seconds.
        lib.bpm_x100.push(clamp_u32(num(r, 7)?));
        lib.length_sec.push(clamp_u32(num(r, 8)?));
        lib.rating.push(clamp_u8(num(r, 9)?, 5));
        lib.color.push(clamp_u8(num(r, 10)?, u8::MAX));

        // A cloud-library track's path names rekordbox's Dropbox folder, not
        // a place on this disk; resolved here once so every reader sees a
        // file that exists.
        let folder_path = text_or_default(r, 11)?;
        lib.folder_path.push(&rbl_db::resolve_folder_path(
            &folder_path,
            cloud_root.as_deref(),
        ));
        lib.file_name.push(&text_or_default(r, 12)?);
        lib.analysis_path.push(&text_or_default(r, 13)?);
        lib.artwork_path.push(&text_or_default(r, 19)?);
        lib.play_count.push(clamp_u16(num(r, 14)?));
        lib.date_added.push(&text_or_default(r, 15)?);
        lib.release_date.push(&text_or_default(r, 16)?);
        lib.comment.push(&text_or_default(r, 17)?);
        // `Analysed` is a bitfield whose values are not yet all understood
        // (105/104/16/17/1 observed); non-zero means rekordbox analysed it.
        lib.analysed.push(u8::from(num(r, 18)? != 0));
        lib.bitrate.push(clamp_u32(num(r, 20)?));
        lib.sample_rate.push(clamp_u32(num(r, 21)?));
        lib.file_size.push(u64::try_from(num(r, 22)?).unwrap_or(0));
        lib.year
            .push(u16::try_from(num(r, 23)?.clamp(0, i64::from(u16::MAX))).unwrap_or(0));
        lib.track_number.push(clamp_u32(num(r, 24)?));
        lib.disc_no.push(clamp_u16(num(r, 25)?));
        lib.file_type.push(clamp_u8(num(r, 26)?, u8::MAX));
        lib.bit_depth.push(clamp_u16(num(r, 27)?));
        lib.lyricist.push(&text_or_default(r, 28)?);
        lib.date_created.push(&text_or_default(r, 29)?);
        // `"on"`, `""` or NULL on the reference library; only `"on"` ticks
        // the box, as `rbl_db::details` reads it for the same column.
        lib.publish.push(u8::from(text(r, 30)?.as_deref() == Some("on")));
        lib.message.push(&text_or_default(r, 31)?);

        // Keyed by the parsed id, not the text: the map is only ever looked
        // up from a membership row, and parsing 75,386 of those is cheaper
        // than allocating 38,681 strings to key it by.
        let numeric_id = id_text.parse::<u64>().unwrap_or(0);
        content_row.insert(numeric_id, row_index);
        lib.ids.push(numeric_id);
    }
    lib.count = lib.ids.len();
    stats.tracks = lib.count;
    std::thread::scope(|scope| -> rusqlite::Result<()> {
        let cue_job = cue_reader.and_then(|reader| {
            let content_row = &content_row;
            let count = lib.len();
            std::thread::Builder::new()
                .name("startup-cues".into())
                .spawn_scoped(scope, move || {
                    read_cues(reader.connection(), count, content_row)
                })
                .ok()
        });
        if cue_job.is_none() {
            lib.set_cues(read_cues(conn, lib.len(), &content_row)?);
        }
        load_playlists(conn, &mut lib, &content_row, &mut stats)?;
        load_histories(conn, &mut lib, &content_row, &mut stats)?;
        lib.set_tag_list(read_tag_list(conn, &content_row)?);
        lib.set_my_tags(read_my_tags(conn)?);
        lib.set_track_my_tags(read_track_my_tags(conn, &content_row)?);
        load_search_extra(conn, &mut lib)?;
        if let Some(job) = cue_job {
            lib.set_cues(
                job.join()
                    .unwrap_or_else(|_| read_cues(conn, lib.len(), &content_row))?,
            );
        }
        Ok(())
    })?;
    stats.read_ms = t0.elapsed().as_millis();

    let t1 = Instant::now();
    lib.build_indexes();
    stats.index_ms = t1.elapsed().as_millis();
    stats.heap_bytes = lib.heap_bytes();

    Ok((lib, stats))
}

fn load_search_extra(conn: &Connection, lib: &mut Library) -> rusqlite::Result<()> {
    // Resolve uncommon search metadata once, rather than querying SQLite on each keystroke.
    let mut extra = conn.prepare(
        "SELECT c.ID, composer.Name, album_artist.Name, remixer.Name, original.Name, c.Subtitle
        FROM djmdContent c
        LEFT JOIN djmdAlbum album ON album.ID=c.AlbumID
        LEFT JOIN djmdArtist album_artist ON album_artist.ID=album.AlbumArtistID
        LEFT JOIN djmdArtist composer ON composer.ID=c.ComposerID
        LEFT JOIN djmdArtist remixer ON remixer.ID=c.RemixerID
        LEFT JOIN djmdArtist original ON original.ID=c.OrgArtistID
        WHERE c.rb_local_deleted=0",
    )?;
    let mut by_id = HashMap::new();
    let mut rows = extra.query([])?;
    while let Some(row) = rows.next()? {
        let id = text_or_default(row, 0)?.parse::<u64>().unwrap_or(0);
        let mut values = Vec::with_capacity(5);
        for column in 1..=5 {
            values.push(text_or_default(row, column)?);
        }
        by_id.insert(id, values);
    }
    for id in &lib.ids {
        for (index, column) in lib.search_extra.iter_mut().enumerate() {
            column.push(
                by_id
                    .get(id)
                    .and_then(|values| values.get(index))
                    .map_or("", String::as_str),
            );
        }
    }
    Ok(())
}

/// Reads `djmdCue`, keeping only cues whose track is still live.
///
/// `ContentID` names 198,855 tracks against 38,681 live ones — it retains cues
/// for content long deleted — so this joins rather than trusting the table.
fn read_cues(
    conn: &Connection,
    tracks: usize,
    content_row: &HashMap<u64, Row>,
) -> rusqlite::Result<Cues> {
    // Gathered per track first, because the table is not in track order and
    // the index wants each track's cues contiguous.
    let mut per_track: Vec<Vec<Cue>> = vec![Vec::new(); tracks];

    // Drive the join from live content, using the existing cue ContentID index.
    // A plain cue scan also decrypts cues for hundreds of thousands of deleted
    // tracks. CROSS JOIN keeps SQLite from reversing this join order.
    let mut stmt = conn.prepare(
        "SELECT q.ContentID, q.ID, q.Kind, q.InMsec, q.OutMsec, q.ColorTableIndex
         FROM djmdContent c CROSS JOIN djmdCue q ON q.ContentID = c.ID
         WHERE c.rb_local_deleted = 0 AND q.rb_local_deleted = 0",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let Some(content) = text(r, 0)? else {
            continue;
        };
        let Ok(key) = content.parse::<u64>() else {
            continue;
        };
        let Some(&row) = content_row.get(&key) else {
            continue;
        };
        if let Some(list) = per_track.get_mut(row as usize) {
            list.push(read_cue(r, 1)?);
        }
    }
    Ok(Cues::from_per_track(per_track))
}

/// The columns a cue is read from, after whatever names its track.
const CUE_COLUMNS: &str = "ID, Kind, InMsec, OutMsec, ColorTableIndex";

/// One cue from a row whose [`CUE_COLUMNS`] start at `first`.
fn read_cue(r: &rusqlite::Row<'_>, first: usize) -> rusqlite::Result<Cue> {
    // An id that is not a number under 2^32 — none in the reference library
    // is — reads as 0, which the interface shows but will not edit.
    let id = text(r, first)?
        .and_then(|text| text.parse::<u32>().ok())
        .unwrap_or(0);
    let kind = u8::try_from(num(r, first + 1)?).unwrap_or(0);
    let position_ms = u32::try_from(num(r, first + 2)?.max(0)).unwrap_or(0);
    // -1 or NULL on a plain cue; `num` reads NULL as 0 and the max folds -1
    // into it.
    let out_ms = u32::try_from(num(r, first + 3)?.max(0)).unwrap_or(0);
    // NULL and 0 both mean "no colour chosen"; the writer stores 0 too.
    let colour = u8::try_from(num(r, first + 4)?).unwrap_or(0);
    Ok(Cue {
        id,
        position_ms,
        out_ms,
        kind,
        colour,
    })
}

/// Re-reads one track's cues after an edit, leaving everything else in place.
///
/// One indexed query — `djmdCue` carries an index on `(ContentID,
/// rb_local_deleted)` in the reference library [OBS], and the read measured
/// 0.6 ms there against 233 ms for a full reload. A track the index does not
/// hold is left alone rather than reported: its cues have nowhere to go.
pub fn reload_cues_of(db: &Db, library: &Library, track_id: &str) -> rusqlite::Result<()> {
    let Some(row) = library.row_of(track_id) else {
        return Ok(());
    };
    let mut stmt = db.connection().prepare(&format!(
        "SELECT {CUE_COLUMNS} FROM djmdCue WHERE ContentID = ?1 AND rb_local_deleted = 0"
    ))?;
    let cues = stmt
        .query_map([track_id], |r| read_cue(r, 0))?
        .collect::<rusqlite::Result<Vec<Cue>>>()?;
    library.set_cues_of(row, cues);
    Ok(())
}

fn load_playlists(
    conn: &Connection,
    lib: &mut Library,
    content_row: &HashMap<u64, Row>,
    stats: &mut LoadStats,
) -> rusqlite::Result<()> {
    let (playlists, memberships) = read_playlists(conn, content_row)?;
    stats.playlists = playlists.ids.len();
    stats.memberships = memberships;
    lib.set_playlists(playlists);
    Ok(())
}

/// Reads the history tree: the sessions rekordbox recorded, and their tracks.
///
/// Read-only. Nothing in this application records a session, so unlike the
/// playlist tree there is no reload path — what is here is what rekordbox
/// wrote before the library was opened.
fn load_histories(
    conn: &Connection,
    lib: &mut Library,
    content_row: &HashMap<u64, Row>,
    stats: &mut LoadStats,
) -> rusqlite::Result<()> {
    let (histories, plays) = read_lists(conn, content_row, HISTORY_TABLES)?;
    stats.histories = histories.ids.len();
    stats.plays = plays;
    lib.set_histories(histories);
    Ok(())
}

/// Reads the Tag List: `djmdSongTagList`, one row per track in `TrackNo`
/// order [OBS: 45 rows in the reference library, written by rekordbox
/// with `usn` and `rb_local_usn` NULL]. A library without the table has
/// an empty one; a row naming a track that is not there is skipped.
fn read_tag_list(conn: &Connection, content_row: &HashMap<u64, Row>) -> rusqlite::Result<Vec<Row>> {
    if !has_table(conn, "djmdSongTagList") {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT ContentID FROM djmdSongTagList WHERE rb_local_deleted = 0 ORDER BY TrackNo, created_at",
    )?;
    let rows = stmt.query_map([], |r| text(r, 0))?;
    let mut out = Vec::new();
    for content in rows {
        let Some(content) = content? else { continue };
        if let Some(&row) = content
            .parse::<u64>()
            .ok()
            .and_then(|id| content_row.get(&id))
        {
            out.push(row);
        }
    }
    Ok(out)
}

/// Re-reads the Tag List after an edit, reusing the track columns already
/// indexed.
pub fn reload_tag_list(db: &Db, library: &Library) -> rusqlite::Result<Vec<Row>> {
    let mut content_row: HashMap<u64, Row> = HashMap::with_capacity(library.len());
    for (row, id) in library.ids.iter().enumerate() {
        content_row.insert(*id, u32::try_from(row).unwrap_or(u32::MAX));
    }
    read_tag_list(db.connection(), &content_row)
}

/// Re-read history membership without scanning tracks or cues.
pub fn reload_histories(db: &Db, library: &Library) -> rusqlite::Result<Playlists> {
    let rows = library
        .ids
        .iter()
        .enumerate()
        .map(|(row, &id)| (id, Row::try_from(row).unwrap_or(Row::MAX)))
        .collect();
    read_lists(db.connection(), &rows, HISTORY_TABLES).map(|(lists, _)| lists)
}

/// Refresh the editable metadata of existing tracks. Row identities, cues,
/// paths, and all unaffected sort/search indexes stay in place.
pub fn reload_metadata(db: &Db, library: &mut Library, ids: &[String]) -> rusqlite::Result<()> {
    let mut statement = db.connection().prepare_cached(
        "SELECT Rating, ColorID, DJPlayCount, Commnt FROM djmdContent WHERE ID = ?1 AND rb_local_deleted = 0",
    )?;
    let mut updates = Vec::with_capacity(ids.len());
    for id in ids {
        let row = library
            .row_of(id)
            .ok_or(rusqlite::Error::QueryReturnedNoRows)? as usize;
        let fields = statement.query_row([id], |r| {
            Ok((
                clamp_u8(num(r, 0)?, 5),
                clamp_u8(num(r, 1)?, u8::MAX),
                clamp_u16(num(r, 2)?),
                text_or_default(r, 3)?,
            ))
        })?;
        updates.push((row, fields));
    }
    let mut rating_changed = false;
    let mut color_changed = false;
    let mut play_count_changed = false;
    let mut comments = HashMap::new();
    for (row, (rating, color, plays, comment)) in updates {
        rating_changed |= library.rating[row] != rating;
        color_changed |= library.color[row] != color;
        play_count_changed |= library.play_count[row] != plays;
        library.rating[row] = rating;
        library.color[row] = color;
        library.play_count[row] = plays;
        if library.comment.get(row) != comment {
            comments.insert(row, comment);
        }
    }
    if rating_changed {
        library.rebuild_ranks(&[crate::SortColumn::Rating]);
    }
    if color_changed {
        library.rebuild_ranks(&[crate::SortColumn::Color]);
    }
    if play_count_changed {
        library.rebuild_ranks(&[crate::SortColumn::PlayCount]);
    }
    if !comments.is_empty() {
        library.comment.replace_rows(&comments);
        library.rebuild_ranks(&[crate::SortColumn::Comment]);
        let search = comments
            .keys()
            .map(|&row| (row, library.search_text(row)))
            .collect();
        library.search.replace_rows(&search);
    }
    Ok(())
}

/// Re-reads only the playlist tree, reusing the track columns already indexed.
///
/// A playlist edit changes nothing about the tracks, and re-reading everything
/// costs 233 ms against 24 ms for the playlist tables alone on the reference
/// library. Measured with `cargo run --release -p rbl-index --example
/// reload_split`.
pub fn reload_playlists(db: &Db, library: &Library) -> rusqlite::Result<Playlists> {
    // The content map is keyed by the id text, and the ids were parsed from
    // exactly that, so it rebuilds without touching the database.
    // Nothing is allocated here: the ids are already the map's keys.
    let mut content_row: HashMap<u64, Row> = HashMap::with_capacity(library.len());
    for (row, id) in library.ids.iter().enumerate() {
        content_row.insert(*id, u32::try_from(row).unwrap_or(u32::MAX));
    }
    let (playlists, _) = read_playlists(db.connection(), &content_row)?;
    Ok(playlists)
}

/// Reads the My Tag categories and the tags under each, by name.
///
/// `djmdMyTag` is one table holding both: a category is a row with
/// `Attribute = 1` under `ParentID = 'root'`, a tag a row with `Attribute = 0`
/// under its category's id. Read-only on the reference library: 181 rows, 99
/// live, four categories — `Lexicon Tags` and three named `Empty Category`,
/// which is rekordbox's own name for an unused slot, not a placeholder of
/// ours. Memberships (`djmdSongMyTag`) are read separately, by
/// `read_track_my_tags`.
///
/// A library without the table opens with no categories rather than an error.
fn read_my_tags(conn: &Connection) -> rusqlite::Result<Vec<TagCategory>> {
    if !has_table(conn, "djmdMyTag") {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT ID, Name, Attribute, ParentID FROM djmdMyTag
         WHERE rb_local_deleted = 0 ORDER BY Seq, ID",
    )?;
    let mut categories: Vec<TagCategory> = Vec::new();
    let mut index_by_id: HashMap<String, usize> = HashMap::new();
    let mut tags: Vec<(String, String)> = Vec::new();
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let Some(id) = text(r, 0)? else {
            continue;
        };
        let name = text_or_default(r, 1)?;
        let parent = text_or_default(r, 3)?;
        if num(r, 2)? == 1 {
            index_by_id.insert(id, categories.len());
            categories.push(TagCategory {
                name,
                tags: Vec::new(),
            });
        } else {
            tags.push((parent, name));
        }
    }
    // Tags after every category is known: `Seq` numbers restart per parent, so
    // a tag can precede its category in the read order.
    for (parent, name) in tags {
        if let Some(&at) = index_by_id.get(&parent) {
            if let Some(category) = categories.get_mut(at) {
                category.tags.push(name);
            }
        }
    }
    Ok(categories)
}

/// Reads which tracks carry which My Tag, for the intelligent playlists'
/// `myTag` conditions: one `(row, tag)` pair per live `djmdSongMyTag` row
/// whose track is in the collection, the tag id read as rekordbox compares
/// it ([`crate::smart::my_tag_key`]). A library without the table has none.
fn read_track_my_tags(conn: &Connection, content_row: &HashMap<u64, Row>) -> rusqlite::Result<Vec<(Row, i32)>> {
    if !has_table(conn, "djmdSongMyTag") {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT ContentID, MyTagID FROM djmdSongMyTag WHERE rb_local_deleted = 0",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        let (Some(content), Some(tag)) = (text(r, 0)?, text(r, 1)?) else {
            continue;
        };
        if let Some(&row) = content.parse::<u64>().ok().and_then(|id| content_row.get(&id)) {
            out.push((row, crate::smart::my_tag_key(&tag)));
        }
    }
    Ok(out)
}

/// Which pair of tables a list tree is read from.
///
/// Playlists and histories are the same shape in the schema — a tree of named
/// rows with a parent, and a membership table naming the tracks in `TrackNo`
/// order. One reader serves both rather than two that drift apart.
#[derive(Debug, Clone, Copy)]
pub struct ListTables {
    /// The tree table: `ID`, `Name`, `ParentID`, `Seq`.
    pub lists: &'static str,
    /// The membership table: `ContentID`, `TrackNo`, and the column below.
    pub members: &'static str,
    /// What the membership table calls its list: `PlaylistID` or `HistoryID`.
    pub list_key: &'static str,
}

pub const PLAYLIST_TABLES: ListTables = ListTables {
    lists: "djmdPlaylist",
    members: "djmdSongPlaylist",
    list_key: "PlaylistID",
};

/// Sessions, filed under a folder per year and per month.
///
/// The same two-table shape, and the same `Attribute` convention: 0 is a
/// session, 1 a folder. Read read-only against the live library — 187 rows in
/// `djmdHistory`, 8,258 in `djmdSongHistory`.
pub const HISTORY_TABLES: ListTables = ListTables {
    lists: "djmdHistory",
    members: "djmdSongHistory",
    list_key: "HistoryID",
};

fn read_playlists(
    conn: &Connection,
    content_row: &HashMap<u64, Row>,
) -> rusqlite::Result<(Playlists, usize)> {
    read_lists(conn, content_row, PLAYLIST_TABLES)
}

/// Fills in `SmartList` for the intelligent playlists.
fn read_smart_lists(
    conn: &Connection,
    table: &str,
    index_by_id: &HashMap<String, usize>,
    playlists: &mut Playlists,
) {
    let Ok(mut stmt) = conn.prepare(&format!(
        "SELECT ID, SmartList FROM `{table}`
         WHERE rb_local_deleted = 0 AND SmartList IS NOT NULL AND SmartList != ''"
    )) else {
        return;
    };
    let mut rules: Vec<Option<String>> = vec![None; playlists.len()];
    let Ok(mut rows) = stmt.query([]) else { return };
    while let Ok(Some(r)) = rows.next() {
        let (Ok(Some(id)), Ok(Some(xml))) = (text(r, 0), text(r, 1)) else {
            continue;
        };
        if let Some(slot) = index_by_id.get(&id).and_then(|&index| rules.get_mut(index)) {
            *slot = Some(xml);
        }
    }
    let mut smart = StrColumn::with_capacity(playlists.len(), 256);
    for rule in &rules {
        smart.push(rule.as_deref().unwrap_or(""));
    }
    playlists.smart = smart;
}

/// Whether a table exists, so a schema without it degrades to an empty tree.
///
/// Histories are read from tables the required-column probe does not insist
/// on: a library that has never recorded one still opens, and opens with an
/// empty Histories section rather than an error.
fn has_table(conn: &Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table],
        |r| r.get::<_, i64>(0),
    )
    .is_ok_and(|n| n > 0)
}

fn read_lists(
    conn: &Connection,
    content_row: &HashMap<u64, Row>,
    tables: ListTables,
) -> rusqlite::Result<(Playlists, usize)> {
    let mut playlists = Playlists::default();
    let mut index_by_id: HashMap<String, usize> = HashMap::new();
    if !has_table(conn, tables.lists) || !has_table(conn, tables.members) {
        return Ok((playlists, 0));
    }

    let mut stmt = conn.prepare(&format!(
        "SELECT ID, Name, ParentID, Seq, Attribute FROM `{}`
         WHERE rb_local_deleted = 0 ORDER BY Seq",
        tables.lists,
    ))?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let Some(id) = text(r, 0)? else {
            continue;
        };
        let numeric_id = id.parse::<u64>().unwrap_or(0);
        index_by_id.insert(id, playlists.ids.len()); // moved, not cloned
        playlists.ids.push(numeric_id);
        playlists.names.push(&text_or_default(r, 1)?);
        // Parent is resolved after every playlist is known.
        playlists.parent.push(NO_ID);
        playlists.seq.push(clamp_u32(num(r, 3)?));
        // 0 a playlist (or a session), 1 a folder, 4 an intelligent
        // playlist; anything else is treated as a playlist, which is the
        // safer reading of a value nobody has seen.
        playlists
            .attribute
            .push(u8::try_from(num(r, 4)?).unwrap_or(0));
        playlists.smart.push("");
        playlists.members.push(Vec::new());
    }

    // The rules of the intelligent playlists. Only the playlist table has the
    // column, and a schema without it still reads: the rules are then empty
    // and every intelligent playlist opens with nothing in it.
    if tables.lists == PLAYLIST_TABLES.lists {
        read_smart_lists(conn, tables.lists, &index_by_id, &mut playlists);
    }

    // Second pass for parents, now that every id has an index.
    let mut stmt = conn.prepare(&format!(
        "SELECT ID, ParentID FROM `{}` WHERE rb_local_deleted = 0",
        tables.lists,
    ))?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let id = text(r, 0)?;
        let parent = text(r, 1)?;
        let Some(id) = id else { continue };
        let (Some(&child), Some(parent_index)) = (
            index_by_id.get(&id),
            parent.as_ref().and_then(|p| index_by_id.get(p)).copied(),
        ) else {
            continue;
        };
        if let Some(slot) = playlists.parent.get_mut(child) {
            *slot = u32::try_from(parent_index).unwrap_or(NO_ID);
        }
    }

    let mut stmt = conn.prepare(&format!(
        "SELECT `{key}`, ContentID FROM `{members}`
         WHERE rb_local_deleted = 0 ORDER BY `{key}`, TrackNo",
        key = tables.list_key,
        members = tables.members,
    ))?;
    let mut rows = stmt.query([])?;
    let mut memberships = 0usize;
    while let Some(r) = rows.next()? {
        let (playlist_id, content_id) = (text(r, 0)?, text(r, 1)?);
        let (Some(playlist_id), Some(content_id)) = (playlist_id, content_id) else {
            continue;
        };
        let content_key = content_id.parse::<u64>().unwrap_or(0);
        let (Some(&pi), Some(&row)) =
            (index_by_id.get(&playlist_id), content_row.get(&content_key))
        else {
            continue; // membership pointing at a deleted track
        };
        if let Some(members) = playlists.members.get_mut(pi) {
            members.push(row);
            memberships += 1;
        }
    }

    Ok((playlists, memberships))
}

#[cfg(test)]
mod refresh_tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn malformed_utf8_is_replaced_instead_of_aborting_the_library_load() {
        let dir = tempfile::tempdir().unwrap();
        let location =
            rbl_db::fixture::build(dir.path(), rbl_db::fixture::Shape::default()).unwrap();
        let db = Db::open(location, rbl_db::OpenMode::ReadWrite).unwrap();
        let track = rbl_db::fixture::track_id(0);
        db.connection()
            .execute_batch(
                "INSERT INTO djmdArtist
                    (ID, Name, rb_local_deleted, created_at, updated_at)
                 VALUES ('malformed-artist', CAST(X'417274FF697374' AS TEXT), 0,
                         '2026-01-01', '2026-01-01');
                 UPDATE djmdContent
                 SET Title = CAST(X'5469746C65FF' AS TEXT), ArtistID = 'malformed-artist'
                 WHERE ID = '10000';
                 UPDATE djmdPlaylist
                 SET Name = CAST(X'4C697374FF' AS TEXT)
                 WHERE ID = '900000';",
            )
            .unwrap();

        let (library, stats) = load(&db).unwrap();
        let row = library.row_of(&track).unwrap();
        assert_eq!(library.title.get(row as usize), "Title�");
        assert_eq!(library.artist_name(row), "Art�ist");
        assert_eq!(library.playlists().name(0), "List�");
        assert_eq!(stats.tracks, 40);
    }

    #[test]
    fn the_browser_detail_columns_load_and_sort() {
        let dir = tempfile::tempdir().unwrap();
        let location =
            rbl_db::fixture::build(dir.path(), rbl_db::fixture::Shape::default()).unwrap();
        let db = Db::open(location, rbl_db::OpenMode::ReadWrite).unwrap();
        let (first, second) = (rbl_db::fixture::track_id(0), rbl_db::fixture::track_id(1));
        db.connection()
            .execute(
                "UPDATE djmdContent SET TrackNo = 12, DiscNo = 2, FileType = 11, BitDepth = 24,
                        Lyricist = 'Words', DateCreated = '2023-08-06', DeliveryControl = 'on',
                        DeliveryComment = 'hello'
                 WHERE ID = ?1",
                [&first],
            )
            .unwrap();
        db.connection()
            .execute("UPDATE djmdContent SET DeliveryControl = '' WHERE ID = ?1", [&second])
            .unwrap();
        let (library, _) = load(&db).unwrap();
        let found = library.row_of(&first).unwrap();
        let row = found as usize;
        assert_eq!(library.track_number[row], 12);
        assert_eq!(library.disc_no[row], 2);
        assert_eq!(library.file_type[row], 11);
        assert_eq!(library.bit_depth[row], 24);
        assert_eq!(library.lyricist.get(row), "Words");
        assert_eq!(library.date_created.get(row), "2023-08-06");
        assert_eq!(library.publish[row], 1);
        assert_eq!(library.message.get(row), "hello");
        let other = library.row_of(&second).unwrap() as usize;
        assert_eq!(library.publish[other], 0, "an empty DeliveryControl is unticked");
        let spec = |sort| crate::ViewSpec {
            source: crate::TrackSource::Collection, sort, descending: false,
            query: String::new(), filter: crate::TrackFilter::default(),
        };
        let view = library.open_view(&spec(crate::SortColumn::PublishTrackInfo));
        assert_eq!(view.rows.first().copied(), Some(found));
        let view = library.open_view(&spec(crate::SortColumn::Message));
        assert_eq!(view.rows.last().copied(), Some(found));
    }

    #[test]
    fn parallel_reads_match_serial_and_exclude_deleted_track_cues() {
        let dir = tempfile::tempdir().unwrap();
        let location =
            rbl_db::fixture::build(dir.path(), rbl_db::fixture::Shape::default()).unwrap();
        let db = Db::open(location.clone(), rbl_db::OpenMode::ReadWrite).unwrap();
        let id = rbl_db::fixture::track_id(1);
        db.connection().execute("INSERT INTO djmdCue (ID, ContentID, Kind, InMsec, rb_local_deleted, created_at, updated_at) VALUES ('123', ?1, 1, 500, 0, '2026-01-01', '2026-01-01')", [&id]).unwrap();
        let reader = Db::open(location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
        let (serial, _) = load(&db).unwrap();
        let (parallel, _) = load_with_cue_reader(&db, Some(reader)).unwrap();
        assert_eq!(parallel.ids, serial.ids);
        assert_eq!(parallel.ranks, serial.ranks);
        assert_eq!(parallel.playlists().members, serial.playlists().members);
        for row in 0..serial.len() {
            assert_eq!(
                parallel.cues_of(u32::try_from(row).unwrap_or(0)),
                serial.cues_of(u32::try_from(row).unwrap_or(0))
            );
            assert_eq!(parallel.search.get(row), serial.search.get(row));
        }
        let live_cues = serial.cues.read().parts().0.len();
        assert!(live_cues > 0);
        db.connection()
            .execute(
                "UPDATE djmdContent SET rb_local_deleted=1 WHERE ID=?1",
                [&id],
            )
            .unwrap();
        let (deleted, _) = load(&db).unwrap();
        assert_eq!(deleted.cues.read().parts().0.len(), live_cues - 1);
    }

    #[test]
    fn metadata_refresh_matches_full_load_including_search_sort_and_history() {
        let dir = tempfile::tempdir().unwrap();
        let location =
            rbl_db::fixture::build(dir.path(), rbl_db::fixture::Shape::default()).unwrap();
        let mut writer = rbl_db::write::Writer::open(location, dir.path().join("backups")).unwrap();
        writer.disable_automatic_backups();
        let (original, _) = load(writer.library()).unwrap();
        let mut incremental = original.clone();
        let ids = [rbl_db::fixture::track_id(1), rbl_db::fixture::track_id(2)];
        for id in &ids {
            writer.set_rating(id, 5).unwrap();
            writer
                .set_comment(id, "Café\tnew searchable phrase")
                .unwrap();
            writer.set_color(id, Some("3")).unwrap();
            writer.record_play(id).unwrap();
        }
        reload_metadata(writer.library(), &mut incremental, &ids).unwrap();
        incremental.set_histories(reload_histories(writer.library(), &incremental).unwrap());
        let (full, _) = load(writer.library()).unwrap();
        assert_eq!(incremental.rating, full.rating);
        assert_eq!(incremental.color, full.color);
        assert_eq!(incremental.play_count, full.play_count);
        assert_eq!(incremental.ranks, full.ranks);
        assert_eq!(incremental.histories().members, full.histories().members);
        for row in 0..full.len() {
            assert_eq!(incremental.comment.get(row), full.comment.get(row));
            assert_eq!(incremental.search.get(row), full.search.get(row));
            assert_eq!(
                incremental.cues_of(Row::try_from(row).unwrap_or(0)),
                original.cues_of(Row::try_from(row).unwrap_or(0))
            );
        }
        let row = incremental.row_of(&ids[0]).unwrap() as usize;
        assert_ne!(original.comment.get(row), incremental.comment.get(row));
        // Repeated replacements must discard old search terms and arena data.
        for _ in 0..3 {
            writer.set_comment(&ids[0], "replacement").unwrap();
            reload_metadata(writer.library(), &mut incremental, &ids).unwrap();
            assert!(!incremental.search.get(row).contains("searchable"));
        }
        let before = incremental.rating.clone();
        assert!(reload_metadata(
            writer.library(),
            &mut incremental,
            &[ids[0].clone(), "missing".into()]
        )
        .is_err());
        assert_eq!(incremental.rating, before);
    }
}
