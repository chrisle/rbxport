//! The native application menu.
//!
//! Built in Rust so it is a real macOS menu bar rather than a strip drawn in
//! the window — which is what rekordbox has, and what a Mac user expects to
//! find their keyboard shortcuts in.
//!
//! Every label comes from `src/i18n/en.json`, which was transcribed from the
//! left-hand keys of rekordbox's `german.lang` (`english.lang` is a stub).
//! Compiled in rather than duplicated here, so the wording cannot drift from
//! the rest of the interface.
//!
//! Only items that do something are here. rekordbox's File menu also offers
//! XML collection export, its Help menu links to Pioneer's manuals, and its
//! View menu toggles panels we have not built; an item that greys out forever
//! or opens somebody else's website is worse than an absent one.

use tauri::menu::{Menu, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// The transcribed English strings, compiled in at build time.
const STRINGS: &str = include_str!("../../src/i18n/en.json");

/// Looks a label up, falling back to the key — which is itself the English
/// string, so a missing entry reads correctly rather than blank.
fn label(key: &str) -> String {
    static PARSED: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    let strings = PARSED.get_or_init(|| serde_json::from_str(STRINGS).unwrap_or(serde_json::Value::Null));
    strings
        .get("menu")
        .and_then(|group| group.get(key))
        .and_then(serde_json::Value::as_str)
        .unwrap_or(key)
        .to_owned()
}

/// The event a menu item sends to the frontend.
///
/// One event with the item's id rather than an event per item: the frontend
/// maps ids to actions in one place, and adding an item does not mean adding
/// another listener.
pub const EVENT: &str = "menu";

#[tauri::command]
#[allow(clippy::needless_pass_by_value, reason = "Tauri's AppHandle extractor is injected by value")]
pub fn set_history_menu<R: Runtime>(
    app: AppHandle<R>,
    undo: Option<String>,
    redo: Option<String>,
) -> Result<(), String> {
    let Some(menu) = app.menu() else { return Ok(()); };
    let Some(edit) = menu.get("edit").and_then(|item| item.as_submenu().cloned()) else {
        return Ok(());
    };
    for (id, title, action) in [("undo", "Undo", undo), ("redo", "Redo", redo)] {
        if let Some(item) = edit.get(id).and_then(|item| item.as_menuitem().cloned()) {
            let text = action.map_or_else(|| title.to_owned(), |action| format!("{title} {action}"));
            item.set_text(text).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let settings = MenuItemBuilder::with_id("settings", "Settings…")
        .accelerator("CmdOrCtrl+,")
        .build(app)?;
    let import = MenuItemBuilder::with_id("import", label("Import"))
        .accelerator("CmdOrCtrl+O")
        .build(app)?;
    // The folder counterpart of Import: one dialog picks a folder and every
    // audio file under it, recursively, is added. ⇧⌘O sits beside ⌘O.
    let import_folder = MenuItemBuilder::with_id("import-folder", "Import Folder…")
        .accelerator("CmdOrCtrl+Shift+O")
        .build(app)?;
    let missing = MenuItemBuilder::with_id("missing", label("Missing File Manager")).build(app)?;
    // rekordbox's own two, worded as its File menu words them.
    let import_xml = MenuItemBuilder::with_id("import-xml", "Import rekordbox xml…").build(app)?;
    // rekordbox reads iTunes as a section of its tree; here it is an
    // import of Music.app's Library.xml, worded like the one above.
    let import_itunes = MenuItemBuilder::with_id("import-itunes", "Import iTunes Library xml…").build(app)?;
    let export_xml = MenuItemBuilder::with_id("export-xml", "Export Collection in xml format…").build(app)?;
    // rekordbox calls its own "Update Manager"; the item is worded the way
    // every other Mac app words it, since that is where people look for it.
    let updates = MenuItemBuilder::with_id("updates", "Check for Updates…").build(app)?;

    // The application menu, whose first item macOS names after the app.
    let application = SubmenuBuilder::new(app, "rbxport")
        .item(&PredefinedMenuItem::about(app, None, None)?)
        .item(&updates)
        .separator()
        .item(&settings)
        .separator()
        .item(&PredefinedMenuItem::hide(app, None)?)
        .item(&PredefinedMenuItem::hide_others(app, None)?)
        .separator()
        .item(&PredefinedMenuItem::quit(app, None)?)
        .build()?;

    let file = SubmenuBuilder::new(app, label("File"))
        .item(&import)
        .item(&import_folder)
        .item(&import_xml)
        .item(&import_itunes)
        .item(&export_xml)
        .item(&missing)
        .separator()
        .item(&PredefinedMenuItem::close_window(app, None)?)
        .build()?;

    let media_player = SubmenuBuilder::new(app, "Media Player")
        .item(&MenuItemBuilder::with_id("tempo-slider", "Display Tempo slider").build(app)?)
        .build()?;
    let layout = SubmenuBuilder::new(app, "Layout").item(&media_player).build()?;
    let view = SubmenuBuilder::new(app, label("View"))
        .item(&layout)
        .item(
            &MenuItemBuilder::with_id("info", label("Information Window"))
                .accelerator("CmdOrCtrl+I")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("sub", label("Sub-Browser Window"))
                .accelerator("CmdOrCtrl+B")
                .build(app)?,
        )
        .separator()
        .item(
            &MenuItemBuilder::with_id("fullscreen", label("Full screen"))
                // rekordbox's own, from its Export key map.
                .accelerator("Shift+CmdOrCtrl+F")
                .build(app)?,
        )
        .separator()
        // The layout switch, on the keys rekordbox's Export preset gives it.
        .item(
            &MenuItemBuilder::with_id("layout-one", label("1 Player"))
                .accelerator("CmdOrCtrl+7")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("layout-two", label("2 Players"))
                .accelerator("CmdOrCtrl+8")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("layout-simple", label("Simple Player"))
                .accelerator("CmdOrCtrl+9")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("layout-browser", label("Full Browser"))
                .accelerator("CmdOrCtrl+0")
                .build(app)?,
        )
        .build()?;

    // History follows frontend focus (text field or active deck). Clipboard
    // items stay predefined so macOS forwards them to the webview's fields.
    let edit = SubmenuBuilder::with_id(app, "edit", "Edit")
        .item(
            &MenuItemBuilder::with_id("undo", "Undo")
                .accelerator("CmdOrCtrl+Z")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("redo", "Redo")
                .accelerator("CmdOrCtrl+Shift+Z")
                .build(app)?,
        )
        .separator()
        .item(&PredefinedMenuItem::cut(app, None)?)
        .item(&PredefinedMenuItem::copy(app, None)?)
        .item(&PredefinedMenuItem::paste(app, None)?)
        .item(&PredefinedMenuItem::select_all(app, None)?)
        .build()?;

    let help = SubmenuBuilder::new(app, "Help")
        .item(&MenuItemBuilder::with_id("report-bug", "Report bug…").build(app)?)
        .build()?;
    Menu::with_items(app, &[&application, &file, &edit, &view, &help])
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
