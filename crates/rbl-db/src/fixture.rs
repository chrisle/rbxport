//! Builds a throwaway library with the real schema, for the write tests.
//!
//! The `CREATE TABLE` statements are transcribed from the user's own database
//! (`cargo run -p rbl-db --example schema_dump`), column for column, so a
//! writer that satisfies the fixture satisfies the real thing. A hand-written
//! approximation would let a wrong column type or a missing `NOT NULL` pass.
//!
//! Fixtures are always `is_real_install: false`, so opening one read-write is
//! allowed even under `RBXPORT_TEST`.

use std::path::Path;

use rusqlite::{params, Connection};

use crate::{DbError, LibraryLocation, Result};

/// The passphrase a fixture is encrypted with. Fixed, because a fixture holds
/// nothing worth protecting and a test must be able to reopen it.
pub const FIXTURE_PASSPHRASE: &str = "rbxport-fixture";

/// Verbatim from the installed library, `DBVersion` 6000.
const SCHEMA: &[&str] = &[
    "CREATE TABLE `djmdContent` (`ID` VARCHAR(255) PRIMARY KEY, `FolderPath` VARCHAR(255) DEFAULT NULL, `FileNameL` VARCHAR(255) DEFAULT NULL, `FileNameS` VARCHAR(255) DEFAULT NULL, `Title` VARCHAR(255) DEFAULT NULL, `ArtistID` VARCHAR(255) DEFAULT NULL, `AlbumID` VARCHAR(255) DEFAULT NULL, `GenreID` VARCHAR(255) DEFAULT NULL, `BPM` INTEGER DEFAULT NULL, `Length` INTEGER DEFAULT NULL, `TrackNo` INTEGER DEFAULT NULL, `BitRate` INTEGER DEFAULT NULL, `BitDepth` INTEGER DEFAULT NULL, `Commnt` TEXT DEFAULT NULL, `FileType` INTEGER DEFAULT NULL, `Rating` INTEGER DEFAULT NULL, `ReleaseYear` INTEGER DEFAULT NULL, `RemixerID` VARCHAR(255) DEFAULT NULL, `LabelID` VARCHAR(255) DEFAULT NULL, `OrgArtistID` VARCHAR(255) DEFAULT NULL, `KeyID` VARCHAR(255) DEFAULT NULL, `StockDate` VARCHAR(255) DEFAULT NULL, `ColorID` VARCHAR(255) DEFAULT NULL, `DJPlayCount` INTEGER DEFAULT NULL, `ImagePath` VARCHAR(255) DEFAULT NULL, `MasterDBID` VARCHAR(255) DEFAULT NULL, `MasterSongID` VARCHAR(255) DEFAULT NULL, `AnalysisDataPath` VARCHAR(255) DEFAULT NULL, `SearchStr` VARCHAR(255) DEFAULT NULL, `FileSize` INTEGER DEFAULT NULL, `DiscNo` INTEGER DEFAULT NULL, `ComposerID` VARCHAR(255) DEFAULT NULL, `Subtitle` VARCHAR(255) DEFAULT NULL, `SampleRate` INTEGER DEFAULT NULL, `DisableQuantize` INTEGER DEFAULT NULL, `Analysed` INTEGER DEFAULT NULL, `ReleaseDate` VARCHAR(255) DEFAULT NULL, `DateCreated` VARCHAR(255) DEFAULT NULL, `ContentLink` INTEGER DEFAULT NULL, `Tag` VARCHAR(255) DEFAULT NULL, `ModifiedByRBM` VARCHAR(255) DEFAULT NULL, `HotCueAutoLoad` VARCHAR(255) DEFAULT NULL, `DeliveryControl` VARCHAR(255) DEFAULT NULL, `DeliveryComment` VARCHAR(255) DEFAULT NULL, `CueUpdated` VARCHAR(255) DEFAULT NULL, `AnalysisUpdated` VARCHAR(255) DEFAULT NULL, `TrackInfoUpdated` VARCHAR(255) DEFAULT NULL, `Lyricist` VARCHAR(255) DEFAULT NULL, `ISRC` VARCHAR(255) DEFAULT NULL, `SamplerTrackInfo` INTEGER DEFAULT NULL, `SamplerPlayOffset` INTEGER DEFAULT NULL, `SamplerGain` FLOAT DEFAULT NULL, `VideoAssociate` VARCHAR(255) DEFAULT NULL, `LyricStatus` INTEGER DEFAULT NULL, `ServiceID` INTEGER DEFAULT NULL, `OrgFolderPath` VARCHAR(255) DEFAULT NULL, `Reserved1` TEXT DEFAULT NULL, `Reserved2` TEXT DEFAULT NULL, `Reserved3` TEXT DEFAULT NULL, `Reserved4` TEXT DEFAULT NULL, `ExtInfo` TEXT DEFAULT NULL, `rb_file_id` VARCHAR(255) DEFAULT NULL, `DeviceID` VARCHAR(255) DEFAULT NULL, `rb_LocalFolderPath` VARCHAR(255) DEFAULT NULL, `SrcID` VARCHAR(255) DEFAULT NULL, `SrcTitle` VARCHAR(255) DEFAULT NULL, `SrcArtistName` VARCHAR(255) DEFAULT NULL, `SrcAlbumName` VARCHAR(255) DEFAULT NULL, `SrcLength` INTEGER DEFAULT NULL, `UUID` VARCHAR(255) DEFAULT NULL, `rb_data_status` INTEGER DEFAULT 0, `rb_local_data_status` INTEGER DEFAULT 0, `rb_local_deleted` TINYINT(1) DEFAULT 0, `rb_local_synced` TINYINT(1) DEFAULT 0, `usn` BIGINT DEFAULT NULL, `rb_local_usn` BIGINT DEFAULT NULL, `created_at` DATETIME NOT NULL, `updated_at` DATETIME NOT NULL)",
    "CREATE TABLE `djmdPlaylist` (`ID` VARCHAR(255) PRIMARY KEY, `Seq` INTEGER DEFAULT NULL, `Name` VARCHAR(255) DEFAULT NULL, `ImagePath` VARCHAR(255) DEFAULT NULL, `Attribute` INTEGER DEFAULT NULL, `ParentID` VARCHAR(255) DEFAULT NULL, `SmartList` TEXT DEFAULT NULL, `UUID` VARCHAR(255) DEFAULT NULL, `rb_data_status` INTEGER DEFAULT 0, `rb_local_data_status` INTEGER DEFAULT 0, `rb_local_deleted` TINYINT(1) DEFAULT 0, `rb_local_synced` TINYINT(1) DEFAULT 0, `usn` BIGINT DEFAULT NULL, `rb_local_usn` BIGINT DEFAULT NULL, `created_at` DATETIME NOT NULL, `updated_at` DATETIME NOT NULL)",
    "CREATE TABLE `djmdSongPlaylist` (`ID` VARCHAR(255) PRIMARY KEY, `PlaylistID` VARCHAR(255) DEFAULT NULL, `ContentID` VARCHAR(255) DEFAULT NULL, `TrackNo` INTEGER DEFAULT NULL, `UUID` VARCHAR(255) DEFAULT NULL, `rb_data_status` INTEGER DEFAULT 0, `rb_local_data_status` INTEGER DEFAULT 0, `rb_local_deleted` TINYINT(1) DEFAULT 0, `rb_local_synced` TINYINT(1) DEFAULT 0, `usn` BIGINT DEFAULT NULL, `rb_local_usn` BIGINT DEFAULT NULL, `created_at` DATETIME NOT NULL, `updated_at` DATETIME NOT NULL)",
    "CREATE TABLE `djmdHistory` (`ID` VARCHAR(255) PRIMARY KEY, `Seq` INTEGER DEFAULT NULL, `Name` VARCHAR(255) DEFAULT NULL, `Attribute` INTEGER DEFAULT NULL, `ParentID` VARCHAR(255) DEFAULT NULL, `DateCreated` VARCHAR(255) DEFAULT NULL, `UUID` VARCHAR(255) DEFAULT NULL, `rb_data_status` INTEGER DEFAULT 0, `rb_local_data_status` INTEGER DEFAULT 0, `rb_local_deleted` TINYINT(1) DEFAULT 0, `rb_local_synced` TINYINT(1) DEFAULT 0, `usn` BIGINT DEFAULT NULL, `rb_local_usn` BIGINT DEFAULT NULL, `created_at` DATETIME NOT NULL, `updated_at` DATETIME NOT NULL)",
    "CREATE TABLE `djmdSongHistory` (`ID` VARCHAR(255) PRIMARY KEY, `HistoryID` VARCHAR(255) DEFAULT NULL, `ContentID` VARCHAR(255) DEFAULT NULL, `TrackNo` INTEGER DEFAULT NULL, `UUID` VARCHAR(255) DEFAULT NULL, `rb_data_status` INTEGER DEFAULT 0, `rb_local_data_status` INTEGER DEFAULT 0, `rb_local_deleted` TINYINT(1) DEFAULT 0, `rb_local_synced` TINYINT(1) DEFAULT 0, `usn` BIGINT DEFAULT NULL, `rb_local_usn` BIGINT DEFAULT NULL, `created_at` DATETIME NOT NULL, `updated_at` DATETIME NOT NULL)",
    "CREATE TABLE `djmdArtist` (`ID` VARCHAR(255) PRIMARY KEY, `Name` VARCHAR(255) DEFAULT NULL, `SearchStr` VARCHAR(255) DEFAULT NULL, `UUID` VARCHAR(255) DEFAULT NULL, `rb_data_status` INTEGER DEFAULT 0, `rb_local_data_status` INTEGER DEFAULT 0, `rb_local_deleted` TINYINT(1) DEFAULT 0, `rb_local_synced` TINYINT(1) DEFAULT 0, `usn` BIGINT DEFAULT NULL, `rb_local_usn` BIGINT DEFAULT NULL, `created_at` DATETIME NOT NULL, `updated_at` DATETIME NOT NULL)",
    "CREATE TABLE `djmdAlbum` (`ID` VARCHAR(255) PRIMARY KEY, `Name` VARCHAR(255) DEFAULT NULL, `AlbumArtistID` VARCHAR(255) DEFAULT NULL, `ImagePath` VARCHAR(255) DEFAULT NULL, `Compilation` INTEGER DEFAULT NULL, `SearchStr` VARCHAR(255) DEFAULT NULL, `UUID` VARCHAR(255) DEFAULT NULL, `rb_data_status` INTEGER DEFAULT 0, `rb_local_data_status` INTEGER DEFAULT 0, `rb_local_deleted` TINYINT(1) DEFAULT 0, `rb_local_synced` TINYINT(1) DEFAULT 0, `usn` BIGINT DEFAULT NULL, `rb_local_usn` BIGINT DEFAULT NULL, `created_at` DATETIME NOT NULL, `updated_at` DATETIME NOT NULL)",
    "CREATE TABLE `djmdGenre` (`ID` VARCHAR(255) PRIMARY KEY, `Name` VARCHAR(255) DEFAULT NULL, `UUID` VARCHAR(255) DEFAULT NULL, `rb_data_status` INTEGER DEFAULT 0, `rb_local_data_status` INTEGER DEFAULT 0, `rb_local_deleted` TINYINT(1) DEFAULT 0, `rb_local_synced` TINYINT(1) DEFAULT 0, `usn` BIGINT DEFAULT NULL, `rb_local_usn` BIGINT DEFAULT NULL, `created_at` DATETIME NOT NULL, `updated_at` DATETIME NOT NULL)",
    "CREATE TABLE `djmdKey` (`ID` VARCHAR(255) PRIMARY KEY, `ScaleName` VARCHAR(255) DEFAULT NULL, `Seq` INTEGER DEFAULT NULL, `UUID` VARCHAR(255) DEFAULT NULL, `rb_data_status` INTEGER DEFAULT 0, `rb_local_data_status` INTEGER DEFAULT 0, `rb_local_deleted` TINYINT(1) DEFAULT 0, `rb_local_synced` TINYINT(1) DEFAULT 0, `usn` BIGINT DEFAULT NULL, `rb_local_usn` BIGINT DEFAULT NULL, `created_at` DATETIME NOT NULL, `updated_at` DATETIME NOT NULL)",
    "CREATE TABLE `djmdLabel` (`ID` VARCHAR(255) PRIMARY KEY, `Name` VARCHAR(255) DEFAULT NULL, `UUID` VARCHAR(255) DEFAULT NULL, `rb_data_status` INTEGER DEFAULT 0, `rb_local_data_status` INTEGER DEFAULT 0, `rb_local_deleted` TINYINT(1) DEFAULT 0, `rb_local_synced` TINYINT(1) DEFAULT 0, `usn` BIGINT DEFAULT NULL, `rb_local_usn` BIGINT DEFAULT NULL, `created_at` DATETIME NOT NULL, `updated_at` DATETIME NOT NULL)",
    "CREATE TABLE `djmdColor` (`ID` VARCHAR(255) PRIMARY KEY, `ColorCode` INTEGER DEFAULT NULL, `SortKey` INTEGER DEFAULT NULL, `Commnt` VARCHAR(255) DEFAULT NULL, `UUID` VARCHAR(255) DEFAULT NULL, `rb_data_status` INTEGER DEFAULT 0, `rb_local_data_status` INTEGER DEFAULT 0, `rb_local_deleted` TINYINT(1) DEFAULT 0, `rb_local_synced` TINYINT(1) DEFAULT 0, `usn` BIGINT DEFAULT NULL, `rb_local_usn` BIGINT DEFAULT NULL, `created_at` DATETIME NOT NULL, `updated_at` DATETIME NOT NULL)",
    // The My Tag tables, as pyrekordbox documents rekordbox 6's schema [DOC];
    // the reference library's `djmdSongMyTag` held no rows to transcribe.
    "CREATE TABLE `djmdMyTag` (`ID` VARCHAR(255) PRIMARY KEY, `Seq` INTEGER DEFAULT NULL, `Name` VARCHAR(255) DEFAULT NULL, `Attribute` INTEGER DEFAULT NULL, `ParentID` VARCHAR(255) DEFAULT NULL, `UUID` VARCHAR(255) DEFAULT NULL, `rb_data_status` INTEGER DEFAULT 0, `rb_local_data_status` INTEGER DEFAULT 0, `rb_local_deleted` TINYINT(1) DEFAULT 0, `rb_local_synced` TINYINT(1) DEFAULT 0, `usn` BIGINT DEFAULT NULL, `rb_local_usn` BIGINT DEFAULT NULL, `created_at` DATETIME NOT NULL, `updated_at` DATETIME NOT NULL)",
    // The Tag List, as the reference library has it [OBS 2026-09-13].
    "CREATE TABLE `djmdSongTagList` (`ID` VARCHAR(255) PRIMARY KEY, `ContentID` VARCHAR(255) DEFAULT NULL, `TrackNo` INTEGER DEFAULT NULL, `UUID` VARCHAR(255) DEFAULT NULL, `rb_data_status` INTEGER DEFAULT 0, `rb_local_data_status` INTEGER DEFAULT 0, `rb_local_deleted` TINYINT(1) DEFAULT 0, `rb_local_synced` TINYINT(1) DEFAULT 0, `usn` BIGINT DEFAULT NULL, `rb_local_usn` BIGINT DEFAULT NULL, `created_at` DATETIME NOT NULL, `updated_at` DATETIME NOT NULL)",
    "CREATE TABLE `djmdSongMyTag` (`ID` VARCHAR(255) PRIMARY KEY, `MyTagID` VARCHAR(255) DEFAULT NULL, `ContentID` VARCHAR(255) DEFAULT NULL, `TrackNo` INTEGER DEFAULT NULL, `UUID` VARCHAR(255) DEFAULT NULL, `rb_data_status` INTEGER DEFAULT 0, `rb_local_data_status` INTEGER DEFAULT 0, `rb_local_deleted` TINYINT(1) DEFAULT 0, `rb_local_synced` TINYINT(1) DEFAULT 0, `usn` BIGINT DEFAULT NULL, `rb_local_usn` BIGINT DEFAULT NULL, `created_at` DATETIME NOT NULL, `updated_at` DATETIME NOT NULL)",
    "CREATE TABLE `djmdCue` (`ID` VARCHAR(255) PRIMARY KEY, `ContentID` VARCHAR(255) DEFAULT NULL, `InMsec` INTEGER DEFAULT NULL, `InFrame` INTEGER DEFAULT NULL, `InMpegFrame` INTEGER DEFAULT NULL, `InMpegAbs` INTEGER DEFAULT NULL, `OutMsec` INTEGER DEFAULT NULL, `OutFrame` INTEGER DEFAULT NULL, `OutMpegFrame` INTEGER DEFAULT NULL, `OutMpegAbs` INTEGER DEFAULT NULL, `Kind` INTEGER DEFAULT NULL, `Color` INTEGER DEFAULT NULL, `ColorTableIndex` INTEGER DEFAULT NULL, `ActiveLoop` INTEGER DEFAULT NULL, `Comment` VARCHAR(255) DEFAULT NULL, `BeatLoopSize` INTEGER DEFAULT NULL, `CueMicrosec` INTEGER DEFAULT NULL, `InPointSeekInfo` VARCHAR(255) DEFAULT NULL, `OutPointSeekInfo` VARCHAR(255) DEFAULT NULL, `ContentUUID` VARCHAR(255) DEFAULT NULL, `UUID` VARCHAR(255) DEFAULT NULL, `rb_data_status` INTEGER DEFAULT 0, `rb_local_data_status` INTEGER DEFAULT 0, `rb_local_deleted` TINYINT(1) DEFAULT 0, `rb_local_synced` TINYINT(1) DEFAULT 0, `usn` BIGINT DEFAULT NULL, `rb_local_usn` BIGINT DEFAULT NULL, `created_at` DATETIME NOT NULL, `updated_at` DATETIME NOT NULL)",
    "CREATE TABLE `djmdProperty` (`DBID` VARCHAR(255) PRIMARY KEY, `DBVersion` VARCHAR(255) DEFAULT NULL, `BaseDBDrive` VARCHAR(255) DEFAULT NULL, `CurrentDBDrive` VARCHAR(255) DEFAULT NULL, `DeviceID` VARCHAR(255) DEFAULT NULL, `Reserved1` TEXT DEFAULT NULL, `Reserved2` TEXT DEFAULT NULL, `Reserved3` TEXT DEFAULT NULL, `Reserved4` TEXT DEFAULT NULL, `Reserved5` TEXT DEFAULT NULL, `created_at` DATETIME NOT NULL, `updated_at` DATETIME NOT NULL)",
    "CREATE TABLE `agentRegistry` (`registry_id` VARCHAR(255) PRIMARY KEY, `id_1` VARCHAR(255) DEFAULT NULL, `id_2` VARCHAR(255) DEFAULT NULL, `int_1` BIGINT DEFAULT NULL, `int_2` BIGINT DEFAULT NULL, `str_1` VARCHAR(255) DEFAULT NULL, `str_2` VARCHAR(255) DEFAULT NULL, `date_1` DATETIME DEFAULT NULL, `date_2` DATETIME DEFAULT NULL, `text_1` TEXT DEFAULT NULL, `text_2` TEXT DEFAULT NULL, `created_at` DATETIME NOT NULL, `updated_at` DATETIME NOT NULL)",
];

