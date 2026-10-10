//! Process-entry to frontend paint opportunities, including webview startup.
use std::collections::BTreeSet;
use std::sync::{atomic::{AtomicU8, Ordering}, Mutex, OnceLock};
use std::time::{Duration, Instant};

static START: OnceLock<Instant> = OnceLock::new();
static REPORTED: AtomicU8 = AtomicU8::new(0);

const TEST_HEADLESS_ENV: &str = "RBXPORT_TEST_HEADLESS";

/// How long a window waits for its page to ask to be shown before it is shown
/// without being asked.
///
/// The page asks within a second of React's first commit, and gives up
/// waiting for data a second after that (`src/lib/windowReady.ts`). Five
/// seconds leaves a slow machine's first launch room to get there, and is
/// still short enough that nobody concludes the app did not start.
pub const REVEAL_DEADLINE: Duration = Duration::from_secs(5);

/// The windows whose page has asked to be shown, by label.
static REVEALS: Reveals = Reveals::new();

pub fn begin() { let _ = START.set(Instant::now()); }

/// Which windows' pages have asked to be shown since the window was built.
///
/// Keyed by label, and forgotten when a window of that label is built again:
/// Preferences and the Sync Manager close and reopen under the same label,
/// and the reopened window's page has to ask for itself.
pub struct Reveals(Mutex<BTreeSet<String>>);

impl Reveals {
    pub const fn new() -> Self { Self(Mutex::new(BTreeSet::new())) }

    /// A window with this label was just built; its page has asked nothing.
    pub fn forget(&self, label: &str) {
        if let Ok(mut asked) = self.0.lock() {
            asked.remove(label);
        }
    }

    /// The page in this window asked to be shown.
    pub fn asked(&self, label: &str) {
        if let Ok(mut asked) = self.0.lock() {
            asked.insert(label.to_owned());
        }
    }

    pub fn has_asked(&self, label: &str) -> bool {
        self.0.lock().is_ok_and(|asked| asked.contains(label))
    }
}

/// Whether a window still hidden at [`REVEAL_DEADLINE`] is shown anyway.
///
/// Only one the page never asked for: a page that asked and was then
/// minimised is not visible either, and is not to be pulled back up. Only
/// one that is still there and still hidden: `visible` is `None` once the
/// window has been closed. And never in the headless test mode, which keeps
/// windows off the operator's desktop on purpose.
#[must_use]
pub fn needs_rescue(page_asked: bool, visible: Option<bool>, headless: bool) -> bool {
    !page_asked && visible == Some(false) && !headless
}

/// The compiled-app integration suites still need the webview to render so
/// their test port can evaluate the page, but the CDJ rigs do not need to
/// put that window on the operator's desktop. Requiring the debug-only test
/// port as well as the explicit opt-in keeps an inherited environment
/// variable from hiding an ordinary app launch.
fn headless() -> bool {
    cfg!(debug_assertions)
        && std::env::var_os(crate::test_port::PORT_ENV).is_some()
        && std::env::var_os(TEST_HEADLESS_ENV).is_some()
}

/// Shows `window` after [`REVEAL_DEADLINE`] if its page has not asked by then.
///
/// Every window is built hidden and the page shows it once React has drawn
/// (`show_window`). That leaves the window's visibility resting entirely on
/// the page: a script that stops before React's first commit, or a webview
/// too old to run it, and the app is running with its menu bar and Dock icon
/// and no window at all (#247). Shown empty, the window can at least be seen,
/// moved and closed, and the log says why it was empty.
pub fn reveal_if_page_stalls<R: tauri::Runtime>(window: &tauri::Window<R>) {
    let label = window.label().to_owned();
    REVEALS.forget(&label);
    let window = window.clone();
    let spawned = std::thread::Builder::new().name("reveal-window".into()).spawn(move || {
        std::thread::sleep(REVEAL_DEADLINE);
        let asked = REVEALS.has_asked(&label);
        if asked {
            return;
        }
        let visible = window.is_visible().ok();
        if visible.is_none() {
            // Closed before the deadline; nothing to show.
            return;
        }
        tracing::warn!(
            window = %label,
            deadline_s = REVEAL_DEADLINE.as_secs(),
            visible = ?visible,
            "the window's page did not ask to be shown: it did not finish its first render, or could not reach the app"
        );
        if needs_rescue(asked, visible, headless()) {
            if let Err(e) = window.show() {
                tracing::warn!(window = %label, error = %e, "could not show the window");
                return;
            }
            let _ = window.set_focus();
        }
    });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "could not start the window's reveal timer");
    }
}

/// Reveals a webview only after React has committed its window-specific UI.
/// All app windows start hidden, preventing the platform's empty webview
/// background from flashing before the first useful frame is available.
#[tauri::command]
#[allow(clippy::needless_pass_by_value, reason = "Tauri injects the calling window by value")]
pub fn show_window(window: tauri::WebviewWindow) -> Result<(), String> {
    // Recorded before the headless check: in that mode the page still ran,
    // and the reveal timer must not report it as having failed.
    REVEALS.asked(window.label());
    if headless() {
        return Ok(());
    }
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value, reason = "a Tauri command's injected window and deserialized argument are owned")]
pub fn startup_milestone(window: tauri::WebviewWindow, phase: String) {
    if window.label() != "main" { return; }
    let flag = match phase.as_str() {
        "shell-painted" => 1,
        "first-rows-painted" => 2,
        _ => return,
    };
    if REPORTED.fetch_or(flag, Ordering::Relaxed) & flag != 0 { return; }
    if let Some(start) = START.get() {
        tracing::debug!(phase, elapsed_ms = start.elapsed().as_millis(), "startup milestone");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_whose_page_never_asked_is_shown_at_the_deadline() {
        assert!(needs_rescue(false, Some(false), false));
    }

    #[test]
    fn a_window_whose_page_asked_is_left_as_it_is() {
        // Including when it is hidden again by now, as a minimised window is.
        assert!(!needs_rescue(true, Some(false), false));
        assert!(!needs_rescue(true, Some(true), false));
    }

    #[test]
    fn a_window_already_showing_or_gone_is_not_touched() {
        assert!(!needs_rescue(false, Some(true), false));
        assert!(!needs_rescue(false, None, false));
    }

    #[test]
    fn the_headless_test_mode_keeps_every_window_hidden() {
        assert!(!needs_rescue(false, Some(false), true));
    }

    #[test]
    fn a_rebuilt_window_has_to_ask_again() {
        let reveals = Reveals::new();
        assert!(!reveals.has_asked("preferences"));
        reveals.asked("preferences");
        assert!(reveals.has_asked("preferences"));
        assert!(!reveals.has_asked("main"), "asking is per window");
        // Closed and opened again under the same label.
        reveals.forget("preferences");
        assert!(!reveals.has_asked("preferences"));
    }
}
