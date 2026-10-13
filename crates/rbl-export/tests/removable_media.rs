//! Exports to FAT32, exFAT and HFS+ volumes as macOS mounts them: the
//! filesystems DJ sticks use, whose drivers behave unlike the APFS temporary
//! directory the other tests write to (#122, #161).
//!
//! [OBS 2026-10-08, macOS 26.6.2, `hdiutil` images]: the FAT32 and exFAT
//! drivers list names in NFD whatever form they were written in; FAT32
//! renames, and exFAT deletes, a file only by the form it was stored in;
//! exFAT has no exclusive rename (`renameatx_np(RENAME_EXCL)` answers
//! ENOTSUP, os error 45).
//!
//! HFS+ sticks take the hidden `.PIONEER` root, as rekordbox gives a blank
//! HFS+ volume [OBS: see `docs/reference/usb-export-db.md`, Root selection],
//! so each volume exports under the root the app would choose for it.
//!
//! By default every test runs on disk images, one per format, and skips a
//! format where `hdiutil` cannot create or attach its image.
//!
//! With `RBX_REAL_USB=1`, every test runs on the real sticks mounted as
//! `/Volumes/USB A` and `/Volumes/USB B` instead, one test at a time. Each
//! stick must hold a `.rbxport-disposable` file at its root, which says what
//! rbxport and rekordbox keep on it may be deleted; without it the tests
//! refuse to write. Before each test, `PIONEER`, `.PIONEER`, `Contents`, the
//! tests' own folders and rbxport's working files are deleted from the
//! stick. What the last test leaves need not be a library a player can read;
//! run `a_sync_writes_every_stick_at_once` on its own for one at each stick's
//! root. Its tracks are placeholder bytes, so a player can browse them but
//! not play them.
#![cfg(target_os = "macos")]
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard, PoisonError};

use rbl_export::cache::PreparationCache;
use rbl_export::{
    export_with_options, verify, ExportOptions, ExportReport, ExportRoot, SourcePlaylist, SourceTrack, SyncNode,
    SyncSource,
};

/// Set to `1` to test on real sticks instead of disk images.
const REAL_USB_ENV: &str = "RBX_REAL_USB";
/// The volume names of the real sticks, as mounted under `/Volumes`.
const REAL_STICKS: [&str; 2] = ["USB A", "USB B"];
/// The file on a real stick that says it may be overwritten.
const DISPOSABLE_MARKER: &str = ".rbxport-disposable";
/// What the tests write at a stick's root, deleted from a real stick before
/// each test.
const TEST_OUTPUT: [&str; 5] = ["PIONEER", ".PIONEER", "Contents", "export", "retained"];

/// Disk image formats, as `hdiutil create -fs` names them. Both HFS+
/// variants are included: Disk Utility's default "Mac OS Extended
/// (Journaled)" and plain HFS+.
const IMAGE_FORMATS: [&str; 4] = ["MS-DOS FAT32", "ExFAT", "HFS+", "JHFS+"];

/// Held while a test uses the real sticks, which every test shares.
static REAL_STICKS_IN_USE: Mutex<()> = Mutex::new(());

/// A mounted volume one test exports to: a disk image, detached on drop, or
/// a real stick.
struct Volume {
    /// Names the volume in assertion failures.
    label: String,
    mount: PathBuf,
    root: Option<ExportRoot>,
    /// The disk image's directory; `None` for a real stick.
    image: Option<tempfile::TempDir>,
}

impl Volume {
    /// A fresh disk image in `format`, or `None` where `hdiutil` cannot make one.
    fn image(format: &str) -> Option<Self> {
        let dir = tempfile::tempdir().ok()?;
        let image = dir.path().join("stick.dmg");
        let mount = dir.path().join("mnt");
        let created = Command::new("hdiutil")
            .args(["create", "-quiet", "-size", "64m", "-fs", format, "-volname", "RBXTEST", "-o"])
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
        if !attached.success() {
            return None;
        }
        let root = ExportRoot::for_file_system(&mounted_type(&mount));
        Some(Self { label: format.to_owned(), mount, root, image: Some(dir) })
    }

