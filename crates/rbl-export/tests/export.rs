//! End-to-end: write an export to a temp directory, then read it back the way
//! a player would.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use rbl_export::{export, verify, SourcePlaylist, SourceTrack};

/// Writes a dummy audio file and returns a track that points at it.
fn track(dir: &std::path::Path, n: u32, title: &str, artist: &str) -> SourceTrack {
    let path = dir.join(format!("source-{n}.mp3"));
    std::fs::write(&path, vec![n as u8; 2048]).unwrap();
    SourceTrack {
        source_path: path,
        title: title.into(),
        artist: artist.into(),
        album: "Single".into(),
        genre: "House".into(),
        key: "Am".into(),
        bpm_x100: 12_800,
        duration_sec: 300,
        rating: 4,
        date_added: "2026-09-06".into(),
        analysis: vec![("DAT".into(), rbl_anlz::AnlzBuilder::new().path("/x.mp3").finish())],
        ..SourceTrack::default()
    }
}

#[test]
fn writes_a_tree_a_player_can_browse() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let tracks = vec![
        track(src.path(), 1, "All U Need", "TRIODE"),
        track(src.path(), 2, "The Abyss", "ARTBAT"),
    ];
    let playlists = vec![SourcePlaylist { name: "Melodic Vox".into(), track_indices: vec![0, 1], ..Default::default() }];

    let report = export(dest.path(), &tracks, &playlists).unwrap();
    assert_eq!(report.tracks, 2);
    assert_eq!(report.playlists, 1);
    assert_eq!(report.analysis_files, 2);
    assert!(report.skipped.is_empty());

    // The layout a CDJ expects.
    assert!(dest.path().join("PIONEER/rekordbox/export.pdb").is_file());
    assert!(dest.path().join("Contents/TRIODE/Single/source-1.mp3").is_file());
    assert!(dest.path().join("Contents/ARTBAT/Single/source-2.mp3").is_file());

    let check = verify(dest.path()).unwrap();
    assert!(check.parsed);
    assert_eq!(check.tracks, 2);
    assert_eq!(check.playlists, 1);
    assert_eq!(check.playlist_entries, 2);
    assert_eq!(check.audio_present, 2, "every track must point at audio that exists");
    assert_eq!(check.analysis_present, 2);
    assert!(check.is_ok());
}

#[test]
fn hfs_export_uses_hidden_filesystem_and_database_paths() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let tracks = [track(src.path(), 1, "Hidden", "Artist")];
    rbl_export::export_with_options(
        dest.path(),
        &tracks,
        &[],
        &[],
        &rbl_export::ExportOptions {
            root: Some(rbl_export::ExportRoot::Hidden),
            ..Default::default()
        },
        &mut |_| {},
    )
    .unwrap();

    assert!(dest.path().join(".PIONEER/rekordbox/export.pdb").is_file());
    assert!(dest.path().join(".PIONEER/rekordbox/exportLibrary.db").is_file());
    assert!(dest.path().join(".PIONEER/USBANLZ").is_dir());
    assert!(!dest.path().join("PIONEER").exists());

    let snapshot = rbl_export::snapshot::Snapshot::read(dest.path()).unwrap();
    let legacy = snapshot.legacy.unwrap();
    let one = snapshot.one.unwrap();
    assert_eq!(legacy, one, "both exported databases use the same paths");
    assert!(legacy.tracks[0].analysis.starts_with("/.PIONEER/USBANLZ/"), "{}", legacy.tracks[0].analysis);
    assert!(rbl_export::verify(dest.path()).unwrap().is_ok());
}

#[test]
fn metadata_survives_the_round_trip() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let mut t = track(src.path(), 1, "Ébano — Tiësto Remix", "Tiësto");
    t.comment = "8A - C - 128".into();
    t.bpm_x100 = 12_345;
    t.key = "Ebm".into();

    export(dest.path(), &[t], &[]).unwrap();

    let bytes = std::fs::read(dest.path().join("PIONEER/rekordbox/export.pdb")).unwrap();
    let pdb = rbl_pdb::Pdb::parse(&bytes).unwrap();
    let rows = pdb.track_rows(pdb.table(rbl_pdb::PageType::Tracks).unwrap());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "Ébano — Tiësto Remix");
    assert_eq!(rows[0].tempo_x100, 12_345);
    assert_eq!(rows[0].comment, "8A - C - 128");

    // The key and artist tables must carry the names, referenced by id.
    let keys = pdb.named_rows(pdb.table(rbl_pdb::PageType::Keys).unwrap());
    assert!(keys.iter().any(|k| k.name == "Ebm"), "{keys:?}");
    let artists = pdb.named_rows(pdb.table(rbl_pdb::PageType::Artists).unwrap());
    assert!(artists.iter().any(|a| a.name == "Tiësto"), "{artists:?}");
}

