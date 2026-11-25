//! Source-identical ordinary-export safety regression run on v1.9 and current dev.
//!
//! Every source, image, journal and recovery target is a fresh local tempfile.
//! This file deliberately does not use the optional physical-stick test helper.
//! Uses the actual normal exporter, including its automatic failure rollback.
#![allow(
    clippy::pedantic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout
)]

use rbl_export::{ExportOptions, ExportRoot, Manifest, SourcePlaylist, SourceTrack, SyncSource};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

const JOURNAL: &str = ".rbxport-publication";

fn put(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}
fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, at: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(at).unwrap() {
            let entry = entry.unwrap();
            assert!(!entry.file_type().unwrap().is_symlink());
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}
fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for (path, bytes) in tree(from) {
        put(&to.join(&path), &bytes);
        let modified = fs::metadata(from.join(&path)).unwrap().modified().unwrap();
        fs::File::options()
            .write(true)
            .open(to.join(path))
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
    }
}
fn root_name(mode: ExportRoot) -> &'static str {
    match mode {
        ExportRoot::Standard => "PIONEER",
        ExportRoot::Hidden => ".PIONEER",
    }
}
fn sync(root: &Path, tracks: &[SourceTrack], mode: ExportRoot) {
    let source = SyncSource {
        db_id: 123,
        tree: vec![],
        automatic: false,
        ..Default::default()
    };
    let options = ExportOptions {
        sync: Some(&source),
        root: Some(mode),
        ..Default::default()
    };
    rbl_export::export_with_options(
        root,
        tracks,
        &[SourcePlaylist {
            id: 10,
            name: "Generated Set".into(),
            track_indices: (0..tracks.len()).collect(),
            ..Default::default()
        }],
        &[],
        &options,
        &mut |_| {},
    )
    .unwrap();
}
fn track(id: u64, source: &Path, artist: &str) -> SourceTrack {
    SourceTrack {
        id,
        source_path: source.to_owned(),
        title: format!("Generated{id}"),
        artist: artist.into(),
        album: "Album".into(),
        analysis: vec![(
            "DAT".into(),
            rbl_anlz::AnlzBuilder::new().path("/generated.mp3").finish(),
        )],
        ..Default::default()
    }
}

