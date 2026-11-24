//! Read-only colour comments are scoped to the selected on-stick database.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use rbl_export::device_library::{color_names_without_recovery, Format};

#[test]
fn selected_database_names_remain_independent_and_reads_change_no_bytes() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("PIONEER/rekordbox");
    std::fs::create_dir_all(&directory).unwrap();
    let mut pdb = rbl_pdb::build::FileBuilder::new(4096);
    pdb.add_table(6, &[rbl_pdb::rows::color_row(1, " PDB Play "), rbl_pdb::rows::color_row(8, "End Ω")]);
    let pdb_path = directory.join("export.pdb");
    std::fs::write(&pdb_path, pdb.finish()).unwrap();
    let one_path = directory.join("exportLibrary.db");
    let mut settings = rbl_onelibrary::settings::StickSettings::default();
    settings.colors[0].name = "OneLibrary".into();
    settings.colors[7].name = String::new();
    rbl_onelibrary::build::Builder::create_with(&one_path, &settings).unwrap()
        .finish("Fixture", "2026-11-24", 0).unwrap();
    let before_pdb = std::fs::read(&pdb_path).unwrap();
    let before_one = std::fs::read(&one_path).unwrap();
    let pdb_names = color_names_without_recovery(root.path(), Format::DeviceLibrary).unwrap();
    let one_names = color_names_without_recovery(root.path(), Format::OneLibrary).unwrap();
    assert_eq!(pdb_names[0], " PDB Play ");
    assert_eq!(pdb_names[7], "End Ω");
    assert_eq!(pdb_names[1], "Red");
    assert_eq!(one_names[0], "OneLibrary");
    assert_eq!(one_names[7], "Purple");
    assert_eq!(std::fs::read(&pdb_path).unwrap(), before_pdb);
    assert_eq!(std::fs::read(&one_path).unwrap(), before_one);
    // An older PDB-only stick does not need OneLibrary or master names.
    std::fs::remove_file(one_path).unwrap();
    assert_eq!(color_names_without_recovery(root.path(), Format::DeviceLibrary).unwrap(), pdb_names);
    assert!(color_names_without_recovery(root.path(), Format::OneLibrary).is_err());
    assert_eq!(std::fs::read(pdb_path).unwrap(), before_pdb);
}
