//! Disposable legacy-only menu ownership across sync and sibling creation.
#![allow(clippy::pedantic, clippy::unwrap_used)]

use std::path::Path;
use rbl_onelibrary::settings::StickSettings;

fn settings() -> StickSettings {
    // An inactive sort can be the sub-column.
    let mut settings = StickSettings { sub_column: Some(21), ..StickSettings::default() };
    for slots in [&mut settings.categories, &mut settings.sorts] {
        let a = slots.iter().position(|slot| slot.visible && slot.seq == 1).unwrap();
        let b = slots.iter().position(|slot| slot.visible && slot.seq == 2).unwrap();
        slots[a].seq = 2;
        slots[b].seq = 1;
    }
    settings
}

fn menu_rows(root: &Path) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let bytes = std::fs::read(root.join("PIONEER/rekordbox/export.pdb")).unwrap();
    let pdb = rbl_pdb::Pdb::parse(&bytes).unwrap();
    let rows = |kind| pdb.rows(pdb.table(kind).unwrap()).iter()
        .map(|row| bytes[row.offset..row.offset + 8].to_vec()).collect();
    ordered((rows(rbl_pdb::PageType::Categories), rows(rbl_pdb::PageType::Sorts)))
}

fn ordered(mut rows: (Vec<Vec<u8>>, Vec<Vec<u8>>)) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    rows.0.sort(); rows.1.sort(); rows
}

fn files(root: &Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
    fn walk(root: &Path, at: &Path, result: &mut std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(at).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() { walk(root, &path, result); }
            else { result.insert(path.strip_prefix(root).unwrap().to_path_buf(), std::fs::read(path).unwrap()); }
        }
    }
    let mut result = std::collections::BTreeMap::new();
    walk(root, root, &mut result); result
}

fn edit_legacy_menus(root: &Path, settings: &StickSettings) {
    let path = root.join("PIONEER/rekordbox/export.pdb");
    let mut bytes = std::fs::read(&path).unwrap();
    let settings = rbl_export::menu_rows(settings);
    for (kind, replacement) in [(rbl_pdb::PageType::Categories, settings.0), (rbl_pdb::PageType::Sorts, settings.1)] {
        let rows = { let pdb = rbl_pdb::Pdb::parse(&bytes).unwrap(); pdb.rows(pdb.table(kind).unwrap()) };
        for row in rows {
            let replacement = replacement.iter().find(|value| value[2..4] == bytes[row.offset + 2..row.offset + 4]).unwrap();
            bytes[row.offset..row.offset + 8].copy_from_slice(replacement);
        }
    }
    std::fs::write(path, bytes).unwrap();
}

fn assert_menus(root: &Path, expected: &StickSettings) {
    let one = StickSettings::read(&root.join("PIONEER/rekordbox/exportLibrary.db")).unwrap();
    assert_eq!(ordered(rbl_export::menu_rows(&one)), ordered(rbl_export::menu_rows(expected)));
    assert_eq!(menu_rows(root), ordered(rbl_export::menu_rows(expected)));
    assert_eq!(one.sub_column, expected.sub_column);
}

#[test]
fn legacy_menus_survive_sync_and_create_library_upgrade() {
    for upgrade in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let audio = source.path().join("track.mp3");
        std::fs::write(&audio, b"disposable audio bytes").unwrap();
        let track = rbl_export::SourceTrack { id: 1, source_path: audio.clone(), title: "Track".into(), ..Default::default() };
        let expected = settings();
        rbl_export::export(root.path(), std::slice::from_ref(&track), &[]).unwrap();
        edit_legacy_menus(root.path(), &expected);
        let manifest = rbl_export::Manifest::load(root.path()).unwrap();
        let exported = root.path().join(manifest.tracks[0].audio.trim_start_matches('/'));
        let audio_before = (std::fs::read(&exported).unwrap(), std::fs::metadata(&exported).unwrap().modified().unwrap());
        let source_before = std::fs::read(&audio).unwrap();
        std::fs::remove_file(root.path().join("PIONEER/rekordbox/exportLibrary.db")).unwrap();
        if upgrade {
            let defaults = StickSettings { sub_column: Some(5), ..StickSettings::default() };
            assert!(rbl_export::create_library(root.path(), Some(&defaults), &[], None).unwrap());
        } else {
            rbl_export::export(root.path(), std::slice::from_ref(&track), &[]).unwrap();
        }
        assert_menus(root.path(), &expected);
        assert_eq!(std::fs::read(&audio).unwrap(), source_before);
        assert_eq!((std::fs::read(&exported).unwrap(), std::fs::metadata(&exported).unwrap().modified().unwrap()), audio_before);
        if upgrade {
            assert!(!rbl_export::create_library(root.path(), None, &[], None).unwrap());
        } else {
            let again = rbl_export::export(root.path(), &[track], &[]).unwrap();
            assert_eq!(again.reused, 1);
        }
        assert_eq!((std::fs::read(&exported).unwrap(), std::fs::metadata(&exported).unwrap().modified().unwrap()), audio_before);
        assert_menus(root.path(), &expected);
    }
}