#[test]
fn verification_rejects_player_incompatible_records_and_sync_repairs_them() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let mut t = track(src.path(), 1, "Kept", "Artist");
    t.id = 1;
    let tracks = vec![t];
    let playlists = vec![SourcePlaylist {
        id: 1, name: "Playlist".into(), track_indices: vec![0], ..Default::default()
    }];
    export(dest.path(), &tracks, &playlists).unwrap();
    let path = dest.path().join("PIONEER/rekordbox/export.pdb");
    for (field, message) in [(0, "record subtype"), (0x56, "record trailer"), (0x5a, "audio format"), (0x5c, "record trailer")] {
        let mut bytes = std::fs::read(&path).unwrap();
        let parsed = rbl_pdb::Pdb::parse(&bytes).unwrap();
        let offset = parsed.rows(parsed.table(rbl_pdb::PageType::Tracks).unwrap())[0].offset;
        bytes[offset + field..offset + field + 2].fill(0);
        std::fs::write(&path, bytes).unwrap();
        let bad = verify(dest.path()).unwrap();
        assert_eq!(bad.tracks, 1, "semantic read-back still sees the track");
        assert_eq!(bad.playlist_entries, 1);
        assert!(!bad.is_ok());
        assert!(bad.errors.iter().any(|e| e.contains(message)), "{:?}", bad.errors);

        // Existing broken exports must remain readable for reconciliation,
        // so a normal re-sync can repair them without clearing the stick.
        export(dest.path(), &tracks, &playlists).unwrap();
        let repaired = verify(dest.path()).unwrap();
        assert!(repaired.is_ok(), "{:?}", repaired.errors);
        assert_eq!(repaired.tracks, 1);
        assert_eq!(repaired.playlist_entries, 1);
    }
}

#[test]
fn every_export_carries_rekordboxs_eight_colours() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    export(dest.path(), &[track(src.path(), 1, "T", "A")], &[]).unwrap();

    let bytes = std::fs::read(dest.path().join("PIONEER/rekordbox/export.pdb")).unwrap();
    let pdb = rbl_pdb::Pdb::parse(&bytes).unwrap();
    let colors = pdb.named_rows(pdb.table(rbl_pdb::PageType::Colors).unwrap());
    let names: Vec<String> = colors.into_iter().map(|c| c.name).collect();
    assert_eq!(
        names,
        vec!["Pink", "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple"]
    );
}

#[test]
fn a_missing_source_file_skips_that_track_rather_than_failing_the_export() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let good = track(src.path(), 1, "Present", "A");
    let mut bad = track(src.path(), 2, "Missing", "B");
    bad.source_path = PathBuf::from("/definitely/not/here.mp3");

    let report = export(dest.path(), &[good, bad], &[]).unwrap();
    assert_eq!(report.tracks, 1);
    assert_eq!(report.skipped, vec!["Missing".to_owned()]);
    assert!(verify(dest.path()).unwrap().is_ok());
}

#[test]
fn names_that_fat32_cannot_hold_are_made_safe() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let mut t = track(src.path(), 1, "Title", "AC/DC: Live?");
    t.album = "Best of *".into();
    export(dest.path(), &[t], &[]).unwrap();

    assert!(dest.path().join("Contents/AC_DC_ Live_/Best of _/source-1.mp3").is_file());
    // And the database must point at exactly where the file landed.
    assert!(verify(dest.path()).unwrap().is_ok());
}

#[test]
fn a_large_export_keeps_every_track_and_playlist_entry() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let tracks: Vec<SourceTrack> = (1..=300)
        .map(|i| track(src.path(), i, &format!("Track {i:03}"), &format!("Artist {}", i % 20)))
        .collect();
    let playlists = vec![
        SourcePlaylist { name: "All".into(), track_indices: (0..300).collect(), ..Default::default() },
        SourcePlaylist { name: "First ten".into(), track_indices: (0..10).collect(), ..Default::default() },
    ];

    let report = export(dest.path(), &tracks, &playlists).unwrap();
    assert_eq!(report.tracks, 300);

    let check = verify(dest.path()).unwrap();
    assert_eq!(check.tracks, 300, "tracks must survive spanning pages");
    assert_eq!(check.playlists, 2);
    assert_eq!(check.playlist_entries, 310);
    assert_eq!(check.audio_present, 300);
    assert!(check.missing_audio.is_empty());
}

#[test]
fn an_empty_export_writes_two_valid_empty_libraries() {
    let dest = tempfile::tempdir().unwrap();
    export(dest.path(), &[], &[]).unwrap();
    assert!(verify(dest.path()).unwrap().is_ok());
}

#[test]
fn analysis_files_land_where_the_database_says_they_do() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    export(dest.path(), &[track(src.path(), 1, "T", "A")], &[]).unwrap();

    let bytes = std::fs::read(dest.path().join("PIONEER/rekordbox/export.pdb")).unwrap();
    let pdb = rbl_pdb::Pdb::parse(&bytes).unwrap();
    let rows = pdb.track_rows(pdb.table(rbl_pdb::PageType::Tracks).unwrap());
    let analyze = &rows[0].analyze_path;
    assert!(analyze.starts_with("/PIONEER/USBANLZ/"), "{analyze}");
    assert!(analyze.ends_with("ANLZ0000.DAT"), "{analyze}");

    let on_disk = dest.path().join(analyze.trim_start_matches('/'));
    assert!(on_disk.is_file(), "the database points at {analyze}, which does not exist");
    // And it must still be a valid analysis file.
    let analysis = rbl_anlz::parse(&std::fs::read(&on_disk).unwrap()).unwrap();
    assert!(analysis.section(b"PVBR").is_some(), "an export repairs pre-RBX-18 DAT files");
    assert_eq!(analysis.sections.iter().filter(|section| section.tag == rbl_core::FourCc::new(b"PPTH")).count(), 1);
}

