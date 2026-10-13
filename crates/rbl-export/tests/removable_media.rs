//! Exports to FAT32, exFAT and HFS+ volumes as macOS mounts them, from disk
//! images: the filesystems DJ sticks use, whose drivers behave unlike the
//! APFS temporary directory the other tests write to (#122, #161, #285).
//!
//! [OBS 2026-10-08, macOS 26.6.2, `hdiutil` images]: both drivers list names
//! in NFD whatever form they were written in; FAT32 renames, and exFAT
//! deletes, a file only by the form it was stored in; exFAT has no exclusive
//! rename (`renameatx_np(RENAME_EXCL)` answers ENOTSUP, os error 45).
//!
//! Skipped where `hdiutil` cannot create or attach an image.
#![cfg(target_os = "macos")]
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

use rbl_export::{
    export_full, export_with_options, verify, ExportOptions, ExportRoot, SourcePlaylist, SourceTrack, SyncNode, SyncSource,
};

/// A disk image formatted and mounted for one test, detached on drop.
struct Volume {
    mount: PathBuf,
    _dir: tempfile::TempDir,
}

impl Volume {
    fn new(file_system: &str) -> Option<Self> {
        let dir = tempfile::tempdir().ok()?;
        let image = dir.path().join("stick.dmg");
        let mount = dir.path().join("mnt");
        let created = Command::new("hdiutil")
            .args(["create", "-quiet", "-size", "64m", "-fs", file_system, "-volname", "RBXTEST", "-o"])
            .arg(&image)
            .status()
            .ok()?;
        if !created.success() {
            return None;
        }
        std::fs::create_dir_all(&mount).ok()?;
        let attached = Command::new("hdiutil")
            .args(["attach", "-quiet", "-nobrowse", "-mountpoint"])
            .arg(&mount)
            .arg(&image)
            .status()
            .ok()?;
        attached.success().then_some(Self { mount, _dir: dir })
    }
}

impl Drop for Volume {
    fn drop(&mut self) {
        let _ = Command::new("hdiutil").args(["detach", "-quiet", "-force"]).arg(&self.mount).status();
    }
}

fn volumes() -> Vec<(&'static str, Volume)> {
    let mut out = Vec::new();
    for file_system in ["MS-DOS FAT32", "ExFAT"] {
        match Volume::new(file_system) {
            Some(volume) => out.push((file_system, volume)),
            None => eprintln!("skipping {file_system}: hdiutil could not create or attach an image here"),
        }
    }
    out
}

/// Tracks whose names arrive decomposed, as macOS hands them over.
fn tracks(src: &Path) -> Vec<SourceTrack> {
    let files = ["Kesa\u{308} (On Kaunis).mp3", "E\u{301}bano \u{2014} Tie\u{308}sto.mp3", "plain.mp3"];
    let artists = ["Bjo\u{308}rk", "TRIODE", "ARTBAT"];
    (0..files.len())
        .map(|i| {
            let path = src.join(files[i]);
            std::fs::write(&path, vec![i as u8 + 1; 4096]).unwrap();
            SourceTrack {
                id: i as u64 + 1,
                source_path: path,
                title: format!("Track {}", i + 1),
                artist: artists[i].into(),
                album: "Album".into(),
                analysis: vec![
                    ("DAT".into(), rbl_anlz::AnlzBuilder::new().path("/x.mp3").finish()),
                    ("EXT".into(), rbl_anlz::AnlzBuilder::new().path("/x.mp3").finish()),
                ],
                ..Default::default()
            }
        })
        .collect()
}

fn sync(root: &Path, tracks: &[SourceTrack]) -> rbl_export::Result<rbl_export::ExportReport> {
    let playlists = vec![SourcePlaylist { id: 10, name: "Set".into(), track_indices: (0..tracks.len()).collect(), ..Default::default() }];
    let source = SyncSource { db_id: 123, tree: vec![SyncNode { id: 10, parent: 0, attribute: 0 }], automatic: false };
    export_full(root, tracks, &playlists, &[], None, Some(&source), &mut |_| {})
}

/// Whether `path` was stored under exactly this form of its name: a FAT32
/// stick renames a file only by its stored form.
fn stored_as(path: &Path) -> bool {
    let moved = path.with_file_name("probe.tmp");
    std::fs::rename(path, &moved).is_ok() && std::fs::rename(&moved, path).is_ok()
}

fn leftovers(root: &Path) -> Vec<String> {
    std::fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".rbxport-staging-") || n.starts_with(".rbxport-retired-") || n.starts_with(".tmp") || n == ".rbxport-publication")
        .collect()
}

