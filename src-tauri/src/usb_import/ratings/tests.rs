#![allow(clippy::unwrap_used)]
use std::path::PathBuf;
use crate::{state::AppState, grid::GridEditor, usb_import::{ImportReport, import}};

struct Fixture {
    _dir: tempfile::TempDir,
    usb: PathBuf,
    state: AppState,
    tracks: Vec<rbl_export::SourceTrack>,
    sync: rbl_export::SyncSource,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let location = rbl_db::fixture::build(dir.path(), rbl_db::fixture::Shape { tracks: 3, playlists: 0, history_sessions: 0, ..Default::default() }).unwrap();
        let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadWrite).unwrap();
        let mut tracks = Vec::new();
        for index in 0..3 {
            let id = rbl_db::fixture::track_id(index);
            let path = dir.path().join(format!("{index}.mp3"));
            std::fs::write(&path, [0; 2048]).unwrap();
            db.connection().execute("UPDATE djmdContent SET FolderPath=?1, Rating=2 WHERE ID=?2", rusqlite::params![path.to_string_lossy(), id]).unwrap();
            tracks.push(rbl_export::SourceTrack { id: id.parse().unwrap(), source_path: path, title: format!("Track {index}"), rating: 2,
                analysis: vec![("DAT".into(), rbl_anlz::AnlzBuilder::new().path("/x.mp3").finish())], ..Default::default() });
        }
        let sync = rbl_export::SyncSource { db_id: rbl_db::export_info::db_id(db.connection()).unwrap(), ..Default::default() };
        let (library, _) = rbl_index::load(&db).unwrap();
        let state = AppState::with_backups(dir.path().join("backups"));
        state.set_library(library, false, db.schema().db_version, 0, location);
        drop(db);
        let usb = dir.path().join("usb");
        rbl_export::export_full(&usb, &tracks, &[], &[], None, Some(&sync), &mut |_| {}).unwrap();
        Self { _dir: dir, usb, state, tracks, sync }
    }
    fn import(&self) -> Result<ImportReport, crate::AppError> {
        import(&self.state, &GridEditor::at(self.state.backup_dir()), &self.usb, false, false, false, true)
    }
    fn one(&self, sql: &str) {
        let db = rusqlite::Connection::open(self.usb.join("PIONEER/rekordbox/exportLibrary.db")).unwrap();
        db.pragma_update(None, "cipher", "sqlcipher").unwrap();
        db.pragma_update(None, "legacy", 4).unwrap();
        db.pragma_update(None, "key", rbl_onelibrary::key::passphrase().unwrap()).unwrap();
        db.execute_batch(sql).unwrap();
    }
    fn legacy(&self, id: u32, rating: u8) {
        let path = self.usb.join("PIONEER/rekordbox/export.pdb");
        let mut bytes = std::fs::read(&path).unwrap();
        let pdb = rbl_pdb::Pdb::parse(&bytes).unwrap();
        let row = pdb.rows(pdb.table(rbl_pdb::PageType::Tracks).unwrap()).into_iter().find(|row| pdb.u4_at(*row, 0x48) == id).unwrap();
        bytes[row.offset + 0x59] = rating;
        std::fs::write(path, bytes).unwrap();
    }
    fn desktop(&self, index: usize) -> (u8, i64) {
        self.state.read_db(|db| Ok(db.connection().query_row("SELECT Rating, rb_local_usn FROM djmdContent WHERE ID=?1", [rbl_db::fixture::track_id(index)], |row| Ok((row.get(0)?, row.get(1)?)))?)).unwrap()
    }
    fn rate(&self, index: usize, stars: u8) {
        self.state.write(|writer| writer.set_rating(&rbl_db::fixture::track_id(index), stars)).unwrap();
    }
}

#[test]
fn imports_either_format_clears_ratings_and_reexports_both_without_repeat_writes() {
    let mut fixture = Fixture::new();
    fixture.rate(0, 4); // A simultaneous desktop edit loses to the device.
    fixture.rate(2, 5); // An unchanged device must preserve a desktop-only edit.
    let untouched = fixture.desktop(2);
    fixture.one("UPDATE content SET rating=0 WHERE content_id=1");
    fixture.legacy(2, 3);
    let report = fixture.import().unwrap();
    assert_eq!((report.ratings, report.tracks, report.histories, report.settings), (2, 0, 0, 0));
    assert_eq!(report.changed, Vec::<String>::new());
    assert!(report.warnings.iter().any(|warning| warning.contains("replaced 1")));
    assert_eq!(fixture.desktop(0).0, 0);
    assert_eq!(fixture.desktop(1).0, 3);
    assert_eq!(fixture.desktop(2), untouched);
    let stamps: Vec<_> = (0..3).map(|index| fixture.desktop(index)).collect();
    assert_eq!(fixture.import().unwrap().ratings, 0);
    assert_eq!((0..3).map(|index| fixture.desktop(index)).collect::<Vec<_>>(), stamps);
    for index in 0..3 { fixture.tracks[index].rating = fixture.desktop(index).0; }
    rbl_export::export_full(&fixture.usb, &fixture.tracks, &[], &[], None, Some(&fixture.sync), &mut |_| {}).unwrap();
    let snapshot = rbl_export::snapshot::Snapshot::read(&fixture.usb).unwrap();
    assert_eq!(snapshot.one, snapshot.legacy);
    assert_eq!(snapshot.one.unwrap().tracks.iter().map(|track| track.rating).collect::<Vec<_>>(), vec![0, 3, 5]);
    assert_eq!(fixture.import().unwrap().ratings, 0);
}