    /// The real stick mounted as `/Volumes/<name>`, cleared of the last
    /// test's output. Panics rather than skip: the switch asked for it.
    fn stick(name: &str) -> Self {
        let mount = Path::new("/Volumes").join(name);
        let mounted_on = rustix::fs::statfs(&mount).map(|stat| c_string(&stat.f_mntonname)).unwrap_or_default();
        assert!(
            Path::new(&mounted_on) == mount,
            "{REAL_USB_ENV}=1, but no stick is mounted at {}",
            mount.display()
        );
        assert!(
            mount.join(DISPOSABLE_MARKER).is_file(),
            "refusing to write to {}: it has no {DISPOSABLE_MARKER}. Create that file on the stick to confirm \
             its rekordbox library and anything rbxport's tests write there may be deleted.",
            mount.display()
        );
        clear_test_output(&mount);
        let file_system = mounted_type(&mount);
        let root = ExportRoot::for_file_system(&file_system);
        Self { label: format!("{name} ({file_system})"), mount, root, image: None }
    }

    /// The rekordbox folder an export to this volume writes.
    fn library_dir(&self) -> &'static str {
        match self.root {
            Some(ExportRoot::Hidden) => ".PIONEER",
            _ => "PIONEER",
        }
    }

    /// Exports `playlists` to `destination` on this volume as a SYNC does.
    fn export(&self, destination: &Path, tracks: &[SourceTrack], playlists: &[SourcePlaylist]) -> ExportReport {
        self.export_with(destination, tracks, playlists, None)
    }

    /// [`Self::export`], sharing `cache` with exports to other volumes as the
    /// app shares one preparation cache across every stick.
    fn export_with(
        &self,
        destination: &Path,
        tracks: &[SourceTrack],
        playlists: &[SourcePlaylist],
        cache: Option<&PreparationCache>,
    ) -> ExportReport {
        let tree = playlists.iter().map(|p| SyncNode { id: p.id, parent: 0, attribute: 0 }).collect();
        let sync = SyncSource { db_id: 123, tree, automatic: false };
        let options = ExportOptions { sync: Some(&sync), root: self.root, cache, ..Default::default() };
        export_with_options(destination, tracks, playlists, &[], &options, &mut |_| {})
            .unwrap_or_else(|e| panic!("{}: {e}", self.label))
    }
}

impl Drop for Volume {
    fn drop(&mut self) {
        if self.image.is_some() {
            let _ = Command::new("hdiutil").args(["detach", "-quiet", "-force"]).arg(&self.mount).status();
        }
    }
}

/// The volumes one test runs on. While it holds real sticks, no other test
/// can take them.
struct Volumes {
    // Declared first so the volumes are dropped before the lock is released.
    list: Vec<Volume>,
    _in_use: Option<MutexGuard<'static, ()>>,
}

impl<'a> IntoIterator for &'a Volumes {
    type Item = &'a Volume;
    type IntoIter = std::slice::Iter<'a, Volume>;

    fn into_iter(self) -> Self::IntoIter {
        self.list.iter()
    }
}

fn volumes() -> Volumes {
    if std::env::var_os(REAL_USB_ENV).is_some_and(|value| value == "1") {
        let in_use = REAL_STICKS_IN_USE.lock().unwrap_or_else(PoisonError::into_inner);
        let list = REAL_STICKS.iter().map(|name| Volume::stick(name)).collect();
        return Volumes { list, _in_use: Some(in_use) };
    }
    let list = IMAGE_FORMATS
        .iter()
        .filter_map(|format| {
            let volume = Volume::image(format);
            if volume.is_none() {
                eprintln!("skipping {format}: hdiutil could not create or attach an image here");
            }
            volume
        })
        .collect();
    Volumes { list, _in_use: None }
}

/// The filesystem type macOS reports for the volume mounted at `mount`
/// (`statfs` `f_fstypename`, such as `msdos`, `exfat` or `hfs`): what the
/// app hands to [`ExportRoot::for_file_system`].
fn mounted_type(mount: &Path) -> String {
    let stat = rustix::fs::statfs(mount).unwrap_or_else(|e| panic!("{}: {e}", mount.display()));
    c_string(&stat.f_fstypename)
}

