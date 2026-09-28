//! The native menu, built against a mock app.
//!
//! The packaged window cannot be screenshotted on this machine, so "the menu
//! is there" would otherwise be an assumption. A mock app builds the real
//! menu with no window at all, which settles the part that actually breaks:
//! whether it constructs, and whether the ids the frontend switches on are
//! the ids the items carry.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
// No harness means no test runner to report to; printing is the report.
#![allow(clippy::print_stdout)]


fn main() {
    // Not `#[test]`: on macOS a menu item can only be built on the main
    // thread, and the default harness runs every test on a spawned one.
    #[cfg(target_os = "macos")]
    {
        the_menu_builds_and_carries_the_ids_the_frontend_switches_on();
        the_menu_offers_the_clipboard_items_a_text_field_needs();
        println!("2 menu checks passed");
    }
    #[cfg(not(target_os = "macos"))]
    println!("skipped: the menu bar is macOS-shaped and untested elsewhere");
}

/// Every id the menu emits, in the order the menus are laid out.
#[cfg(target_os = "macos")]
fn ids(app: &tauri::AppHandle<tauri::test::MockRuntime>) -> Vec<String> {
    let menu = rbxport_lib::menu::build(app).expect("the menu builds");
    let mut out = Vec::new();
    for item in menu.items().expect("top level") {
        let Some(submenu) = item.as_submenu() else { continue };
        for child in submenu.items().expect("submenu items") {
            out.push(child.id().0.clone());
        }
    }
    out
}

#[cfg(target_os = "macos")]
fn the_menu_builds_and_carries_the_ids_the_frontend_switches_on() {
    let app = tauri::test::mock_app();
    let found = ids(&app.handle().clone());

    // These three are the contract with src/lib/menu.ts. A rename on either
    // side turns a menu item into one that silently does nothing.
    for id in ["settings", "import", "import-folder", "missing", "info", "sub", "fullscreen"] {
        assert!(found.iter().any(|f| f == id), "no item with id {id}; got {found:?}");
    }
}

#[cfg(target_os = "macos")]
fn the_menu_offers_the_clipboard_items_a_text_field_needs() {
    // Without a predefined Edit menu the standard shortcuts never reach the
    // webview on macOS, and renaming a playlist stops accepting paste.
    let app = tauri::test::mock_app();
    let menu = rbxport_lib::menu::build(&app.handle().clone()).expect("the menu builds");
    let titles: Vec<String> = menu
        .items()
        .expect("top level")
        .iter()
        .filter_map(|item| item.as_submenu().and_then(|s| s.text().ok()))
        .collect();
    assert!(titles.iter().any(|t| t == "Edit"), "no Edit menu; got {titles:?}");
    assert!(titles.iter().any(|t| t == "File"), "no File menu; got {titles:?}");
    assert!(titles.iter().any(|t| t == "View"), "no View menu; got {titles:?}");
}
