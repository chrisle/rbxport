//! Writing an `exportLibrary.db`.
//!
//! Everything here — the schema, the menu definitions, the colour names — was
//! transcribed from a real rekordbox-authored export
//! (`~/code/cdj3k-emu/tests/fixtures/usb-real.img`), read with
//! `cargo run -p rbl-onelibrary --example inspect`. None of it is invented.
//!
//! The reference tables matter more than they look. `menuItem`, `category` and
//! `sort` are how rekordbox knows which browse columns exist and in what
//! order; an export without them opens but browses wrong.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use crate::settings::StickSettings;
use crate::{key, unlock, Error, Result};

use reference::{COLORS, MENU_ITEMS};

/// `dbVersion` written into `property`, as the reference export carries it.
pub const DB_VERSION: &str = "1000";

/// The schema, verbatim from a real export. Twenty-two tables, several of
/// which stay empty but must exist.
const SCHEMA: &[&str] = &[
    "CREATE TABLE album(album_id integer primary key, name varchar, artist_id integer, image_id integer, isComplation integer, nameForSearch varchar)",
    "CREATE TABLE artist(artist_id integer primary key, name varchar, nameForSearch varchar)",
    "CREATE TABLE category(category_id integer primary key, menuItem_id integer, sequenceNo integer, isVisible integer)",
    "CREATE TABLE color(color_id integer primary key, name varchar)",
    "CREATE TABLE content(content_id integer primary key, title varchar, titleForSearch varchar, subtitle varchar, bpmx100 integer, length integer, trackNo integer, discNo integer, artist_id_artist integer, artist_id_remixer integer, artist_id_originalArtist integer, artist_id_composer integer, artist_id_lyricist integer, album_id integer, genre_id integer, label_id integer, key_id integer, color_id integer, image_id integer, djComment varchar, rating integer, releaseYear integer, releaseDate varchar, dateCreated varchar, dateAdded varchar, path varchar, fileName varchar, fileSize integer, fileType integer, bitrate integer, bitDepth integer, samplingRate integer, isrc varchar, djPlayCount integer, isHotCueAutoLoadOn integer, isKuvoDeliverStatusOn integer, kuvoDeliveryComment varchar, masterDbId integer, masterContentId integer, analysisDataFilePath varchar, analysedBits integer, contentLink integer, hasModified integer, cueUpdateCount integer, analysisDataUpdateCount integer, informationUpdateCount integer)",
    "CREATE TABLE cue(cue_id integer primary key, content_id integer, kind integer, colorTableIndex integer, cueComment varchar, isActiveLoop integer, beatLoopNumerator integer, beatLoopDenominator integer, inUsec integer, outUsec integer, in150FramePerSec integer, out150FramePerSec integer, inMpegFrameNumber integer, outMpegFrameNumber integer, inMpegAbs integer, outMpegAbs integer, inDecodingStartFramePosition integer, outDecodingStartFramePosition integer, inFileOffsetInBlock integer, OutFileOffsetInBlock integer, inNumberOfSampleInBlock integer, outNumberOfSampleInBlock integer)",
    "CREATE TABLE genre(genre_id integer primary key, name varchar)",
    "CREATE TABLE history(history_id integer primary key, sequenceNo integer, name varchar, attribute integer, history_id_parent integer)",
    "CREATE TABLE history_content(history_id integer, content_id integer, sequenceNo integer)",
    "CREATE TABLE hotCueBankList(hotCueBankList_id integer primary key, sequenceNo integer, name varchar, image_id integer, attribute integer, hotCueBankList_id_parent integer)",
    "CREATE TABLE hotCueBankList_cue(hotCueBankList_id integer, cue_id integer, sequenceNo integer)",
    "CREATE TABLE image(image_id integer primary key, path varchar)",
    "CREATE TABLE key(key_id integer primary key, name varchar)",
    "CREATE TABLE label(label_id integer primary key, name varchar)",
    "CREATE TABLE menuItem(menuItem_id integer primary key, kind integer, name varchar)",
    "CREATE TABLE myTag(myTag_id integer primary key, sequenceNo integer, name varchar, attribute integer, myTag_id_parent integer)",
    "CREATE TABLE myTag_content(myTag_id integer, content_id integer)",
    "CREATE TABLE playlist(playlist_id integer primary key, sequenceNo integer, name varchar, image_id integer, attribute integer, playlist_id_parent integer)",
    "CREATE TABLE playlist_content(playlist_id integer, content_id integer, sequenceNo integer)",
    "CREATE TABLE property(deviceName varchar, dbVersion varchar, numberOfContents integer, createdDate varchar, backGroundColorType integer, myTagMasterDBID integer)",
    "CREATE TABLE recommendedLike(content_id_1 integer, content_id_2 integer, rating integer, createdDate integer)",
    "CREATE TABLE sort(sort_id integer primary key, menuItem_id integer, sequenceNo integer, isVisible integer, isSelectedAsSubColumn integer)",
];