#[test]
fn a_fat32_or_exfat_stick_takes_an_export_and_its_resync() {
    for (file_system, volume) in volumes() {
        let root = volume.mount.join("export");
        std::fs::create_dir_all(&root).unwrap();
        let src = tempfile::tempdir().unwrap();
        let tracks = tracks(src.path());

        // exFAT failed here with "could not write exportLibrary.db:
        // Operation not supported (os error 45)", FAT32 with "No such file
        // or directory (os error 2)" at 99%.
        let first = sync(&root, &tracks).unwrap_or_else(|e| panic!("{file_system}: {e}"));
        assert_eq!(first.tracks, 3, "{file_system}");
        let again = sync(&root, &tracks).unwrap_or_else(|e| panic!("{file_system}: {e}"));
        assert_eq!(again.reused, 3, "{file_system}");
        let fewer = sync(&root, &tracks[1..]).unwrap_or_else(|e| panic!("{file_system}: {e}"));
        assert_eq!(fewer.removed, 1, "{file_system}");

        let check = verify(&root).unwrap();
        assert!(check.is_ok(), "{file_system}: {:?}", check.errors);
        assert_eq!(check.tracks, 2, "{file_system}");
        // Published under the NFC name both databases carry.
        let audio = root.join("Contents/TRIODE/Album/\u{c9}bano \u{2014} Ti\u{eb}sto.mp3");
        assert!(stored_as(&audio), "{file_system}: not stored in NFC");
        // Nothing left behind: a stage exFAT could not delete would hold a
        // second copy of the audio.
        assert!(leftovers(&root).is_empty(), "{file_system}: {:?}", leftovers(&root));
    }
}

/// What a stick looked like after the earlier version failed to publish on
/// FAT32 (#161): the journal's paths are the stage's NFD listing while its
/// images were written in NFC. Every later sync and USB import failed to
/// recover it with "No such file or directory (os error 2)".
#[test]
fn an_interrupted_export_left_by_the_earlier_version_is_recovered() {
    for (file_system, volume) in volumes() {
        let root = volume.mount.join("export");
        let journal = root.join(".rbxport-publication");
        let image = journal.join("Contents/Bj\u{f6}rk/Album/Kes\u{e4}.mp3");
        std::fs::create_dir_all(image.parent().unwrap()).unwrap();
        std::fs::write(&image, b"audio").unwrap();
        std::fs::create_dir_all(journal.join("PIONEER/rekordbox")).unwrap();
        std::fs::write(journal.join("PIONEER/rekordbox/export.pdb"), b"pdb").unwrap();
        let entries = serde_json::json!([
            {"path": "Contents/Bjo\u{308}rk/Album/Kesa\u{308}.mp3", "present": true, "had_target": false},
            {"path": "PIONEER/rekordbox/export.pdb", "present": true, "had_target": false},
        ]);
        std::fs::write(journal.join("publication.json"), serde_json::to_vec(&entries).unwrap()).unwrap();

        rbl_export::recover(&root).unwrap_or_else(|e| panic!("{file_system}: {e}"));

        assert!(!journal.exists(), "{file_system}");
        let published = root.join("Contents/Bj\u{f6}rk/Album/Kes\u{e4}.mp3");
        assert_eq!(std::fs::read(&published).unwrap(), b"audio", "{file_system}");
        assert!(stored_as(&published), "{file_system}: not stored in NFC");
        assert!(leftovers(&root).is_empty(), "{file_system}: {:?}", leftovers(&root));
    }
}

/// #283: a stick synced once takes a second sync that adds a playlist of
/// tracks it does not hold yet, at the volume's root as the Sync Manager
/// writes it.
#[test]
fn a_fat32_or_exfat_stick_takes_another_playlist_on_a_later_sync() {
    for (file_system, volume) in volumes() {
        let root = &volume.mount;
        let src = tempfile::tempdir().unwrap();
        let tracks = tracks(src.path());
        let first = SourcePlaylist { id: 10, name: "Set".into(), track_indices: vec![0, 1], ..Default::default() };
        let added = SourcePlaylist { id: 11, name: "Warm-up".into(), track_indices: vec![2], ..Default::default() };
        let node = |id| SyncNode { id, parent: 0, attribute: 0 };
        let once = SyncSource { db_id: 123, tree: vec![node(10)], automatic: false };
        export_full(root, &tracks[..2], std::slice::from_ref(&first), &[], None, Some(&once), &mut |_| {})
            .unwrap_or_else(|e| panic!("{file_system}: {e}"));
        let twice = SyncSource { db_id: 123, tree: vec![node(10), node(11)], automatic: false };
        let report = export_full(root, &tracks, &[first, added], &[], None, Some(&twice), &mut |_| {})
            .unwrap_or_else(|e| panic!("{file_system}: {e}"));
        assert_eq!((report.tracks, report.reused), (3, 2), "{file_system}");

        let check = verify(root).unwrap();
        assert!(check.is_ok(), "{file_system}: {:?}", check.errors);
        assert_eq!(check.tracks, 3, "{file_system}");
        assert!(leftovers(root).is_empty(), "{file_system}: {:?}", leftovers(root));
    }
}