/// A NUL-terminated `statfs` name field as a string.
fn c_string(field: &[std::ffi::c_char]) -> String {
    let bytes: Vec<u8> = field.iter().take_while(|c| **c != 0).map(|c| *c as u8).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Deletes what an earlier test left at a real stick's root, keeping
/// everything else on it, the marker included.
fn clear_test_output(mount: &Path) {
    for entry in std::fs::read_dir(mount).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        let ours = TEST_OUTPUT.contains(&name.as_str()) || (name.starts_with(".rbxport-") && name != DISPOSABLE_MARKER);
        if !ours {
            continue;
        }
        let path = entry.path();
        let removed = if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
        removed.unwrap_or_else(|e| panic!("could not clear {}: {e}", path.display()));
    }
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

/// One playlist holding every track in `track_indices`.
fn playlist(id: u64, name: &str, track_indices: Vec<usize>) -> SourcePlaylist {
    SourcePlaylist { id, name: name.into(), track_indices, ..Default::default() }
}

/// Whether `path` was stored under exactly this form of its name: a FAT32
/// stick renames a file only by its stored form. HFS+ looks names up in any
/// normalization form, so there it shows only that the file is reachable.
fn stored_as(path: &Path) -> bool {
    let moved = path.with_file_name("probe.tmp");
    std::fs::rename(path, &moved).is_ok() && std::fs::rename(&moved, path).is_ok()
}

/// rbxport's staging and publication entries still at `root`.
fn leftovers(root: &Path) -> Vec<String> {
    std::fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".rbxport-staging-") || n.starts_with(".rbxport-retired-") || n.starts_with(".tmp") || n == ".rbxport-publication")
        .collect()
}

#[test]
fn a_stick_takes_an_export_and_its_resync() {
    for volume in &volumes() {
        let name = &volume.label;
        let root = volume.mount.join("export");
        std::fs::create_dir_all(&root).unwrap();
        let src = tempfile::tempdir().unwrap();
        let tracks = tracks(src.path());
        let every = |tracks: &[SourceTrack]| vec![playlist(10, "Set", (0..tracks.len()).collect())];

        // exFAT failed here with "could not write exportLibrary.db:
        // Operation not supported (os error 45)", FAT32 with "No such file
        // or directory (os error 2)" at 99%.
        let first = volume.export(&root, &tracks, &every(&tracks));
        assert_eq!(first.tracks, 3, "{name}");
        let again = volume.export(&root, &tracks, &every(&tracks));
        assert_eq!(again.reused, 3, "{name}");
        let fewer = volume.export(&root, &tracks[1..], &every(&tracks[1..]));
        assert_eq!(fewer.removed, 1, "{name}");

        let check = verify(&root).unwrap();
        assert!(check.is_ok(), "{name}: {:?}", check.errors);
        assert_eq!(check.tracks, 2, "{name}");
        // Published under the NFC name both databases carry.
        let audio = root.join("Contents/TRIODE/Album/\u{c9}bano \u{2014} Ti\u{eb}sto.mp3");
        assert!(stored_as(&audio), "{name}: not stored in NFC");
        // Nothing left behind: a stage exFAT could not delete would hold a
        // second copy of the audio.
        assert!(leftovers(&root).is_empty(), "{name}: {:?}", leftovers(&root));
    }
}

