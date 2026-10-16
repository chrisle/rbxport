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

use tauri::menu::{
    AboutMetadata, Menu, MenuItemBuilder, MenuItemKind, PredefinedMenuItem, Submenu, SubmenuBuilder,
};
use tauri::{AppHandle, Emitter, Manager, Runtime};

type Labels = HashMap<String, String>;
/// Menu item ids and whether each is live.
type Enabled = HashMap<String, bool>;

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
    // rekordbox's own wording and place: File › Display All Missing Files,
    // between Import and the library items, opens the Missing File Manager
    // [OBS rekordbox 7.2.14, issue #201].
    let missing =
        MenuItemBuilder::with_id("missing", label(labels, "Display All Missing Files")).build(app)?;
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

    let file = SubmenuBuilder::new(app, label(labels, "File"))
        .item(&import)
        .item(&import_folder)
        .item(&import_xml)
        .item(&import_itunes)
        .separator()
        .item(&missing)
        .separator()
        .item(&export_xml)
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

    let track = track_menu(app, labels)?;
    let playlist = playlist_menu(app, labels)?;

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
    Menu::with_items(
        app,
        &[&application, &file, &edit, &track, &playlist, &view, &help],
    )
}

/// A row of the Track or Playlist menu: an item the frontend runs and greys
/// (see [`set_menu_enabled`]), a separator, or a submenu of more rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    /// An item: its id, which is also what the click sends, and its label key.
    Item(&'static str, &'static str),
    Separator,
    /// A submenu: its id, its label key and its rows.
    Sub(&'static str, &'static str, &'static [Row]),
}

/// rekordbox's Track menu, in its order and with its separators, holding
/// only what rbxport has [OBS static: `MainComponent::getMenuForIndex`
/// @0x100964638, rekordbox 7.2.19 macOS; OBS: the Windows menu bar, issue
/// #513]. Left out: Add To Playlist and Export Track (lists of playlists and
/// sticks, not yet here), Cloud Library Sync and Show in Spotify.
///
/// The items carry no key: rekordbox's Export key map binds none of them
/// (`Analyze Track`'s shift + command + A is the browser's own key, kept
/// out of the menu so that a Windows accelerator never fires twice).
const TRACK_MENU: &[Row] = &[
    Row::Item("import-to-collection", "Import To Collection"),
    Row::Item("analyze-track", "Analyze Track"),
    // `addSubMenu` with the two command items under it; the items show the
    // commands' own names, 0x4012 and 0x4013.
    Row::Sub(
        "analysis-lock",
        "Analysis Lock",
        &[
            Row::Item("analysis-lock-on", "Analysis Lock On"),
            Row::Item("analysis-lock-off", "Analysis Lock Off"),
        ],
    ),
    Row::Separator,
    Row::Item("reload-tag", "Reload Tag"),
    Row::Separator,
    Row::Item("remove-from-collection", "Remove from Collection"),
    Row::Separator,
    Row::Item("show-in-finder", "Show in Finder"),
    Row::Item("auto-relocate", "Auto Relocate"),
    Row::Item("relocate", "Relocate"),
];

/// rekordbox's Playlist menu, in its order, holding only what rbxport has
/// [OBS static: `getMenuForIndex` @0x100966490-@0x100966988, rekordbox
/// 7.2.19 macOS; OBS: the Windows menu bar, issue #513]. Left out: Batch
/// Auto Upload setting, Import Playlist, Copy to Playlists category, Playlist
/// display setting and the sharing items.
const PLAYLIST_MENU: &[Row] = &[
    Row::Item("create-playlist", "Create New Playlist"),
    Row::Item(
        "create-intelligent-playlist",
        "Create New Intelligent Playlist",
    ),
    Row::Item("create-folder", "Create New Folder"),
    Row::Separator,
    Row::Item("delete-playlist", "Delete Playlist"),
    Row::Separator,
    // Sort Items (0x500d) goes here, before Renumber Track Order (#443).
    Row::Item("renumber-track-order", "Renumber Track Order"),
    // Titled with the String rekordbox builds at 0x101b99558; the items show
    // the commands' own names, 0x5005 and 0x5006.
    Row::Sub(
        "export-playlist-file",
        "Export a playlist to a file",
        &[
            Row::Item("export-playlist-txt", "Export a playlist to a file (*.txt)"),
            Row::Item(
                "export-playlist-m3u8",
                "Export a playlist to a file for music apps (*.m3u8)",
            ),
        ],
    ),
];

/// Which of the rows above are greyed. Nothing is live until the frontend
/// says so: rekordbox greys them all until a track list or the tree has the
/// keyboard focus [OBS: a fresh rekordbox, issue #513].
fn saved_enabled() -> &'static RwLock<Enabled> {
    static ENABLED: OnceLock<RwLock<Enabled>> = OnceLock::new();
    ENABLED.get_or_init(|| RwLock::new(Enabled::new()))
}

