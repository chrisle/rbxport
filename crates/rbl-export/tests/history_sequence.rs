//! Ordinary Device Library snapshots order history by its 16-bit sequence.
#![allow(clippy::unwrap_used)]
use rbl_pdb::build::{device_sql_string, FileBuilder};

#[test]
fn mixed_on_air_flags_do_not_reorder_played_history() {
    let root = tempfile::tempdir().unwrap();
    let mut file = FileBuilder::new(4096);
    let names: Vec<_> = [(7_u32, "First"), (8, "Second")]
        .into_iter()
        .map(|(id, name)| {
            let mut row = id.to_le_bytes().to_vec();
            row.extend_from_slice(&device_sql_string(name));
            row
        })
        .collect();
    file.add_table(11, &names);
    let rows: Vec<_> = [
        (2_u32, 7_u32, 2_u16, 0_u8),
        (1, 7, 1, 1),
        (1, 7, 3, 0),
        (4, 8, 2, 0),
        (3, 8, 1, 1),
    ]
    .into_iter()
    .map(|(track, history, sequence, on_air)| {
        let mut row = track.to_le_bytes().to_vec();
        row.extend_from_slice(&history.to_le_bytes());
        row.extend_from_slice(&sequence.to_le_bytes());
        row.extend_from_slice(&[on_air, 0]);
        row
    })
    .collect();
    file.add_table(12, &rows);
    let bytes = file.finish();
    let directory = root.path().join("PIONEER/rekordbox");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("export.pdb");
    std::fs::write(&path, &bytes).unwrap();
    for _ in 0..2 {
        let snapshot = rbl_export::snapshot::Snapshot::read(root.path()).unwrap();
        for history in [&snapshot.history, &snapshot.legacy_history] {
            assert_eq!(
                history
                    .iter()
                    .map(|h| (h.id, h.name.as_str(), h.tracks.as_slice()))
                    .collect::<Vec<_>>(),
                [
                    (7, "First", [1, 2, 1].as_slice()),
                    (8, "Second", [3, 4].as_slice())
                ]
            );
        }
        assert!(snapshot.one.is_none());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn an_oversized_history_from_the_current_writer_keeps_its_order() {
    let root = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let tracks: Vec<_> = (1_u64..=2)
        .map(|id| {
            let path = source.path().join(format!("generated-{id}.mp3"));
            std::fs::write(&path, [7; 128]).unwrap();
            rbl_export::SourceTrack {
                id,
                source_path: path,
                title: format!("Track {id}"),
                ..Default::default()
            }
        })
        .collect();
    let playlists = [rbl_export::SourcePlaylist {
        id: 1,
        name: "Generated".into(),
        track_indices: vec![0, 1],
        ..Default::default()
    }];
    rbl_export::export(root.path(), &tracks, &playlists).unwrap();
    let mut database = rbl_onelibrary::playlists::open_read_write(
        &root.path().join("PIONEER/rekordbox/exportLibrary.db"),
    )
    .unwrap();
    let transaction = database.transaction().unwrap();
    transaction
        .execute("INSERT INTO history VALUES(7,1,'Oversized',0,0)", [])
        .unwrap();
    {
        let mut insert = transaction
            .prepare("INSERT INTO history_content(history_id,content_id,sequenceNo) VALUES(7,?2,?1)")
            .unwrap();
        for sequence in 1..=65_536 {
            let content = if sequence == 65_536 { 2 } else { 1 };
            insert.execute([sequence, content]).unwrap();
        }
    }
    transaction.commit().unwrap();
    drop(database);
    rbl_export::export(root.path(), &tracks, &playlists).unwrap();
    let path = root.path().join("PIONEER/rekordbox/export.pdb");
    let before = std::fs::read(&path).unwrap();
    let parsed = rbl_pdb::Pdb::parse(&before).unwrap();
    let raw_rows = parsed.rows(parsed.table(rbl_pdb::PageType::Other(12)).unwrap());
    assert_eq!(raw_rows.len(), 65_536);
    let last = raw_rows.last().unwrap().offset;
    assert_eq!(
        u32::from_le_bytes(before[last + 8..last + 12].try_into().unwrap()),
        65_536
    );
    for _ in 0..2 {
        let snapshot = rbl_export::snapshot::Snapshot::read(root.path()).unwrap();
        for histories in [&snapshot.history, &snapshot.legacy_history] {
            assert_eq!(histories.len(), 1);
            assert_eq!(histories[0].tracks.len(), 65_536);
            assert!(histories[0].tracks[..65_535].iter().all(|id| *id == 1));
            assert_eq!(histories[0].tracks[65_535], 2);
        }
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}
