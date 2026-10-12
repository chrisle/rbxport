//! The file system's creation time as a browser column (#308): read from the
//! files, sorted in the index, blank where it cannot be read.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use rbl_index::file_time::created_secs;
use rbl_index::testing::{library_from, TestTrack};
use rbl_index::{SortColumn, TrackSource, ViewSpec};

fn leak(path: &Path) -> &'static str {
    Box::leak(path.to_string_lossy().into_owned().into_boxed_str())
}

/// What the platform itself reports for the file, in whole seconds.
fn platform_created(path: &Path) -> Option<i64> {
    let created = std::fs::metadata(path).ok()?.created().ok()?;
    Some(created.duration_since(UNIX_EPOCH).ok()?.as_secs() as i64)
}

fn track(id: u64, path: &'static str) -> TestTrack {
    TestTrack { id, title: "t", path, ..TestTrack::default() }
}

fn spec(descending: bool) -> ViewSpec {
    ViewSpec {
        source: TrackSource::Collection,
        sort: SortColumn::FileCreated,
        descending,
        query: String::new(),
        filter: Default::default(),
    }
}

#[test]
fn a_files_creation_time_is_what_the_platform_reports() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.mp3");
    std::fs::write(&file, b"x").unwrap();
    assert_eq!(created_secs(&file), platform_created(&file));
    // Where the file system records one (macOS, Windows, Linux with statx),
    // it is about now.
    if let Some(secs) = created_secs(&file) {
        let now = std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
        assert!((now - secs).abs() < 300, "created {secs}, now {now}");
    }
}

#[test]
fn a_missing_or_empty_path_has_no_creation_time() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(created_secs(dir.path().join("gone.mp3")), None);
    assert_eq!(created_secs(""), None);
}

#[test]
fn a_track_reads_its_own_files_creation_time() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.mp3");
    std::fs::write(&file, b"x").unwrap();
    let library = library_from(&[track(1, leak(&file)), track(2, "/no/such/file.mp3")]);
    assert_eq!(library.file_created(0), platform_created(&file));
    assert_eq!(library.file_created(1), None);
    assert_eq!(library.file_created(99), None, "a row past the end is blank, not a panic");
}

#[test]
fn sorting_by_creation_puts_unknown_first_then_oldest_to_newest() {
    let dir = tempfile::tempdir().unwrap();
    let older = dir.path().join("older.mp3");
    let newer = dir.path().join("newer.mp3");
    std::fs::write(&older, b"x").unwrap();
    // Whole seconds apart, which is what the column orders by.
    std::thread::sleep(Duration::from_millis(1100));
    std::fs::write(&newer, b"x").unwrap();
    if platform_created(&older).is_none() {
        eprintln!("this file system records no creation time; only the blank case is checked");
    }

    let library = library_from(&[
        track(1, leak(&newer)),
        track(2, "/no/such/file.mp3"),
        track(3, leak(&older)),
    ]);
    let ids = |descending| {
        let view = library.open_view(&spec(descending));
        view.rows.iter().map(|&row| library.ids[row as usize]).collect::<Vec<_>>()
    };
    if platform_created(&older).is_some() {
        assert_eq!(ids(false), vec![2, 3, 1]);
        assert_eq!(ids(true), vec![1, 3, 2]);
    } else {
        // Every value blank: row order, as any tie.
        assert_eq!(ids(false), vec![1, 2, 3]);
    }
    // The cells now read from what the sort read.
    assert_eq!(library.file_created(0), platform_created(&newer));
}

#[test]
fn moving_the_files_forgets_the_creation_times_read_before() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.mp3");
    std::fs::write(&file, b"x").unwrap();
    let mut library = library_from(&[track(1, leak(&file))]);
    let _ = library.open_view(&spec(false));
    library.map_folder_paths(|_| "/no/such/mount/a.mp3".to_owned());
    assert_eq!(library.file_created(0), None);
}

#[test]
fn a_large_library_reads_on_worker_threads_to_the_same_result() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.mp3");
    std::fs::write(&file, b"x").unwrap();
    let present = leak(&file);
    // Every third track has no file, so the parallel read has to keep each
    // row's own answer in its own place.
    let tracks: Vec<_> = (0..5000u64)
        .map(|i| track(i + 1, if i % 3 == 0 { "/no/such/file.mp3" } else { present }))
        .collect();
    let library = library_from(&tracks);
    let view = library.open_view(&spec(false));
    let expected = platform_created(&file);
    for row in 0..tracks.len() {
        let want = if row % 3 == 0 { None } else { expected };
        assert_eq!(library.file_created(row as u32), want, "row {row}");
    }
    if expected.is_some() {
        // The blanks first, each group in row order.
        let blanks = tracks.len().div_ceil(3);
        assert!(view.rows[..blanks].iter().all(|&row| row % 3 == 0));
        assert!(view.rows[..blanks].windows(2).all(|pair| pair[0] < pair[1]));
        assert!(view.rows[blanks..].iter().all(|&row| row % 3 != 0));
    }
}

#[test]
fn a_folder_sorts_its_tracks_and_loose_files_by_creation_together() {
    use rbl_index::folder::FolderEntry;
    let dir = tempfile::tempdir().unwrap();
    let held = dir.path().join("held.mp3");
    let loose = dir.path().join("loose.mp3");
    std::fs::write(&held, b"x").unwrap();
    std::thread::sleep(Duration::from_millis(1100));
    std::fs::write(&loose, b"x").unwrap();
    let library = library_from(&[track(1, leak(&held))]);
    // Listed newest first, with a name the disk no longer has.
    let files = vec![
        ("loose.mp3".to_owned(), loose.clone()),
        ("gone.mp3".to_owned(), dir.path().join("gone.mp3")),
        ("held.mp3".to_owned(), held.clone()),
    ];
    let view = library.open_folder(files, &spec(false));
    if platform_created(&held).is_some() {
        assert_eq!(view.entries, [FolderEntry::File(1), FolderEntry::Track(0), FolderEntry::File(0)]);
    } else {
        assert_eq!(view.entries, [FolderEntry::File(0), FolderEntry::File(1), FolderEntry::Track(0)]);
    }
}
