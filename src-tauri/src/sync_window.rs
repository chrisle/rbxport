//! The Sync Manager window.
//!
//! rekordbox opens its Sync Manager as a window of its own from the ⟳ at the
//! foot of the browse tree, and so does this. It loads the same bundle at
//! `index.html#sync`, which the frontend renders as the manager alone. The
//! window reads the preferences from the `localStorage` both windows share,
//! so the DJ System defaults a fresh stick is given are the ones set in
//! Preferences, whichever window set them.

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

use crate::error::{AppError, AppResult, ErrorKind};

pub const WINDOW: &str = "sync";

/// The window's content — see design/tokens `syncW`, `syncH`. rekordbox's
/// dialog is 1146 by 666 with its iTunes column; ours carries the same three
/// columns, and its two SYNC buttons take a little more width than they do.
const WIDTH: f64 = 1440.0;
const HEIGHT: f64 = 620.0;

/// Opens the window, or brings the open one to the front.
#[tauri::command]
// `async` for the same reason `open_preferences` is: a synchronous command
// runs on the main thread, and on Windows WebView2 finishes creating a
// webview by posting back to that thread, which is blocked — the window
// stays a bare white frame. Tauri hands the handle over by value.
#[allow(clippy::needless_pass_by_value)]
pub async fn open_sync_window(app: tauri::AppHandle) -> AppResult<()> {
    if let Some(window) = app.get_webview_window(WINDOW) {
        let _ = window.unminimize();
        let _ = window.set_focus();
        return Ok(());
    }
    // The hash is set from an initialization script rather than carried in
    // the URL: `index.html#sync` opens blank on Windows, where a `#` inside
    // an `App` path does not survive the `http://tauri.localhost` route
    // (what 0.5.1's Preferences window taught).
    let builder = WebviewWindowBuilder::new(&app, WINDOW, WebviewUrl::App("index.html".into()))
        // Top frame only: on Windows wry runs it in every frame (see `report.rs`).
        .initialization_script("if (window === window.top && !location.hash) location.hash = '#sync';")
        .title("Sync Manager");
    // The same debugging port as the main window's, when one was asked for;
    // a no-op off Windows (see `crate::browser_args`).
    let builder = match crate::browser_args() {
        Some(args) => builder.additional_browser_args(&args),
        None => builder,
    };
    // The window draws its own title bar, as the main window and the
    // Preferences do, so the name sits in the app's grey rather than the
    // one macOS paints. Both calls are macOS-only in Tauri.
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    builder
        // In the log, because a window that opens blank says nothing itself.
        .on_navigation(|url| {
            tracing::debug!(%url, "sync window navigation");
            true
        })
        .on_page_load(|_, payload| {
            tracing::debug!(url = %payload.url(), event = ?payload.event(), "sync window page load");
        })
        .inner_size(WIDTH, HEIGHT)
        .min_inner_size(720.0, 420.0)
        .visible(false)
        .resizable(true)
        .accept_first_mouse(true)
        .build()
        .map(|window| {
            // On Windows and Linux the application menu is a bar on every
            // window it is set on; this one has no use for File › Import.
            let _ = window.hide_menu();
        })
        .map_err(|e| {
            AppError::new(ErrorKind::Internal, format!("the Sync Manager window could not open: {e}"))
        })
}
