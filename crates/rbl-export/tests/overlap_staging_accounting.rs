#![allow(
    clippy::pedantic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stdout
)]
use rbl_export::{ExportOptions, SourcePlaylist, SourceTrack, SyncSource};
use std::{fs, path::Path};
fn tree_bytes(root: &Path) -> u64 {
    fs::read_dir(root)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            if e.file_type().unwrap().is_dir() {
                tree_bytes(&e.path())
            } else {
                e.metadata().unwrap().len()
            }
        })
        .sum()
}
#[test]
fn ordinary_overlapping_relocations_are_included_in_temporary_staging_space() {
    let temporary = tempfile::tempdir().unwrap();
    let base = temporary.path();
    for (old, new) in [("A", "Z"), ("Z", "A")] {
        let root = base.join(format!("stick-{old}-{new}"));
        let src = base.join(format!("sources-{old}-{new}"));
        fs::create_dir_all(&src).unwrap();
        let make = |id: u64, artist: &str, size: usize| {
            let dir = src.join(format!("source{id}"));
            fs::create_dir_all(&dir).unwrap();
            let file = dir.join("same.mp3");
            fs::write(&file, vec![id as u8; size]).unwrap();
            SourceTrack {
                id,
                source_path: file,
                title: format!("Generated{id}"),
                artist: artist.into(),
                album: "Album".into(),
                analysis: vec![(
                    "DAT".into(),
                    rbl_anlz::AnlzBuilder::new().path("/generated.mp3").finish(),
                )],
                ..Default::default()
            }
        };
        let mut a = make(1, old, 4 * 1024 * 1024);
        let source = SyncSource {
            db_id: 123,
            tree: vec![rbl_export::SyncNode {
                id: 10,
                parent: 0,
                attribute: 0,
            }],
            automatic: false,
            cloud_db_id: 0,
        };
        let options = ExportOptions {
            sync: Some(&source),
            ..Default::default()
        };
        let playlist = |ids| SourcePlaylist {
            id: 10,
            name: "Generated".into(),
            track_indices: ids,
            ..Default::default()
        };
        rbl_export::export_with_options(
            &root,
            &[a.clone()],
            &[playlist(vec![0])],
            &[],
            &options,
            &mut |_| {},
        )
        .unwrap();
        a.artist = new.into();
        let b = make(2, old, 128);
        let mut tracks = [a, b];
        let lists = [playlist(vec![0, 1])];
        let plan = rbl_export::plan::preview(&root, &tracks, &lists, &[], &options).unwrap();
        let mut staged = 0;
        let report =
            rbl_export::export_with_options(&root, &tracks, &lists, &[], &options, &mut |p| {
                if p.stage == "publishing" {
                    staged = fs::read_dir(&root)
                        .unwrap()
                        .map(Result::unwrap)
                        .filter(|e| {
                            e.file_name()
                                .to_string_lossy()
                                .starts_with(".rbxport-staging-")
                        })
                        .map(|e| tree_bytes(&e.path()))
                        .sum();
                }
            })
            .unwrap();
        println!("order={old}-{new} planned_copy={} planned_reuse={} planned_peak={} unknown={} actual_copy={} stage_raw={} verified={}",plan.estimate.copy_bytes,plan.estimate.reuse_bytes,plan.peak_bytes,plan.unknown_size,report.bytes_copied,staged,report.verified);
        assert_eq!(plan.estimate.copy_bytes, 128);
        assert!(!plan.unknown_size);
        assert_eq!(report.bytes_copied, 4 * 1024 * 1024 + 128);
        assert!(plan.peak_bytes >= staged);
        assert!(plan.overlap_bytes >= 4 * 1024 * 1024);
        assert!(
            plan.net_additional_bytes < 4 * 1024 * 1024,
            "a temporary relocation image is not permanent audio growth"
        );
        assert!(report.verified);
        for format in [
            rbl_export::device_library::Format::DeviceLibrary,
            rbl_export::device_library::Format::OneLibrary,
        ] {
            let lib = rbl_export::device_library::read(&root, format).unwrap();
            assert_eq!(lib.tracks.len(), 2);
            for (id, artist, size) in [(1, new, 4 * 1024 * 1024), (2, old, 128)] {
                let row = lib
                    .tracks
                    .iter()
                    .find(|t| t.master_content_id == id)
                    .unwrap();
                assert_eq!(row.path, format!("/Contents/{artist}/Album/same.mp3"));
                assert_eq!(
                    fs::read(root.join(row.path.trim_start_matches('/'))).unwrap(),
                    vec![id as u8; size]
                );
            }
        }
        let repeat =
            rbl_export::export_with_options(&root, &tracks, &lists, &[], &options, &mut |_| {})
                .unwrap();
        assert!(repeat.up_to_date);
        assert_eq!(repeat.bytes_copied, 0);
        println!("repeat=true copy=0 both_db_paths_and_bytes=true");
        let no_op = rbl_export::plan::preview(&root, &tracks, &lists, &[], &options).unwrap();
        assert_eq!((no_op.peak_bytes, no_op.overlap_bytes), (0, 0));
        tracks[0].artist = "Disjoint".into();
        let disjoint = rbl_export::plan::preview(&root, &tracks, &lists, &[], &options).unwrap();
        assert_eq!(disjoint.overlap_bytes, 0);
        assert_eq!(disjoint.estimate.copy_bytes, 0);
        let moved =
            rbl_export::export_with_options(&root, &tracks, &lists, &[], &options, &mut |_| {})
                .unwrap();
        assert!(moved.verified);
        assert_eq!(moved.bytes_copied, 0);
        assert_eq!(
            fs::read(root.join("Contents/Disjoint/Album/same.mp3")).unwrap(),
            vec![1; 4 * 1024 * 1024]
        );
    }
}