/// The reference stick's browse tables: what a fresh export writes, and what
/// [`StickSettings::default`] is built from so a stick's own settings can
/// replace them row for row.
pub mod reference {
/// Browse menu definitions.
///
/// The names are wrapped in U+FFFA and U+FFFB — interlinear annotation
/// markers, which is how rekordbox flags a string for translation at display
/// time. Writing the bare word instead leaves a player showing English
/// whatever its language is set to, so the wrapping is reproduced.
pub const MENU_ITEMS: &[(i64, i64, &str)] = &[
    (1, 128, "GENRE"),
    (2, 129, "ARTIST"),
    (3, 130, "ALBUM"),
    (4, 131, "TRACK"),
    (5, 133, "BPM"),
    (6, 134, "RATING"),
    (7, 135, "YEAR"),
    (8, 136, "REMIXER"),
    (9, 137, "LABEL"),
    (10, 138, "ORIGINAL ARTIST"),
    (11, 139, "KEY"),
    (12, 141, "CUE"),
    (13, 142, "COLOR"),
    (14, 146, "TIME"),
    (15, 147, "BITRATE"),
    (16, 148, "FILE NAME"),
    (17, 132, "PLAYLIST"),
    (18, 152, "HOT CUE BANK"),
    (19, 149, "HISTORY"),
    (20, 145, "SEARCH"),
    (21, 150, "COMMENTS"),
    (22, 140, "DATE ADDED"),
    (23, 151, "DJ PLAY COUNT"),
    (24, 144, "FOLDER"),
    (25, 161, "DEFAULT"),
    (26, 162, "ALPHABET"),
    (27, 170, "MATCHING"),
];

/// Which menu items appear as browse categories, and in what order:
/// `(category_id, menuItem_id, sequenceNo, isVisible)`.
pub const CATEGORIES: &[(i64, i64, i64, i64)] = &[
    (1, 1, 0, 0),
    (2, 2, 1, 1),
    (3, 3, 2, 1),
    (4, 4, 3, 1),
    (5, 17, 5, 1),
    (6, 5, 0, 0),
    (7, 6, 0, 0),
    (8, 7, 0, 0),
    (9, 8, 0, 0),
    (10, 9, 0, 0),
    (11, 10, 0, 0),
    (12, 11, 4, 1),
    (15, 13, 0, 0),
    (17, 24, 9, 1),
    (18, 20, 7, 1),
    (19, 14, 0, 0),
    (20, 15, 0, 0),
    (21, 16, 0, 0),
    (22, 19, 6, 1),
    (23, 18, 0, 0),
    (26, 27, 8, 1),
    (27, 22, 10, 1),
];

/// Which menu items appear as sort columns:
/// `(sort_id, menuItem_id, sequenceNo, isVisible, isSelectedAsSubColumn)`.
pub const SORTS: &[(i64, i64, i64, i64, i64)] = &[
    (0, 25, 1, 1, 0),
    (1, 26, 2, 1, 0),
    (2, 2, 3, 1, 0),
    (3, 3, 4, 1, 0),
    (4, 5, 5, 1, 0),
    (5, 6, 6, 1, 0),
    (6, 1, 0, 0, 0),
    (7, 21, 0, 0, 1),
    (8, 14, 0, 0, 0),
    (9, 8, 0, 0, 0),
    (10, 9, 0, 0, 0),
    (11, 10, 0, 0, 0),
    (12, 11, 7, 1, 0),
    (13, 15, 0, 0, 0),
    (15, 13, 0, 0, 0),
    (16, 23, 0, 0, 0),
    (17, 22, 0, 0, 0),
];

/// The eight colours in rekordbox's own order, so `color_id` lines up with the
/// `ColorID` stored against a track.
pub const COLORS: &[&str] = &[
    "Pink",
    "Red",
    "Orange",
    "Yellow",
    "Green",
    "Aqua",
    "Blue",
    "Purple",
];
}