/// What a stick looked like after the earlier version failed to publish on
/// FAT32 (#161): the journal's paths are the stage's NFD listing while its
/// images were written in NFC. Every later sync and USB import failed to
/// recover it with "No such file or directory (os error 2)".
#[test]
fn an_interrupted_export_left_by_the_earlier_version_is_recovered() {
    for volume in &volumes() {
        let name = &volume.label;
        let root = volume.mount.join("export");
        let database = format!("{}/rekordbox/export.pdb", volume.library_dir());
        let journal = root.join(".rbxport-publication");
        let image = journal.join("Contents/Bj\u{f6}rk/Album/Kes\u{e4}.mp3");
        std::fs::create_dir_all(image.parent().unwrap()).unwrap();
        std::fs::write(&image, b"audio").unwrap();
        std::fs::create_dir_all(journal.join(&database).parent().unwrap()).unwrap();
        std::fs::write(journal.join(&database), b"pdb").unwrap();
        let entries = serde_json::json!([
            {"path": "Contents/Bjo\u{308}rk/Album/Kesa\u{308}.mp3", "present": true, "had_target": false},
            {"path": database, "present": true, "had_target": false},
        ]);
        std::fs::write(journal.join("publication.json"), serde_json::to_vec(&entries).unwrap()).unwrap();

        rbl_export::recover(&root).unwrap_or_else(|e| panic!("{name}: {e}"));

        assert!(!journal.exists(), "{name}");
        let published = root.join("Contents/Bj\u{f6}rk/Album/Kes\u{e4}.mp3");
        assert_eq!(std::fs::read(&published).unwrap(), b"audio", "{name}");
        assert!(stored_as(&published), "{name}: not stored in NFC");
        assert!(leftovers(&root).is_empty(), "{name}: {:?}", leftovers(&root));
    }
}

/// #283: a stick synced once takes a second sync that adds a playlist of
/// tracks it does not hold yet, at the volume's root as the Sync Manager
/// writes it.
#[test]
fn a_stick_takes_another_playlist_on_a_later_sync() {
    for volume in &volumes() {
        let name = &volume.label;
        let root = &volume.mount;
        let src = tempfile::tempdir().unwrap();
        let tracks = tracks(src.path());
        let set = playlist(10, "Set", vec![0, 1]);
        let warm_up = playlist(11, "Warm-up", vec![2]);

        volume.export(root, &tracks[..2], std::slice::from_ref(&set));
        let report = volume.export(root, &tracks, &[set, warm_up]);
        assert_eq!((report.tracks, report.reused), (3, 2), "{name}");

        let check = verify(root).unwrap();
        assert!(check.is_ok(), "{name}: {:?}", check.errors);
        assert_eq!(check.tracks, 3, "{name}");
        assert!(root.join(volume.library_dir()).is_dir(), "{name}: wrong rekordbox root");
        assert!(leftovers(root).is_empty(), "{name}: {:?}", leftovers(root));
    }
}

/// A generation whose databases do not read back is rolled back to the
/// previous one, and recovering again afterwards changes nothing.
#[test]
fn published_invalid_databases_roll_back_on_every_stick_format() {
    for volume in &volumes() {
        let name = &volume.label;
        let root = volume.mount.join("retained");
        let source = tempfile::tempdir().unwrap();
        let tracks = tracks(source.path());
        volume.export(&root, &tracks, &[playlist(10, "Set", (0..tracks.len()).collect())]);

        let databases = ["export.pdb", "exportLibrary.db"]
            .map(|file| PathBuf::from(format!("{}/rekordbox/{file}", volume.library_dir())));
        let original: Vec<Vec<u8>> = databases.iter().map(|p| std::fs::read(root.join(p)).unwrap()).collect();

        // Publish a generation whose databases are garbage, retaining the old one.
        {
            let publication = rbl_core::durable::Publication::new(&root, ".rbxport-publication").unwrap();
            for path in &databases {
                let staged = publication.stage().join(path);
                rbl_core::durable::create_dir_all(staged.parent().unwrap()).unwrap();
                rbl_core::durable::write(&staged, b"invalid generation").unwrap();
            }
            publication.commit_retained(&databases).unwrap();
        }

        assert!(rbl_export::recover(&root).is_err(), "{name}: invalid generation was accepted");
        for (path, bytes) in databases.iter().zip(&original) {
            assert_eq!(&std::fs::read(root.join(path)).unwrap(), bytes, "{name}: {} not rolled back", path.display());
        }
        rbl_export::recover(&root).unwrap_or_else(|e| panic!("{name}: {e}"));
        rbl_export::recover(&root).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(verify(&root).unwrap().is_ok(), "{name}");
        assert!(leftovers(&root).is_empty(), "{name}: {:?}", leftovers(&root));
    }
}

