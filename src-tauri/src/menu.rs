//! The native application menu.
//!
//! Built in Rust so it is a real macOS menu bar rather than a strip drawn in
//! the window — which is what rekordbox has, and what a Mac user expects to
//! find their keyboard shortcuts in.
//!
//! The frontend sends this menu the active entries from its localization
//! catalog. Rebuilding it when that catalog changes keeps the native menu in
//! the same language as every webview without maintaining a second set of
//! translations in Rust.
//!
//! Only items that do something are here. rekordbox's File menu also offers
//! XML collection export, its Help menu links to Pioneer's manuals, and its
//! View menu toggles panels we have not built; an item that greys out forever
//! or opens somebody else's website is worse than an absent one.

use std::{
    collections::HashMap,
    sync::{OnceLock, RwLock},
};

use tauri::menu::{AboutMetadata, Menu, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Manager, Runtime};

type Labels = HashMap<String, String>;

fn saved_labels() -> &'static RwLock<Labels> {
    static LABELS: OnceLock<RwLock<Labels>> = OnceLock::new();
    LABELS.get_or_init(|| RwLock::new(Labels::new()))
}

/// Looks a label up, falling back to the English key.
fn label(labels: &Labels, key: &str) -> String {
    labels
        .get(key)
        .filter(|text| !text.is_empty())
        .map_or_else(|| key.to_owned(), Clone::clone)
}

fn current_label(key: &str) -> String {
    saved_labels()
        .read()
        .ok()
        .map_or_else(|| key.to_owned(), |labels| label(&labels, key))
}

/// The event a menu item sends to the frontend.
///
/// One event with the item's id rather than an event per item: the frontend
/// maps ids to actions in one place, and adding an item does not mean adding
/// another listener.
pub const EVENT: &str = "menu";

#[tauri::command]
#[allow(
    clippy::needless_pass_by_value,
    reason = "Tauri's AppHandle extractor is injected by value"
)]
pub fn set_history_menu<R: Runtime>(
    app: AppHandle<R>,
    undo: Option<String>,
    redo: Option<String>,
) -> Result<(), String> {
    let Some(menu) = app.menu() else {
        return Ok(());
    };
    let Some(edit) = menu.get("edit").and_then(|item| item.as_submenu().cloned()) else {
        return Ok(());
    };
    for (id, title, action) in [
        ("undo", current_label("Undo"), undo),
        ("redo", current_label("Redo"), redo),
    ] {
        if let Some(item) = edit.get(id).and_then(|item| item.as_menuitem().cloned()) {
            let text = action.map_or_else(|| title.clone(), |action| format!("{title} {action}"));
            item.set_text(text).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

/// Whether the File menu offers the Missing File Manager. Its pane in
/// Preferences is hidden for now (see `MISSING_FILES_ENABLED` in
/// `src/views/settings/AdvancedPane.tsx`), so the item that opens it is too.
const MISSING_FILE_MANAGER: bool = false;

/// What the About item shows: the product name, its version, the copyright
/// line and the window icon, all from the bundle's own configuration.
///
/// rekordbox's About "displays your installed version" (rekordbox 7 manual,
/// menu list p.252). On macOS the standard About panel reads all of this from
/// the app bundle by itself. On Linux and Windows the predefined About item
/// does nothing at all unless it is given these fields (muda 0.19
/// `platform_impl/gtk` and `platform_impl/windows` only act on
/// `About(Some(..))`), which is why About never appeared there.
pub fn about_metadata<R: Runtime>(app: &AppHandle<R>) -> AboutMetadata<'static> {
    let package = app.package_info();
    AboutMetadata {
        name: Some(package.name.clone()),
        version: Some(package.version.to_string()),
        copyright: app.config().bundle.copyright.clone(),
        icon: app
            .default_window_icon()
            .cloned()
            .map(tauri::image::Image::to_owned),
        ..AboutMetadata::default()
    }
}

/// The About item's metadata for this platform: none on macOS, where the
/// system panel fills itself in from the bundle (and passing ours would swap
/// the bundle's full-size icon for the small window one).
pub fn about_metadata_for_platform<R: Runtime>(
    app: &AppHandle<R>,
) -> Option<AboutMetadata<'static>> {
    if cfg!(target_os = "macos") {
        None
    } else {
        Some(about_metadata(app))
    }
}

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    build_with_labels(app, &Labels::new())
}