/// Wraps a menu name in the annotation markers rekordbox uses.
fn annotated(name: &str) -> String {
    format!("\u{FFFA}{name}\u{FFFB}")
}

/// A track as `exportLibrary.db` stores it.
///
/// Ids are the stick-local ones an export assigns, not rekordbox's.
#[derive(Debug, Clone, Default)]
pub struct Track {
    pub metadata: rbl_core::ExportMetadata,
    pub file_type: i64,
    pub year: i64,
    pub release_date: String,
    pub bitrate: i64,
    pub sample_rate: i64,
    pub master_db_id: i64,
    pub master_content_id: i64,
    pub content_id: i64,
    pub title: String,
    pub artist_id: Option<i64>,
    pub album_id: Option<i64>,
    pub genre_id: Option<i64>,
    pub label_id: Option<i64>,
    pub key_id: Option<i64>,
    pub color_id: Option<i64>,
    pub bpm_x100: i64,
    /// Playing time in seconds.
    pub length: i64,
    pub track_no: i64,
    /// Stick-relative, e.g. `/Contents/ARTBAT/The Abyss.mp3`.
    pub path: String,
    pub file_name: String,
    pub file_size: i64,
    /// Stick-relative path of the analysis file.
    pub analysis_path: String,
    /// Star count, 0 to 5.
    pub rating: i64,
    pub comment: String,
    pub date_added: String,
    /// The `image` row of the track's artwork; `None` for none.
    pub image_id: Option<i64>,
}

/// Builds an `exportLibrary.db`.
#[derive(Debug)]
pub struct Builder {
    conn: Connection,
    staged: tempfile::TempPath,
    target: std::path::PathBuf,
    tracks: i64,
    /// Carried from the settings into `property` when the database is
    /// finished; never interpreted here.
    background_color_type: i64,
}

impl Builder {
    /// Creates a database at `path` with the schema and reference tables in
    /// place. Refuses to overwrite an existing file.
    pub fn create(path: &Path) -> Result<Self> {
        Self::create_with(path, &StickSettings::default())
    }

    /// Like [`Builder::create`], with the browse tables and colour names
    /// taken from `settings` — what a sync reads off the stick beforehand, so
    /// a rebuilt database keeps the categories, sorts and colour comments the
    /// stick already had.
    ///
    /// `menuItem` always comes from the reference: names are never
    /// user-edited, and a slot naming an item the reference lacks is skipped.
    pub fn create_with(path: &Path, settings: &StickSettings) -> Result<Self> {
        if path.exists() {
            return Err(Error::Exists(path.display().to_string()));
        }
        let staged = tempfile::NamedTempFile::new_in(path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new(".")))?.into_temp_path();
        let conn = Connection::open(&staged)
            .map_err(|source| Error::Open { path: path.display().to_string(), source })?;
        // The cipher settings must precede every other statement, or the file
        // is created as plain SQLite and no player can read it.
        unlock(&conn, &key::passphrase()?)?;

