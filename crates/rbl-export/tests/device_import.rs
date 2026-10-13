//! Copying a stick's tracks to the computer for Import Playlist. Every test
//! works in its own tempdir.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use rbl_export::device_import::{candidate, destination, numbered, Copies};
use std::path::Path;

#[test]
fn copies_go_under_pioneerdj_imported_from_device_in_the_music_folder() {
    // `getSpecialLocation(userMusicDirectory)` + "PioneerDJ/Imported from Device"
    // [static, rekordbox 7.2.19 @0x1001d6ee4..0x1001d6f04].
    assert_eq!(destination(Path::new("/Users/dj/Music")), Path::new("/Users/dj/Music/PioneerDJ/Imported from Device"));
}

#[test]
fn a_copy_is_laid_out_as_an_export_lays_it_out() {
    // `get_device_file_path_candidate` @0x1009ba39c: Contents/<Artist>/<Album>/<file>.
    let root = Path::new("/m");
    assert_eq!(candidate(root, "TRIODE", "Live", "a.mp3"), Path::new("/m/Contents/TRIODE/Live/a.mp3"));
    assert_eq!(candidate(root, "", " ", "a.mp3"), Path::new("/m/Contents/UnknownArtist/UnknownAlbum/a.mp3"));
    assert_eq!(candidate(root, "AC/DC", "Why?", "x:y.wav"), Path::new("/m/Contents/AC_DC/Why_/x_y.wav"));
}

#[test]
fn a_clash_is_numbered_before_the_extension_within_48_characters() {
    // `calc_device_file_path` @0x1009b6eb0..0x1009b70cc.
    assert_eq!(numbered("Track.mp3", 1), "Track-1.mp3");
    assert_eq!(numbered("Track.mp3", 12), "Track-12.mp3");
    assert_eq!(numbered("NoExtension", 2), "NoExtension-2");
    let long = format!("{}.aiff", "x".repeat(44)); // 49 characters
    let made = numbered(&long, 1);
    assert_eq!(made.chars().count(), 48);
    assert_eq!(made, format!("{}-1.aiff", "x".repeat(41)));
    // 46 + "-1" is 48: no cut.
    let fits = format!("{}.mp3", "y".repeat(42));
    assert_eq!(numbered(&fits, 1), format!("{}-1.mp3", "y".repeat(42)));
}

#[test]
fn a_copy_never_replaces_a_file_and_matches_the_source() {
    let stick = common::stick();
    let music = tempfile::tempdir().unwrap();
    let source = stick.path().join("a.mp3");
    std::fs::write(&source, b"device audio").unwrap();
    let wanted = candidate(music.path(), "TRIODE", "Live", "a.mp3");
    std::fs::create_dir_all(wanted.parent().unwrap()).unwrap();
    std::fs::write(&wanted, b"the user's own file").unwrap();

    let mut copies = Copies::new();
    let made = copies.copy(&source, &wanted).unwrap();
    assert_eq!(made, wanted.with_file_name("a-1.mp3"));
    assert_eq!(std::fs::read(&wanted).unwrap(), b"the user's own file", "untouched");
    assert_eq!(std::fs::read(&made).unwrap(), b"device audio");
    let again = copies.copy(&source, &wanted).unwrap();
    assert_eq!(again, wanted.with_file_name("a-2.mp3"));
    // No temporary file is left behind.
    let leftovers: Vec<_> = std::fs::read_dir(wanted.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".part"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn rolling_back_removes_only_what_the_import_made() {
    let stick = common::stick();
    let music = tempfile::tempdir().unwrap();
    let source = stick.path().join("b.wav");
    std::fs::write(&source, b"wav").unwrap();
    let root = destination(music.path());
    // The user's own file in a folder the import also uses.
    let theirs = candidate(&root, "Shared", "Album", "theirs.wav");
    std::fs::create_dir_all(theirs.parent().unwrap()).unwrap();
    std::fs::write(&theirs, b"keep").unwrap();

    let mut copies = Copies::new();
    let shared = copies.copy(&source, &candidate(&root, "Shared", "Album", "b.wav")).unwrap();
    let fresh = copies.copy(&source, &candidate(&root, "New", "Album", "b.wav")).unwrap();
    assert_eq!(copies.files(), [shared.clone(), fresh.clone()]);
    copies.roll_back().unwrap();

    assert!(!shared.exists() && !fresh.exists());
    assert_eq!(std::fs::read(&theirs).unwrap(), b"keep");
    assert!(theirs.parent().unwrap().is_dir(), "a folder that was there stays");
    assert!(!root.join("Contents/New").exists(), "a folder the import made goes");
}

#[test]
fn a_missing_source_copies_nothing() {
    let music = tempfile::tempdir().unwrap();
    let mut copies = Copies::new();
    let wanted = candidate(music.path(), "A", "B", "gone.mp3");
    assert!(copies.copy(Path::new("/nonexistent/issue-198/gone.mp3"), &wanted).is_err());
    assert!(copies.files().is_empty());
    assert!(!wanted.exists());
}

#[test]
fn an_earlier_copy_of_the_same_file_is_found_and_another_file_is_not() {
    let stick = common::stick();
    let music = tempfile::tempdir().unwrap();
    let source = stick.path().join("a.mp3");
    std::fs::write(&source, b"device audio").unwrap();
    let wanted = candidate(music.path(), "A", "B", "a.mp3");
    assert_eq!(Copies::existing_copy(&source, &wanted).unwrap(), None, "nothing there yet");
    std::fs::create_dir_all(wanted.parent().unwrap()).unwrap();
    std::fs::write(&wanted, b"the user's own a.mp3").unwrap();
    assert_eq!(Copies::existing_copy(&source, &wanted).unwrap(), None, "a different file of that name");
    let made = Copies::new().copy(&source, &wanted).unwrap();
    assert_eq!(Copies::existing_copy(&source, &wanted).unwrap(), Some(made));
}

#[test]
fn a_copy_keeps_the_device_files_modification_time() {
    let stick = common::stick();
    let music = tempfile::tempdir().unwrap();
    let source = stick.path().join("a.mp3");
    std::fs::write(&source, b"device audio").unwrap();
    let then = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
    std::fs::File::options().write(true).open(&source).unwrap().set_modified(then).unwrap();
    let made = Copies::new().copy(&source, &candidate(music.path(), "A", "B", "a.mp3")).unwrap();
    assert_eq!(std::fs::metadata(made).unwrap().modified().unwrap(), then);
}
