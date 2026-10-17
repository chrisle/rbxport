//! The Preferences window.
//!
//! rekordbox opens Preferences as a window of its own, with a title bar to
//! drag it by, beside the main one; so does this. It loads the same bundle
//! at `index.html#preferences/<pane>`, which the frontend renders as the
//! Preferences alone. The two windows share `localStorage`, so a choice made
//! here reaches the main window through its `storage` event.

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

use crate::error::{AppError, AppResult, ErrorKind};

pub const WINDOW: &str = "preferences";

/// The window's content, from the capture: 798 by 828pt with its 28pt
/// title bar, which macOS draws itself — see design/tokens `prefW`, `prefH`.
const WIDTH: f64 = 798.0;
const HEIGHT: f64 = 800.0;

/// Opens the window on `pane`, or brings the open one to the front and turns
/// it to that pane.
#[tauri::command]
// `async`, and not for anything it awaits: a synchronous command runs on
// the main thread, and on Windows WebView2 finishes creating a webview by
// posting back to that same thread — which is blocked, so the control never
// comes up and the window stays a bare white frame. That was 0.5.1's blank
// Preferences on Windows (no navigation was ever logged; the menu bar could
// not be hidden because there was no webview under it). An async command
// runs on a worker instead, and the main thread is free to finish the job.
// Tauri hands the handle and the argument over by value; the command owns both.
#[allow(clippy::needless_pass_by_value)]
pub async fn open_preferences(app: tauri::AppHandle, pane: String) -> AppResult<()> {
    // Only a pane name reaches the URL: anything else is not one.
    let pane: String = pane.chars().filter(char::is_ascii_alphanumeric).collect();
    if let Some(window) = app.get_webview_window(WINDOW) {
        // The window reads its pane from the hash, and listens for it to change.
        let _ = window.eval(format!("location.hash = '#preferences/{pane}?open=' + Date.now()"));
        let _ = window.unminimize();
        let _ = window.set_focus();
        return Ok(());
    }
    // The page, then the pane. The pane used to ride in the URL as
    // `index.html#preferences/<pane>`, which opened fine on macOS and a
    // blank white window on Windows — the `#` inside an `App` path does
    // not survive the `http://tauri.localhost` route there, and nothing
    // loads at all (0.5.1, verified on chris-win11). Setting the hash from
    // an initialization script runs before the page's own scripts on every
    // platform and puts no fragment in a path.
    let builder = WebviewWindowBuilder::new(&app, WINDOW, WebviewUrl::App("index.html".into()))
        .initialization_script(format!(
            // Top frame only: on Windows wry runs it in every frame (see `report.rs`).
            "if (window === window.top && !location.hash) location.hash = '#preferences/{pane}';"
        ))
        .title("Preferences");
    #[cfg(target_os = "macos")]
    let builder = match crate::test_port::data_store() {
        Some(store) => builder.data_store_identifier(store),
        None => builder,
    };
    // The same debugging port as the main window's, when one was asked for;
    // both webviews share one browser process, but the arguments are the
    // window's to declare (see `crate::browser_args`). A no-op off Windows.
    let builder = match crate::browser_args() {
        Some(args) => builder.additional_browser_args(&args),
        None => builder,
    };
    // The window draws its own title bar, as the main window does, so the
    // name sits in the middle of the app's grey rather than in the one macOS
    // paints. Both calls are macOS-only in Tauri.
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    builder
        // Said in the log, because a window that opens blank says nothing
        // itself: which URL the webview was sent to and whether the page
        // started and finished loading. What 0.5.1's blank white window on
        // Windows was diagnosed with.
        .on_navigation(|url| {
            tracing::debug!(%url, "preferences navigation");
            true
        })
        .on_page_load(|_, payload| {
            tracing::debug!(url = %payload.url(), event = ?payload.event(), "preferences page load");
        })
        .inner_size(WIDTH, HEIGHT)
        .min_inner_size(WIDTH, 480.0)
        .visible(false)
        .resizable(true)
        .accept_first_mouse(true)
        .build()
        .map(|window| {
            // On Windows and Linux the application menu is a bar on every
            // window it is set on, and it was set on this one too (0.5.1):
            // File › Import at the top of Preferences. macOS has one menu
            // bar for the app, and there this is documented as doing nothing.
            let _ = window.hide_menu();
        })
        .map_err(|e| {
            AppError::new(ErrorKind::Internal, format!("the Preferences window could not open: {e}"))
        })
}