#[test]
fn an_export_carries_a_readable_export_library_beside_the_pdb() {
    // A player never opens this file; rekordbox does, to read a stick back.
    let dir = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let tracks = vec![
        track(source.path(), 1, "The Abyss", "ARTBAT"),
        track(source.path(), 2, "Take Me Home", "MORTEN"),
    ];
    let playlists = vec![rbl_export::SourcePlaylist {
        name: "Friday".to_owned(),
        track_indices: vec![1, 0],
        ..Default::default()
    }];

    let report = rbl_export::export(dir.path(), &tracks, &playlists).expect("export");
    assert!(report.one_library, "the report must say it was written");

    let path = dir.path().join("PIONEER/rekordbox/exportLibrary.db");
    assert!(path.exists(), "exportLibrary.db is missing");

    let db = rbl_onelibrary::ExportLibrary::open_read_only(&path).expect("open");
    assert_eq!(db.count("content").unwrap(), 2);
    assert_eq!(db.count("playlist").unwrap(), 1);
    assert_eq!(db.count("playlist_content").unwrap(), 2);

    // The two databases must agree: the playlist order here is the order the
    // export was asked for, not the order the tracks were listed in.
    let mut stmt = db
        .connection()
        .prepare("SELECT content_id FROM playlist_content WHERE playlist_id = 1 ORDER BY sequenceNo")
        .unwrap();
    let order: Vec<i64> =
        stmt.query_map([], |r| r.get(0)).unwrap().filter_map(Result::ok).collect();
    assert_eq!(order, vec![2, 1]);

    // And a path in one is the same path as in the other.
    let audio: String = db
        .connection()
        .query_row("SELECT path FROM content WHERE content_id = 1", [], |r| r.get(0))
        .unwrap();
    assert!(audio.starts_with("/Contents/"), "{audio}");
    assert!(dir.path().join(audio.trim_start_matches('/')).exists(), "{audio} is not on the stick");
}

#[test]
fn a_fresh_stick_takes_the_defaults_it_is_given_and_keeps_them_after() {
    use rbl_onelibrary::settings::StickSettings;

    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let tracks = vec![track(src.path(), 1, "All U Need", "TRIODE")];

    // The Preferences window's choices: GENRE turned on as the first
    // category, and BPM as the column beside the title.
    let mut defaults = StickSettings::default();
    let genre = defaults.categories.iter_mut().find(|c| c.name == "GENRE").unwrap();
    genre.visible = true;
    genre.seq = 1;
    defaults.sub_column = Some(5);

    rbl_export::export_with(dest.path(), &tracks, &[], Some(&defaults)).unwrap();
    let db = dest.path().join("PIONEER/rekordbox/exportLibrary.db");
    let written = StickSettings::read(&db).unwrap();
    assert!(written.categories.iter().find(|c| c.name == "GENRE").unwrap().visible);
    assert_eq!(written.sub_column, Some(5));

    // A second export with different defaults changes nothing: the stick's
    // settings are its own now.
    let other = StickSettings { sub_column: Some(2), ..StickSettings::default() };
    rbl_export::export_with(dest.path(), &tracks, &[], Some(&other)).unwrap();
    let kept = StickSettings::read(&db).unwrap();
    assert_eq!(kept.sub_column, Some(5));
    assert!(kept.categories.iter().find(|c| c.name == "GENRE").unwrap().visible);
}

#[test]
fn the_pdb_property_row_names_the_stick_and_keeps_its_background_colour() {
    use rbl_onelibrary::settings::StickSettings;

    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let tracks = vec![track(src.path(), 1, "One", "A"), track(src.path(), 2, "Two", "B")];
    let defaults = StickSettings { device_name: "FRIDAY".into(), ..StickSettings::default() };
    rbl_export::export_with(dest.path(), &tracks, &[], Some(&defaults)).unwrap();

    let pdb_path = dest.path().join("PIONEER/rekordbox/export.pdb");
    let bytes = std::fs::read(&pdb_path).unwrap();
    let property = rbl_pdb::Pdb::parse(&bytes).unwrap().property().expect("a property row");
    // The same name and count as exportLibrary.db's property row.
    assert_eq!(property.device_name, "FRIDAY");
    assert_eq!(property.contents, 2);
    assert_eq!(property.db_version, "1000");
    assert_eq!(property.created_date.len(), 10);
    assert_eq!(property.background_color, 0);

    // rekordbox sets "Background Color : Device Library" to Blue.
    let blue = rbl_pdb::rows::PdbProperty { background_color: 7, ..property };
    let row = rbl_pdb::rows::property_row(&blue).unwrap();
    let patched = rbl_pdb::build::replace_single_page_table(&bytes, 19, &[row]).unwrap();
    std::fs::write(&pdb_path, patched).unwrap();

    // A sync rebuilds export.pdb and keeps the colour.
    rbl_export::export_with(dest.path(), &tracks[..1], &[], None).unwrap();
    let bytes = std::fs::read(&pdb_path).unwrap();
    let kept = rbl_pdb::Pdb::parse(&bytes).unwrap().property().unwrap();
    assert_eq!(kept.background_color, 7);
    assert_eq!(kept.device_name, "FRIDAY");
    assert_eq!(kept.contents, 1);
}

