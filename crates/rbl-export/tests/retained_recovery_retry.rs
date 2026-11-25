//! Exercise the normal export entry point on a retained journal emitted by v1.9.
#![cfg(target_os = "linux")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};

use rbl_export::manifest::ManifestTrack;
use rbl_export::{ExportOptions, ExportRoot, Manifest, SourcePlaylist, SourceTrack, SyncSource};

#[allow(dead_code)]
#[path = "common/legacy_overlap.rs"]
mod fixtures;
use fixtures::{before_tree, image, put, rel, stamps, tree};

fn collection_track(
    collection: &Path,
    old_track: &ManifestTrack,
    old_device_source: &SourceTrack,
    previous_files: &BTreeMap<PathBuf, Vec<u8>>,
) -> SourceTrack {
    let source_path = collection.join("song.mp3");
    put(&source_path, &previous_files[&rel(&old_track.audio)]);
    let source_modified =
        UNIX_EPOCH + Duration::from_nanos(u64::try_from(old_track.modified).unwrap());
    fs::File::options()
        .write(true)
        .open(&source_path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(source_modified))
        .unwrap();
    let primary_analysis = rel(&format!("{}/ANLZ0000.DAT", old_track.anlz_dir));
    SourceTrack {
        id: old_track.library_id,
        source_path,
        title: old_device_source.title.clone(),
        artist: old_device_source.artist.clone(),
        album: old_device_source.album.clone(),
        analysis: vec![("DAT".into(), previous_files[&primary_analysis].clone())],
        ..Default::default()
    }
}

#[test]
fn ordinary_sync_retry_recovers_and_reuses_published_audio() {
    for (name, mode) in [
        ("PIONEER", ExportRoot::Standard),
        (".PIONEER", ExportRoot::Hidden),
    ] {
        let device = image(name, false);
        let previous_files = before_tree(name, false);
        let manifest_path = PathBuf::from(name).join("rbxport/manifest.json");
        let previous: Manifest = serde_json::from_slice(&previous_files[&manifest_path]).unwrap();
        let old_track = &previous.tracks[0];
        let old_device_source =
            &previous.generation.as_ref().unwrap().device_sources[&old_track.export_id];
        let old_audio = rel(&old_track.audio);

        let collection = tempfile::tempdir().unwrap();
        let track = collection_track(
            collection.path(),
            old_track,
            old_device_source,
            &previous_files,
        );
        let playlists = [SourcePlaylist {
            id: previous.playlists[0].library_id,
            name: previous.playlists[0].name.clone(),
            track_indices: vec![0],
            ..Default::default()
        }];
        let sync = SyncSource {
            db_id: previous.db_id,
            ..Default::default()
        };
        let options = ExportOptions {
            root: Some(mode),
            sync: Some(&sync),
            ..Default::default()
        };

        assert!(rbl_export::recovery_pending(&device));
        let retained = tree(&device);
        let retained_stamps = stamps(&device);
        let preview = rbl_export::plan::preview(
            &device,
            std::slice::from_ref(&track),
            &playlists,
            &[],
            &options,
        )
        .expect_err("preview should ask for explicit recovery");
        assert!(preview.to_string().contains("Sync it to recover"));
        assert_eq!(tree(&device), retained);
        assert_eq!(stamps(&device), retained_stamps);

        let sync_report = rbl_export::export_with_options(
            &device,
            std::slice::from_ref(&track),
            &playlists,
            &[],
            &options,
            &mut |_| {},
        )
        .unwrap();
        assert!(sync_report.verified);
        assert_eq!(sync_report.bytes_copied, 0);
        assert!(!rbl_export::recovery_pending(&device));
        assert_eq!(
            fs::read(device.join(&old_audio)).unwrap(),
            previous_files[&old_audio]
        );
        assert_eq!(
            fs::read(device.join("unknown/keep.bin")).unwrap(),
            previous_files[&PathBuf::from("unknown/keep.bin")]
        );

        let published = tree(&device);
        let published_stamps = stamps(&device);
        let repeat_report = rbl_export::export_with_options(
            &device,
            &[track],
            &playlists,
            &[],
            &options,
            &mut |_| {},
        )
        .unwrap();
        assert!(repeat_report.verified && repeat_report.up_to_date);
        assert_eq!(repeat_report.bytes_copied, 0);
        assert_eq!(tree(&device), published);
        assert_eq!(stamps(&device), published_stamps);
    }
}
