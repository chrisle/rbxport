//! A new, empty library, for a machine on which
//! [`locate`](crate::locate) found nothing to open.
//!
//! A new library is made where rekordbox makes one when nothing says
//! otherwise — `master.db` and `share/` in its default folder — so the
//! detector finds it on every later start exactly as it finds an installed
//! library. Nothing of rekordbox's own is written: its agent's `options.json`
//! is rekordbox's to write, and rekordbox rewrites it on every launch from
//! `masterDbDirectory` anyway [OBS 7.2.11]. A library is never made in a
//! configured folder that is missing, which is most often a drive that is
//! not connected; rekordbox refuses the same [OBS 7.2.11].
//!
//! The schema is every table and index of rekordbox's own database,
//! verbatim, and the passphrase is the one rekordbox uses
//! ([`REKORDBOX_DP`](crate::key::REKORDBOX_DP)). The rows it starts with are
//! the ones rekordbox 7.2.14 put in a library it made afresh; see the note
//! above [`MENU_ITEMS`].

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use crate::locate::{locate_with, Located, Sources};
use crate::{DbError, LibraryLocation, Result};

/// Every `CREATE` statement in rekordbox's `master.db`.
const SCHEMA: &str = include_str!("master_schema.sql");

/// The value rekordbox's `djmdProperty.DBVersion` holds, which the schema
/// probe reads to decide the library is one it knows.
const DB_VERSION: &str = "6000";

// The rows below are what rekordbox 7.2.14 puts in a library it makes
// itself: chris-win11 with no library, rekordbox launched, "Empty Library"
// chosen at its conversion prompt [OBS 2026-09-24]. Its schema was the one
// above to the byte. They are written in the order rekordbox wrote them —
// its `rb_local_usn` values run colours, menu items, categories, sorts,
// My Tag, sampler, related tracks, the playlist — and left out are the rows
// that name the machine or the account: `djmdDevice`, `djmdCloudProperty`,
// and the agent's sign-in and notification entries in `agentRegistry`.
// `djmdKey` is empty there too: rekordbox makes a key's row when a track
// first has that key.

/// The browser's category list, as `djmdMenuItems` holds it: id, class, name.
const MENU_ITEMS: &[(&str, i64, &str)] = &[
    ("1", -128, "GENRE"), ("2", -127, "ARTIST"), ("3", -126, "ALBUM"), ("4", -125, "TRACK"),
    ("5", -123, "BPM"), ("6", -122, "RATING"), ("7", -121, "YEAR"), ("8", -120, "REMIXER"),
    ("9", -119, "LABEL"), ("10", -118, "ORIGINAL ARTIST"), ("11", -117, "KEY"), ("12", -115, "CUE"),
    ("13", -114, "COLOR"), ("14", -110, "TIME"), ("15", -109, "BITRATE"), ("16", -108, "FILE NAME"),
    ("17", -124, "PLAYLIST"), ("18", -104, "HOT CUE BANK"), ("19", -107, "HISTORY"),
    ("20", -111, "SEARCH"), ("21", -106, "COMMENTS"), ("22", -116, "DATE ADDED"),
    ("23", -105, "DJ PLAY COUNT"), ("24", -112, "FOLDER"), ("25", -95, "DEFAULT"),
    ("26", -94, "ALPHABET"), ("27", -86, "MATCHING"),
];

/// `djmdCategory`: id, menu item, seq, disable, info order.
const CATEGORIES: &[(&str, &str, i64, i64, i64)] = &[
    ("1", "1", 0, 1, 99), ("2", "2", 1, 0, 2), ("3", "3", 2, 0, 3), ("4", "4", 3, 0, 1),
    ("5", "17", 5, 0, 99), ("6", "5", 0, 1, 5), ("7", "6", 0, 1, 99), ("8", "7", 0, 1, 99),
    ("9", "8", 0, 1, 99), ("10", "9", 0, 1, 99), ("11", "10", 0, 1, 99), ("12", "11", 4, 0, 99),
    ("15", "13", 0, 1, 99), ("17", "24", 9, 0, 99), ("18", "20", 7, 0, 99), ("19", "14", 0, 1, 4),
    ("20", "15", 0, 1, 6), ("21", "16", 0, 1, 99), ("22", "19", 6, 0, 99), ("23", "18", 0, 1, 99),
    ("26", "27", 8, 2, 99), ("27", "22", 10, 0, 99),
];