#[test]
fn a_blank_stick_is_given_the_database_folders_rekordbox_creates_on_connect() {
    let stick = tempfile::tempdir().unwrap();
    assert!(rbl_export::create_library(stick.path(), None, &[], None).expect("create"), "a blank stick gets a database");
    assert!(stick.path().join("PIONEER/rekordbox/export.pdb").is_file());
    assert!(stick.path().join("PIONEER/rekordbox/exportLibrary.db").is_file());
    assert!(stick.path().join("PIONEER/USBANLZ").is_dir());
    assert!(stick.path().join("Contents").is_dir());

    let bytes = std::fs::read(stick.path().join("PIONEER/rekordbox/export.pdb")).unwrap();
    let pdb = rbl_pdb::Pdb::parse(&bytes).expect("parses");
    // The twenty tables rekordbox writes, in its order, with the constant
    // rows in place and nothing in the ones the library fills.
    let types: Vec<u32> = pdb.tables.iter().map(|t| match t.page_type {
        rbl_pdb::PageType::Other(v) => v,
        known => (0..20).find(|&v| rbl_pdb::PageType::name(known) == rbl_pdb::PageType::name(match v {
            0 => rbl_pdb::PageType::Tracks, 1 => rbl_pdb::PageType::Genres, 2 => rbl_pdb::PageType::Artists,
            3 => rbl_pdb::PageType::Albums, 4 => rbl_pdb::PageType::Labels, 5 => rbl_pdb::PageType::Keys,
            6 => rbl_pdb::PageType::Colors, 7 => rbl_pdb::PageType::PlaylistTree, 8 => rbl_pdb::PageType::PlaylistEntries,
            13 => rbl_pdb::PageType::Artwork, 16 => rbl_pdb::PageType::Columns, 17 => rbl_pdb::PageType::HistoryPlaylists,
            18 => rbl_pdb::PageType::HistoryEntries, 19 => rbl_pdb::PageType::Property, other => rbl_pdb::PageType::Other(other),
        })).unwrap_or(u32::MAX),
    }).collect();
    assert_eq!(types, (0..20).collect::<Vec<u32>>());
    let census = pdb.census();
    assert_eq!(census.get("tracks"), Some(&0));
    assert_eq!(census.get("colors"), Some(&8));
    assert_eq!(census.get("columns"), Some(&27));
    assert_eq!(census.get("history_playlists"), Some(&22));
    assert_eq!(census.get("history_entries"), Some(&17));
    assert_eq!(census.get("property"), Some(&1));

    // Its settings can be read and written like any stick's.
    let settings = rbl_onelibrary::settings::StickSettings::read(&stick.path().join("PIONEER/rekordbox/exportLibrary.db")).expect("settings");
    assert_eq!(settings.categories.len(), 22);

    // Asking again leaves what is there.
    assert!(!rbl_export::create_library(stick.path(), None, &[], None).expect("second"));
}

#[test]
fn a_blank_hfs_stick_gets_only_the_hidden_rekordbox_root() {
    let stick = tempfile::tempdir().unwrap();
    assert!(rbl_export::create_library_with_root(
        stick.path(),
        None,
        &[],
        None,
        Some(rbl_export::ExportRoot::Hidden),
    )
    .unwrap());
    assert!(stick.path().join(".PIONEER/rekordbox/export.pdb").is_file());
    assert!(stick.path().join(".PIONEER/rekordbox/exportLibrary.db").is_file());
    assert!(stick.path().join(".PIONEER/USBANLZ").is_dir());
    assert!(!stick.path().join("PIONEER").exists());
}

