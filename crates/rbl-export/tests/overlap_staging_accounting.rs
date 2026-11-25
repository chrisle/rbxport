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
fn ordinary_overlapping_relocations_need_more_stage_bytes_than_the_current_plan() {
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
        let tracks = [a, b];
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
        assert!(staged > plan.peak_bytes);
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
    }
}