/// How the fixture is populated.
#[derive(Debug, Clone, Copy)]
pub struct Shape {
    pub tracks: usize,
    /// Playlists created directly under `root`.
    pub playlists: usize,
    /// Tracks placed in each playlist.
    pub tracks_per_playlist: usize,
    /// History sessions, filed under one year folder and one month folder —
    /// which is how rekordbox files them in the real library.
    pub history_sessions: usize,
    /// The starting value of `agentRegistry.localUpdateCount`.
    pub start_usn: i64,
}

impl Default for Shape {
    fn default() -> Self {
        Self {
            tracks: 40,
            playlists: 3,
            tracks_per_playlist: 5,
            history_sessions: 2,
            start_usn: 1000,
        }
    }
}

/// Creates an encrypted fixture library at `dir/master.db`.
pub fn build(dir: &Path, shape: Shape) -> Result<LibraryLocation> {
    let master_db = dir.join("master.db");
    let share_root = dir.join("share");
    std::fs::create_dir_all(&share_root)?;

    let conn = Connection::open(&master_db)
        .map_err(|e| DbError::Open(format!("{}: {e}", master_db.display())))?;
    crate::cipher::ready()?;
    conn.pragma_update(None, "cipher", "sqlcipher")?;
    conn.pragma_update(None, "legacy", 4)?;
    conn.pragma_update(None, "key", FIXTURE_PASSPHRASE)?;

    for statement in SCHEMA {
        conn.execute(statement, [])?;
    }

    let stamp = rbl_core::time::now();
    // A number, as a real library's DBID is: a stick's sync record names
    // it, and is only this library's when the numbers agree.
    conn.execute(
        "INSERT INTO djmdProperty (DBID, DBVersion, created_at, updated_at)
         VALUES ('1000000001', '6000', ?1, ?1)",
        params![stamp],
    )?;
    conn.execute(
        "INSERT INTO agentRegistry (registry_id, int_1, created_at, updated_at)
         VALUES ('localUpdateCount', ?1, ?2, ?2)",
        params![shape.start_usn, stamp],
    )?;

    // One My Tag category with two tags, in the shape the reader expects: a
    // category is `Attribute = 1` under `root`, a tag `Attribute = 0` under it.
    for (id, seq, name, attribute, parent) in [
        (MY_TAG_CATEGORY, 1, "Situation", 1, "root"),
        (MY_TAG_PEAK, 1, "Peak", 0, MY_TAG_CATEGORY),
        (MY_TAG_WARM_UP, 2, "Warm-up", 0, MY_TAG_CATEGORY),
    ] {
        conn.execute(
            "INSERT INTO djmdMyTag
                (ID, Seq, Name, Attribute, ParentID, UUID,
                 rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced,
                 usn, rb_local_usn, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 0, 0, 0, NULL, ?7, ?8, ?8)",
            params![id, seq, name, attribute, parent, format!("fixture-mytag-{id}"), shape.start_usn, stamp],
        )?;
    }

    for i in 0..shape.tracks {
        // Ids are sequential so a test can name a track without querying.
        conn.execute(
            "INSERT INTO djmdContent
                (ID, Title, FolderPath, FileNameL, BPM, Length, Rating, Analysed, UUID,
                 rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced,
                 usn, rb_local_usn, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 300, 0, 105, ?8, 256, 0, 0, 0, ?6, ?6, ?7, ?7)",
            params![
                track_id(i),
                format!("Track {i:03}"),
                format!("/fixture/audio/track{i:03}.mp3"),
                format!("track{i:03}.mp3"),
                12_800 + i64::try_from(i).unwrap_or(0),
                shape.start_usn,
                stamp,
                format!("fixture-content-{i:08}-0000-4000-8000-000000000000")
            ],
        )?;
    }

    for p in 0..shape.playlists {
        conn.execute(
            "INSERT INTO djmdPlaylist
                (ID, Seq, Name, Attribute, ParentID, UUID,
                 rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced,
                 usn, rb_local_usn, created_at, updated_at)
             VALUES (?1, ?2, ?3, 0, 'root', ?4, 256, 0, 0, 0, ?5, ?5, ?6, ?6)",
            params![
                playlist_id(p),
                i64::try_from(p).unwrap_or(0),
                format!("Playlist {p}"),
                format!("fixture-playlist-{p:08}-0000-4000-8000-000000000000"),
                shape.start_usn,
                stamp
            ],
        )?;
        for t in 0..shape.tracks_per_playlist {
            let track = (p * shape.tracks_per_playlist + t) % shape.tracks.max(1);
            conn.execute(
                "INSERT INTO djmdSongPlaylist
                    (ID, PlaylistID, ContentID, TrackNo, UUID,
                     rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced,
                     usn, rb_local_usn, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 256, 0, 0, 0, ?6, ?6, ?7, ?7)",
                params![
                    format!("fixture-song-{p:04}-{t:04}-4000-8000-000000000000"),
                    playlist_id(p),
                    track_id(track),
                    i64::try_from(t + 1).unwrap_or(1),
                    format!("fixture-spuuid-{p:04}-{t:04}-4000-8000-00000000"),
                    shape.start_usn,
                    stamp
                ],
            )?;
        }
    }

    add_histories(&conn, shape, &stamp)?;
    drop(conn);
    Ok(LibraryLocation {
        master_db,
        share_root,
        passphrase: FIXTURE_PASSPHRASE.to_owned(),
        // The whole point: a fixture is never mistaken for the real install,
        // so opening it read-write is allowed even under RBXPORT_TEST.
        is_real_install: false,
    })
}

