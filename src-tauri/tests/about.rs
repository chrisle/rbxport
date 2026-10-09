//! The About item has something to show off macOS (issue #172).
//!
//! On Linux and Windows a predefined About item given no metadata does
//! nothing when clicked (muda 0.19 acts only on `About(Some(..))`); only
//! macOS fills a standard panel in by itself. So the item must carry the
//! name, version and copyright everywhere but macOS. No menu item is built
//! here, so the default harness's spawned threads are fine.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};

/// A mock app carrying the real bundle configuration, named and versioned
/// the way `generate_context!` names and versions the shipped app.
fn app_with_real_config() -> tauri::App<MockRuntime> {
    let text = include_str!("../tauri.conf.json");
    let config: tauri::Config = serde_json::from_str(text).expect("tauri.conf.json parses");
    let mut context = mock_context(noop_assets());
    let name = config.product_name.clone().expect("productName is set");
    let version = config.version.clone().expect("version is set");
    *context.config_mut() = config;
    context.package_info_mut().name = name;
    context.package_info_mut().version = version.parse().expect("version is semver");
    mock_builder().build(context).expect("mock app builds")
}

#[test]
fn about_carries_metadata_everywhere_but_macos() {
    let app = app_with_real_config();
    let metadata = rbxport_lib::menu::about_metadata_for_platform(app.handle());
    if cfg!(target_os = "macos") {
        assert!(
            metadata.is_none(),
            "macOS reads its About panel from the bundle"
        );
    } else {
        assert!(
            metadata.is_some(),
            "without metadata the About item does nothing on Linux and Windows"
        );
    }
}

#[test]
fn about_shows_the_name_version_and_copyright_of_the_bundle() {
    let app = app_with_real_config();
    let metadata = rbxport_lib::menu::about_metadata(app.handle());
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();

    assert_eq!(metadata.name.as_deref(), config["productName"].as_str());
    assert_eq!(metadata.version.as_deref(), config["version"].as_str());
    assert_eq!(
        metadata.copyright.as_deref(),
        config["bundle"]["copyright"].as_str()
    );
    assert!(metadata.name.is_some() && metadata.version.is_some());
}