#[test]
fn directory_names_are_cut_where_rekordbox_cuts_them_and_the_file_name_is_not() {
    let source = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let long_album = "Disco Lines & Tinashe - No Broke Boys (DANSYN Remix) (Extended Edition)";
    let mut t = track(source.path(), 1, "No Broke Boys", "Disco Lines & Tinashe");
    t.album = long_album.to_owned();
    t.file_size = 12_965_026;
    let report = rbl_export::export(dir.path(), &[t], &[]).expect("export");
    assert_eq!(report.tracks, 1);
    let album_dir = std::fs::read_dir(dir.path().join("Contents/Disco Lines & Tinashe"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .file_name()
        .to_string_lossy()
        .into_owned();
    assert_eq!(album_dir, "Disco Lines & Tinashe - No Broke Boys (DANSYN Re");
    assert_eq!(album_dir.chars().count(), 48);

    // The database carries the library's file size, not the copy's.
    let bytes = std::fs::read(dir.path().join("PIONEER/rekordbox/export.pdb")).unwrap();
    let pdb = rbl_pdb::Pdb::parse(&bytes).unwrap();
    let rows = pdb.track_rows(pdb.table(rbl_pdb::PageType::Tracks).unwrap());
    assert_eq!(rows[0].file_size, 12_965_026);
    // No AppleDouble sidecar beside the copy: only the bytes were copied.
    let names: Vec<String> = std::fs::read_dir(dir.path().join("Contents/Disco Lines & Tinashe").join(&album_dir))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().all(|n| !n.starts_with("._")), "{names:?}");
}

#[test]
fn my_settings_are_copied_from_rekordbox_and_a_sticks_own_are_kept() {
    let source = tempfile::tempdir().unwrap();
    let stick = tempfile::tempdir().unwrap();
    for name in rbl_export::MY_SETTINGS_FILES {
        std::fs::write(source.path().join(name), format!("from rekordbox: {name}")).unwrap();
    }
    std::fs::create_dir_all(stick.path().join("PIONEER")).unwrap();
    std::fs::write(stick.path().join("PIONEER/MYSETTING.DAT"), b"set on a player").unwrap();
    let written = rbl_export::copy_my_settings(stick.path(), source.path()).expect("copy");
    assert_eq!(written, 3, "the one the stick had is kept");
    assert_eq!(std::fs::read(stick.path().join("PIONEER/MYSETTING.DAT")).unwrap(), b"set on a player");
    assert_eq!(
        std::fs::read_to_string(stick.path().join("PIONEER/djprofile.nxs")).unwrap(),
        "from rekordbox: djprofile.nxs"
    );
    // A machine without rekordbox has nothing to give and that is fine.
    let empty = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    assert_eq!(rbl_export::copy_my_settings(other.path(), empty.path()).unwrap(), 0);

    let hidden = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(hidden.path().join(".PIONEER")).unwrap();
    std::fs::write(hidden.path().join(".PIONEER/DEVSETTING.DAT"), b"hidden library").unwrap();
    assert_eq!(rbl_export::copy_my_settings(hidden.path(), source.path()).unwrap(), 4);
    assert!(hidden.path().join(".PIONEER/MYSETTING.DAT").is_file());
    assert!(!hidden.path().join("PIONEER").exists());
}

#[test]
fn artwork_and_my_tags_go_to_the_stick_with_the_tracks() {
    use rbl_export::{export_full, SourceMyTag};

    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    // The library keeps three sizes in one folder; the track names the big
    // one, the stick gets the small and the medium.
    let folder = src.path().join("Artwork/abc/def");
    std::fs::create_dir_all(&folder).unwrap();
    let image = folder.join("artwork.jpg");
    std::fs::write(&image, b"\xff\xd8not really a jpeg\xff\xd9").unwrap();
    let small = folder.join("artwork_s.jpg");
    std::fs::write(&small, b"\xff\xd8small\xff\xd9").unwrap();
    let medium = folder.join("artwork_m.jpg");
    std::fs::write(&medium, b"\xff\xd8medium, a little bigger\xff\xd9").unwrap();
    let mut tracks = vec![
        track(src.path(), 1, "All U Need", "TRIODE"),
        track(src.path(), 2, "The Abyss", "ARTBAT"),
        track(src.path(), 3, "No Cover", "Nobody"),
    ];
    // Two tracks of one album share the image; the third has none.
    tracks[0].artwork = Some(image.clone());
    tracks[1].artwork = Some(image.clone());
    tracks[0].my_tags = vec![11, 12];
    tracks[1].my_tags = vec![12, 99]; // 99 is not a tag the library has
    let my_tags = vec![
        SourceMyTag { id: 1, seq: 1, name: "Genre".into(), attribute: 1, parent: 0 },
        SourceMyTag { id: 11, seq: 1, name: "Peak".into(), attribute: 0, parent: 1 },
        SourceMyTag { id: 12, seq: 2, name: "Warm-up".into(), attribute: 0, parent: 1 },
    ];
    let playlists = vec![SourcePlaylist { device_id: 0, device_only: false, parent_id: 0, folder: false, id: 0, name: "Set".into(), track_indices: vec![0, 1, 2] }];

    let mut seen: Vec<(usize, usize)> = Vec::new();
    let report = export_full(dest.path(), &tracks, &playlists, &my_tags, None, None, &mut |p| {
        if p.stage == "copying" { seen.push((p.done, p.total)); }
    }).unwrap();
    assert_eq!(report.tracks, 3);
    assert_eq!(seen, vec![(0, 3), (1, 3), (2, 3)], "progress is reported per track");
    assert_eq!(report.artwork_files, 4, "one image, written under its four names");
    for (name, source) in [("a1.jpg", &small), ("a1_m.jpg", &medium), ("b1.jpg", &small), ("b1_m.jpg", &medium)] {
        let file = dest.path().join("PIONEER/Artwork/00001").join(name);
        assert_eq!(std::fs::read(&file).unwrap(), std::fs::read(source).unwrap(), "{name}");
    }

    // The pdb names the image and the tracks point at it.
    let bytes = std::fs::read(dest.path().join("PIONEER/rekordbox/export.pdb")).unwrap();
    let pdb = rbl_pdb::Pdb::parse(&bytes).unwrap();
    let artwork = pdb.table(rbl_pdb::PageType::Artwork).expect("an artwork table");
    let rows = pdb.named_rows(artwork);
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].id, rows[0].name.as_str()), (1, "/PIONEER/Artwork/00001/a1.jpg"));
    let mut by_title: Vec<(String, u32)> = pdb
        .track_rows(pdb.table(rbl_pdb::PageType::Tracks).unwrap())
        .into_iter()
        .map(|t| (t.title, t.artwork_id))
        .collect();
    by_title.sort();
    assert_eq!(by_title, vec![("All U Need".to_string(), 1), ("No Cover".to_string(), 0), ("The Abyss".to_string(), 1)]);

    // And exportLibrary.db carries the image, every tag, and the memberships
    // of the tags that exist.
    let lib = rbl_onelibrary::ExportLibrary::open_read_only(&dest.path().join("PIONEER/rekordbox/exportLibrary.db")).unwrap();
    assert_eq!(lib.count("image").unwrap(), 1);
    assert_eq!(lib.count("myTag").unwrap(), 3);
    assert_eq!(lib.count("myTag_content").unwrap(), 3);
    let with_image: i64 = lib
        .connection()
        .query_row("SELECT COUNT(*) FROM content WHERE image_id = 1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(with_image, 2);
    let path: String = lib.connection().query_row("SELECT path FROM image WHERE image_id = 1", [], |r| r.get(0)).unwrap();
    assert_eq!(path, "/PIONEER/Artwork/00001/a1.jpg");

    // A second export finds the artwork in place and writes none again.
    let again = export_full(dest.path(), &tracks, &playlists, &my_tags, None, None, &mut |_| {}).unwrap();
    assert_eq!(again.artwork_files, 0);
}

