//! OneLibrary stores star counts, independently of rekordbox XML's 51 scale.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

fn export_stars(root: &Path, source: &Path) {
    let tracks: Vec<_> = (0..=5)
        .map(|rating| {
            let path = source.join(format!("stars-{rating}.mp3"));
            std::fs::write(&path, [0; 2048]).unwrap();
            rbl_export::SourceTrack {
                source_path: path,
                title: format!("Stars {rating}"),
                rating,
                analysis: vec![("DAT".into(), rbl_anlz::AnlzBuilder::new().path("/x.mp3").finish())],
                ..Default::default()
            }
        })
        .collect();
    rbl_export::export(root, &tracks, &[]).unwrap();
}

#[test]
fn exported_one_library_ratings_are_raw_star_counts() {
    let source = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    export_stars(usb.path(), source.path());

    let db = rbl_onelibrary::ExportLibrary::open_read_only(
        &usb.path().join("PIONEER/rekordbox/exportLibrary.db"),
    ).unwrap();
    let mut query = db.connection().prepare("SELECT rating FROM content ORDER BY content_id").unwrap();
    let ratings: Vec<i64> = query.query_map([], |row| row.get(0)).unwrap()
        .collect::<Result<_, _>>().unwrap();
    assert_eq!(ratings, vec![0, 1, 2, 3, 4, 5]);
}

#[test]
fn one_library_star_counts_survive_snapshot_and_legacy_conversion() {
    let source = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    export_stars(usb.path(), source.path());
    let dir = usb.path().join("PIONEER/rekordbox");
    // Supply independent device-format values rather than trusting the exporter.
    let db = rusqlite::Connection::open(dir.join("exportLibrary.db")).unwrap();
    db.pragma_update(None, "cipher", "sqlcipher").unwrap();
    db.pragma_update(None, "legacy", 4).unwrap();
    db.pragma_update(None, "key", rbl_onelibrary::key::passphrase().unwrap()).unwrap();
    db.execute("UPDATE content SET rating = content_id - 1", []).unwrap();
    drop(db);
    std::fs::remove_file(dir.join("export.pdb")).unwrap();

    let before = rbl_export::snapshot::Snapshot::read(usb.path()).unwrap();
    let ratings: Vec<_> = before.one.unwrap().tracks.iter().map(|t| t.rating).collect();
    assert_eq!(ratings, vec![0, 1, 2, 3, 4, 5]);

    rbl_export::create_library(usb.path(), None, &[], None).unwrap();
    let after = rbl_export::snapshot::Snapshot::read(usb.path()).unwrap();
    assert_eq!(after.one, after.legacy);
    let ratings: Vec<_> = after.legacy.unwrap().tracks.iter().map(|t| t.rating).collect();
    assert_eq!(ratings, vec![0, 1, 2, 3, 4, 5]);
}

#[test]
fn resync_repairs_the_old_xml_scale_without_discarding_other_device_edits() {
    let source = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    export_stars(usb.path(), source.path());
    let db = rusqlite::Connection::open(usb.path().join("PIONEER/rekordbox/exportLibrary.db")).unwrap();
    db.pragma_update(None, "cipher", "sqlcipher").unwrap();
    db.pragma_update(None, "legacy", 4).unwrap();
    db.pragma_update(None, "key", rbl_onelibrary::key::passphrase().unwrap()).unwrap();
    db.execute("UPDATE content SET rating=rating*51", []).unwrap();
    drop(db);
    export_stars(usb.path(), source.path());
    let snapshot = rbl_export::snapshot::Snapshot::read(usb.path()).unwrap();
    assert_eq!(snapshot.one, snapshot.legacy);
    assert_eq!(snapshot.one.unwrap().tracks.iter().map(|track| track.rating).collect::<Vec<_>>(), vec![0, 1, 2, 3, 4, 5]);

    let db = rusqlite::Connection::open(usb.path().join("PIONEER/rekordbox/exportLibrary.db")).unwrap();
    db.pragma_update(None, "cipher", "sqlcipher").unwrap();
    db.pragma_update(None, "legacy", 4).unwrap();
    db.pragma_update(None, "key", rbl_onelibrary::key::passphrase().unwrap()).unwrap();
    db.execute("UPDATE content SET rating=rating*51, djComment='Device edit' WHERE content_id=6", []).unwrap();
    drop(db);
    let tracks = (0..=5).map(|rating| rbl_export::SourceTrack {
        source_path: source.path().join(format!("stars-{rating}.mp3")), title: format!("Stars {rating}"), rating,
        analysis: vec![("DAT".into(), rbl_anlz::AnlzBuilder::new().path("/x.mp3").finish())], ..Default::default()
    }).collect::<Vec<_>>();
    assert!(matches!(rbl_export::export(usb.path(), &tracks, &[]), Err(rbl_export::ExportError::Conflict(_))));
}
