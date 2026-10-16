//! Cue and beat-grid edits made on a player, and what the next export does
//! with them (#439, #522).
//!
//! An export (SYNC) goes from the computer to the USB, so the computer's
//! cues and beat grids win for the tracks it exports. A player-side edit to
//! those tracks is replaced, not a reason to stop the run and demand an
//! import the user may not want. Importing goes the other way (Update
//! Collection in rekordbox), and is a separate, explicit action.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use rbl_anlz::cues::ExportCue;
use rbl_anlz::{Anlz, AnlzBuilder, Beat};
use rbl_export::{export_full, verify, Manifest, SourcePlaylist, SourceTrack, SyncSource};
use std::path::{Path, PathBuf};

fn library_cues() -> Vec<ExportCue> {
    vec![
        ExportCue { kind: 1, time_ms: 1000, color_code: 46, comment: "Drop".into(), ..Default::default() },
        ExportCue { time_ms: 3000, ..Default::default() },
    ]
}

fn library_grid() -> Vec<Beat> {
    vec![Beat { beat_number: 1, tempo_x100: 12800, time_ms: 0 }, Beat { beat_number: 2, tempo_x100: 12800, time_ms: 469 }]
}

fn track(root: &Path, id: u64) -> SourceTrack {
    let path = root.join(format!("issue-522-{id}.mp3"));
    std::fs::write(&path, vec![id as u8; 256]).unwrap();
    let dat = AnlzBuilder::new().path("/original.mp3").beat_grid(&library_grid()).finish();
    let ext = AnlzBuilder::new().path("/original.mp3").finish();
    SourceTrack {
        id,
        source_path: path,
        title: format!("Track {id}"),
        artist: "Artist".into(),
        album: "Album".into(),
        bpm_x100: 12800,
        analysis: vec![("DAT".into(), dat), ("EXT".into(), ext)],
        cues: Some(library_cues()),
        ..Default::default()
    }
}

fn playlist(id: u64, indices: &[usize]) -> SourcePlaylist {
    SourcePlaylist { id, name: format!("Playlist {id}"), track_indices: indices.to_vec(), ..Default::default() }
}

fn sync(root: &Path, tracks: &[SourceTrack], playlists: &[SourcePlaylist]) -> rbl_export::Result<rbl_export::ExportReport> {
    export_full(
        root,
        tracks,
        playlists,
        &[],
        None,
        Some(&SyncSource {
            db_id: 123,
            tree: playlists.iter().map(|p| rbl_export::SyncNode { id: p.id, parent: p.parent_id, attribute: 0 }).collect(),
            automatic: false,
        }),
        &mut |_| {},
    )
}

fn edit(root: &Path, sql: &str) {
    let conn = rusqlite::Connection::open(rbl_export::export_root(root).join("rekordbox/exportLibrary.db")).unwrap();
    conn.pragma_update(None, "cipher", "sqlcipher").unwrap();
    conn.pragma_update(None, "legacy", 4).unwrap();
    conn.pragma_update(None, "key", rbl_onelibrary::key::passphrase().unwrap()).unwrap();
    conn.execute_batch(sql).unwrap();
}

/// The stick's analysis file `extension` of the manifest's track `index`.
fn stick_anlz(root: &Path, index: usize, extension: &str) -> PathBuf {
    let manifest = Manifest::load(root).unwrap();
    root.join(manifest.tracks[index].anlz_dir.trim_start_matches('/')).join(format!("ANLZ0000.{extension}"))
}

/// Rewrites a stick analysis file the way a player storing an edit would:
/// same file, new cue lists or a new beat grid.
fn edit_on_player(path: &Path, cues: Option<&[ExportCue]>, grid: Option<&[Beat]>) {
    let extended = path.extension().is_some_and(|e| e == "EXT");
    let mut anlz = Anlz::read(path).unwrap();
    if let Some(cues) = cues {
        anlz.sections.retain(|s| !s.is_cue_list());
        anlz.sections.extend(rbl_anlz::cues::sections(cues, extended));
    }
    if let Some(grid) = grid {
        anlz.sections.retain(|s| s.as_beat_grid().is_none());
        anlz.sections.push(rbl_anlz::write::beat_grid_section(grid));
    }
    std::fs::write(path, anlz.to_bytes()).unwrap();
}

fn player_cues() -> Vec<ExportCue> {
    vec![ExportCue { kind: 2, time_ms: 7000, comment: "set on the CDJ".into(), ..Default::default() }]
}

/// The cue lists of the stick file at `path` match `cues` as the export
/// writes them (`PCOB` in a DAT; `PCOB` and `PCO2` in an EXT).
fn has_cues(path: &Path, cues: &[ExportCue]) -> bool {
    let extended = path.extension().is_some_and(|e| e == "EXT");
    let lists: Vec<_> = Anlz::read(path).unwrap().sections.into_iter().filter(|s| s.is_cue_list()).collect();
    lists == rbl_anlz::cues::sections(cues, extended)
}

fn cue_times(anlz: &Anlz) -> Vec<u32> {
    let mut times: Vec<u32> = anlz.cue_entries().iter().map(|c| c.time_ms).collect();
    times.sort_unstable();
    times
}

