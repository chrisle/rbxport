//! Regression coverage for adopting USB playlist identities without losing folders or playlists.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use rbl_export::sync_record::SyncNode;
use rbl_export::{SourcePlaylist, SyncSource};
use std::collections::BTreeMap;

fn selection() -> (Vec<SourcePlaylist>, SyncSource) {
    let playlists = vec![
        SourcePlaylist {
            id: 10,
            name: "CHANGEOVER DRONE".into(),
            ..Default::default()
        },
        SourcePlaylist {
            id: 20,
            name: "2026 SHOWS".into(),
            folder: true,
            ..Default::default()
        },
        SourcePlaylist {
            id: 30,
            parent_id: 20,
            name: "Radius Chicago".into(),
            ..Default::default()
        },
        SourcePlaylist {
            id: 40,
            parent_id: 20,
            name: "Doof Manchester".into(),
            ..Default::default()
        },
    ];
    let sync = SyncSource {
        db_id: 217,
        tree: playlists
            .iter()
            .map(|p| SyncNode {
                id: p.id,
                parent: p.parent_id,
                attribute: u8::from(p.folder),
            })
            .collect(),
        automatic: false,
        cloud_db_id: 0,
    };
    (playlists, sync)
}

#[test]
fn first_sync_preserves_unchecked_foreign_playlists_on_repeat_syncs() {
    let dest = tempfile::tempdir().unwrap();
    let (playlists, sync) = selection();
    rbl_export::export_full(
        dest.path(),
        &[],
        &playlists,
        &[],
        None,
        Some(&sync),
        &mut |_| {},
    )
    .unwrap();
    // A valid device and sync record, with no rbxport baseline, as on first adoption.
    std::fs::remove_file(rbl_export::Manifest::path(dest.path())).unwrap();
    let before = rbl_export::snapshot::Snapshot::read(dest.path()).unwrap();
    rbl_export::export_full(
        dest.path(),
        &[],
        &playlists[1..],
        &[],
        None,
        Some(&sync),
        &mut |_| {},
    )
    .unwrap();
    assert_contents(
        &before,
        &rbl_export::snapshot::Snapshot::read(dest.path()).unwrap(),
    );
    rbl_export::export_full(
        dest.path(),
        &[],
        &playlists[1..],
        &[],
        None,
        Some(&sync),
        &mut |_| {},
    )
    .unwrap();
    assert_contents(
        &before,
        &rbl_export::snapshot::Snapshot::read(dest.path()).unwrap(),
    );
}

#[test]
fn valid_mapping_with_every_playlist_selected_syncs_without_a_baseline() {
    let dest = tempfile::tempdir().unwrap();
    let (playlists, sync) = selection();
    rbl_export::export_full(
        dest.path(),
        &[],
        &playlists,
        &[],
        None,
        Some(&sync),
        &mut |_| {},
    )
    .unwrap();
    std::fs::remove_file(rbl_export::Manifest::path(dest.path())).unwrap();
    rbl_export::export_full(
        dest.path(),
        &[],
        &playlists,
        &[],
        None,
        Some(&sync),
        &mut |_| {},
    )
    .unwrap();
}

#[test]
fn without_the_ownership_record_the_unchecked_playlist_is_preserved() {
    let dest = tempfile::tempdir().unwrap();
    let (playlists, sync) = selection();
    rbl_export::export_full(
        dest.path(),
        &[],
        &playlists,
        &[],
        None,
        Some(&sync),
        &mut |_| {},
    )
    .unwrap();
    std::fs::remove_file(rbl_export::Manifest::path(dest.path())).unwrap();
    for file in rbl_export::sync_record::FILES {
        std::fs::remove_file(dest.path().join(file)).unwrap();
    }
    rbl_export::export_full(
        dest.path(),
        &[],
        &playlists[1..],
        &[],
        None,
        Some(&sync),
        &mut |_| {},
    )
    .unwrap();
    let after = rbl_export::snapshot::Snapshot::read(dest.path()).unwrap();
    assert!(after
        .legacy
        .unwrap()
        .playlists
        .iter()
        .any(|p| p.name == "CHANGEOVER DRONE"));
    assert!(after
        .one
        .unwrap()
        .playlists
        .iter()
        .any(|p| p.name == "CHANGEOVER DRONE"));
}

