//! Disposable Windows records from the former collision-path writer.
#![cfg(windows)]
#![allow(clippy::pedantic, clippy::unwrap_used)]
use std::{fs, path::{Path, PathBuf}, time::SystemTime};
use rbl_export::{Manifest, SourceTrack, SourcePlaylist, SyncSource};

const EXTENSIONS: [&str; 3] = ["DAT", "EXT", "2EX"];
fn physical(root: &Path, path: &str) -> PathBuf { root.join(path.trim_start_matches('/')) }

// The established USB path hash, evaluated for the old writer's spelling.
fn analysis_dir(audio: &str) -> String {
    let hash = audio.encode_utf16().fold(0_u32, |h, c|
        h.wrapping_mul(0x34f5_501d).wrapping_add(u32::from(c) * 0x93b6)) % 0x0003_0d43;
    let shard = [0, 2, 6, 7, 9, 13, 16].iter().enumerate()
        .fold(0_u32, |s, (i, b)| s | ((hash >> b) & 1) << i);
    format!("/PIONEER/USBANLZ/P{shard:03X}/{hash:08X}")
}

fn replace_path(bytes: &mut [u8], from: &str, to: &str) {
    assert_eq!(from.len(), to.len());
    let needle = from.as_bytes();
    let at = bytes.windows(needle.len()).position(|part| part == needle).unwrap();
    assert!(bytes[at + needle.len()..].windows(needle.len()).all(|part| part != needle));
    bytes[at..at + needle.len()].copy_from_slice(to.as_bytes());
}

fn assert_verified(root: &Path) {
    assert!(rbl_export::verify(root).unwrap().is_ok());
    for track in Manifest::load(root).unwrap().tracks {
        assert_eq!(rbl_export::manifest::hash(&fs::read(physical(root, &track.audio)).unwrap()), track.audio_hash);
        for (extension, hash) in track.analysis_hashes {
            let file = physical(root, &format!("{}/ANLZ0000.{extension}", track.anlz_dir));
            assert_eq!(rbl_export::manifest::hash(&fs::read(file).unwrap()), hash);
        }
    }
}