/// A year folder, two months inside it, and the sessions filed under one.
///
/// Which is how rekordbox files them: `djmdHistory` holds all three, told
/// apart by `Attribute` — 1 for a folder, 0 for a session — and the tracks
/// played are in `djmdSongHistory` in `TrackNo` order.
fn add_histories(conn: &Connection, shape: Shape, stamp: &str) -> Result<()> {
    if shape.history_sessions == 0 {
        return Ok(());
    }
    let folder = |id: &str, name: &str, seq: i64, parent: &str| -> Result<()> {
        conn.execute(
            "INSERT INTO djmdHistory
                (ID, Seq, Name, Attribute, ParentID, DateCreated, UUID,
                 rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced,
                 usn, rb_local_usn, created_at, updated_at)
             VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6, 0, 0, 0, 0, NULL, ?7, ?8, ?8)",
            params![
                id,
                seq,
                name,
                parent,
                "2026-09-04 20:38:59",
                format!("fixture-{id}"),
                shape.start_usn,
                stamp
            ],
        )?;
        Ok(())
    };
    folder("2026", "2026", 1, "root")?;
    folder("202609", "9", 9, "2026")?;
    // A month made after September's folder, so its `Seq` is later although
    // the month is earlier: the tree has to file it by date, not by `Seq`.
    folder("202608", "8", 10, "2026")?;

    for h in 0..shape.history_sessions {
        conn.execute(
            "INSERT INTO djmdHistory
                (ID, Seq, Name, Attribute, ParentID, DateCreated, UUID,
                 rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced,
                 usn, rb_local_usn, created_at, updated_at)
             VALUES (?1, ?2, ?3, 0, '202609', ?4, ?5, 0, 0, 0, 0, NULL, ?6, ?7, ?7)",
            params![
                history_id(h),
                i64::try_from(h + 1).unwrap_or(1),
                format!("HISTORY 2026-09-0{}", h + 1),
                format!("2026-09-0{} 20:38:59", h + 1),
                format!("fixture-history-{h:08}-0000-4000-8000-000000000000"),
                shape.start_usn,
                stamp
            ],
        )?;
        for t in 0..shape.tracks_per_playlist {
            let track = (h * shape.tracks_per_playlist + t) % shape.tracks.max(1);
            conn.execute(
                "INSERT INTO djmdSongHistory
                    (ID, HistoryID, ContentID, TrackNo, UUID,
                     rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced,
                     usn, rb_local_usn, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, 0, 0, NULL, ?6, ?7, ?7)",
                params![
                    format!("fixture-play-{h:04}-{t:04}-4000-8000-000000000000"),
                    history_id(h),
                    track_id(track),
                    i64::try_from(t + 1).unwrap_or(1),
                    format!("fixture-shuuid-{h:04}-{t:04}-4000-8000-00000000"),
                    shape.start_usn,
                    stamp
                ],
            )?;
        }
    }
    Ok(())
}