#[test]
fn artwork_without_the_library_sizes_is_copied_as_it_is_and_folders_hold_twenty() {
    use rbl_export::export_full;

    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    // Twenty-one images, each its own file with no `_s`/`_m` beside it.
    let mut tracks = Vec::new();
    for i in 1..=21u32 {
        let image = src.path().join(format!("cover{i}.jpg"));
        std::fs::write(&image, format!("\u{ff}\u{d8}cover {i}\u{ff}\u{d9}")).unwrap();
        let mut t = track(src.path(), i, &format!("Track {i}"), "Someone");
        t.artwork = Some(image);
        tracks.push(t);
    }
    let playlists = vec![SourcePlaylist { id: 0, name: "Set".into(), track_indices: (0..21).collect(), ..Default::default() }];
    let report = export_full(dest.path(), &tracks, &playlists, &[], None, None, &mut |_| {}).unwrap();
    assert_eq!(report.artwork_files, 84);
    // 1–19 in the first folder, 20 and 21 in the second, as rekordbox lays
    // them out.
    assert!(dest.path().join("PIONEER/Artwork/00001/a19_m.jpg").is_file());
    assert!(dest.path().join("PIONEER/Artwork/00002/a20.jpg").is_file());
    assert!(dest.path().join("PIONEER/Artwork/00002/b21_m.jpg").is_file());
    assert_eq!(
        std::fs::read(dest.path().join("PIONEER/Artwork/00002/a21.jpg")).unwrap(),
        std::fs::read(src.path().join("cover21.jpg")).unwrap()
    );
}

#[test]
fn failed_export_staging_preserves_previous_databases_and_audio() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let mut tracks = vec![track(src.path(), 1, "First", "Artist"), track(src.path(), 2, "Second", "Artist")];
    let playlists = vec![SourcePlaylist { device_id: 0, device_only: false, parent_id: 0, folder: false, id: 1, name: "Set".into(), track_indices: vec![0, 1] }];
    export(dest.path(), &tracks, &playlists).unwrap();
    let manifest = rbl_export::Manifest::load(dest.path()).unwrap();
    let audio = dest.path().join(manifest.tracks[0].audio.trim_start_matches('/'));
    let old_audio = std::fs::read(&audio).unwrap();
    let database = dest.path().join("PIONEER/rekordbox/export.pdb");
    let old_db = std::fs::read(&database).unwrap();
    // The first track stages a replacement; reading the second must fail.
    std::fs::write(&tracks[0].source_path, vec![9; 4096]).unwrap();
    tracks[1].source_path = src.path().to_path_buf();
    assert!(export(dest.path(), &tracks, &playlists).is_err());
    assert_eq!(std::fs::read(&audio).unwrap(), old_audio);
    assert_eq!(std::fs::read(&database).unwrap(), old_db);
    assert!(!dest.path().join(".rbxport-publication").exists());
    assert!(verify(dest.path()).unwrap().is_ok());
}

