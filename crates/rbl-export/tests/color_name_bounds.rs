//! Fixture-only checks: oversized colour names must never publish databases.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use rbl_onelibrary::settings::StickSettings;

#[test]
fn oversized_color_names_refuse_blank_creation_and_export() {
    for name in ["X".repeat(5000), "界".repeat(2500)] {
        let mut settings = StickSettings::default();
        settings.colors[0].name = name;
        for create in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let result = if create {
                rbl_export::create_library(dir.path(), Some(&settings), &[], None).map(|_| ())
            } else {
                rbl_export::export_with(dir.path(), &[], &[], Some(&settings)).map(|_| ())
            };
            let error = result.expect_err("oversized colour must be refused").to_string();
            assert!(error.contains("Color 1's name is longer"), "{error}");
            for root in ["PIONEER", "ALPHATHETA"] {
                assert!(!dir.path().join(root).join("rekordbox/export.pdb").exists());
                assert!(!dir.path().join(root).join("rekordbox/exportLibrary.db").exists());
            }
        }
    }
}