struct Case {
    _source: tempfile::TempDir,
    root: tempfile::TempDir,
    tracks: Vec<SourceTrack>,
    playlists: Vec<SourcePlaylist>,
    origin: SyncSource,
    normal: String,
    old: String,
    old_dir: String,
    cached: bool,
    old_companions: Vec<(PathBuf, Vec<u8>)>,
    identity: PathBuf,
    bytes: Vec<u8>,
    modified: SystemTime,
}
impl Case {
    fn sync(&self) -> rbl_export::ExportReport {
        rbl_export::export_full(self.root.path(), &self.tracks, &self.playlists,
            &[], None, Some(&self.origin), &mut |_| {}).unwrap()
    }
    fn seed(cached: bool, protected: bool) -> Self {
        let source = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let mut tracks = Vec::new();
        for id in [1, 2] {
            let folder = source.path().join(id.to_string());
            fs::create_dir(&folder).unwrap();
            let audio = folder.join("same.mp3");
            fs::write(&audio, vec![id as u8; 2048]).unwrap();
            let analysis = EXTENSIONS.map(|ext| (ext.into(),
                rbl_anlz::AnlzBuilder::new().path(audio.to_str().unwrap()).finish())).to_vec();
            tracks.push(SourceTrack { id, source_path: audio, title: format!("Track{id}"),
                artist: "Artist".into(), album: "Album".into(), analysis, ..Default::default() });
        }
        let playlists = vec![SourcePlaylist { id: 10, name: "Set".into(),
            track_indices: vec![0, 1], ..Default::default() }];
        let origin = SyncSource { db_id: 123, tree: vec![rbl_export::SyncNode { id: 10, parent: 0, attribute: 0 }],
            automatic: false, cloud_db_id: 0 };
        rbl_export::export_full(root.path(), &tracks, &playlists, &[], None, Some(&origin), &mut |_| {}).unwrap();
        let mut manifest = Manifest::load(root.path()).unwrap();
        let slot = manifest.tracks.iter().position(|track| track.library_id == 2).unwrap();
        let normal = manifest.tracks[slot].audio.clone();
        let (parent, name) = normal.rsplit_once('/').unwrap();
        // Exactly the former writer, rather than arbitrary malformed bytes.
        let old = Path::new(parent).join(name).to_string_lossy().into_owned();
        assert_ne!(normal, old);
        let audio = physical(root.path(), &normal);
        assert_eq!(audio.canonicalize().unwrap(), physical(root.path(), &old).canonicalize().unwrap());
        let bytes = fs::read(&audio).unwrap();
        let modified = fs::metadata(&audio).unwrap().modified().unwrap();
        let identity = source.path().join("retained-audio-identity");
        fs::hard_link(&audio, &identity).unwrap();
        let current_dir = manifest.tracks[slot].anlz_dir.clone();
        let old_dir = analysis_dir(&old);
        assert_ne!(current_dir, old_dir);
        let current_analysis = format!("{current_dir}/ANLZ0000.DAT");
        let old_analysis = format!("{old_dir}/ANLZ0000.DAT");
        let mut old_companions = Vec::new();
        for ext in EXTENSIONS {
            let before = physical(root.path(), &format!("{current_dir}/ANLZ0000.{ext}"));
            let after = physical(root.path(), &format!("{old_dir}/ANLZ0000.{ext}"));
            fs::create_dir_all(after.parent().unwrap()).unwrap();
            let rewritten = rbl_anlz::Anlz::read(&before).unwrap().with_path(&old);
            fs::write(&after, &rewritten).unwrap();
            manifest.tracks[slot].analysis_hashes.insert(ext.into(), rbl_export::manifest::hash(&rewritten));
            fs::remove_file(before).unwrap();
            old_companions.push((after, rewritten));
        }
        let pdb_path = root.path().join("PIONEER/rekordbox/export.pdb");
        let mut pdb = fs::read(&pdb_path).unwrap();
        replace_path(&mut pdb, &normal, &old);
        replace_path(&mut pdb, &current_analysis, &old_analysis);
        fs::write(&pdb_path, pdb).unwrap();
        let conn = rusqlite::Connection::open(root.path().join("PIONEER/rekordbox/exportLibrary.db")).unwrap();
        conn.pragma_update(None, "cipher", "sqlcipher").unwrap();
        conn.pragma_update(None, "legacy", 4).unwrap();
        conn.pragma_update(None, "key", rbl_onelibrary::key::passphrase().unwrap()).unwrap();
        conn.execute("UPDATE content SET path=?1,analysisDataFilePath=?2 WHERE content_id=?3",
            rusqlite::params![old, old_analysis, manifest.tracks[slot].export_id]).unwrap();
        drop(conn);
        manifest.tracks[slot].audio = old.clone();
        manifest.tracks[slot].anlz_dir.clone_from(&old_dir);
        manifest.tracks[slot].preserved = protected.then_some(true);
        if cached {
            // A previous resync can have published the bad spelling with a
            // current receipt. All public receipt checks must genuinely match.
            manifest.baseline = Some(rbl_export::snapshot::Snapshot::read(root.path()).unwrap());
            let outputs = rbl_export::incremental::output_revisions(root.path(), &manifest.tracks).unwrap();
            let databases = rbl_export::incremental::databases(root.path()).unwrap();
            let generation = manifest.generation.as_mut().unwrap();
            generation.outputs = outputs;
            generation.databases = databases;
            generation.analysis_recipes.insert(manifest.tracks[slot].key(),
                rbl_export::incremental::analysis_recipe(&tracks[1], &old).unwrap());
            let structure = rbl_export::incremental::manifest_structure(&manifest).unwrap();
            manifest.generation.as_mut().unwrap().structure = structure;
        } else {
            manifest.version = 1;
            manifest.generation = None;
            manifest.baseline = None;
        }
        manifest.save(root.path()).unwrap();
        let loaded = Manifest::load(root.path()).unwrap();
        assert_eq!(loaded.generation.is_some(), cached);
        if let Some(generation) = &loaded.generation {
            assert!(rbl_export::incremental::receipts_match(root.path(), generation).unwrap());
        }
        assert_verified(root.path());
        Self { _source: source, root, tracks, playlists, origin, normal, old, old_dir, cached, old_companions, identity, bytes, modified }
    }
    fn assert_audio(&self) {
        let audio = physical(self.root.path(), &self.normal);
        assert_eq!(fs::read(&audio).unwrap(), self.bytes);
        assert_eq!(fs::metadata(&audio).unwrap().modified().unwrap(), self.modified);
        assert!(rbl_core::durable::same_root(&audio, &self.identity).unwrap(), "no replacement of the physical file");
    }
    fn assert_portable(&self) {
        let manifest = Manifest::load(self.root.path()).unwrap();
        let track = manifest.tracks.iter().find(|track| track.library_id == 2).unwrap();
        assert_eq!(track.audio, self.normal, "historical collision spelling must migrate");
        assert_eq!(track.anlz_dir, analysis_dir(&self.normal));
        let analysis = format!("{}/ANLZ0000.DAT", track.anlz_dir);
        let bytes = fs::read(self.root.path().join("PIONEER/rekordbox/export.pdb")).unwrap();
        let pdb = rbl_pdb::Pdb::parse(&bytes).unwrap();
        let row = pdb.track_rows(pdb.table(rbl_pdb::PageType::Tracks).unwrap()).into_iter()
            .find(|row| row.id == track.export_id).unwrap();
        assert_eq!((row.file_path, row.analyze_path), (self.normal.clone(), analysis.clone()));
        let one = rbl_onelibrary::ExportLibrary::open_read_only(&self.root.path().join("PIONEER/rekordbox/exportLibrary.db")).unwrap();
        let paths: (String, String) = one.connection().query_row(
            "SELECT path,analysisDataFilePath FROM content WHERE content_id=?1", [track.export_id],
            |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!(paths, (self.normal.clone(), analysis));
        for ext in EXTENSIONS {
            let file = physical(self.root.path(), &format!("{}/ANLZ0000.{ext}", track.anlz_dir));
            assert_eq!(rbl_anlz::Anlz::read(&file).unwrap().path(), Some(self.normal.clone()));
        }
        if self.cached {
            assert!(!physical(self.root.path(), &self.old_dir).exists());
        } else {
            // Version-1 records cannot prove ownership of the old companions;
            // the existing reclamation policy deliberately leaves them alone.
            for (file, bytes) in &self.old_companions { assert_eq!(fs::read(file).unwrap(), *bytes); }
        }
        assert_verified(self.root.path());
    }
}

fn assert_migration(cached: bool) {
        let case = Case::seed(cached, false);
        let report = case.sync();
        case.assert_audio();
        assert_eq!(report.reused, 2);
        assert_eq!(report.bytes_copied, 0);
        assert!(!report.up_to_date, "a cached bad generation must reach the repair");
        case.assert_portable();
        let repeat = case.sync();
        assert!(repeat.up_to_date, "well-formed generations retain the fast no-op");
        case.assert_audio();
        case.assert_portable();
}

#[test]
fn legacy_manifest_paths_migrate_without_recopy_or_same_file_rename() { assert_migration(false); }

#[test]
fn cached_mixed_paths_reach_the_repair_and_then_fast_noop() { assert_migration(true); }

#[test]
fn protected_claimed_paths_do_not_force_a_migration() {
    let case = Case::seed(true, true);
    assert!(case.sync().up_to_date);
    let manifest = Manifest::load(case.root.path()).unwrap();
    assert_eq!(manifest.tracks.iter().find(|track| track.library_id == 2).unwrap().audio, case.old);
    case.assert_audio();
}
