#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
// Manual probe: prints the exported fixture values for inspection; file sizes fit in i64.
#![allow(clippy::print_stdout, clippy::cast_possible_wrap)]

use std::{path::PathBuf, sync::Arc};
use rbl_db::{fixture::{self, Shape}, Library as Db, OpenMode};
use rbxport_lib::{commands, state::AppState};
use tauri::Manager;

#[test]
#[ignore = "manual probe; needs ISSUE217_OUT"]
fn export_issue217_controlled_mp3_fixtures() {
    let root = PathBuf::from(std::env::var("ISSUE217_OUT").expect("ISSUE217_OUT"));
    let library_dir = root.join("fixture-library");
    if library_dir.exists() { std::fs::remove_dir_all(&library_dir).unwrap(); }
    std::fs::create_dir_all(&library_dir).unwrap();
    let location = fixture::build(&library_dir, Shape { tracks: 2, playlists: 1, tracks_per_playlist: 2, history_sessions: 0, ..Default::default() }).unwrap();

    let sources = [
        (root.join("source/mp3-32k-16khz.mp3"), 16_000_i64, 32_i64),
        (root.join("source/mp3-64k-24khz.mp3"), 24_000_i64, 64_i64),
    ];
    for (index, (path, _, _)) in sources.iter().enumerate() {
        fixture::point_at_audio(&location, index, path.to_str().unwrap(), 30).unwrap();
    }
    {
        let db = Db::open(location.clone(), OpenMode::ReadWrite).unwrap();
        for (index, (_, sample_rate, bitrate)) in sources.iter().enumerate() {
            db.connection().execute(
                "UPDATE djmdContent SET FileType=1, SampleRate=?1, BitRate=?2, FileSize=?3, BPM=12000, Analysed=0 WHERE ID=?4",
                (sample_rate, bitrate, std::fs::metadata(&sources[index].0).unwrap().len() as i64, fixture::track_id(index)),
            ).unwrap();
        }
    }

    let state = AppState::with_backups(root.join("backups"));
    let db = Db::open(location.clone(), OpenMode::ReadOnly).unwrap();
    let (library, _) = rbl_index::load(&db).unwrap();
    state.set_library(library, false, db.schema().db_version, 0, location);
    drop(db);
    let app = tauri::test::mock_app();
    app.manage(Arc::new(state));
    let ids = vec![fixture::track_id(0), fixture::track_id(1)];
    for (name, conversion) in [("export-off", None), ("export-on", Some(rbl_export::CompatibilityFormat::Wav))] {
        let destination = root.join(name);
        std::fs::create_dir_all(&destination).unwrap();
        let result = tauri::async_runtime::block_on(commands::export_tracks_to_device(
            app.handle().clone(), app.state::<Arc<AppState>>(), ids.clone(), destination.to_string_lossy().into_owned(), None, conversion,
        )).unwrap();
        println!("{name}: tracks={} failed={:?}", result.tracks, result.failed);
        let snapshot = rbl_export::snapshot::Snapshot::read(&destination).unwrap();
        let tracks = snapshot.legacy.unwrap().tracks;
        let bytes = std::fs::read(destination.join("PIONEER/rekordbox/export.pdb")).unwrap();
        let pdb = rbl_pdb::Pdb::parse(&bytes).unwrap();
        let rows = pdb.track_rows(pdb.table(rbl_pdb::PageType::Tracks).unwrap());
        for (track, row) in tracks.iter().zip(rows) {
            let audio = destination.join(track.path.trim_start_matches('/'));
            println!("{name}: title={} file={} rate={} bitrate={} duration={} bytes={} audio={}",
                track.title, track.path, row.sample_rate, row.bitrate, row.duration_sec, std::fs::metadata(&audio).unwrap().len(), audio.display());
        }
    }
}