        crate::durable_writes(&conn)?;
        conn.execute_batch("BEGIN IMMEDIATE")?;

        for statement in SCHEMA {
            conn.execute(statement, [])?;
        }
        for (id, kind, name) in MENU_ITEMS {
            conn.execute(
                "INSERT INTO menuItem (menuItem_id, kind, name) VALUES (?1, ?2, ?3)",
                params![id, kind, annotated(name)],
            )?;
        }
        let known = |menu_item: i64| MENU_ITEMS.iter().any(|(id, _, _)| *id == menu_item);
        for slot in settings.categories.iter().filter(|s| known(s.menu_item)) {
            conn.execute(
                "INSERT INTO category (category_id, menuItem_id, sequenceNo, isVisible)
                 VALUES (?1, ?2, ?3, ?4)",
                params![slot.id, slot.menu_item, slot.seq, i64::from(slot.visible)],
            )?;
        }
        for slot in settings.sorts.iter().filter(|s| known(s.menu_item)) {
            let sub_column = i64::from(settings.sub_column == Some(slot.menu_item));
            conn.execute(
                "INSERT INTO sort
                    (sort_id, menuItem_id, sequenceNo, isVisible, isSelectedAsSubColumn)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![slot.id, slot.menu_item, slot.seq, i64::from(slot.visible), sub_column],
            )?;
        }
        // Always eight rows in rekordbox's order; a stick's own names replace
        // the reference names where it has them.
        for (index, name) in COLORS.iter().enumerate() {
            let id = i64::try_from(index + 1).unwrap_or(1);
            let name = settings
                .colors
                .iter()
                .find(|c| c.id == id)
                .map_or(*name, |c| c.name.as_str());
            conn.execute(
                "INSERT INTO color (color_id, name) VALUES (?1, ?2)",
                params![id, name],
            )?;
        }