/// Points the `index`th track at a real audio file, so a deck can play it.
///
/// The row's path, name and length follow the file; nothing checks that the
/// file exists here, because the fixture may be built on one machine for
/// another — the path is the other machine's.
pub fn point_at_audio(location: &LibraryLocation, index: usize, path: &str, seconds: u32) -> Result<()> {
    let conn = open_fixture(location)?;
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path).to_owned();
    let stem = name.rsplit_once('.').map_or(name.as_str(), |(stem, _)| stem).to_owned();
    conn.execute(
        "UPDATE djmdContent SET FolderPath = ?1, FileNameL = ?2, Title = ?3, Length = ?4 WHERE ID = ?5",
        params![path, name, stem, seconds, track_id(index)],
    )?;
    mark_changed(&conn, index)
}

/// Adds a memory cue to a fixture track with `InMsec` stored exactly as
/// given, as rekordbox libraries hold negative ones (#781). Not for the
/// user's real install: it writes straight to the file.
pub fn add_raw_cue(location: &LibraryLocation, index: usize, in_msec: i64) -> Result<()> {
    let conn = open_fixture(location)?;
    conn.execute(
        "INSERT INTO djmdCue (ID, ContentID, InMsec, OutMsec, Kind, Color, ColorTableIndex, ActiveLoop, Comment, BeatLoopSize, rb_local_deleted, created_at, updated_at)
         VALUES (?1, ?2, ?3, -1, 0, -1, 0, 0, '', 0, 0, '2026-01-01 00:00:00.000 +00:00', '2026-01-01 00:00:00.000 +00:00')",
        params![format!("raw-cue-{index}"), track_id(index), in_msec],
    )?;
    mark_changed(&conn, index)
}