fn is_enabled(id: &str) -> bool {
    saved_enabled()
        .read()
        .ok()
        .and_then(|enabled| enabled.get(id).copied())
        .unwrap_or(false)
}

fn add_rows<'a, R: Runtime>(
    app: &'a AppHandle<R>,
    labels: &Labels,
    mut menu: SubmenuBuilder<'a, R, AppHandle<R>>,
    rows: &[Row],
) -> tauri::Result<SubmenuBuilder<'a, R, AppHandle<R>>> {
    for row in rows {
        menu = match *row {
            Row::Separator => menu.separator(),
            Row::Item(id, key) => menu.item(
                &MenuItemBuilder::with_id(id, label(labels, key))
                    .enabled(is_enabled(id))
                    .build(app)?,
            ),
            Row::Sub(id, key, items) => {
                let sub =
                    SubmenuBuilder::with_id(app, id, label(labels, key)).enabled(is_enabled(id));
                menu.item(&add_rows(app, labels, sub, items)?.build()?)
            }
        };
    }
    Ok(menu)
}

/// The Track menu, and after it Locate track loaded on deck on command + L
/// (b139), which rekordbox keeps out of its menu bar. The frontend runs the
/// same command each item's key or context-menu entry runs.
fn track_menu<R: Runtime>(app: &AppHandle<R>, labels: &Labels) -> tauri::Result<Submenu<R>> {
    add_rows(
        app,
        labels,
        SubmenuBuilder::new(app, label(labels, "Track")),
        TRACK_MENU,
    )?
    .separator()
    .item(
        &MenuItemBuilder::with_id(
            "locate-loaded",
            label(labels, "Locate track loaded on deck"),
        )
        .accelerator("CmdOrCtrl+L")
        .build(app)?,
    )
    .build()
}

fn playlist_menu<R: Runtime>(app: &AppHandle<R>, labels: &Labels) -> tauri::Result<Submenu<R>> {
    add_rows(
        app,
        labels,
        SubmenuBuilder::new(app, label(labels, "Playlist")),
        PLAYLIST_MENU,
    )?
    .build()
}

/// Greys or enables Track and Playlist menu items by id, as the selection and
/// the focus change; the frontend works out which (`src/lib/menuBar.ts`).
/// Kept across [`set_menu_labels`] rebuilds.
#[tauri::command]
#[allow(
    clippy::needless_pass_by_value,
    reason = "Tauri's AppHandle extractor is injected by value"
)]
pub fn set_menu_enabled<R: Runtime>(app: AppHandle<R>, items: Enabled) -> Result<(), String> {
    if let Ok(mut saved) = saved_enabled().write() {
        saved.extend(items.iter().map(|(id, on)| (id.clone(), *on)));
    }
    let Some(menu) = app.menu() else {
        return Ok(());
    };
    apply_enabled(menu.items().map_err(|error| error.to_string())?, &items)
}

fn apply_enabled<R: Runtime>(kinds: Vec<MenuItemKind<R>>, items: &Enabled) -> Result<(), String> {
    for kind in kinds {
        match kind {
            MenuItemKind::MenuItem(item) => {
                if let Some(on) = items.get(&item.id().0) {
                    item.set_enabled(*on).map_err(|error| error.to_string())?;
                }
            }
            MenuItemKind::Submenu(sub) => {
                if let Some(on) = items.get(&sub.id().0) {
                    sub.set_enabled(*on).map_err(|error| error.to_string())?;
                }
                apply_enabled(sub.items().map_err(|error| error.to_string())?, items)?;
            }
            _ => {}
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::{Row, PLAYLIST_MENU, TRACK_MENU};

    fn ids(rows: &[Row]) -> Vec<&'static str> {
        rows.iter()
            .flat_map(|row| match *row {
                Row::Item(id, _) => vec![id],
                Row::Separator => vec!["-"],
                Row::Sub(id, _, items) => {
                    let mut all = vec![id];
                    all.extend(ids(items));
                    all
                }
            })
            .collect()
    }

    // rekordbox 7.2.19's order (getMenuForIndex @0x100964638), less what
    // rbxport does not have.
    #[test]
    fn track_menu_keeps_rekordbox_order() {
        assert_eq!(
            ids(TRACK_MENU),
            [
                "import-to-collection",
                "analyze-track",
                "analysis-lock",
                "analysis-lock-on",
                "analysis-lock-off",
                "-",
                "reload-tag",
                "-",
                "remove-from-collection",
                "-",
                "show-in-finder",
                "auto-relocate",
                "relocate",
            ]
        );
    }

    #[test]
    fn playlist_menu_keeps_rekordbox_order() {
        assert_eq!(
            ids(PLAYLIST_MENU),
            [
                "create-playlist",
                "create-intelligent-playlist",
                "create-folder",
                "-",
                "delete-playlist",
                "-",
                "renumber-track-order",
                "export-playlist-file",
                "export-playlist-txt",
                "export-playlist-m3u8",
            ]
        );
    }
}