fn build_with_labels<R: Runtime>(app: &AppHandle<R>, labels: &Labels) -> tauri::Result<Menu<R>> {
    let settings = MenuItemBuilder::with_id("settings", label(labels, "Settings…"))
        .accelerator("CmdOrCtrl+,")
        .build(app)?;
    let import = MenuItemBuilder::with_id("import", label(labels, "Import"))
        .accelerator("CmdOrCtrl+O")
        .build(app)?;
    // The folder counterpart of Import: one dialog picks a folder and every
    // audio file under it, recursively, is added. ⇧⌘O sits beside ⌘O.
    let import_folder = MenuItemBuilder::with_id("import-folder", label(labels, "Import Folder…"))
        .accelerator("CmdOrCtrl+Shift+O")
        .build(app)?;
    let missing =
        MenuItemBuilder::with_id("missing", label(labels, "Missing File Manager")).build(app)?;
    // rekordbox's own two, worded as its File menu words them.
    let import_xml = MenuItemBuilder::with_id("import-xml", label(labels, "Import rekordbox xml…"))
        .build(app)?;
    // rekordbox reads iTunes as a section of its tree; here it is an
    // import of Music.app's Library.xml, worded like the one above.
    let import_itunes =
        MenuItemBuilder::with_id("import-itunes", label(labels, "Import iTunes Library xml…"))
            .build(app)?;
    let export_xml = MenuItemBuilder::with_id(
        "export-xml",
        label(labels, "Export Collection in xml format…"),
    )
    .build(app)?;
    // rekordbox calls its own "Update Manager"; the item is worded the way
    // every other Mac app words it, since that is where people look for it.
    let updates =
        MenuItemBuilder::with_id("updates", label(labels, "Check for Updates…")).build(app)?;

    // The application menu, whose first item macOS names after the app.
    let application = SubmenuBuilder::new(app, "rbxport")
        .item(&PredefinedMenuItem::about(
            app,
            Some(&label(labels, "About rbxport")),
            about_metadata_for_platform(app),
        )?)
        .item(&updates)
        .separator()
        .item(&settings)
        .separator()
        .item(&PredefinedMenuItem::hide(
            app,
            Some(&label(labels, "Hide rbxport")),
        )?)
        .item(&PredefinedMenuItem::hide_others(
            app,
            Some(&label(labels, "Hide Others")),
        )?)
        .separator()
        .item(&PredefinedMenuItem::quit(
            app,
            Some(&label(labels, "Quit rbxport")),
        )?)
        .build()?;

    let mut file = SubmenuBuilder::new(app, label(labels, "File"))
        .item(&import)
        .item(&import_folder)
        .item(&import_xml)
        .item(&import_itunes)
        .item(&export_xml);
    if MISSING_FILE_MANAGER {
        file = file.item(&missing);
    }
    let file = file
        .separator()
        .item(&PredefinedMenuItem::close_window(
            app,
            Some(&label(labels, "Close Window")),
        )?)
        .build()?;

    let media_player = SubmenuBuilder::new(app, label(labels, "Media Player"))
        .item(
            &MenuItemBuilder::with_id("tempo-slider", label(labels, "Display Tempo slider"))
                .build(app)?,
        )
        .build()?;
    let layout = SubmenuBuilder::new(app, label(labels, "Layout"))
        .item(&media_player)
        .build()?;
    let view = SubmenuBuilder::new(app, label(labels, "View"))
        .item(&layout)
        .item(
            &MenuItemBuilder::with_id("info", label(labels, "Information Window"))
                .accelerator("CmdOrCtrl+I")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("sub", label(labels, "Sub-Browser Window"))
                .accelerator("CmdOrCtrl+B")
                .build(app)?,
        )
        .separator()
        .item(
            &MenuItemBuilder::with_id("fullscreen", label(labels, "Full screen"))
                // rekordbox's own, from its Export key map.
                .accelerator("Shift+CmdOrCtrl+F")
                .build(app)?,
        )
        .separator()
        // The layout switch, on the keys rekordbox's Export preset gives it.
        .item(
            &MenuItemBuilder::with_id("layout-one", label(labels, "1 Player"))
                .accelerator("CmdOrCtrl+7")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("layout-two", label(labels, "2 Players"))
                .accelerator("CmdOrCtrl+8")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("layout-simple", label(labels, "Simple Player"))
                .accelerator("CmdOrCtrl+9")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("layout-browser", label(labels, "Full Browser"))
                .accelerator("CmdOrCtrl+0")
                .build(app)?,
        )
        .build()?;

    // History follows frontend focus (text field or active deck). Clipboard
    // items stay predefined so macOS forwards them to the webview's fields.
    let edit = SubmenuBuilder::with_id(app, "edit", label(labels, "Edit"))
        .item(
            &MenuItemBuilder::with_id("undo", label(labels, "Undo"))
                .accelerator("CmdOrCtrl+Z")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("redo", label(labels, "Redo"))
                .accelerator("CmdOrCtrl+Shift+Z")
                .build(app)?,
        )
        .separator()
        .item(&PredefinedMenuItem::cut(app, Some(&label(labels, "Cut")))?)
        .item(&PredefinedMenuItem::copy(
            app,
            Some(&label(labels, "Copy")),
        )?)
        .item(&PredefinedMenuItem::paste(
            app,
            Some(&label(labels, "Paste")),
        )?)
        .item(&PredefinedMenuItem::select_all(
            app,
            Some(&label(labels, "Select All")),
        )?)
        .build()?;

    let help = SubmenuBuilder::new(app, label(labels, "Help"))
        .item(&MenuItemBuilder::with_id("report-bug", label(labels, "Report bug…")).build(app)?)
        .build()?;
    Menu::with_items(app, &[&application, &file, &edit, &view, &help])
}

/// Rebuilds the native menu from the same translated labels as the web UI.
#[tauri::command]
#[allow(
    clippy::needless_pass_by_value,
    reason = "Tauri's AppHandle extractor is injected by value"
)]
pub fn set_menu_labels<R: Runtime>(app: AppHandle<R>, labels: Labels) -> Result<(), String> {
    let menu = build_with_labels(&app, &labels).map_err(|error| error.to_string())?;
    app.set_menu(menu).map_err(|error| error.to_string())?;
    if let Ok(mut saved) = saved_labels().write() {
        *saved = labels;
    }
    Ok(())
}

/// Handles a menu click.
///
/// Anything the shell can do itself is done here; everything else goes to the
/// frontend as one event carrying the item's id.
pub fn on_event<R: Runtime>(app: &AppHandle<R>, id: &str) {
    if id == "fullscreen" {
        if let Some(window) = app.get_webview_window("main") {
            let full = window.is_fullscreen().unwrap_or(false);
            let _ = window.set_fullscreen(!full);
        }
        return;
    }
    let _ = app.emit(EVENT, id);
}