#[test]
fn compatibility_conversion_reuses_outputs_updates_paths_and_can_be_disabled() {
    use rbl_export::{CompatibilityFormat, ExportOptions, export_with_options};
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let source = src.path().join("source.flac");
    let original = include_bytes!("../../rbl-audio/tests/fixtures/stereo-96k.flac");
    std::fs::write(&source, original).unwrap();
    let beats = vec![rbl_anlz::Beat { beat_number: 1, tempo_x100: 12800, time_ms: 75 }];
    let mut builder = rbl_anlz::AnlzBuilder::new();
    builder.path("/original.flac").beat_grid(&beats);
    let tracks = vec![SourceTrack {
        id: 71, source_path: source.clone(), title: "Conversion".into(), artist: "Test".into(),
        sample_rate: 96000, file_size: original.len() as u64,
        analysis: vec![("DAT".into(), builder.finish())], ..Default::default()
    }];
    let playlists = vec![SourcePlaylist { id: 1, name: "Set".into(), track_indices: vec![0], ..Default::default() }];
    let mut previous_audio = None;
    let mut export_id = None;
    for (compatibility, ext, bitrate) in [
        (Some(CompatibilityFormat::Wav), "wav", 1411),
        (Some(CompatibilityFormat::Aiff), "aiff", 1411),
        (Some(CompatibilityFormat::Mp3), "mp3", 320),
        (None, "flac", 0),
    ] {
        let report = export_with_options(dest.path(), &tracks, &playlists, &[],
            &ExportOptions { compatibility, ..Default::default() }, &mut |_| {}).unwrap();
        assert_eq!(report.tracks, 1);
        assert_eq!(report.reused, 0);
        let manifest = rbl_export::Manifest::load(dest.path()).unwrap();
        let entry = &manifest.tracks[0];
        assert_eq!(*export_id.get_or_insert(entry.export_id), entry.export_id);
        let audio = dest.path().join(entry.audio.trim_start_matches('/'));
        assert_eq!(audio.extension().unwrap(), ext);
        assert!(audio.is_file());
        if let Some(old) = previous_audio.replace(audio.clone()) { assert!(!old.exists()); }
        let anlz = rbl_anlz::Anlz::read(&dest.path().join(entry.anlz_dir.trim_start_matches('/')).join("ANLZ0000.DAT")).unwrap();
        assert_eq!(anlz.beat_grid().unwrap(), beats);
        if compatibility.is_some() { assert_eq!(anlz.path().as_deref(), Some(entry.audio.as_str())); }
        let pdb_bytes = std::fs::read(dest.path().join("PIONEER/rekordbox/export.pdb")).unwrap();
        let pdb = rbl_pdb::Pdb::parse(&pdb_bytes).unwrap();
        let rows = pdb.track_rows(pdb.table(rbl_pdb::PageType::Tracks).unwrap());
        let raw = pdb.rows(pdb.table(rbl_pdb::PageType::Tracks).unwrap())[0];
        assert_eq!(pdb.u2_at(raw, 0x5a), match ext { "wav" => 11, "aiff" => 12, "mp3" => 1, "flac" => 5, _ => unreachable!() },
            "the CDJ format must describe the exported audio, including conversion");
        assert_eq!(rows[0].file_path, entry.audio);
        assert_eq!(rows[0].sample_rate, if compatibility.is_some() { 44100 } else { 96000 });
        assert_eq!(rows[0].bitrate, bitrate);
        assert_eq!(u64::from(rows[0].file_size), std::fs::metadata(&audio).unwrap().len());
        assert!(verify(dest.path()).unwrap().is_ok());
        let again = export_with_options(dest.path(), &tracks, &playlists, &[],
            &ExportOptions { compatibility, ..Default::default() }, &mut |_| {}).unwrap();
        assert_eq!((again.reused, again.bytes_copied, again.analysis_files), (1, 0, 0));
        if compatibility.is_some() {
            let valid = std::fs::read(&audio).unwrap();
            let mut changed = valid.clone();
            *changed.last_mut().unwrap() ^= 1;
            std::fs::write(&audio, changed).unwrap();
            let repaired = export_with_options(dest.path(), &tracks, &playlists, &[],
                &ExportOptions { compatibility, ..Default::default() }, &mut |_| {}).unwrap();
            assert_eq!(repaired.reused, 0, "edited USB audio must not be reused");
            assert_eq!(std::fs::read(&audio).unwrap(), valid);
        }
        assert_eq!(std::fs::read(&source).unwrap(), original);
    }
    // Failed conversion cannot publish a partial replacement of an existing export.
    let db = dest.path().join("PIONEER/rekordbox/export.pdb");
    let before = std::fs::read(&db).unwrap();
    std::fs::write(&source, b"broken flac").unwrap();
    assert!(export_with_options(dest.path(), &tracks, &playlists, &[],
        &ExportOptions { compatibility: Some(CompatibilityFormat::Wav), ..Default::default() }, &mut |_| {}).is_err());
    assert_eq!(std::fs::read(db).unwrap(), before);
    assert!(previous_audio.unwrap().is_file());
}

#[test]
fn compatibility_does_not_reencode_already_compatible_audio() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let source = src.path().join("compatible.wav");
    rbl_audio::compatibility::convert(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../rbl-audio/tests/fixtures/stereo-96k.flac"),
        &source, rbl_audio::compatibility::Format::Wav,
    ).unwrap();
    let original = std::fs::read(&source).unwrap();
    let tracks = vec![SourceTrack { source_path: source, title: "Compatible".into(), ..Default::default() }];
    rbl_export::export_with_options(dest.path(), &tracks, &[], &[],
        &rbl_export::ExportOptions { compatibility: Some(rbl_export::CompatibilityFormat::Mp3), ..Default::default() },
        &mut |_| {},
    ).unwrap();
    let manifest = rbl_export::Manifest::load(dest.path()).unwrap();
    assert!(manifest.tracks[0].conversion.is_empty());
    assert!(manifest.tracks[0].audio.ends_with(".wav"));
    assert_eq!(std::fs::read(dest.path().join(manifest.tracks[0].audio.trim_start_matches('/'))).unwrap(), original);
}