/// `djmdSort`: id, menu item, seq, disable.
const SORTS: &[(&str, &str, i64, i64)] = &[
    ("0", "25", 1, 0), ("1", "26", 2, 0), ("2", "2", 3, 0), ("3", "3", 4, 0), ("4", "5", 5, 0),
    ("5", "6", 6, 0), ("6", "1", 0, 1), ("7", "21", 0, 1), ("8", "14", 0, 1), ("9", "8", 0, 1),
    ("10", "9", 0, 1), ("11", "10", 0, 1), ("12", "11", 7, 0), ("13", "15", 0, 1),
    ("15", "13", 0, 1), ("16", "23", 0, 1), ("17", "22", 0, 1),
];

/// The eight track colours, `djmdColor`: id and name, sorted by id.
const COLORS: &[&str] = &["Pink", "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple"];

/// The My Tag columns and the tags in each, `djmdMyTag`. A column is its
/// position as id (`Attribute` 1 under `root`); a tag gets a fresh numeric
/// id, as rekordbox gave each a different one.
const MY_TAGS: &[(&str, &[&str])] = &[
    ("Genre", &["Acid House", "Deep House", "Techno", "Nu Disco", "Electro House", "Bass Music", "Trap"]),
    ("Components", &["Synth", "Vocal", "Beat", "Sub Bass", "Percussion", "Piano", "Dark", "Upper"]),
    ("Situation", &["Main Floor", "Second Floor", "Lounge", "Mid Night", "Morning", "Build up", "Peak Time", "Build down"]),
    ("Untitled Column", &["My Comment"]),
];

/// `djmdSampler`: id, seq, name, attribute, parent.
const SAMPLER: &[(&str, i64, &str, i64, &str)] = &[
    ("1", 1, "Sampler Root", 3, "4294967295"),
    ("2", 1, "All Samples", 5, "1"),
    ("3", 2, "Capture", 6, "1"),
];