#[test]
fn an_export_replaces_hot_cues_edited_on_a_player_with_the_librarys() {
    let src = tempfile::tempdir().unwrap();
    let usb = common::stick();
    let t = track(src.path(), 1);
    sync(usb.path(), std::slice::from_ref(&t), &[playlist(10, &[0])]).unwrap();
    let dat = stick_anlz(usb.path(), 0, "DAT");
    let ext = stick_anlz(usb.path(), 0, "EXT");
    edit_on_player(&dat, Some(&player_cues()), None);
    edit_on_player(&ext, Some(&player_cues()), None);
    assert_eq!(cue_times(&Anlz::read(&ext).unwrap()), [7000]);

    // Was: "USB sync conflict: USB cues or beat grids changed since the last
    // sync. Import the USB cues/grids before exporting."
    sync(usb.path(), &[t], &[playlist(10, &[0])]).unwrap();

    assert!(has_cues(&dat, &library_cues()), "the library's cues win on export");
    assert!(has_cues(&ext, &library_cues()));
    assert_eq!(cue_times(&Anlz::read(&ext).unwrap()), [1000, 3000]);
    let check = verify(usb.path()).unwrap();
    assert!(check.is_ok(), "{:?}", check.errors);
}

#[test]
fn an_export_replaces_a_beat_grid_edited_on_a_player_with_the_librarys() {
    let src = tempfile::tempdir().unwrap();
    let usb = common::stick();
    let t = track(src.path(), 1);
    sync(usb.path(), std::slice::from_ref(&t), &[playlist(10, &[0])]).unwrap();
    let dat = stick_anlz(usb.path(), 0, "DAT");
    let shifted = [Beat { beat_number: 1, tempo_x100: 12600, time_ms: 25 }];
    edit_on_player(&dat, None, Some(&shifted));
    assert_eq!(Anlz::read(&dat).unwrap().beat_grid().unwrap(), shifted);

    sync(usb.path(), &[t], &[playlist(10, &[0])]).unwrap();

    assert_eq!(Anlz::read(&dat).unwrap().beat_grid().unwrap(), library_grid(), "the library's grid wins on export");
    assert!(verify(usb.path()).unwrap().is_ok());
}

/// The library's cues changed too: the export still writes the library's,
/// whichever side changed first.
#[test]
fn an_export_writes_the_librarys_new_cues_over_a_player_edit() {
    let src = tempfile::tempdir().unwrap();
    let usb = common::stick();
    let mut t = track(src.path(), 1);
    sync(usb.path(), std::slice::from_ref(&t), &[playlist(10, &[0])]).unwrap();
    edit_on_player(&stick_anlz(usb.path(), 0, "EXT"), Some(&player_cues()), None);
    let changed = vec![ExportCue { kind: 3, time_ms: 9000, ..Default::default() }];
    t.cues = Some(changed.clone());

    sync(usb.path(), &[t], &[playlist(10, &[0])]).unwrap();

    assert_eq!(cue_times(&Anlz::read(&stick_anlz(usb.path(), 0, "EXT")).unwrap()), [9000]);
    assert!(has_cues(&stick_anlz(usb.path(), 0, "DAT"), &changed));
}

/// A player that stores its cues in `exportLibrary.db` (the `cue` table)
/// rather than, or as well as, the analysis files.
#[test]
fn an_export_replaces_onelibrary_cue_rows_added_on_a_player_for_an_exported_track() {
    let src = tempfile::tempdir().unwrap();
    let usb = common::stick();
    let t = track(src.path(), 1);
    sync(usb.path(), std::slice::from_ref(&t), &[playlist(10, &[0])]).unwrap();
    edit(usb.path(), "INSERT INTO cue(content_id,kind,inUsec,cueComment) VALUES(1,2,7000000,'set on the CDJ')");

    // Was: "USB sync conflict: OneLibrary contains cue records that this
    // export would replace. Import the cues in rekordbox first."
    sync(usb.path(), &[t], &[playlist(10, &[0])]).unwrap();

    let db = rbl_onelibrary::ExportLibrary::open_read_only(&rbl_export::export_root(usb.path()).join("rekordbox/exportLibrary.db")).unwrap();
    let mut q = db.connection().prepare("SELECT inUsec FROM cue WHERE content_id=1 ORDER BY inUsec").unwrap();
    let times: Vec<i64> = q.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
    assert_eq!(times, [1_000_000, 3_000_000], "the library's cue rows win on export");
}

/// Cue rows a player added to a track that this sync takes off the stick
/// are not being exported over; they would just be lost. That still stops
/// the sync.
#[test]
fn a_sync_still_refuses_to_drop_cue_rows_a_player_added_to_a_track_it_removes() {
    let src = tempfile::tempdir().unwrap();
    let usb = common::stick();
    let tracks = [track(src.path(), 1), track(src.path(), 2)];
    sync(usb.path(), &tracks, &[playlist(10, &[0, 1])]).unwrap();
    let manifest = Manifest::load(usb.path()).unwrap();
    let removed = manifest.tracks.iter().find(|t| t.library_id == 2).unwrap().export_id;
    edit(usb.path(), &format!("INSERT INTO cue(content_id,kind,inUsec,cueComment) VALUES({removed},2,7000000,'set on the CDJ')"));
    let before = rbl_export::snapshot::Snapshot::read(usb.path()).unwrap();

    let result = sync(usb.path(), &tracks[..1], &[playlist(10, &[0])]);

    assert!(matches!(&result, Err(rbl_export::ExportError::Conflict(m)) if m.contains("cue records")), "{result:?}");
    assert_eq!(rbl_export::snapshot::Snapshot::read(usb.path()).unwrap(), before);
}