#[test]
fn exports_cues_metadata_and_all_companions_and_repairs_a_missing_ext() {
    use rbl_anlz::cues::ExportCue;
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let mut t = track(src.path(), 1, "Complete", "Artist");
    t.id = 11;
    t.metadata = rbl_core::ExportMetadata {
        track_number: 7, disc_number: 2, bit_depth: 24, play_count: 42,
        analysed: 105, hot_cue_auto_load: true, ..Default::default()
    };
    t.cues = Some(vec![
        ExportCue { kind: 1, time_ms: 1000, color_code: 46, comment: "Drop".into(), ..Default::default() },
        ExportCue { kind: 5, time_ms: 2000, ..Default::default() },
        ExportCue { time_ms: 3000, loop_time_ms: Some(5000), ..Default::default() },
    ]);
    let dat = rbl_anlz::AnlzBuilder::new().path("?/library.mp3")
        .beat_grid(&[rbl_anlz::Beat {beat_number:1,tempo_x100:12800,time_ms:0}])
        .raw(rbl_core::FourCc::new(b"PWAV"), vec![0;8], vec![1,2,3]).finish();
    let ext = rbl_anlz::AnlzBuilder::new().path("?/library.mp3")
        .raw(rbl_core::FourCc::new(b"PWV3"), vec![0;12], vec![4,5,6]).finish();
    t.analysis = vec![("DAT".into(),dat),("EXT".into(),ext),("2EX".into(),rbl_anlz::AnlzBuilder::new().path("?/library.mp3").finish())];
    export(dest.path(), &[t.clone()], &[]).unwrap();
    let check = verify(dest.path()).unwrap();
    assert!(check.is_ok(), "{:?}", check.errors);
    assert_eq!((check.overview_waveforms,check.detail_waveforms,check.beat_grids,check.hot_cues,check.memory_cues),(1,1,1,2,1));
    let manifest = rbl_export::Manifest::load(dest.path()).unwrap();
    let saved = &manifest.tracks[0];
    let ext = dest.path().join(saved.anlz_dir.trim_start_matches('/')).join("ANLZ0000.EXT");
    let parsed = rbl_anlz::Anlz::read(&ext).unwrap();
    assert_eq!(parsed.path().as_deref(),Some(saved.audio.as_str()));
    assert_eq!(parsed.waveform(b"PWV3").unwrap().1, &[4,5,6]);
    let db = rbl_onelibrary::ExportLibrary::open_read_only(&dest.path().join("PIONEER/rekordbox/exportLibrary.db")).unwrap();
    let metadata: (i64,i64,i64,i64,i64,i64) = db.connection().query_row("SELECT trackNo,discNo,bitDepth,djPlayCount,analysedBits,isHotCueAutoLoadOn FROM content",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).unwrap();
    assert_eq!(metadata,(7,2,24,42,41,1));
    let bytes = std::fs::read(dest.path().join("PIONEER/rekordbox/export.pdb")).unwrap();
    let pdb = rbl_pdb::Pdb::parse(&bytes).unwrap();
    let row = pdb.rows(pdb.table(rbl_pdb::PageType::Tracks).unwrap())[0];
    assert_eq!(pdb.string_ref(row, 0x5e + 7 * 2), "ON");
    let count:i64 = db.connection().query_row("SELECT count(*) FROM cue",[],|r|r.get(0)).unwrap();
    assert_eq!(count,3);
    drop(db);
    let mut corrupted = bytes;
    let empty_string = corrupted[row.offset + 0x5e..row.offset + 0x60].to_vec();
    corrupted[row.offset + 0x5e + 14..row.offset + 0x60 + 14].copy_from_slice(&empty_string);
    std::fs::write(dest.path().join("PIONEER/rekordbox/export.pdb"), corrupted).unwrap();
    assert!(verify(dest.path()).unwrap().errors.iter().any(|e| e.contains("hot-cue auto-load disagrees")));
    export(dest.path(), &[t.clone()], &[]).unwrap();
    assert!(verify(dest.path()).unwrap().is_ok());
    std::fs::remove_file(&ext).unwrap();
    assert!(verify(dest.path()).unwrap().errors.iter().any(|e|e.contains("Missing exported analysis companion")));
    export(dest.path(), &[t.clone()], &[]).unwrap();
    assert!(verify(dest.path()).unwrap().is_ok());
    t.cues = Some(Vec::new());
    t.metadata.hot_cue_auto_load = false;
    export(dest.path(), &[t], &[]).unwrap();
    let check = verify(dest.path()).unwrap();
    assert!(check.is_ok(), "{:?}", check.errors);
    assert_eq!((check.hot_cues,check.memory_cues),(0,0));
}

#[test]
fn a_firmware_path_hash_collision_keeps_both_analysis_bundles() {
    let src = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    let mut tracks = Vec::new();
    for number in [890,7142] {
        let mut t = track(src.path(), number, "Collision", "Artist");
        let path = src.path().join(format!("collision-{number}.mp3"));
        std::fs::rename(&t.source_path, &path).unwrap();
        t.source_path = path;
        t.id = u64::from(number);
        tracks.push(t);
    }
    export(dest.path(), &tracks, &[]).unwrap();
    let before = rbl_export::Manifest::load(dest.path()).unwrap();
    assert_ne!(before.tracks[0].anlz_dir,before.tracks[1].anlz_dir);
    assert!(before.tracks[0].anlz_dir.ends_with("0001C095"));
    let report = export(dest.path(), &tracks, &[]).unwrap();
    assert_eq!(report.analysis_files,0);
    let after = rbl_export::Manifest::load(dest.path()).unwrap();
    assert_eq!(before.tracks[1].audio,after.tracks[1].audio);
    assert!(verify(dest.path()).unwrap().is_ok());
}
