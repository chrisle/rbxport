//! The Log window.
//!
//! The status bar says a thing for a moment and then says the next one; this
//! window is where what was said can be read back. It shows the lines this
//! process has logged since it started (`crate::logging`'s buffer), the same
//! lines the daily log file has. It loads the same bundle at `index.html#log`,
//! as the Preferences and the Sync Manager do.

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_opener::OpenerExt;

use crate::error::{AppError, AppResult, ErrorKind};

pub const WINDOW: &str = "log";

const WIDTH: f64 = 900.0;
const HEIGHT: f64 = 560.0;

/// Opens the window, or brings the open one to the front.
#[tauri::command]
// `async` for the same reason `open_preferences` is: on Windows a
// synchronous command blocks the thread WebView2 needs to finish the window.
#[allow(clippy::needless_pass_by_value)]
pub async fn open_log_window(app: tauri::AppHandle) -> AppResult<()> {
    if let Some(window) = app.get_webview_window(WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }
    // The hash from an initialization script rather than the URL, for the
    // reason `sync_window.rs` gives; top frame only (see `report.rs`).
    let builder = WebviewWindowBuilder::new(&app, WINDOW, WebviewUrl::App("index.html".into()))
        .initialization_script("if (window === window.top && !location.hash) location.hash = '#log';")
        .title("Log");
    let builder = match crate::browser_args() {
        Some(args) => builder.additional_browser_args(&args),
        None => builder,
    };
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    builder
        .inner_size(WIDTH, HEIGHT)
        .min_inner_size(520.0, 320.0)
        .visible(false)
        .resizable(true)
        .build()
        .map(|window| {
            let _ = window.hide_menu();
        })
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("the Log window could not open: {e}")))
}

/// The lines this process has logged after line `after` (0 for all held).
#[tauri::command]
pub async fn log_lines(after: u64) -> AppResult<crate::logging::LogPage> {
    Ok(crate::logging::lines_after(after))
}

/// Puts a message the status bar showed in the log: an error at `warn`,
/// anything else at `info`. Most of what the bar says comes from the
/// frontend and was not logged anywhere, so it was gone once the bar moved on.
#[tauri::command]
pub async fn log_status_message(failed: bool, text: String) -> AppResult<()> {
    log_status(failed, &text);
    Ok(())
}

fn log_status(failed: bool, text: &str) {
    if failed {
        tracing::warn!(target: "rbxport::status", "{text}");
    } else {
        tracing::info!(target: "rbxport::status", "{text}");
    }
}

/// Opens the folder the daily log files are in, in the Finder or Explorer.
#[tauri::command]
pub async fn open_log_folder<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> AppResult<()> {
    crate::commands::blocking("open_log_folder", move || {
        let dir = crate::logging::log_dir();
        app.opener().open_path(dir.to_string_lossy().into_owned(), None::<&str>).map_err(|e| {
            AppError::internal("The log folder could not be opened.").with_detail(e.to_string())
        })
    })
    .await
}

/// Writes the lines the window shows to the file the save dialog picked.
#[tauri::command]
pub async fn save_log_text(path: String, text: String) -> AppResult<()> {
    crate::commands::blocking("save_log_text", move || {
        crate::grid::write_atomically(std::path::Path::new(&path), text.as_bytes()).map_err(|e| {
            AppError::internal("The log could not be saved.").with_detail(e.to_string())
        })
    })
    .await
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use std::sync::Mutex;

    use tracing_subscriber::layer::SubscriberExt;

    use super::*;
    use crate::logging::{BufferLayer, LogBuffer};

    #[test]
    fn a_status_message_reaches_the_log_at_its_level() {
        let held: &'static Mutex<LogBuffer> = Box::leak(Box::new(Mutex::new(LogBuffer::new(8))));
        let subscriber = tracing_subscriber::registry().with(BufferLayer(held));
        tracing::subscriber::with_default(subscriber, || {
            log_status(true, "The playlist could not be renamed.");
            log_status(false, "Copied 3 tracks.");
        });
        let page = held.lock().expect("buffer").after(0);
        let lines: Vec<_> = page.lines.iter().map(|l| (l.level, l.target.as_str(), l.message.as_str())).collect();
        assert_eq!(lines, [
            ("warn", "rbxport::status", "The playlist could not be renamed."),
            ("info", "rbxport::status", "Copied 3 tracks."),
        ]);
    }
}