/// A SYNC to several sticks at once, as the Sync Manager runs it: one export
/// per stick on its own thread, all sharing one preparation cache. Each
/// stick ends up with the whole selection and nothing of another's.
#[test]
fn a_sync_writes_every_stick_at_once() {
    let volumes = volumes();
    let source = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    let cache = PreparationCache::new(cache_dir.path().to_owned());
    let tracks = tracks(source.path());
    let set = [playlist(10, "Set", (0..tracks.len()).collect())];

    let reports: Vec<ExportReport> = std::thread::scope(|scope| {
        let workers: Vec<_> = volumes
            .into_iter()
            .map(|volume| scope.spawn(|| volume.export_with(&volume.mount, &tracks, &set, Some(&cache))))
            .collect();
        workers.into_iter().map(|worker| worker.join().unwrap()).collect()
    });

    for (volume, report) in volumes.into_iter().zip(&reports) {
        let name = &volume.label;
        assert_eq!(report.tracks, 3, "{name}");
        assert!(report.failed.is_empty() && report.skipped.is_empty(), "{name}: {report:?}");
        let check = verify(&volume.mount).unwrap();
        assert!(check.is_ok(), "{name}: {:?}", check.errors);
        assert_eq!(check.tracks, 3, "{name}");
        assert!(volume.mount.join(volume.library_dir()).is_dir(), "{name}: wrong rekordbox root");
        assert!(leftovers(&volume.mount).is_empty(), "{name}: {:?}", leftovers(&volume.mount));
    }
}

/// A blank exFAT image with 128 KiB clusters, as Windows formats a stick
/// over 32 GB, mounted at a fresh folder; `None` where the tools cannot
/// make one. Detached on drop through [`Volume`].
fn large_cluster_exfat() -> Option<Volume> {
    let dir = tempfile::tempdir().ok()?;
    let image = dir.path().join("stick.dmg");
    let mount = dir.path().join("mnt");
    let run = |command: &mut Command| command.output().ok().filter(|o| o.status.success());
    run(Command::new("hdiutil").args(["create", "-quiet", "-size", "512m", "-layout", "NONE", "-type", "UDIF", "-o"]).arg(&image))?;
    let attached = run(Command::new("hdiutil").args(["attach", "-nomount"]).arg(&image))?;
    let device = String::from_utf8_lossy(&attached.stdout).split_whitespace().next()?.to_owned();
    let formatted = run(Command::new("newfs_exfat").args(["-b", "131072", "-v", "RBXTEST", &device]));
    std::fs::create_dir_all(&mount).ok()?;
    let mounted = formatted.and_then(|_| run(Command::new("diskutil").args(["mount", "-mountPoint"]).arg(&mount).arg(&device)));
    if mounted.is_none() {
        let _ = Command::new("hdiutil").args(["detach", "-quiet", "-force", &device]).status();
        return None;
    }
    Some(Volume { label: "exFAT, 128 KiB clusters".into(), mount, root: None, image: Some(dir) })
}