#[test]
fn onelibrary_keeps_precedence_over_conflicting_legacy_menus() {
    let root = tempfile::tempdir().unwrap();
    let expected = settings();
    rbl_export::create_library(root.path(), Some(&expected), &[], None).unwrap();
    let other = tempfile::tempdir().unwrap();
    rbl_export::create_library(other.path(), None, &[], None).unwrap();
    std::fs::copy(other.path().join("PIONEER/rekordbox/export.pdb"), root.path().join("PIONEER/rekordbox/export.pdb")).unwrap();
    rbl_export::export_full(root.path(), &[], &[], &[], None, None, &mut |_| {}).unwrap();
    assert_menus(root.path(), &expected);
}

#[test]
fn malformed_or_unsupported_legacy_menus_refuse_without_publication() {
    for case in ["out-of-file", "unknown", "duplicate-id", "duplicate-item", "sequence", "multiple-columns", "flags", "truncated", "chain"] {
        let root = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let audio = source.path().join("source.mp3");
        std::fs::write(&audio, b"original fixture source audio").unwrap();
        let track = rbl_export::SourceTrack { id: 1, source_path: audio.clone(), title: "Original".into(), ..Default::default() };
        rbl_export::export_full(root.path(), &[track], &[], &[], Some(&settings()), None, &mut |_| {}).unwrap();
        let one = root.path().join("PIONEER/rekordbox/exportLibrary.db");
        std::fs::remove_file(&one).unwrap();
        let path = root.path().join("PIONEER/rekordbox/export.pdb");
        let mut bytes = std::fs::read(&path).unwrap();
        let (rows, size) = {
            let pdb = rbl_pdb::Pdb::parse(&bytes).unwrap();
            (pdb.rows(pdb.table(rbl_pdb::PageType::Sorts).unwrap()), pdb.page_size as usize)
        };
        let a = rows[0].offset;
        let b = rows[1].offset;
        match case {
            "out-of-file" => {
                let page = rows.last().unwrap().page_offset;
                assert!(page + rbl_pdb::build::PAGE_HEADER_LEN + usize::from(u16::MAX) >= bytes.len());
                let count = u32::from_le_bytes(bytes[page + 0x18..page + 0x1c].try_into().unwrap()) & 0x1fff;
                let last = count as usize - 1;
                let at = page + size - last / 16 * 0x24 - 6 - 2 * (last % 16);
                bytes[at..at + 2].copy_from_slice(&u16::MAX.to_le_bytes());
            },
            "unknown" => bytes[a..a + 2].copy_from_slice(&999_u16.to_le_bytes()),
            "duplicate-id" => { let id = bytes[a + 2..a + 4].to_vec(); bytes[b + 2..b + 4].copy_from_slice(&id); },
            "duplicate-item" => { let id = bytes[a..a + 2].to_vec(); bytes[b..b + 2].copy_from_slice(&id); },
            "sequence" => {
                let active: Vec<_> = rows.iter().filter(|row| bytes[row.offset + 4] & 1 == 0).collect();
                bytes[active[1].offset + 5] = bytes[active[0].offset + 5];
            },
            "multiple-columns" => { bytes[a + 4] |= 2; bytes[b + 4] |= 2; },
            "flags" => bytes[a + 6] = 1,
            "truncated" => bytes[rows[0].page_offset + 0x1e..rows[0].page_offset + 0x20].copy_from_slice(&4_u16.to_le_bytes()),
            "chain" => {
                // The last page must be reached, not silently abandoned.
                let pdb = rbl_pdb::Pdb::parse(&bytes).unwrap();
                let index = pdb.tables.iter().position(|table| table.page_type == rbl_pdb::PageType::Sorts).unwrap();
                let outside = u32::try_from(bytes.len() / size + 1).unwrap();
                bytes[28 + index * 16 + 12..28 + index * 16 + 16].copy_from_slice(&outside.to_le_bytes());
            },
            _ => unreachable!(),
        }
        std::fs::write(&path, &bytes).unwrap();
        let published = files(root.path());
        let source_before = std::fs::read(&audio).unwrap();
        let error = rbl_export::create_library(root.path(), None, &[], None).unwrap_err();
        assert!(error.to_string().contains("settings are malformed or unsupported"), "{case}: {error}");
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "{case}");
        assert!(!one.exists(), "{case}: no sibling may be published over an unreadable menu");
        assert_eq!(files(root.path()), published, "{case}: published generation changed on refusal");
        assert_eq!(std::fs::read(&audio).unwrap(), source_before, "{case}");
    }
}