fn run(old: &str, new: &str, mode: ExportRoot) {
    let owned = tempfile::tempdir().unwrap();
    let root = owned.path().join("generated-device");
    let a = owned.path().join("a/song.mp3");
    let b = owned.path().join("b/song.mp3");
    put(&a, &[1; 128]);
    put(&b, &[2; 256]);
    let mut moved = track(1, &a, old);
    let artwork = owned.path().join("Artwork/artwork.jpg");
    if std::env::var_os("RBXPORT_641_ARTWORK").is_some() {
        // Export copies provided artwork without decoding it; no player claim.
        put(&artwork, b"\xff\xd8old generated artwork\xff\xd9");
        moved.artwork = Some(artwork.clone());
    }
    let stable_source = owned.path().join("stable/stay.mp3");
    let stable = if std::env::var_os("RBXPORT_641_STABLE_TRACK").is_some() {
        put(&stable_source, &[9; 64]);
        Some(track(9, &stable_source, "Stable"))
    } else {
        None
    };
    let mut previous_tracks = vec![moved.clone()];
    previous_tracks.extend(stable.iter().cloned());
    sync(&root, &previous_tracks, mode);
    put(
        &root.join("unknown/keep.bin"),
        b"unmanaged generated payload",
    );
    let original = Manifest::load(&root).unwrap();
    let before = tree(&root);
    moved.artist = new.into();
    if moved.artwork.is_some() && std::env::var_os("RBXPORT_641_UNCHANGED_ARTWORK").is_none() {
        put(&artwork, b"\xff\xd8NEW generated artwork\xff\xd9");
    }
    let mut tracks = vec![moved, track(2, &b, old)];
    tracks.extend(stable);
    let source = SyncSource {
        db_id: 123,
        tree: vec![],
        automatic: false,
        ..Default::default()
    };
    let options = ExportOptions {
        sync: Some(&source),
        root: Some(mode),
        ..Default::default()
    };
    let lists = [SourcePlaylist {
        id: 10,
        name: "Generated Set".into(),
        track_indices: (0..tracks.len()).collect(),
        ..Default::default()
    }];
    let mut planned_moves = Vec::new();
    let result =
        rbl_export::export_with_options(&root, &tracks, &lists, &[], &options, &mut |progress| {
            if progress.stage == "publishing" {
                for entry in fs::read_dir(&root).unwrap().map(Result::unwrap) {
                    if entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".rbxport-staging-")
                    {
                        if let Ok(bytes) = fs::read(entry.path().join(".moves.json")) {
                            planned_moves
                                .push(serde_json::from_slice::<serde_json::Value>(&bytes).unwrap());
                        }
                    }
                }
            }
        });
    let published = tree(&root);
    // Retain the actual ordinary-export state before another recovery call,
    // preserving all file bytes and mtimes, even when this RED test panics.
    if let Some(path) = std::env::var_os("RBXPORT_641_EVIDENCE") {
        let dir = PathBuf::from(path).join(format!("{}-{old}-{new}", root_name(mode)));
        let image = dir.join("after-export-tree");
        assert!(
            !image.exists(),
            "use a fresh evidence directory per exact run"
        );
        copy_tree(&root, &image);
        assert_eq!(tree(&image), published);
    }
    let verification = rbl_export::verify(&root);
    let after_verify = tree(&root);
    let files: BTreeMap<_,_> = [(old,1),(new,1)].into_iter().map(|(artist,_)| {
        let path = format!("Contents/{artist}/Album/song.mp3");
        let bytes = fs::read(root.join(&path)).ok();
        (path, bytes.map(|bytes|serde_json::json!({"size":bytes.len(),"hash":rbl_export::manifest::hash(&bytes)})))
    }).collect();
    let receipt = serde_json::json!({
        "result":format!("{result:?}"),"displayError":result.as_ref().err().map(ToString::to_string),
        "plannedMoves":planned_moves,"journalAfterExport":published.keys().any(|p|p.starts_with(JOURNAL)),
        "rollbackAfterExport":published.contains_key(&PathBuf::from(format!("{JOURNAL}/.rollback"))),
        "verify":format!("{verification:?}"),"verifyMutatedTree":published!=after_verify,
        "oldDatabasesRestored":rbl_export::incremental::databases(&root).unwrap()==original.generation.as_ref().unwrap().databases,
        "files":files,
    });
    let label = format!("{}-{old}-{new}", root_name(mode));
    if let Some(path) = std::env::var_os("RBXPORT_641_EVIDENCE") {
        let dir = PathBuf::from(path).join(&label);
        put(
            &dir.join("before.json"),
            &serde_json::to_vec(&before).unwrap(),
        );
        put(
            &dir.join("after-export.json"),
            &serde_json::to_vec(&published).unwrap(),
        );
        put(
            &dir.join("after-verify.json"),
            &serde_json::to_vec(&after_verify).unwrap(),
        );
        put(
            &dir.join("result.json"),
            &serde_json::to_vec_pretty(&receipt).unwrap(),
        );
    }
    println!("{label}: {receipt}");
    // Deliberately identical safety expectations on exact old and current source.
    // Capture actual old failures/rollback/images above before the RED assertion.
    assert!(result.is_ok(), "ordinary overlap export failed: {result:?}");
    assert!(result.as_ref().unwrap().verified);
    assert!(
        verification.unwrap().is_ok(),
        "ordinary output must include both audio paths"
    );
    assert!(!root.join(JOURNAL).exists());
    for format in [
        rbl_export::device_library::Format::DeviceLibrary,
        rbl_export::device_library::Format::OneLibrary,
    ] {
        let library = rbl_export::device_library::read(&root, format).unwrap();
        assert_eq!(library.tracks.len(), 2);
        for (id, artist, bytes) in [(1, new, vec![1; 128]), (2, old, vec![2; 256])] {
            let row = library
                .tracks
                .iter()
                .find(|t| t.master_content_id == id)
                .unwrap();
            assert_eq!(row.path, format!("/Contents/{artist}/Album/song.mp3"));
            assert_eq!(
                fs::read(root.join(row.path.trim_start_matches('/'))).unwrap(),
                bytes
            );
        }
    }
    assert_eq!(
        fs::read(root.join("unknown/keep.bin")).unwrap(),
        b"unmanaged generated payload"
    );
    sync(&root, &tracks, mode);
    assert!(rbl_export::verify(&root).unwrap().is_ok());
    assert_eq!(
        fs::read(root.join("unknown/keep.bin")).unwrap(),
        b"unmanaged generated payload"
    );
}
#[test]
fn ordinary_move_before_source_replacement_standard() {
    run("Z", "A", ExportRoot::Standard);
}
#[test]
fn ordinary_source_replacement_before_move_standard() {
    run("A", "Z", ExportRoot::Standard);
}
#[test]
fn ordinary_move_before_source_replacement_hidden() {
    run("Z", "A", ExportRoot::Hidden);
}
#[test]
fn ordinary_source_replacement_before_move_hidden() {
    run("A", "Z", ExportRoot::Hidden);
}