#[test]
fn invalid_kind_mappings_do_not_discard_the_usb_folder_tree() {
    let dest = tempfile::tempdir().unwrap();
    let (playlists, sync) = selection();
    rbl_export::export_full(
        dest.path(),
        &[],
        &playlists,
        &[],
        None,
        Some(&sync),
        &mut |_| {},
    )
    .unwrap();
    std::fs::remove_file(rbl_export::Manifest::path(dest.path())).unwrap();
    // Deliberately inconsistent mapping: local Doof now claims the device folder;
    // the local folder claims CHANGEOVER. Radius has no mapping and is retained.
    let ids = BTreeMap::from([(20, 1), (40, 2)]);
    let bytes = rbl_export::sync_record::render_with_ids(&sync, &[40], 1, &BTreeMap::new(), &ids);
    for file in rbl_export::sync_record::FILES {
        std::fs::write(dest.path().join(file), &bytes).unwrap();
    }
    let before = rbl_export::snapshot::Snapshot::read(dest.path()).unwrap();
    let chosen = vec![playlists[1].clone(), playlists[3].clone()];
    rbl_export::export_full(
        dest.path(),
        &[],
        &chosen,
        &[],
        None,
        Some(&sync),
        &mut |_| {},
    )
    .unwrap();
    let after = rbl_export::snapshot::Snapshot::read(dest.path()).unwrap();
    for old in semantic(before.one.as_ref().unwrap()) {
        assert!(semantic(after.one.as_ref().unwrap()).contains(&old));
        assert!(semantic(after.legacy.as_ref().unwrap()).contains(&old));
    }
}

