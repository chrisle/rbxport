//! Private recovery tests using actual retained v1.9 ordinary-export images.
//! All mutations are confined to fresh tempfile copies of exact generated v1.9 images.
//! Captured Linux nanosecond mtimes are retained without weakening receipt checks.
#![cfg(target_os = "linux")]
#![allow(
    clippy::pedantic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout
)]
use rbl_export::Manifest;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[allow(dead_code)]
#[path = "common/legacy_overlap.rs"]
mod fixtures;
use fixtures::{JOURNAL, before_stamps, before_tree, copy_owned, image, put, rel, stamps, tree};

fn authority(root: &Path) -> Result<Manifest, &'static str> {
    let journal = root.join(JOURNAL);
    // Fixture-only out-of-band witness, not an available production authority:
    // the exact old exporter saved this complete tree BEFORE its ordinary call.
    // Current Manifest::load legitimately drops old typed structure receipts
    // because ExportMetadata gained content_link; never weaken that loader here.
    let previous = journal.join(".previous");
    let manifest_path = Manifest::path(&previous);
    let old_bytes = fs::read(&manifest_path).map_err(|_| "old manifest unavailable")?;
    let old: Manifest = serde_json::from_slice(&old_bytes).map_err(|_| "old manifest invalid")?;
    let relative = manifest_path.strip_prefix(&previous).unwrap();
    let mode = relative
        .components()
        .next()
        .unwrap()
        .as_os_str()
        .to_str()
        .unwrap();
    let witness = before_tree(
        mode,
        old.tracks.iter().any(|track| !track.artwork.is_empty()),
    );
    if witness.get(relative) != Some(&old_bytes) {
        return Err("old manifest differs from independent before witness");
    }
    let generation = old.generation.as_ref().ok_or("old receipts unavailable")?;
    if generation.databases.is_empty() {
        return Err("old structure unavailable");
    }
    let expected_keys = rbl_export::incremental::databases(root).unwrap();
    if generation.databases.keys().ne(expected_keys.keys()) {
        return Err("incomplete database receipt key set");
    }
    for (name, expected) in &generation.databases {
        let previous = journal.join(".previous").join(rel(name));
        let candidate = if previous.is_file() {
            previous
        } else {
            root.join(rel(name))
        };
        if rbl_export::incremental::content_digest(&candidate)
            .ok()
            .flatten()
            != *expected
        {
            return Err("old database receipt mismatch");
        }
    }
    for track in &old.tracks {
        if track.in_place || track.preserved == Some(true) || track.audio_hash == 0 {
            return Err("old audio not byte-bound");
        }
        let (size, _) = track.copy_stamp.ok_or("old copy length unavailable")?;
        let paths = [
            root.join(rel(&track.audio)),
            journal.join(".previous").join(rel(&track.audio)),
        ];
        let matching: Vec<_> = paths
            .iter()
            .filter_map(|p| fs::read(p).ok())
            .filter(|b| b.len() as u64 == size && rbl_export::manifest::hash(b) == track.audio_hash)
            .collect();
        if matching.is_empty() {
            return Err("old audio unavailable");
        }
        // Duplicate identical old images are legitimate after finalize's repair copy.
        if matching.iter().any(|b| b != &matching[0]) {
            return Err("old image ambiguous");
        }
        for ext in &track.analysis_extensions {
            let expected = track
                .analysis_hashes
                .get(ext)
                .filter(|h| **h != 0)
                .ok_or("old analysis receipt unavailable")?;
            let name = rel(&format!("{}/ANLZ0000.{ext}", track.anlz_dir));
            if ![root.join(&name), journal.join(".previous").join(name)]
                .iter()
                .filter_map(|p| fs::read(p).ok())
                .any(|b| rbl_export::manifest::hash(&b) == *expected)
            {
                return Err("old required analysis unavailable");
            }
        }
    }
    // Affected art has only a size/mtime output receipt, not a byte digest.
    if generation.outputs.keys().any(|p| {
        p.to_ascii_uppercase().contains("/ARTWORK/")
            && journal.join(".previous").join(rel(p)).is_file()
    }) {
        return Err("affected artwork has no output-byte authority");
    }
    let new: Manifest = serde_json::from_slice(
        &fs::read(Manifest::path(root)).map_err(|_| "new sealed manifest unavailable")?,
    )
    .map_err(|_| "new sealed manifest invalid")?;
    let sealed = journal
        .join(".sealed")
        .join(Manifest::path(root).strip_prefix(root).unwrap());
    if fs::read(sealed).ok() != fs::read(Manifest::path(root)).ok() {
        return Err("new manifest not sealed");
    }
    let moves: BTreeMap<String, String> =
        serde_json::from_slice(&fs::read(journal.join(".moves.json")).unwrap()).unwrap();
    if moves.len() != 1 {
        return Err("not a single legal relocation");
    }
    let (to, from) = moves.iter().next().unwrap();
    let replacement = new
        .tracks
        .iter()
        .find(|t| rel(&t.audio) == rel(from))
        .ok_or("replacement missing")?;
    let moved = fs::read(root.join(rel(to))).map_err(|_| "moved image missing")?;
    if replacement.audio_hash == 0
        || rbl_export::manifest::hash(&moved) != replacement.audio_hash
        || replacement
            .copy_stamp
            .is_none_or(|(size, _)| size != moved.len() as u64)
    {
        return Err("moved image not known replacement");
    }
    Ok(old)
}
fn record(label: &str, before: &BTreeMap<PathBuf, Vec<u8>>, root: &Path, result: &str) {
    if let Some(path) = std::env::var_os("RBXPORT_641_RECOVERY_EVIDENCE") {
        let path = PathBuf::from(path).join(label);
        put(
            &path.join("before.json"),
            &serde_json::to_vec(before).unwrap(),
        );
        put(
            &path.join("after.json"),
            &serde_json::to_vec(&tree(root)).unwrap(),
        );
        put(&path.join("result.txt"), result.as_bytes());
    }
    println!("{label}: {result}");
}
fn normal(mode: &str) {
    let original = image(mode, false);
    let original_tree = tree(&original);
    let original_stamps = stamps(&original);
    let (owned, root) = copy_owned(&original);
    let old = authority(&root)
        .expect("complete actual old DB/audio/ANLZ and exact known-new replacement");
    let before = tree(&root);
    let result = rbl_export::recover(&root);
    record(
        &format!("{mode}-normal"),
        &before,
        &root,
        &format!("{result:?}"),
    );
    if result.is_err() {
        assert_eq!(
            tree(&root),
            before,
            "current refusal must preserve all images"
        );
    }
    assert_eq!(tree(&original), original_tree);
    assert_eq!(stamps(&original), original_stamps);
    assert!(
        result.is_ok(),
        "complete old generation cannot currently recover: {result:?}"
    );
    assert_eq!(
        rbl_export::incremental::databases(&root).unwrap(),
        old.generation.as_ref().unwrap().databases
    );
    assert!(rbl_export::verify(&root).unwrap().is_ok());
    assert_eq!(tree(&root), before_tree(mode, false));
    let expected_stamps = before_stamps(mode, false);
    assert_eq!(stamps(&root), expected_stamps);
    assert!(!rbl_export::recovery_pending(&root));
    rbl_export::recover(&root).unwrap();
    assert_eq!(tree(&root), before_tree(mode, false));
    assert_eq!(stamps(&root), expected_stamps);

    // A genuine collection source with the same bytes and source revision,
    // rather than an in-place/device-preservation shortcut.
    let previous = &old.tracks[0];
    assert!(previous.conversion.is_empty());
    let source = owned.path().join("collection/song.mp3");
    put(&source, &fs::read(root.join(rel(&previous.audio))).unwrap());
    let modified = u128::try_from(previous.modified).unwrap();
    let time = std::time::UNIX_EPOCH
        + std::time::Duration::new(
            u64::try_from(modified / 1_000_000_000).unwrap(),
            (modified % 1_000_000_000) as u32,
        );
    fs::File::options()
        .write(true)
        .open(&source)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(time))
        .unwrap();
    let device = &old.generation.as_ref().unwrap().device_sources[&previous.export_id];
    let track = rbl_export::SourceTrack {
        id: previous.library_id,
        source_path: source,
        title: device.title.clone(),
        artist: device.artist.clone(),
        album: device.album.clone(),
        analysis: vec![(
            "DAT".into(),
            fs::read(root.join(rel(&format!("{}/ANLZ0000.DAT", previous.anlz_dir)))).unwrap(),
        )],
        ..Default::default()
    };
    let sync = rbl_export::SyncSource {
        db_id: old.db_id,
        ..Default::default()
    };
    let options = rbl_export::ExportOptions {
        sync: Some(&sync),
        root: Some(if mode.starts_with('.') {
            rbl_export::ExportRoot::Hidden
        } else {
            rbl_export::ExportRoot::Standard
        }),
        ..Default::default()
    };
    let playlists = [rbl_export::SourcePlaylist {
        id: old.playlists[0].library_id,
        name: old.playlists[0].name.clone(),
        track_indices: vec![0],
        ..Default::default()
    }];
    let first = rbl_export::export_with_options(
        &root,
        std::slice::from_ref(&track),
        &playlists,
        &[],
        &options,
        &mut |_| {},
    )
    .unwrap();
    assert!(first.verified);
    assert_eq!(first.bytes_copied, 0);
    let repeat =
        rbl_export::export_with_options(&root, &[track], &playlists, &[], &options, &mut |_| {})
            .unwrap();
    assert!(repeat.verified);
    assert_eq!(repeat.bytes_copied, 0);
    assert!(repeat.up_to_date);
    assert_eq!(
        fs::read(root.join("unknown/keep.bin")).unwrap(),
        b"unmanaged generated payload"
    );
}
#[test]
fn normal_recovery_of_actual_retained_standard_should_restore_old() {
    normal("PIONEER");
}
#[test]
fn normal_recovery_of_actual_retained_hidden_should_restore_old() {
    normal(".PIONEER");
}
#[test]
fn default_rollback_refuses_but_known_replacement_quarantine_proves_old_restore_feasible() {
    for mode in ["PIONEER", ".PIONEER"] {
        let original = image(mode, false);
        let original_tree = tree(&original);
        let original_stamps = stamps(&original);
        let (owned, root) = copy_owned(&original);
        authority(&root).unwrap();
        let before = tree(&root);
        let result = rbl_core::durable::Publication::rollback_pending(&root, JOURNAL);
        record(
            &format!("{mode}-default-rollback"),
            &before,
            &root,
            &format!("{result:?}"),
        );
        assert!(
            result
                .unwrap_err()
                .to_string()
                .starts_with("Relocated audio changed during rollback")
        );
        let mut after = tree(&root);
        assert_eq!(
            after.remove(&PathBuf::from(format!("{JOURNAL}/.rollback"))),
            Some(b"1".to_vec())
        );
        assert_eq!(
            after, before,
            "default rollback may only record its durable intent before refusing"
        );
        // Feasibility-only intervention on this disposable copy, NOT a proposed
        // production protocol: normal recovery above made no such intervention.
        let moves: BTreeMap<String, String> =
            serde_json::from_slice(&fs::read(root.join(JOURNAL).join(".moves.json")).unwrap())
                .unwrap();
        let (to, _) = moves.iter().next().unwrap();
        let moved = root.join(rel(to));
        let bytes = fs::read(&moved).unwrap();
        authority(&root).unwrap(); // includes exact sealed replacement hash+length
        let quarantine = owned.path().join("known-new-quarantine.bin");
        fs::copy(&moved, &quarantine).unwrap();
        assert_eq!(fs::read(&quarantine).unwrap(), bytes);
        fs::remove_file(moved).unwrap();
        rbl_export::recover(&root).unwrap();
        let old_tree: BTreeMap<PathBuf, Vec<u8>> = serde_json::from_slice(
            &fs::read(original.parent().unwrap().join("before.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            tree(&root),
            old_tree,
            "all old output and unknown user data must be restored"
        );
        assert!(rbl_export::verify(&root).unwrap().is_ok());
        let restored: Manifest =
            serde_json::from_slice(&fs::read(Manifest::path(&root)).unwrap()).unwrap();
        assert!(
            rbl_export::incremental::receipts_match(&root, restored.generation.as_ref().unwrap())
                .unwrap(),
            "old output size/mtime receipts must match after restoration"
        );
        assert_eq!(
            fs::read(quarantine).unwrap(),
            bytes,
            "known replacement retained outside restored volume"
        );
        assert_eq!(tree(&original), original_tree);
        assert_eq!(stamps(&original), original_stamps);
        record(
            &format!("{mode}-quarantine-feasibility"),
            &before,
            &root,
            "verified old tree exactly restored; known-new image retained in separate owned quarantine",
        );
    }
}
#[test]
fn missing_or_mismatched_required_authority_refuses_unchanged() {
    for mode in ["PIONEER", ".PIONEER"] {
        for damage in [
            "manifest",
            "database",
            "missing-dat",
            "changed-dat",
            "audio",
            "replacement",
        ] {
            let original = image(mode, false);
            let original_tree = tree(&original);
            let original_stamps = stamps(&original);
            let (_owned, root) = copy_owned(&original);
            let old = authority(&root).unwrap();
            match damage {
                "manifest" => {
                    fs::remove_file(Manifest::path(&root.join(JOURNAL).join(".previous"))).unwrap()
                }
                "database" => {
                    let mut old = old.clone();
                    *old.generation
                        .as_mut()
                        .unwrap()
                        .databases
                        .values_mut()
                        .find_map(Option::as_mut)
                        .unwrap() = "0".repeat(64);
                    old.save_at(&root.join(JOURNAL).join(".previous"), mode)
                        .unwrap();
                }
                "missing-dat" | "changed-dat" => {
                    let name = rel(&format!("{}/ANLZ0000.DAT", old.tracks[0].anlz_dir));
                    for base in [&root, &root.join(JOURNAL).join(".previous")] {
                        let path = base.join(&name);
                        if path.is_file() {
                            if damage == "missing-dat" {
                                fs::remove_file(path).unwrap();
                            } else {
                                put(&path, b"changed generated analysis");
                            }
                        }
                    }
                }
                "audio" => {
                    for base in [&root, &root.join(JOURNAL).join(".previous")] {
                        let path = base.join(rel(&old.tracks[0].audio));
                        if path.is_file() {
                            put(&path, b"unknown old audio replacement");
                        }
                    }
                }
                "replacement" => {
                    let moves: BTreeMap<String, String> = serde_json::from_slice(
                        &fs::read(root.join(JOURNAL).join(".moves.json")).unwrap(),
                    )
                    .unwrap();
                    put(
                        &root.join(rel(moves.keys().next().unwrap())),
                        b"unknown occupied relocation target",
                    );
                }
                _ => unreachable!(),
            }
            assert!(authority(&root).is_err(), "{damage}");
            let before = tree(&root);
            let result = rbl_export::recover(&root);
            record(
                &format!("{mode}-{damage}"),
                &before,
                &root,
                &format!("{result:?}"),
            );
            assert!(result.is_err());
            assert_eq!(tree(&root), before);
            assert_eq!(tree(&original), original_tree);
            assert_eq!(stamps(&original), original_stamps);
        }
    }
}
#[test]
fn affected_artwork_without_byte_receipt_is_not_recovery_authority() {
    for mode in ["PIONEER", ".PIONEER"] {
        let original = image(mode, true);
        let before_original = tree(&original);
        let (_owned, root) = copy_owned(&original);
        assert_eq!(
            authority(&root).unwrap_err(),
            "affected artwork has no output-byte authority"
        );
        let before = tree(&root);
        let result = rbl_export::recover(&root);
        record(
            &format!("{mode}-artwork"),
            &before,
            &root,
            &format!("{result:?}"),
        );
        assert!(result.is_err());
        assert_eq!(tree(&root), before);
        assert_eq!(tree(&original), before_original);
    }
}

#[test]
fn untouched_artwork_keeps_existing_receipt_authority_and_bytes() {
    for mode in ["PIONEER", ".PIONEER"] {
        let original = fixtures::image_kind(mode, true, true);
        let original_tree = tree(&original);
        let original_stamps = stamps(&original);
        let (_owned, root) = copy_owned(&original);
        rbl_export::recover(&root).unwrap();
        assert_eq!(tree(&root), fixtures::before_kind(mode, true, true));
        assert_eq!(
            stamps(&root),
            fixtures::before_stamps_kind(mode, true, true)
        );
        assert!(rbl_export::verify(&root).unwrap().is_ok());
        assert!(!rbl_export::recovery_pending(&root));
        assert_eq!(tree(&original), original_tree);
        assert_eq!(stamps(&original), original_stamps);
    }
}