/// `djmdRelatedTracks`: the root and the three presets under it, with the
/// switched-on parts of each preset's `Criteria`. `{from}` and `{to}` are the
/// year range, which read 2025 to 2026 in a library made in 2026
/// [ASSUME: last year to this year, from that one library].
const RELATED_TRACKS: &[(&str, i64, &str, i64, &str, &str)] = &[
    ("1", 1, "Related Tracks Root", 10, "root", r#"{"Ver": 1, "Hist": {"Diff": 3}, "Matc": {}, "BPM": {"True": 1, "Type": 1, "Diff": {"Diff": 5, "HaDo": 1}, "Rang": {"Type": 1, "Min": 10000, "Max": 12000}}, "Key": {"Typ1": 1, "Typ2": 2}, "DAdd": {"Days": 30}, "Genr": {"True": 1}, "Comp": {}, "Remi": {}, "Labe": {}, "Colo": {}, "Rate": {"Rate": 0}, "MTag": {"Type": 0}, "Arti": {"Titl": 1}, "Comm": {"Type": 1, "Word": []}, "Year": {"Type": 1, "Diff": {"Diff": 0}, "Rang": {"Min": {from}, "Max": {to}}}, "Form": {"Form": 65535, "BitR": -1}}"#),
    ("", 1, "BPM + KEY", 11, "1", r#"{"Ver": 1, "Hist": {"Diff": 3}, "Matc": {}, "BPM": {"True": 1, "Type": 1, "Diff": {"Diff": 5, "HaDo": 1}, "Rang": {"Type": 1, "Min": 10000, "Max": 12000}}, "Key": {"True": 1, "Typ1": 1, "Typ2": 2}, "DAdd": {"Days": 30}, "Genr": {}, "Comp": {}, "Remi": {}, "Labe": {}, "Colo": {}, "Rate": {"Rate": 0}, "MTag": {"Type": 0}, "Arti": {"Titl": 1}, "Comm": {"Type": 1, "Word": []}, "Year": {"Type": 1, "Diff": {"Diff": 0}, "Rang": {"Min": {from}, "Max": {to}}}, "Form": {"Form": 65535, "BitR": -1}}"#),
    ("", 2, "Same Genre in 30day", 11, "1", r#"{"Ver": 1, "Hist": {"Diff": 3}, "Matc": {}, "BPM": {"Type": 1, "Diff": {"Diff": 5, "HaDo": 1}, "Rang": {"Type": 1, "Min": 10000, "Max": 12000}}, "Key": {"Typ1": 1, "Typ2": 2}, "DAdd": {"True": 1, "Days": 30}, "Genr": {"True": 1}, "Comp": {}, "Remi": {}, "Labe": {}, "Colo": {}, "Rate": {"Rate": 0}, "MTag": {"Type": 0}, "Arti": {"Titl": 1}, "Comm": {"Type": 1, "Word": []}, "Year": {"Type": 1, "Diff": {"Diff": 0}, "Rang": {"Min": {from}, "Max": {to}}}, "Form": {"Form": 65535, "BitR": -1}}"#),
    ("", 3, "Same Artist", 11, "1", r#"{"Ver": 1, "Hist": {"Diff": 3}, "Matc": {}, "BPM": {"Type": 1, "Diff": {"Diff": 5, "HaDo": 1}, "Rang": {"Type": 1, "Min": 10000, "Max": 12000}}, "Key": {"Typ1": 1, "Typ2": 2}, "DAdd": {"Days": 30}, "Genr": {}, "Comp": {}, "Remi": {}, "Labe": {}, "Colo": {}, "Rate": {"Rate": 0}, "MTag": {"Type": 0}, "Arti": {"True": 1, "Titl": 1}, "Comm": {"Type": 1, "Word": []}, "Year": {"Type": 1, "Diff": {"Diff": 0}, "Rang": {"Min": {from}, "Max": {to}}}, "Form": {"Form": 65535, "BitR": -1}}"#),
];

/// The one playlist a new library has, under a fixed id.
const CUE_ANALYSIS_PLAYLIST: (&str, &str) = ("200000", "CUE Analysis Playlist");

/// What making a library here would do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The database to make. Never an existing file, and never in a
    /// configured folder that is missing.
    pub master_db: PathBuf,
    passphrase: String,
}

/// Whether a library can be made on this machine; see [`plan_with`].
pub fn plan() -> Result<Option<Plan>> {
    plan_with(&Sources::installed()?)
}

/// Where a new library would go, if one may be made.
///
/// The default folder, when nothing is configured there or elsewhere and it
/// holds no database. `None` when there is a library to open, and when the
/// configured library is on a drive that is missing: rekordbox asks for the
/// drive instead, and only its "open Master Database in the default drive"
/// (`locate::use_default`) leads back here.
pub fn plan_with(sources: &Sources) -> Result<Option<Plan>> {
    let master_db = match locate_with(sources)? {
        Located::Found { .. } | Located::Unavailable { .. } => return Ok(None),
        Located::Absent { master_db } => master_db,
    };
    if master_db.exists() {
        return Ok(None);
    }
    Ok(Some(Plan { master_db, passphrase: sources.passphrase()? }))
}

/// Makes the library the plan describes: the folders and an empty
/// `master.db`.
///
/// The database is built beside its final name and moved into place only
/// when complete, and the move refuses to replace a file, so a library that
/// appeared meanwhile is never overwritten and a failure part-way leaves no
/// half-made `master.db` for the next start to find.
pub fn create(plan: &Plan) -> Result<LibraryLocation> {
    let dir = plan
        .master_db
        .parent()
        .ok_or_else(|| DbError::Open(format!("{} has no folder", plan.master_db.display())))?;
    let share_root = dir.join("share");
    for folder in ["PIONEER/Artwork", "PIONEER/USBANLZ"] {
        std::fs::create_dir_all(share_root.join(folder))?;
    }

    let staging = tempfile::Builder::new().prefix(".master.db-").tempdir_in(dir)?;
    let built = staging.path().join("master.db");
    build(&built, &plan.passphrase, &share_root)?;
    tempfile::TempPath::try_from_path(&built)?
        .persist_noclobber(&plan.master_db)
        .map_err(|e| {
            if e.error.kind() == std::io::ErrorKind::AlreadyExists {
                DbError::Open(format!("{} appeared while the new library was being made", plan.master_db.display()))
            } else {
                DbError::Io(e.error)
            }
        })?;
    drop(staging);

    Ok(LibraryLocation {
        master_db: plan.master_db.clone(),
        share_root,
        passphrase: plan.passphrase.clone(),
        is_real_install: true,
    })
}

/// The empty database: the schema and the rows rekordbox's browser and
/// this application's writer expect to find.
fn build(path: &Path, passphrase: &str, share_root: &Path) -> Result<()> {
    let mut conn = Connection::open(path).map_err(|e| DbError::Open(format!("{}: {e}", path.display())))?;
    conn.pragma_update(None, "cipher", "sqlcipher")?;
    conn.pragma_update(None, "legacy", 4)?;
    conn.pragma_update(None, "key", passphrase)?;

    let tx = conn.transaction()?;
    tx.execute_batch(SCHEMA)?;

    let stamp = rbl_core::time::now();
    let mut rng = rbl_core::ids::Rng::from_entropy();
    // A library's DBID is a decimal number: a stick's sync record names it.
    tx.execute(
        "INSERT INTO djmdProperty (DBID, DBVersion, BaseDBDrive, CurrentDBDrive, DeviceID, created_at, updated_at)
         VALUES (?1, ?2, '', '', ?3, ?4, ?4)",
        params![rng.numeric_id(1 << 31), DB_VERSION, rng.uuid4(), stamp],
    )?;

    // Each row takes the next local update number, as the writer's rows do.
    let mut usn = 0_i64;
    let mut next = || {
        usn += 1;
        usn
    };
    for (index, name) in (1_i64..).zip(COLORS) {
        tx.execute(
            "INSERT INTO djmdColor (ID, SortKey, Commnt, UUID, rb_local_usn, created_at, updated_at)
             VALUES (?1, ?1, ?2, ?1, ?3, ?4, ?4)",
            params![index.to_string(), name, next(), stamp],
        )?;
    }
    for &(id, class, name) in MENU_ITEMS {
        tx.execute(
            "INSERT INTO djmdMenuItems (ID, Class, Name, UUID, rb_local_usn, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?1, ?4, ?5, ?5)",
            params![id, class, name, next(), stamp],
        )?;
    }
    for &(id, item, seq, disable, info) in CATEGORIES {
        tx.execute(
            "INSERT INTO djmdCategory (ID, MenuItemID, Seq, Disable, InfoOrder, UUID, rb_local_usn, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?1, ?6, ?7, ?7)",
            params![id, item, seq, disable, info, next(), stamp],
        )?;
    }
    for &(id, item, seq, disable) in SORTS {
        tx.execute(
            "INSERT INTO djmdSort (ID, MenuItemID, Seq, Disable, UUID, rb_local_usn, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?1, ?5, ?6, ?6)",
            params![id, item, seq, disable, next(), stamp],
        )?;
    }
    let my_tag = "INSERT INTO djmdMyTag (ID, Seq, Name, Attribute, ParentID, UUID, rb_local_usn, created_at, updated_at)
                  VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)";
    for (column, (name, tags)) in (1_i64..).zip(MY_TAGS) {
        let id = column.to_string();
        tx.execute(my_tag, params![id, column, name, 1, "root", id, next(), stamp])?;
        for (seq, tag) in (1_i64..).zip(*tags) {
            let tag_id = unused_id(&tx, "djmdMyTag", &mut rng)?;
            tx.execute(my_tag, params![tag_id, seq, tag, 0, id, rng.uuid4(), next(), stamp])?;
        }
    }
    for &(id, seq, name, attribute, parent) in SAMPLER {
        tx.execute(
            "INSERT INTO djmdSampler (ID, Seq, Name, Attribute, ParentID, UUID, rb_local_usn, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?1, ?6, ?7, ?7)",
            params![id, seq, name, attribute, parent, next(), stamp],
        )?;
    }
    let year = rbl_core::time::now().get(..4).and_then(|y| y.parse::<i64>().ok()).unwrap_or(2026);
    for &(id, seq, name, attribute, parent, criteria) in RELATED_TRACKS {
        // The root is id 1 with its id for a UUID; the presets are numbered
        // and UUID'd afresh, as rekordbox's were.
        let (id, uuid) = if id.is_empty() {
            (unused_id(&tx, "djmdRelatedTracks", &mut rng)?, rng.uuid4())
        } else {
            (id.to_owned(), id.to_owned())
        };
        let criteria = criteria.replace("{from}", &(year - 1).to_string()).replace("{to}", &year.to_string());
        tx.execute(
            "INSERT INTO djmdRelatedTracks (ID, Seq, Name, Attribute, ParentID, Criteria, UUID, rb_local_usn, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
            params![id, seq, name, attribute, parent, criteria, uuid, next(), stamp],
        )?;
    }
    let (playlist_id, playlist_name) = CUE_ANALYSIS_PLAYLIST;
    tx.execute(
        "INSERT INTO djmdPlaylist (ID, Seq, Name, Attribute, ParentID, UUID, rb_local_usn, created_at, updated_at)
         VALUES (?1, 1, ?2, 0, 'root', ?3, ?4, ?5, ?5)",
        params![playlist_id, playlist_name, rng.uuid4(), next(), stamp],
    )?;
    tx.execute(
        "INSERT INTO agentRegistry (registry_id, int_1, created_at, updated_at)
         VALUES ('localUpdateCount', ?1, ?2, ?2)",
        params![usn, stamp],
    )?;
    tx.execute(
        "INSERT INTO agentRegistry (registry_id, str_1, created_at, updated_at)
         VALUES ('SyncAnalysisDataRootPath', ?1, ?2, ?2)",
        params![share_root.to_string_lossy(), stamp],
    )?;
    tx.commit()?;

    // rekordbox's own database is in WAL mode, and the mode is kept in the
    // file. Closing checkpoints the log away, leaving the one file to move.
    let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(DbError::Open(format!("the new library would not take WAL mode: {mode}")));
    }
    conn.close().map_err(|(_, e)| DbError::Sqlite(e))?;
    Ok(())
}

/// A decimal id below 2^32 that `table` does not hold yet, as rekordbox's
/// generated ids are.
fn unused_id(conn: &Connection, table: &str, rng: &mut rbl_core::ids::Rng) -> Result<String> {
    let sql = format!("SELECT COUNT(*) FROM {table} WHERE ID = ?1");
    loop {
        let candidate = rng.numeric_id(1 << 32);
        let taken: i64 = conn.query_row(&sql, [&candidate], |r| r.get(0))?;
        if taken == 0 {
            return Ok(candidate);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::{Library, OpenMode, SchemaSupport};

    /// A machine in a temp folder with nothing on it, and its default
    /// library folder.
    fn fresh() -> (tempfile::TempDir, Sources, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let sources = Sources::under(root.path());
        let library = sources.default_dir.clone();
        (root, sources, library)
    }

    fn found(sources: &Sources) -> LibraryLocation {
        match locate_with(sources).unwrap() {
            Located::Found { location, .. } => location,
            other => panic!("expected a library, got {other:?}"),
        }
    }

    /// rekordbox's settings naming `dir` as its library folder.
    fn rekordbox_settings(sources: &Sources, dir: &Path) {
        let file = sources.rekordbox_settings.as_deref().unwrap();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let dir = rbl_core::xml::escape(&dir.to_string_lossy());
        std::fs::write(file, format!("<PROPERTIES>\n  <VALUE name=\"masterDbDirectory\" val=\"{dir}\"/>\n</PROPERTIES>\n")).unwrap();
    }

    #[test]
    fn the_stored_dp_unwraps_to_rekordbox_s_passphrase() {
        let key = crate::key::derive_password(crate::key::REKORDBOX_DP).unwrap();
        assert_eq!(key.len(), 64);
        assert!(key.starts_with("402fd"), "the published rekordbox 6/7 key starts 402fd");
    }

    #[test]
    fn nothing_there_plans_a_library_in_the_default_folder() {
        let (_root, sources, library) = fresh();
        let plan = plan_with(&sources).unwrap().expect("a plan");
        assert_eq!(plan.master_db, library.join("master.db"));
    }

    #[test]
    fn a_made_library_is_found_opened_and_writable_like_an_installed_one() {
        let (_root, sources, library) = fresh();
        let plan = plan_with(&sources).unwrap().unwrap();
        let made = create(&plan).unwrap();

        assert!(library.join("share/PIONEER/Artwork").is_dir());
        assert!(library.join("share/PIONEER/USBANLZ").is_dir());
        let leftovers: Vec<_> = std::fs::read_dir(&library)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name != "master.db" && name != "share")
            .collect();
        assert!(leftovers.is_empty(), "nothing else is left beside it: {leftovers:?}");

        assert!(!sources.agent_options.as_deref().unwrap().exists(), "rekordbox's options.json is not written");
        let found = found(&sources);
        assert_eq!(found.master_db, made.master_db);
        assert_eq!(found.share_root, library.join("share"));
        assert_eq!(found.passphrase, made.passphrase);

        let mut location = found;
        location.is_real_install = false;
        let db = Library::open(location.clone(), OpenMode::ReadOnly).unwrap();
        assert_eq!(db.schema().db_version, Some(6000));
        assert_eq!(db.schema().support, SchemaSupport::Full);
        assert_eq!(db.schema().table_count, 47, "46 tables and sqlite_sequence");
        let journal: String = db.connection().query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        assert_eq!(journal, "wal");
        drop(db);

        let backups = tempfile::tempdir().unwrap();
        let mut writer = crate::write::Writer::open(location, backups.path()).unwrap();
        let id = writer.create_playlist("First", "root").unwrap();
        assert_ne!(id, "");
        let usn: i64 = writer
            .library()
            .connection()
            .query_row("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'", [], |r| r.get(0))
            .unwrap();
        let seeded: i64 = ["djmdColor", "djmdMenuItems", "djmdCategory", "djmdSort", "djmdMyTag", "djmdSampler", "djmdRelatedTracks"]
            .iter()
            .map(|t| writer.library().connection().query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get::<_, i64>(0)).unwrap())
            .sum::<i64>()
            + 1;
        assert!(usn > seeded, "the writer counts on from the seeded rows: {usn} after {seeded}");

        assert_eq!(plan_with(&sources).unwrap(), None, "made once, it is not offered again");
    }

    /// What rekordbox 7.2.14 wrote into a library it made itself, table by
    /// table, less the rows naming the machine and the account.
    #[test]
    fn a_made_library_starts_with_what_rekordbox_starts_one_with() {
        let (_root, sources, _library) = fresh();
        let mut location = create(&plan_with(&sources).unwrap().unwrap()).unwrap();
        location.is_real_install = false;
        let db = Library::open(location, OpenMode::ReadOnly).unwrap();
        let count = |sql: &str| -> i64 { db.connection().query_row(sql, [], |r| r.get(0)).unwrap() };
        for (table, rows) in [
            ("djmdColor", 8), ("djmdMenuItems", 27), ("djmdCategory", 22), ("djmdSort", 17),
            ("djmdMyTag", 28), ("djmdSampler", 3), ("djmdRelatedTracks", 4), ("djmdPlaylist", 1),
            ("djmdProperty", 1), ("djmdKey", 0), ("djmdContent", 0),
        ] {
            assert_eq!(count(&format!("SELECT COUNT(*) FROM {table}")), rows, "{table}");
        }
        assert_eq!(count("SELECT COUNT(*) FROM djmdMyTag WHERE ParentID = 'root' AND Attribute = 1"), 4);
        assert_eq!(count("SELECT COUNT(*) FROM djmdMyTag WHERE ParentID = '3' AND Attribute = 0"), 8, "Situation's tags");
        assert_eq!(count("SELECT COUNT(*) FROM djmdPlaylist WHERE ID = '200000' AND Name = 'CUE Analysis Playlist'"), 1);
        // The last update number handed out is the counter.
        assert_eq!(
            count("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'"),
            count("SELECT MAX(u) FROM (SELECT rb_local_usn u FROM djmdMyTag UNION ALL SELECT rb_local_usn FROM djmdPlaylist)"),
        );
        let criteria: String = db
            .connection()
            .query_row("SELECT Criteria FROM djmdRelatedTracks WHERE Name = 'Same Artist'", [], |r| r.get(0))
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&criteria).expect("the criteria are JSON");
        let year = parsed["Year"]["Rang"]["Max"].as_i64().unwrap();
        assert_eq!(parsed["Year"]["Rang"]["Min"].as_i64(), Some(year - 1));
        assert_eq!(parsed["Arti"]["True"].as_i64(), Some(1));
    }

    #[test]
    fn an_existing_database_is_never_replaced() {
        let (_root, sources, library) = fresh();
        let plan = plan_with(&sources).unwrap().unwrap();
        std::fs::create_dir_all(&library).unwrap();
        std::fs::write(library.join("master.db"), b"someone else's").unwrap();

        assert!(create(&plan).is_err());
        assert_eq!(std::fs::read(library.join("master.db")).unwrap(), b"someone else's");
    }

    #[test]
    fn a_missing_configured_drive_is_never_where_a_library_is_made() {
        let (root, sources, _library) = fresh();
        let drive = root.path().join("Volumes/DJ SSD/PIONEER/Master");
        rekordbox_settings(&sources, &drive);
        assert_eq!(plan_with(&sources).unwrap(), None, "rekordbox asks for the drive instead");
        assert!(!root.path().join("Volumes").exists());
    }

    #[test]
    fn an_options_file_naming_a_missing_library_is_not_where_one_is_made() {
        let (root, sources, _library) = fresh();
        let elsewhere = root.path().join("elsewhere/master.db");
        let options = sources.agent_options.clone().unwrap();
        std::fs::create_dir_all(options.parent().unwrap()).unwrap();
        crate::fixture::write_options_json(&options, &elsewhere.to_string_lossy(), "their-own-key").unwrap();
        let before = std::fs::read(&options).unwrap();

        assert_eq!(plan_with(&sources).unwrap(), None);
        assert!(!elsewhere.parent().unwrap().exists());
        assert_eq!(std::fs::read(&options).unwrap(), before, "options.json is only read");
    }

    #[test]
    fn an_unreadable_options_file_does_not_hide_the_default_folder() {
        let (_root, sources, library) = fresh();
        let options = sources.agent_options.clone().unwrap();
        std::fs::create_dir_all(options.parent().unwrap()).unwrap();
        std::fs::write(&options, b"{ not json").unwrap();
        assert_eq!(plan_with(&sources).unwrap().unwrap().master_db, library.join("master.db"));
    }

    #[test]
    fn a_default_database_without_an_options_file_is_found_not_offered() {
        let (_root, sources, library) = fresh();
        std::fs::create_dir_all(&library).unwrap();
        std::fs::write(library.join("master.db"), b"x").unwrap();
        assert_eq!(plan_with(&sources).unwrap(), None);
        assert_eq!(found(&sources).master_db, library.join("master.db"));
    }
}