        Ok(Self { conn, staged, target: path.to_owned(), tracks: 0, background_color_type: settings.background_color_type })
    }

    /// Adds a lookup row and returns its id, reusing one that already matches.
    ///
    /// An empty name is id 0, which is how the reference export spells "none".
    pub fn intern(&mut self, table: LookupTable, name: &str) -> Result<i64> {
        if name.is_empty() {
            return Ok(0);
        }
        let (table, id_column) = (table.name(), table.id_column());
        let existing: Option<i64> = self
            .conn
            .query_row(
                &format!("SELECT {id_column} FROM {table} WHERE name = ?1"),
                params![name],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = existing {
            return Ok(id);
        }
        let next: i64 = self
            .conn
            .query_row(
                &format!("SELECT COALESCE(MAX({id_column}), 0) + 1 FROM {table}"),
                [],
                |r| r.get(0),
            )
            ?;
        if table == "artist" {
            // Artist is the only lookup the reference export fills a search
            // column for.
            self.conn.execute(
                "INSERT INTO artist (artist_id, name, nameForSearch) VALUES (?1, ?2, ?2)",
                params![next, name],
            )?;
        } else {
            self.conn.execute(
                &format!("INSERT INTO {table} ({id_column}, name) VALUES (?1, ?2)"),
                params![next, name],
            )?;
        }
        Ok(next)
    }

    /// Adds a track.
    pub fn add_track(&mut self, track: &Track) -> Result<()> {
        self.conn.execute(
            "INSERT INTO content
                (content_id, title, titleForSearch, bpmx100, length, trackNo,
                 artist_id_artist, album_id, genre_id, label_id, key_id, color_id,
                 djComment, rating, dateAdded, path, fileName, fileSize,
                 analysisDataFilePath, djPlayCount, hasModified, image_id, masterDbId, masterContentId, releaseYear, releaseDate, bitrate, samplingRate)
             VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11,
                     ?12, ?13, ?14, ?15, ?16, ?17, ?18, 0, 0, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
            params![
                track.content_id,
                track.title,
                track.bpm_x100,
                track.length,
                track.track_no,
                track.artist_id,
                track.album_id,
                track.genre_id,
                track.label_id,
                track.key_id,
                track.color_id,
                track.comment,
                track.rating,
                track.date_added,
                track.path,
                track.file_name,
                track.file_size,
                track.analysis_path,
            
                track.image_id,
                track.master_db_id,
                track.master_content_id,
                track.year, track.release_date, track.bitrate, track.sample_rate,
            ],
        )?;
        self.conn.execute(
            "UPDATE content SET discNo=?2,bitDepth=?3,djPlayCount=?4,analysedBits=?5,isHotCueAutoLoadOn=?6,dateCreated=?7,isrc=?8,fileType=?9 WHERE content_id=?1",
            params![track.content_id,track.metadata.disc_number,track.metadata.bit_depth,track.metadata.play_count,
                track.metadata.analysed & !64,track.metadata.hot_cue_auto_load,track.metadata.date_created,track.metadata.isrc,track.file_type],
        )?;
        self.tracks += 1;
        Ok(())
    }

    /// Adds a playlist.
    ///
    /// `parent` is 0 for the top level — a number, unlike `master.db`, which
    /// spells the same thing as the string `"root"`.
    pub fn add_playlist(&mut self, id: i64, name: &str, parent: i64, seq: i64) -> Result<()> {
        self.add_playlist_node(id, name, parent, seq, false)
    }

    pub fn add_playlist_node(&mut self, id: i64, name: &str, parent: i64, seq: i64, folder: bool) -> Result<()> {
        self.conn.execute(
            "INSERT INTO playlist
                (playlist_id, sequenceNo, name, image_id, attribute, playlist_id_parent)
             VALUES (?1, ?2, ?3, NULL, ?5, ?4)",
            params![id, seq, name, parent, i64::from(folder)],
        )?;
        Ok(())
    }

    /// Places a track in a playlist. `seq` is one-based.
    pub fn add_to_playlist(&mut self, playlist: i64, content: i64, seq: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO playlist_content (playlist_id, content_id, sequenceNo)
             VALUES (?1, ?2, ?3)",
            params![playlist, content, seq],
        )?;
        Ok(())
    }

    /// Adds an artwork row: the stick-relative path of the image, e.g.
    /// `/PIONEER/Artwork/00001/a1.jpg`, under the id the pdb's `artwork`
    /// table uses for it.
    pub fn add_image(&mut self, id: i64, path: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO image (image_id, path) VALUES (?1, ?2)",
            params![id, path],
        )?;
        Ok(())
    }

    /// Adds a My Tag row: a category (`attribute` 1, parent 0) or a tag
    /// under one (`attribute` 0), under the library's own id for it.
    pub fn add_my_tag(&mut self, id: i64, seq: i64, name: &str, attribute: i64, parent: i64) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO myTag (myTag_id, sequenceNo, name, attribute, myTag_id_parent)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, seq, name, attribute, parent],
        )?;
        Ok(())
    }

    /// Marks a track with a My Tag.
    pub fn tag_track(&mut self, my_tag: i64, content: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO myTag_content (myTag_id, content_id) VALUES (?1, ?2)",
            params![my_tag, content],
        )?;
        Ok(())
    }

    /// Carry format-specific cue fields without guessing their encoding. Track IDs
    /// remain stable; records for deliberately removed tracks are omitted.
    pub fn preserve_cues(&mut self, source: &Path) -> Result<()> {
        let db = crate::ExportLibrary::open_read_only(source)?;
        let mut stmt = db.connection().prepare("SELECT * FROM cue ORDER BY cue_id")?;
        let columns = stmt.column_names().iter().map(|n| format!("\"{}\"", n.replace('"', "\"\""))).collect::<Vec<_>>();
        let placeholders = vec!["?"; columns.len()].join(",");
        let insert = format!("INSERT INTO cue ({}) VALUES ({placeholders})", columns.join(","));
        let content_column = stmt.column_index("content_id")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let content: i64 = row.get(content_column)?;
            let exists: bool = self.conn.query_row("SELECT EXISTS(SELECT 1 FROM content WHERE content_id=?1)", [content], |r| r.get(0))?;
            if exists {
                let values = (0..columns.len()).map(|i| row.get::<_,rusqlite::types::Value>(i)).collect::<std::result::Result<Vec<_>,_>>()?;
                self.conn.execute(&insert, rusqlite::params_from_iter(values))?;
            }
        }
        Ok(())
    }

    /// Replace this selected track's cues after carrying device-only rows.
    pub fn replace_cues(&mut self, content: i64, cues: &[rbl_anlz::cues::ExportCue]) -> Result<()> {
        self.conn.execute("DELETE FROM cue WHERE content_id=?1", [content])?;
        for cue in cues {
            self.conn.execute(
                "INSERT INTO cue (content_id,kind,colorTableIndex,cueComment,isActiveLoop,beatLoopNumerator,beatLoopDenominator,inUsec,outUsec,in150FramePerSec,out150FramePerSec)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                params![content,cue.kind,cue.color_code,cue.comment,cue.active_loop,cue.loop_numerator,cue.loop_denominator,
                    i64::from(cue.time_ms)*1000,cue.loop_time_ms.map_or(-1, |v| i64::from(v)*1000),
                    i64::from(cue.time_ms)*150/1000,cue.loop_time_ms.map_or(-1, |v| i64::from(v)*150/1000)],
            )?;
        }
        Ok(())
    }

    pub fn add_history(&mut self, id: i64, name: &str, parent: i64, seq: i64, folder: bool) -> Result<()> {
        self.conn.execute("INSERT INTO history VALUES (?1,?2,?3,?4,?5)", params![id,seq,name,i64::from(folder),parent])?;
        Ok(())
    }
    pub fn add_history_track(&mut self, history: i64, content: i64, seq: i64) -> Result<()> {
        self.conn.execute("INSERT INTO history_content VALUES (?1,?2,?3)", params![history,content,seq])?;
        Ok(())
    }

    /// Writes the property row and closes the database.
    ///
    /// `created` is a date, `YYYY-MM-DD`, which is what the reference export
    /// carries — not a full timestamp.
    ///
    /// `my_tag_master_db_id` is the number `exportExt.pdb` carries for the
    /// tags; the two files agree on it.
    pub fn finish(self, device_name: &str, created: &str, my_tag_master_db_id: u32) -> Result<()> {
        self.conn.execute(
            "INSERT INTO property
                (deviceName, dbVersion, numberOfContents, createdDate,
                 backGroundColorType, myTagMasterDBID)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![device_name, DB_VERSION, self.tracks, created, self.background_color_type, my_tag_master_db_id],
        )?;
        // A stick must not be left with pages only in the WAL: a device that
        // does not replay it would read a database missing everything written.
        self.conn.execute_batch("COMMIT")?;
        drop(self.conn);
        std::fs::OpenOptions::new().write(true).open(&self.staged)?.sync_all()?;
        self.staged.persist_noclobber(&self.target).map_err(|e| e.error)?;
        rbl_core::durable::sync_dir(self.target.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new(".")))?;
        Ok(())
    }
}

/// The lookup tables a track refers to.
///
/// An enum rather than a string, so a table name can never reach the SQL from
/// outside this file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupTable {
    Artist,
    Album,
    Genre,
    Label,
    Key,
}

impl LookupTable {
    const fn name(self) -> &'static str {
        match self {
            Self::Artist => "artist",
            Self::Album => "album",
            Self::Genre => "genre",
            Self::Label => "label",
            Self::Key => "key",
        }
    }

    const fn id_column(self) -> &'static str {
        match self {
            Self::Artist => "artist_id",
            Self::Album => "album_id",
            Self::Genre => "genre_id",
            Self::Label => "label_id",
            Self::Key => "key_id",
        }
    }
}