#[test]
fn imports_legacy_only_and_deduplicates_matching_edits() {
    let fixture = Fixture::new();
    fixture.one("UPDATE content SET rating=5 WHERE content_id=1");
    fixture.legacy(1, 5);
    assert_eq!(fixture.import().unwrap().ratings, 1);
    std::fs::remove_file(fixture.usb.join("PIONEER/rekordbox/exportLibrary.db")).unwrap();
    fixture.legacy(2, 0);
    assert_eq!(fixture.import().unwrap().ratings, 1);
    assert_eq!(fixture.desktop(1).0, 0);
}

#[test]
fn imports_onelibrary_only_without_a_legacy_database() {
    let fixture = Fixture::new();
    std::fs::remove_file(fixture.usb.join("PIONEER/rekordbox/export.pdb")).unwrap();
    fixture.one("UPDATE content SET rating=5 WHERE content_id=1");
    assert_eq!(fixture.import().unwrap().ratings, 1);
    assert_eq!(fixture.desktop(0).0, 5);
    assert_eq!(fixture.import().unwrap().ratings, 0);
}

#[test]
fn conflicting_device_edits_and_invalid_stars_fail_before_any_rating_writes() {
    let fixture = Fixture::new();
    let before: Vec<_> = (0..3).map(|index| fixture.desktop(index)).collect();
    fixture.one("UPDATE content SET rating=5 WHERE content_id IN (1,2)");
    fixture.legacy(2, 4);
    assert!(fixture.import().unwrap_err().message.contains("disagree"));
    assert_eq!((0..3).map(|index| fixture.desktop(index)).collect::<Vec<_>>(), before);
    fixture.one("UPDATE content SET rating=255 WHERE content_id=1");
    assert!(fixture.import().unwrap_err().message.contains("invalid rating"));
    assert_eq!((0..3).map(|index| fixture.desktop(index)).collect::<Vec<_>>(), before);
}

#[test]
fn missing_baselines_and_foreign_manifests_warn_without_writing() {
    let fixture = Fixture::new();
    fixture.one("UPDATE content SET rating=5 WHERE content_id=1");
    let mut manifest = rbl_export::Manifest::load(&fixture.usb).unwrap();
    manifest.db_id = fixture.sync.db_id + 1;
    manifest.save(&fixture.usb).unwrap();
    let report = fixture.import().unwrap();
    assert_eq!(report.ratings, 0);
    assert_eq!(report.warnings.len(), 1);
    manifest.db_id = fixture.sync.db_id;
    manifest.baseline = None;
    manifest.save(&fixture.usb).unwrap();
    assert_eq!(fixture.import().unwrap().ratings, 0);
    std::fs::remove_file(rbl_export::Manifest::path(&fixture.usb)).unwrap();
    assert_eq!(fixture.import().unwrap().ratings, 0);
    assert_eq!(fixture.desktop(0).0, 2);
}

#[test]
fn rejects_reused_ids_and_unmatched_source_paths() {
    let fixture = Fixture::new();
    fixture.one("UPDATE content SET rating=255, masterContentId=999999 WHERE content_id=1; UPDATE content SET rating=4 WHERE content_id=2");
    let mut manifest = rbl_export::Manifest::load(&fixture.usb).unwrap();
    manifest.tracks[1].source = "/wrong/source.mp3".into();
    manifest.save(&fixture.usb).unwrap();
    let report = fixture.import().unwrap();
    assert_eq!((report.ratings, report.skipped), (0, 2));
    assert_eq!(fixture.desktop(0).0, 2);
    assert_eq!(fixture.desktop(1).0, 2);
}

#[test]
fn native_writer_refuses_an_unsupported_schema_even_without_device_changes() {
    let fixture = Fixture::new();
    let db = rbl_db::Library::open(fixture.state.location().unwrap(), rbl_db::OpenMode::ReadWrite).unwrap();
    db.connection().execute("ALTER TABLE djmdContent DROP COLUMN Rating", []).unwrap();
    drop(db);
    assert!(fixture.import().unwrap_err().message.contains("writes are disabled"));
}