/// Points a fixture track's `AnalysisDataPath` at a file under the share
/// root, as rekordbox does (`/PIONEER/USBANLZ/…/ANLZ0000.DAT`); the caller
/// puts the analysis files there.
pub fn set_analysis_path(location: &LibraryLocation, index: usize, relative: &str) -> Result<()> {
    let conn = open_fixture(location)?;
    conn.execute(
        "UPDATE djmdContent SET AnalysisDataPath = ?1 WHERE ID = ?2",
        params![relative, track_id(index)],
    )?;
    mark_changed(&conn, index)
}

/// Points a fixture track's `ImagePath` at a file under the share root, as
/// rekordbox does (`/PIONEER/Artwork/…/…jpg`); the caller puts the image there.
pub fn set_image_path(location: &LibraryLocation, index: usize, relative: &str) -> Result<()> {
    let conn = open_fixture(location)?;
    conn.execute(
        "UPDATE djmdContent SET ImagePath = ?1 WHERE ID = ?2",
        params![relative, track_id(index)],
    )?;
    mark_changed(&conn, index)
}

/// Sets a fixture track's tempo, BPM x100 as the column holds it, so a row
/// pointed at real audio can carry the tempo its analysis found.
pub fn set_tempo(location: &LibraryLocation, index: usize, bpm_x100: u32) -> Result<()> {
    let conn = open_fixture(location)?;
    conn.execute("UPDATE djmdContent SET BPM = ?1 WHERE ID = ?2", params![bpm_x100, track_id(index)])?;
    mark_changed(&conn, index)
}

