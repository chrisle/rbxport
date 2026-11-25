//! History sequence occupies two bytes; the adjacent on-air flag is not order.
#![allow(clippy::unwrap_used)]
use rbl_pdb::{build::FileBuilder, Pdb};

#[test]
fn played_history_sequence_does_not_include_the_on_air_byte() {
    let mut file = FileBuilder::new(4096);
    let rows: Vec<_> = [
        (22_u32, 2_u16, 0_u8),
        (11, 1, 1),
        (33, 3, 0),
        (44, u16::MAX, 255),
    ]
    .into_iter()
    .map(|(track, sequence, on_air)| {
        let mut row = track.to_le_bytes().to_vec();
        row.extend_from_slice(&7_u32.to_le_bytes());
        row.extend_from_slice(&sequence.to_le_bytes());
        row.extend_from_slice(&[on_air, 0]);
        row
    })
    .collect();
    file.add_table(12, &rows);
    let bytes = file.finish();
    let entries = Pdb::parse(&bytes).unwrap().played_history_entries();
    assert_eq!(
        entries
            .iter()
            .map(|row| (row.track_id, row.playlist_id, row.entry_index))
            .collect::<Vec<_>>(),
        [
            (22, 7, 2),
            (11, 7, 1),
            (33, 7, 3),
            (44, 7, u32::from(u16::MAX))
        ]
    );
}

#[test]
fn only_complete_oversized_histories_keep_the_old_writer_word() {
    const N: u32 = 65_536;
    for case in ["complete", "incomplete", "duplicate", "overflow"] {
        let mut raw: Vec<_> = (1..=N).collect();
        match case {
            "incomplete" => raw.iter_mut().for_each(|n| *n += 1),
            "duplicate" => raw[N as usize - 1] = 1,
            "overflow" => raw[N as usize - 1] = u32::MAX,
            _ => {}
        }
        let mut rows: Vec<_> = raw
            .iter()
            .map(|sequence| {
                let mut row = 1_u32.to_le_bytes().to_vec();
                row.extend_from_slice(&7_u32.to_le_bytes());
                row.extend_from_slice(&sequence.to_le_bytes());
                row
            })
            .collect();
        // Total table size cannot qualify another, ordinary flagged session.
        for (sequence, flag) in [(1_u16, 1_u8), (2, 0)] {
            let mut row = 2_u32.to_le_bytes().to_vec();
            row.extend_from_slice(&8_u32.to_le_bytes());
            row.extend_from_slice(&sequence.to_le_bytes());
            row.extend_from_slice(&[flag, 0]);
            rows.push(row);
        }
        let mut file = FileBuilder::new(4096);
        file.add_table(12, &rows);
        let bytes = file.finish();
        let entries = Pdb::parse(&bytes).unwrap().played_history_entries();
        assert_eq!(entries.len(), N as usize + 2);
        for (entry, sequence) in entries.iter().zip(raw) {
            assert_eq!(
                entry.entry_index,
                if case == "complete" {
                    sequence
                } else {
                    sequence & u32::from(u16::MAX)
                },
                "{case}"
            );
        }
        assert_eq!(entries[N as usize].entry_index, 1, "{case}");
        assert_eq!(entries[N as usize + 1].entry_index, 2, "{case}");
    }
}