#[test]
fn legacy_menu_fallback_does_not_take_background_or_color_ownership() {
    let root = tempfile::tempdir().unwrap();
    rbl_export::create_library(root.path(), Some(&settings()), &[], None).unwrap();
    let path = root.path().join("PIONEER/rekordbox/export.pdb");
    let mut bytes = std::fs::read(&path).unwrap();
    let background = {
        let pdb = rbl_pdb::Pdb::parse(&bytes).unwrap();
        pdb.rows(pdb.table(rbl_pdb::PageType::Property).unwrap())[0].offset + 9
    };
    bytes[background] = 7;
    std::fs::write(&path, bytes).unwrap();
    let one = root.path().join("PIONEER/rekordbox/exportLibrary.db");
    std::fs::remove_file(&one).unwrap();
    let mut defaults = StickSettings { background_color_type: 2, ..StickSettings::default() };
    defaults.colors[0].name = "Caller color".into();
    assert!(rbl_export::create_library(root.path(), Some(&defaults), &[], None).unwrap());
    assert_menus(root.path(), &settings());
    let written = StickSettings::read(&one).unwrap();
    assert_eq!(written.background_color_type, 2);
    assert_eq!(written.colors[0].name, "Caller color");
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(rbl_pdb::Pdb::parse(&bytes).unwrap().property().unwrap().background_color, 7);
}

#[test]
fn deleted_menu_index_entries_are_not_owned_rows() {
    let root = tempfile::tempdir().unwrap();
    let mut expected = settings();
    rbl_export::create_library(root.path(), Some(&expected), &[], None).unwrap();
    let path = root.path().join("PIONEER/rekordbox/export.pdb");
    let mut bytes = std::fs::read(&path).unwrap();
    let (row, size) = {
        let pdb = rbl_pdb::Pdb::parse(&bytes).unwrap();
        (*pdb.rows(pdb.table(rbl_pdb::PageType::Sorts).unwrap()).last().unwrap(), pdb.page_size as usize)
    };
    let item = i64::from(u16::from_le_bytes(bytes[row.offset..row.offset + 2].try_into().unwrap()));
    expected.sorts.retain(|slot| slot.menu_item != item);
    let page = row.page_offset;
    let count = u32::from_le_bytes(bytes[page + 0x18..page + 0x1c].try_into().unwrap()) & 0x1fff;
    let last = count as usize - 1;
    let packed = count | (count - 1) << 13;
    bytes[page + 0x18..page + 0x1b].copy_from_slice(&packed.to_le_bytes()[..3]);
    let base = page + size - last / 16 * 0x24;
    let flags = u16::from_le_bytes(bytes[base - 4..base - 2].try_into().unwrap()) & !(1 << (last % 16));
    bytes[base - 4..base - 2].copy_from_slice(&flags.to_le_bytes());
    let at = base - 6 - 2 * (last % 16);
    bytes[at..at + 2].copy_from_slice(&u16::MAX.to_le_bytes());
    std::fs::write(path, bytes).unwrap();
    std::fs::remove_file(root.path().join("PIONEER/rekordbox/exportLibrary.db")).unwrap();
    assert!(rbl_export::create_library(root.path(), None, &[], None).unwrap());
    assert_menus(root.path(), &expected);
}