/// Writes the `options.json` rekordbox's agent would keep for this library,
/// with `master_db_as` as the path it will have where it is read — the
/// detector resolves `share/` beside it. What `RBXPORT_OPTIONS` points
/// the compiled app at.
pub fn write_options_json(to: &Path, master_db_as: &str, passphrase: &str) -> Result<()> {
    let dp = crate::key::wrap_password(passphrase)?;
    let json = serde_json::json!({ "options": [["db-path", master_db_as], ["dp", dp]] });
    std::fs::write(to, serde_json::to_vec_pretty(&json).map_err(|e| DbError::Open(e.to_string()))?)?;
    Ok(())
}

/// A fixture's database, keyed, for one more statement.
/// Marks a track row as changed the way rekordbox does: the next update
/// number on the row and in `agentRegistry.localUpdateCount`.
///
/// Without it an edit made after [`build`] leaves the library's change counter
/// where it was, and the app's index snapshot, which is keyed to that counter
/// and the database's path, is served in its place wherever the fixture is
/// rebuilt at the same path — as it is on the Windows test host.
fn mark_changed(conn: &Connection, index: usize) -> Result<()> {
    let usn = crate::write::next_usn(conn)?;
    conn.execute("UPDATE djmdContent SET rb_local_usn = ?1 WHERE ID = ?2", params![usn, track_id(index)])?;
    conn.execute(
        "UPDATE agentRegistry SET int_1 = ?1 WHERE registry_id = 'localUpdateCount'",
        params![usn],
    )?;
    Ok(())
}

fn open_fixture(location: &LibraryLocation) -> Result<Connection> {
    let conn = Connection::open(&location.master_db)
        .map_err(|e| DbError::Open(format!("{}: {e}", location.master_db.display())))?;
    crate::cipher::ready()?;
    conn.pragma_update(None, "cipher", "sqlcipher")?;
    conn.pragma_update(None, "legacy", 4)?;
    conn.pragma_update(None, "key", &location.passphrase)?;
    Ok(conn)
}

/// The id of the nth fixture track.
#[must_use]
pub fn track_id(index: usize) -> String {
    format!("{}", 10_000 + index)
}

/// The id of the nth fixture playlist.
#[must_use]
pub fn playlist_id(index: usize) -> String {
    format!("{}", 900_000 + index)
}

/// The fixture's My Tag category and its two tags.
pub const MY_TAG_CATEGORY: &str = "700001";
pub const MY_TAG_PEAK: &str = "700002";
pub const MY_TAG_WARM_UP: &str = "700003";

/// The id of the nth fixture history session.
#[must_use]
pub fn history_id(index: usize) -> String {
    format!("{}", 800_000 + index)
}