#[test]
fn differing_ids_preserve_folders_membership_and_unchecked_playlists() {
    for (select_all, nested) in [(false, false), (true, false), (false, true), (true, true)] {
        let dest = tempfile::tempdir().unwrap();
        let (mut playlists, mut sync) = selection();
        if nested {
            playlists.push(SourcePlaylist {
                id: 50,
                name: "October".into(),
                folder: true,
                parent_id: 20,
                ..Default::default()
            });
            playlists[2].parent_id = 50;
            sync.tree.iter_mut().find(|n| n.id == 30).unwrap().parent = 50;
            sync.tree.push(SyncNode {
                id: 50,
                parent: 20,
                attribute: 1,
            });
        }
        let source = tempfile::tempdir().unwrap();
        let tracks: Vec<rbl_export::SourceTrack> = (1..=2)
            .map(|id| {
                let path = source.path().join(format!("track-{id}.mp3"));
                std::fs::write(&path, vec![1_u8; 2048]).unwrap();
                rbl_export::SourceTrack {
                    id,
                    source_path: path,
                    title: format!("Track {id}"),
                    analysis: vec![(
                        "DAT".into(),
                        rbl_anlz::AnlzBuilder::new().path("/x.mp3").finish(),
                    )],
                    ..Default::default()
                }
            })
            .collect();
        playlists[0].track_indices = vec![1, 0];
        playlists[2].track_indices = vec![0, 1, 0];
        playlists[3].track_indices = vec![1];
        rbl_export::export_full(
            dest.path(),
            &tracks,
            &playlists,
            &[],
            None,
            Some(&sync),
            &mut |_| {},
        )
        .unwrap();
        std::fs::remove_file(rbl_export::Manifest::path(dest.path())).unwrap();
        // Both databases describe the same tree. Only their numeric IDs differ:
        // legacy: CHANGEOVER=1, SHOWS=2, Radius=3, Doof=4
        // Plus:   CHANGEOVER=2, SHOWS=3, Radius=5, Doof=4
        let conn = rbl_onelibrary::playlists::open_read_write(
            &dest.path().join("PIONEER/rekordbox/exportLibrary.db"),
        )
        .unwrap();
        conn.execute_batch("UPDATE playlist SET playlist_id = playlist_id + 100;
        UPDATE playlist SET playlist_id = CASE playlist_id WHEN 101 THEN 2 WHEN 102 THEN 3 WHEN 103 THEN 5 WHEN 104 THEN 4 WHEN 105 THEN 6 END;
        UPDATE playlist SET playlist_id_parent = CASE playlist_id_parent WHEN 2 THEN 3 WHEN 5 THEN 6 ELSE playlist_id_parent END;
        UPDATE playlist_content SET playlist_id = CASE playlist_id WHEN 1 THEN 2 WHEN 2 THEN 3 WHEN 3 THEN 5 WHEN 4 THEN 4 END;").unwrap();
        drop(conn);
        // Each sync file is valid for its corresponding database.
        let plus_ids = BTreeMap::from([(10, 2), (20, 3), (30, 5), (40, 4), (50, 6)]);
        let plus = rbl_export::sync_record::render_with_ids(
            &sync,
            &[10, 30, 40],
            1,
            &BTreeMap::new(),
            &plus_ids,
        );
        std::fs::write(dest.path().join(rbl_export::sync_record::FILES[1]), plus).unwrap();
        let before = rbl_export::snapshot::Snapshot::read(dest.path()).unwrap();
        // Confirm each record resolves every local playlist to the correct row,
        // and every child's parent to the correct folder in its own database.
        for (file, library) in rbl_export::sync_record::FILES.iter().zip([
            before.legacy.as_ref().unwrap(),
            before.one.as_ref().unwrap(),
        ]) {
            let record =
                rbl_export::sync_record::parse(&std::fs::read(dest.path().join(file)).unwrap())
                    .unwrap();
            for source in &playlists {
                let node = library
                    .playlists
                    .iter()
                    .find(|p| p.id == record.device_ids[&source.id])
                    .unwrap();
                assert_eq!((&node.name, node.folder), (&source.name, source.folder));
                assert_eq!(
                    node.parent,
                    record
                        .device_ids
                        .get(&source.parent_id)
                        .copied()
                        .unwrap_or(0)
                );
            }
        }
        let chosen = if select_all {
            playlists.clone()
        } else {
            vec![playlists[1].clone(), playlists[3].clone()]
        };
        let plan = rbl_export::plan::preview(
            dest.path(),
            &tracks,
            &chosen,
            &[],
            &rbl_export::ExportOptions {
                sync: Some(&sync),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(plan.playlists.len(), playlists.len());
        for _ in 0..2 {
            rbl_export::export_full(
                dest.path(),
                &tracks,
                &chosen,
                &[],
                None,
                Some(&sync),
                &mut |_| {},
            )
            .unwrap();
            assert_contents(
                &before,
                &rbl_export::snapshot::Snapshot::read(dest.path()).unwrap(),
            );
            let record = rbl_export::sync_record::read(dest.path()).unwrap();
            assert_eq!(record.ticked.contains(&10), select_all);
            assert_eq!(record.ticked.contains(&30), select_all);
        }
        // A later selection adopts the previously carried playlists without duplicates.
        rbl_export::export_full(
            dest.path(),
            &tracks,
            &playlists,
            &[],
            None,
            Some(&sync),
            &mut |_| {},
        )
        .unwrap();
        assert_contents(
            &before,
            &rbl_export::snapshot::Snapshot::read(dest.path()).unwrap(),
        );
    }
}

#[test]
fn renaming_a_folder_and_moving_a_playlist_with_valid_ids_succeeds() {
    let dest = tempfile::tempdir().unwrap();
    let (mut playlists, mut sync) = selection();
    rbl_export::export_full(
        dest.path(),
        &[],
        &playlists,
        &[],
        None,
        Some(&sync),
        &mut |_| {},
    )
    .unwrap();
    std::fs::remove_file(rbl_export::Manifest::path(dest.path())).unwrap();
    playlists[1].name = "Renamed shows".into();
    playlists[2].parent_id = 0;
    sync.tree.iter_mut().find(|n| n.id == 30).unwrap().parent = 0;
    rbl_export::export_full(
        dest.path(),
        &[],
        &playlists,
        &[],
        None,
        Some(&sync),
        &mut |_| {},
    )
    .unwrap();
    let after = rbl_export::snapshot::Snapshot::read(dest.path()).unwrap();
    for library in [after.legacy.unwrap(), after.one.unwrap()] {
        assert!(library
            .playlists
            .iter()
            .any(|p| p.name == "Renamed shows" && p.folder));
        assert!(library
            .playlists
            .iter()
            .any(|p| p.name == "Radius Chicago" && p.parent == 0));
    }
}

fn semantic(library: &rbl_export::snapshot::Library) -> Vec<(String, String, bool, Vec<u32>)> {
    let mut result: Vec<_> = library
        .playlists
        .iter()
        .map(|p| {
            let parent = if p.parent == 0 {
                String::new()
            } else {
                library
                    .playlists
                    .iter()
                    .find(|n| n.id == p.parent && n.folder)
                    .expect("parent must exist and be a folder")
                    .name
                    .clone()
            };
            (p.name.clone(), parent, p.folder, p.tracks.clone())
        })
        .collect();
    result.sort();
    result
}

fn assert_contents(
    before: &rbl_export::snapshot::Snapshot,
    after: &rbl_export::snapshot::Snapshot,
) {
    let expected = semantic(before.legacy.as_ref().unwrap());
    assert_eq!(semantic(after.legacy.as_ref().unwrap()), expected);
    assert_eq!(semantic(after.one.as_ref().unwrap()), expected);
}