/// The space a sync says it needs covers what it really takes at its fullest
/// on a stick with large clusters, AppleDouble companions included, so a
/// sync the check lets start does not run out of room part way (#591).
/// [OBS 2026-10-19, macOS 26] 120 tracks: estimated peak 727,131,648 B,
/// sampled 725,483,520 B; 4 KiB rounding had said 457,576,388 B.
#[test]
fn the_space_check_covers_a_large_cluster_stick() {
    let Some(volume) = large_cluster_exfat() else {
        eprintln!("skipping: could not make a 128 KiB-cluster exFAT image here");
        return;
    };
    assert_eq!(rbl_export::plan::allocation_unit(&volume.mount), 128 * 1024);
    let source = tempfile::tempdir().unwrap();
    let art = source.path().join("art.jpg");
    std::fs::write(&art, vec![0xff_u8; 30_000]).unwrap();
    let tracks: Vec<SourceTrack> = (1..=40_u64)
        .map(|id| {
            let path = source.path().join(format!("track-{id}.mp3"));
            std::fs::write(&path, vec![id as u8; 500_000 + (id as usize * 7_919_191) % 6_000_000]).unwrap();
            let anlz = |n: usize| rbl_anlz::AnlzBuilder::new().path("/x.mp3").waveform_preview(b"PWAV", &vec![7_u8; n]).finish();
            SourceTrack {
                id, source_path: path, title: format!("Track {id}"), artist: format!("Artist {}", id % 7),
                album: format!("Album {}", id % 13), duration_sec: 120, sample_rate: 44_100,
                analysis: vec![("DAT".into(), anlz(2_000)), ("EXT".into(), anlz(40_000)), ("2EX".into(), anlz(200_000))],
                artwork: (id % 3 == 0).then(|| art.clone()),
                ..Default::default()
            }
        })
        .collect();
    let set = [playlist(10, "Set", (0..tracks.len()).collect())];
    let tree = set.iter().map(|p| SyncNode { id: p.id, parent: 0, attribute: 0 }).collect();
    let sync = SyncSource { db_id: 123, tree, automatic: false };
    let options = ExportOptions { sync: Some(&sync), ..Default::default() };
    let plan = rbl_export::plan::preview(&volume.mount, &tracks, &set, &[], &options).unwrap();
    assert!(!plan.unknown_size);

    let free = || fs2::available_space(&volume.mount).unwrap();
    let before = free();
    let done = std::sync::atomic::AtomicBool::new(false);
    let fullest = std::thread::scope(|scope| {
        let sampler = scope.spawn(|| {
            let mut least = u64::MAX;
            while !done.load(std::sync::atomic::Ordering::Relaxed) {
                least = least.min(free());
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            least
        });
        let report = export_with_options(&volume.mount, &tracks, &set, &[], &options, &mut |_| {}).unwrap();
        done.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(report.failed.is_empty(), "{report:?}");
        before - sampler.join().unwrap().min(free())
    });
    let used = before - free();
    assert!(plan.peak_bytes >= fullest, "estimated peak {} < used at the fullest {fullest}", plan.peak_bytes);
    // And not so far over that it turns away a sync that fits.
    assert!(plan.peak_bytes <= fullest + fullest / 20, "estimated peak {} far over {fullest}", plan.peak_bytes);
    let net = u64::try_from(plan.net_additional_bytes).unwrap();
    assert!(net.abs_diff(used) <= used / 100, "estimated {net}, used {used}");
}

/// A Journaled HFS+ image at its root: what rbxport's Format HFS+ and Disk
/// Utility make, mounted read-write as macOS mounts it.
fn hfs_plus() -> Option<Volume> {
    let volume = Volume::image("JHFS+");
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
    volume.export(&volume.mount, &tracks, &[playlist(10, "Set", (0..tracks.len()).collect())]);
    let before = verify(&volume.mount).unwrap();

    let image = volume.image.as_ref().unwrap().path().join("stick.dmg");
    assert!(Command::new("hdiutil").args(["detach", "-quiet", "-force"]).arg(&volume.mount).status().unwrap().success());
    let attached = Command::new("hdiutil")
        .args(["attach", "-quiet", "-readonly", "-nobrowse", "-mountpoint"])
        .arg(&volume.mount)
        .arg(&image)
        .status()
        .unwrap();
    assert!(attached.success());

    let source = SyncSource { db_id: 123, tree: vec![SyncNode { id: 10, parent: 0, attribute: 0 }], automatic: false };
    let options = ExportOptions { sync: Some(&source), root: volume.root, ..Default::default() };
    let lists = [playlist(10, "Set", vec![0])];
    let error = export_with_options(&volume.mount, &tracks[..1], &lists, &[], &options, &mut |_| {}).unwrap_err();
    let rbl_export::ExportError::Io(io) = &error else { panic!("{error:?}") };
    assert_eq!(io.kind(), std::io::ErrorKind::ReadOnlyFilesystem, "{error}");
    assert!(error.to_string().contains("Read-only file system"), "{error}");
    let after = verify(&volume.mount).unwrap();
    assert_eq!((after.tracks, after.is_ok()), (before.tracks, true));
}