#[test]
fn converted_relocations_reserve_the_device_image_not_the_source_size() {
    for (old, new) in [("A", "Z"), ("Z", "A")] {
        let temporary = tempfile::tempdir().unwrap();
        let source = temporary.path().join("sources");
        let root = temporary.path().join("device");
        fs::create_dir(&source).unwrap();
        // A valid low-rate mono source expands when converted to CD-rate
        // stereo AIFF; every filesystem name is safe on Windows as well.
        let compressed = source.join("song.wav");
        let mut wav = Vec::new();
        wav.extend(b"RIFF");
        wav.extend(16036_u32.to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16_u32.to_le_bytes());
        wav.extend(1_u16.to_le_bytes());
        wav.extend(1_u16.to_le_bytes());
        wav.extend(8000_u32.to_le_bytes());
        wav.extend(16000_u32.to_le_bytes());
        wav.extend(2_u16.to_le_bytes());
        wav.extend(16_u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(16000_u32.to_le_bytes());
        wav.resize(16044, 0);
        fs::write(&compressed, wav).unwrap();
        let mut original = SourceTrack {
            id: 1,
            source_path: compressed.clone(),
            title: "Converted".into(),
            artist: old.into(),
            album: "Album".into(),
            duration_sec: 1,
            sample_rate: 8000,
            analysis: vec![(
                "DAT".into(),
                rbl_anlz::AnlzBuilder::new()
                    .path("/generated.flac")
                    .finish(),
            )],
            ..Default::default()
        };
        let sync = SyncSource {
            db_id: 123,
            ..Default::default()
        };
        let options = ExportOptions {
            sync: Some(&sync),
            compatibility: Some(rbl_export::CompatibilityFormat::Aiff),
            ..Default::default()
        };
        let list = |indices| SourcePlaylist {
            id: 10,
            name: "Generated".into(),
            track_indices: indices,
            ..Default::default()
        };
        rbl_export::export_with_options(
            &root,
            &[original.clone()],
            &[list(vec![0])],
            &[],
            &options,
            &mut |_| {},
        )
        .unwrap();
        let before = rbl_export::Manifest::load(&root).unwrap();
        let device_copy = &before.tracks[0];
        let copied = fs::read(root.join(device_copy.audio.trim_start_matches('/'))).unwrap();
        assert!(copied.len() as u64 > fs::metadata(&compressed).unwrap().len());
        let replacement = source.join(Path::new(&device_copy.audio).file_name().unwrap());
        fs::write(&replacement, &copied).unwrap();
        assert!(!rbl_audio::compatibility::needs_conversion(&replacement).unwrap());
        original.artist = new.into();
        let other = SourceTrack {
            id: 2,
            source_path: replacement,
            title: "Unconverted".into(),
            artist: old.into(),
            album: "Album".into(),
            duration_sec: 2,
            sample_rate: 44_100,
            analysis: original.analysis.clone(),
            ..Default::default()
        };
        let tracks = [original, other];
        let lists = [list(vec![0, 1])];
        let plan = rbl_export::plan::preview(&root, &tracks, &lists, &[], &options).unwrap();
        assert!(
            plan.overlap_bytes >= copied.len() as u64,
            "reserve {} must cover the {}-byte converted device image, not {}-byte original source",
            plan.overlap_bytes,
            copied.len(),
            device_copy.size
        );
        let report =
            rbl_export::export_with_options(&root, &tracks, &lists, &[], &options, &mut |_| {})
                .unwrap();
        assert!(report.verified);
        assert_eq!(report.bytes_copied, 2 * copied.len() as u64);
        for format in [
            rbl_export::device_library::Format::DeviceLibrary,
            rbl_export::device_library::Format::OneLibrary,
        ] {
            let library = rbl_export::device_library::read(&root, format).unwrap();
            assert_eq!(library.tracks.len(), 2);
            for track in &library.tracks {
                assert_eq!(
                    fs::read(root.join(track.path.trim_start_matches('/'))).unwrap(),
                    copied
                );
            }
        }
        assert!(
            rbl_export::export_with_options(&root, &tracks, &lists, &[], &options, &mut |_| {})
                .unwrap()
                .up_to_date
        );
    }
}

#[test]
fn upfront_preparation_checks_cancellation_and_relocation_stamp_before_writing() {
    for cancel in [false, true] {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("device");
        let mut tracks = Vec::new();
        for (id, title) in [(1, "Original"), (2, "Replacement")] {
            let source = temporary.path().join(format!("source-{id}"));
            fs::create_dir(&source).unwrap();
            let path = source.join("same.mp3");
            fs::write(&path, vec![id as u8; 8192]).unwrap();
            tracks.push(SourceTrack {
                id,
                source_path: path,
                title: title.into(),
                artist: "A".into(),
                album: "Album".into(),
                analysis: vec![(
                    "DAT".into(),
                    rbl_anlz::AnlzBuilder::new().path("/generated.mp3").finish(),
                )],
                ..Default::default()
            });
        }
        let sync = SyncSource {
            db_id: 123,
            ..Default::default()
        };
        let options = ExportOptions {
            sync: Some(&sync),
            ..Default::default()
        };
        let list = |indices| SourcePlaylist {
            id: 10,
            name: "Generated".into(),
            track_indices: indices,
            ..Default::default()
        };
        rbl_export::export_with_options(
            &root,
            &tracks[..1],
            &[list(vec![0])],
            &[],
            &options,
            &mut |_| {},
        )
        .unwrap();
        let database_paths = [
            "PIONEER/rekordbox/export.pdb",
            "PIONEER/rekordbox/exportLibrary.db",
            "PIONEER/rbxport/manifest.json",
        ];
        let before: Vec<_> = database_paths
            .iter()
            .map(|p| fs::read(root.join(p)).unwrap())
            .collect();
        tracks[0].artist = "Z".into();
        let acted = std::cell::Cell::new(false);
        let cancelled = std::cell::Cell::new(false);
        let result = rbl_export::export_cancellable(
            &root,
            &tracks,
            &[list(vec![0, 1])],
            &[],
            &options,
            &mut |event| {
                if event.stage == "planning" && event.title == "Replacement" && !acted.replace(true)
                {
                    if cancel {
                        cancelled.set(true);
                    } else {
                        fs::write(root.join("Contents/A/Album/same.mp3"), b"external writer")
                            .unwrap();
                    }
                }
            },
            &|| cancelled.get(),
        );
        assert!(
            acted.get(),
            "upfront work reports progress before the audio loop"
        );
        let error = result.unwrap_err();
        if cancel {
            assert!(matches!(error, rbl_export::ExportError::Cancelled));
        } else {
            assert!(
                error
                    .to_string()
                    .contains("Exported media changed during sync"),
                "{error}"
            );
        }
        for (path, bytes) in database_paths.iter().zip(before) {
            assert_eq!(fs::read(root.join(path)).unwrap(), bytes);
        }
        assert!(!root.join("Contents/Z/Album/same.mp3").exists());
        if cancel {
            assert_eq!(
                fs::read(root.join("Contents/A/Album/same.mp3")).unwrap(),
                vec![1; 8192]
            );
        } else {
            assert_eq!(
                fs::read(root.join("Contents/A/Album/same.mp3")).unwrap(),
                b"external writer"
            );
        }
        assert_eq!(fs::read(&tracks[0].source_path).unwrap(), vec![1; 8192]);
    }
}

#[test]
fn an_untrusted_copy_inspected_upfront_cannot_change_before_publication() {
    for (erase_copy_receipt, rename) in [(false, false), (true, false), (false, true), (true, true)]
    {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("device");
        let source = temporary.path().join("source.mp3");
        fs::write(&source, vec![1; 8192]).unwrap();
        let mut tracks = [SourceTrack {
            id: 1,
            source_path: source.clone(),
            title: "Original".into(),
            artist: "A".into(),
            album: "Album".into(),
            analysis: vec![(
                "DAT".into(),
                rbl_anlz::AnlzBuilder::new().path("/generated.mp3").finish(),
            )],
            ..Default::default()
        }];
        let sync = SyncSource {
            db_id: 123,
            ..Default::default()
        };
        let options = ExportOptions {
            sync: Some(&sync),
            ..Default::default()
        };
        let mut lists = [SourcePlaylist {
            id: 10,
            name: "Generated".into(),
            track_indices: vec![0],
            ..Default::default()
        }];
        rbl_export::export_with_options(&root, &tracks, &lists, &[], &options, &mut |_| {})
            .unwrap();
        let mut legacy = rbl_export::Manifest::load(&root).unwrap();
        legacy.generation = None;
        if erase_copy_receipt {
            legacy.tracks[0].copy_stamp = None;
            legacy.tracks[0].audio_hash = 0;
        }
        legacy.save(&root).unwrap();
        let device_audio = root.join(legacy.tracks[0].audio.trim_start_matches('/'));
        let before = fs::read(root.join("PIONEER/rekordbox/export.pdb")).unwrap();
        let acted = std::cell::Cell::new(false);
        lists[0].name = "Renamed".into();
        if rename {
            tracks[0].artist = "Z".into();
        }
        let result =
            rbl_export::export_with_options(&root, &tracks, &lists, &[], &options, &mut |event| {
                if event.stage == "verifying" && !acted.replace(true) {
                    fs::write(&device_audio, vec![9; 8192]).unwrap();
                    fs::OpenOptions::new()
                        .write(true)
                        .open(&device_audio)
                        .unwrap()
                        .set_modified(
                            std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_800_000_000),
                        )
                        .unwrap();
                }
            });
        assert!(acted.get());
        if erase_copy_receipt && rename {
            // No receipt can authorize relocation. The executor instead
            // copied the valid source into its new path; the old externally
            // changed file is not accepted as input and must remain alone.
            let report = result.unwrap();
            assert!(report.verified);
            assert_eq!(report.bytes_copied, 8192);
            assert_eq!(fs::read(&device_audio).unwrap(), vec![9; 8192]);
            assert_eq!(
                fs::read(root.join("Contents/Z/Album/source.mp3")).unwrap(),
                vec![1; 8192]
            );
            assert_eq!(fs::read(&source).unwrap(), vec![1; 8192]);
            continue;
        }
        let error = result.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Exported media changed during sync"),
            "{error}"
        );
        assert_eq!(
            fs::read(root.join("PIONEER/rekordbox/export.pdb")).unwrap(),
            before
        );
        assert_eq!(fs::read(device_audio).unwrap(), vec![9; 8192]);
        assert_eq!(fs::read(&source).unwrap(), vec![1; 8192]);
        let retried =
            rbl_export::export_with_options(&root, &tracks, &lists, &[], &options, &mut |_| {})
                .unwrap();
        assert!(retried.verified);
        assert_eq!(retried.bytes_copied, 8192);
        assert_eq!(
            fs::read(
                root.join(
                    rbl_export::Manifest::load(&root).unwrap().tracks[0]
                        .audio
                        .trim_start_matches('/')
                )
            )
            .unwrap(),
            vec![1; 8192]
        );
    }
}