/// A Journaled HFS+ image at its root: what rbxport's Format HFS+ and Disk
/// Utility make, mounted read-write as macOS mounts it.
fn hfs_plus() -> Option<Volume> {
    let volume = Volume::new("Journaled HFS+");
    if volume.is_none() {
        eprintln!("skipping Journaled HFS+: hdiutil could not create or attach an image here");
    }
    volume
}

/// The Sync Manager's SYNC to an HFS+ stick: the hidden `.PIONEER` root
/// rekordbox uses there, then a resync that adds a playlist and one that
/// drops a track (#285).
///
/// [OBS 2026-10-13, macOS 26, `hdiutil` image]: HFS+ stores names in NFD
/// but, unlike FAT32 and exFAT, renames and deletes them by either form,
/// and accepts `F_FULLFSYNC`; plain and case-sensitive HFS+ behave the same.
#[test]
fn an_hfs_plus_stick_takes_a_sync_and_its_resyncs_under_the_hidden_root() {
    let Some(volume) = hfs_plus() else { return };
    let root = &volume.mount;
    let src = tempfile::tempdir().unwrap();
    let tracks = tracks(src.path());
    let set = SourcePlaylist { id: 10, name: "Set".into(), track_indices: vec![0, 1], ..Default::default() };
    let added = SourcePlaylist { id: 11, name: "Warm-up".into(), track_indices: vec![2], ..Default::default() };
    let node = |id| SyncNode { id, parent: 0, attribute: 0 };
    let sync = |tracks: &[SourceTrack], playlists: &[SourcePlaylist], tree: Vec<SyncNode>| {
        let source = SyncSource { db_id: 123, tree, automatic: false };
        let options = ExportOptions { sync: Some(&source), root: Some(ExportRoot::Hidden), ..Default::default() };
        export_with_options(root, tracks, playlists, &[], &options, &mut |_| {}).unwrap()
    };

    let first = sync(&tracks[..2], std::slice::from_ref(&set), vec![node(10)]);
    assert_eq!((first.tracks, first.failed.len()), (2, 0));
    let second = sync(&tracks, &[set.clone(), added], vec![node(10), node(11)]);
    assert_eq!((second.tracks, second.reused, second.failed.len()), (3, 2, 0));
    let set = SourcePlaylist { track_indices: vec![0], ..set };
    let third = sync(&tracks[..1], &[set], vec![node(10)]);
    assert_eq!((third.removed, third.failed.len()), (2, 0));

    assert!(root.join(".PIONEER/rekordbox/exportLibrary.db").is_file());
    assert!(!root.join("PIONEER").exists());
    let check = verify(root).unwrap();
    assert!(check.is_ok(), "{:?}", check.errors);
    assert_eq!(check.tracks, 1);
    assert!(root.join("Contents/Bj\u{f6}rk/Album/Kes\u{e4} (On Kaunis).mp3").is_file());
    assert!(leftovers(root).is_empty(), "{:?}", leftovers(root));
}

/// A stick mounted read-only, as Linux mounts a Journaled HFS+ volume
/// without `force` (AlphaTheta's players pass `force`; see alphatheta-docs
/// `devices/xdj-rx3/operating-system/storage-mounting.md`), is refused
/// before anything is written, with the system's reason (#285).
#[test]
fn a_read_only_hfs_plus_stick_is_refused_up_front_with_the_reason() {
    let Some(volume) = hfs_plus() else { return };
    let src = tempfile::tempdir().unwrap();
    let tracks = tracks(src.path());
    sync(&volume.mount, &tracks).unwrap();
    let before = verify(&volume.mount).unwrap();

    let image = volume._dir.path().join("stick.dmg");
    assert!(Command::new("hdiutil").args(["detach", "-quiet", "-force"]).arg(&volume.mount).status().unwrap().success());
    let attached = Command::new("hdiutil")
        .args(["attach", "-quiet", "-readonly", "-nobrowse", "-mountpoint"])
        .arg(&volume.mount)
        .arg(&image)
        .status()
        .unwrap();
    assert!(attached.success());

    let error = sync(&volume.mount, &tracks[..1]).unwrap_err();
    let rbl_export::ExportError::Io(io) = &error else { panic!("{error:?}") };
    assert_eq!(io.kind(), std::io::ErrorKind::ReadOnlyFilesystem, "{error}");
    assert!(error.to_string().contains("Read-only file system"), "{error}");
    let after = verify(&volume.mount).unwrap();
    assert_eq!((after.tracks, after.is_ok()), (before.tracks, true));
}
