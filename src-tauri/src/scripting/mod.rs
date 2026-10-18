//! AppleScript.
//!
//! `cocoa.rs` defines the classes the dictionary (`src-tauri/rbxport.sdef`)
//! names and hooks them into Cocoa Scripting; it is compiled on macOS only,
//! and the rest of this module is there on every platform so the window can
//! call the same commands everywhere.
//!
//! A script reads straight from the backend: the library, the decks, the
//! players on the network, the volumes. What it changes goes one of two ways.
//! An edit to the library runs the backend command the window's own edit
//! runs, and that command's `library:changed` brings the window up to date.
//! Anything the window itself holds — which track a deck shows, PLAY, LINK, a
//! preference, an export and its progress — is asked of the window over
//! `script:request` and answered with `script_reply`, so a script and a click
//! go down the same path.
//!
//! The preferences live in the window's `localStorage`. The window mirrors
//! them here with `script_preferences` whenever they change, so a script
//! reads them without a round trip, and an edit that never passes through
//! the window still honours Library Protection.

// Only `cocoa.rs` drives most of this, and it is compiled on macOS alone;
// elsewhere the model and the request plumbing are there with nothing to
// call them yet.
#![cfg_attr(not(target_os = "macos"), allow(dead_code, reason = "the AppleScript bridge that uses it is macOS-only"))]

pub mod model;
mod my_tags;

#[cfg(target_os = "macos")]
mod cocoa;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use serde_json::Value as Json;
use tauri::{AppHandle, Emitter, Listener, Manager, Runtime, State};

use crate::error::{AppError, ErrorKind};

/// The event a request to the window goes out on.
pub const REQUEST_EVENT: &str = "script:request";

/// The window the requests go to.
const MAIN_WINDOW: &str = "main";

/// How long the window has to answer anything but an export: a deck load
/// waits for the file to open, which is well under a second on a local disk.
pub const ANSWER_TIMEOUT: Duration = Duration::from_secs(30);

/// An export writes every file on a playlist to a stick. AppleScript gives up
/// on the reply after two minutes unless the script says otherwise; the
/// export carries on either way, so this only bounds how long a request can
/// be left waiting for a window that went away.
pub const EXPORT_TIMEOUT: Duration = Duration::from_secs(6 * 60 * 60);

/// An error as a script is given it: an Apple Event error number and the
/// message `on error` receives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptError {
    pub code: i32,
    pub message: String,
}

impl ScriptError {
    /// `errAEEventFailed`.
    pub const FAILED: i32 = -10_000;
    /// `errAENotModifiable`.
    pub const NOT_MODIFIABLE: i32 = -10_003;
    /// `errAENoSuchObject`.
    pub const NO_SUCH_OBJECT: i32 = -1728;
    /// `errAEWrongDataType`.
    pub const WRONG_TYPE: i32 = -1703;
    /// `errAEParamMissed`.
    pub const MISSING_PARAMETER: i32 = -1715;

    pub fn failed(message: impl Into<String>) -> Self {
        Self { code: Self::FAILED, message: message.into() }
    }
    pub fn not_modifiable(message: impl Into<String>) -> Self {
        Self { code: Self::NOT_MODIFIABLE, message: message.into() }
    }
    pub fn no_such_object(message: impl Into<String>) -> Self {
        Self { code: Self::NO_SUCH_OBJECT, message: message.into() }
    }
    pub fn wrong_type(message: impl Into<String>) -> Self {
        Self { code: Self::WRONG_TYPE, message: message.into() }
    }
    pub fn missing_parameter(message: impl Into<String>) -> Self {
        Self { code: Self::MISSING_PARAMETER, message: message.into() }
    }
    fn not_ready() -> Self {
        Self::failed("rbxport's window is still starting. Try again in a moment.")
    }
}

impl From<AppError> for ScriptError {
    fn from(error: AppError) -> Self {
        let code = match error.kind {
            ErrorKind::ReadOnly => Self::NOT_MODIFIABLE,
            ErrorKind::NotFound => Self::NO_SUCH_OBJECT,
            _ => Self::FAILED,
        };
        Self { code, message: error.message }
    }
}

/// What the window is asked to do.
#[derive(Clone, Serialize)]
struct Request<'a> {
    id: u64,
    action: &'a str,
    args: Json,
}

/// The requests waiting on the window, the preferences it last mirrored, and
/// the volumes last seen.
#[derive(Default)]
pub struct Bridge {
    /// Set once the window is listening for requests. Before that a request
    /// would be emitted to nobody and wait out its timeout.
    ready: AtomicBool,
    next: AtomicU64,
    waiting: Mutex<HashMap<u64, mpsc::Sender<Result<Json, String>>>>,
    preferences: RwLock<Option<Json>>,
    /// The export destinations, refreshed off the main thread whenever a
    /// volume comes or goes: one listing is tens of milliseconds and up to
    /// seconds while a card reader wakes [OBS], which a script's reply must
    /// not wait on, and Cocoa Scripting answers on the main thread.
    devices: RwLock<Vec<rbl_devices::Device>>,
}

impl Bridge {
    /// Asks the window to do something and waits for its answer. Blocks, so
    /// it is only ever called on a worker thread.
    pub fn ask<R: Runtime>(&self, app: &AppHandle<R>, action: &str, args: Json, timeout: Duration) -> Result<Json, ScriptError> {
        if !self.ready.load(Ordering::SeqCst) {
            return Err(ScriptError::not_ready());
        }
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        let (sender, answer) = mpsc::channel();
        self.waiting.lock().insert(id, sender);
        if let Err(e) = app.emit_to(MAIN_WINDOW, REQUEST_EVENT, Request { id, action, args }) {
            self.waiting.lock().remove(&id);
            tracing::warn!(action, error = %e, "a script request did not reach the window");
            return Err(ScriptError::not_ready());
        }
        let answer = answer.recv_timeout(timeout);
        self.waiting.lock().remove(&id);
        match answer {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(message)) => Err(ScriptError::failed(message)),
            Err(_) => {
                tracing::warn!(action, "the window did not answer a script request");
                Err(ScriptError::failed("rbxport's window did not answer."))
            }
        }
    }

    /// The preferences as the window last mirrored them.
    pub fn preferences(&self) -> Option<Json> {
        self.preferences.read().clone()
    }

    /// Library Protection, which only the window's preferences know about.
    /// An edit is refused while it is on, as the window refuses one, and
    /// while it is unknown because the window has not said yet.
    pub fn refuse_if_protected(&self) -> Result<(), ScriptError> {
        let protected = self.preferences.read().as_ref().map(|p| p["advanced"]["protectLibrary"].as_bool() != Some(false));
        match protected {
            None => Err(ScriptError::not_ready()),
            // The same words as the window's (`refusal` in `src/lib/menu.ts`).
            Some(true) => Err(ScriptError::not_modifiable(
                "Editing is locked by Library Protection. Turn it off in Preferences to edit.",
            )),
            Some(false) => Ok(()),
        }
    }

    pub fn devices(&self) -> Vec<rbl_devices::Device> {
        self.devices.read().clone()
    }

    fn refresh_devices(self: &Arc<Self>) {
        let bridge = Arc::clone(self);
        tauri::async_runtime::spawn_blocking(move || {
            *bridge.devices.write() = rbl_devices::list();
        });
    }
}

/// Manages the bridge and, on macOS, makes the application scriptable.
/// Called from `setup`, before the event loop hands Cocoa Scripting any
/// Apple Event.
pub fn install(app: &AppHandle) {
    let bridge = Arc::new(Bridge::default());
    bridge.refresh_devices();
    {
        let bridge = Arc::clone(&bridge);
        app.listen_any("devices:changed", move |_| bridge.refresh_devices());
    }
    app.manage(bridge);
    #[cfg(target_os = "macos")]
    cocoa::install(app);
}

/// The window is listening on `script:request`.
#[tauri::command]
pub async fn script_ready(bridge: State<'_, Arc<Bridge>>) -> Result<(), AppError> {
    bridge.ready.store(true, Ordering::SeqCst);
    Ok(())
}

/// The window's answer to a request: the value, or why it could not.
#[tauri::command]
pub async fn script_reply(
    bridge: State<'_, Arc<Bridge>>,
    id: u64,
    value: Option<Json>,
    error: Option<String>,
) -> Result<(), AppError> {
    let outcome = match error {
        Some(message) => Err(message),
        None => Ok(value.unwrap_or(Json::Null)),
    };
    if let Some(waiting) = bridge.waiting.lock().remove(&id) {
        // The receiver is gone when the request timed out; nothing to do.
        let _ = waiting.send(outcome);
    }
    Ok(())
}

/// The preferences as the window holds them now.
#[tauri::command]
// Tauri hands the argument over by value; it is stored as it is.
#[allow(clippy::needless_pass_by_value)]
pub async fn script_preferences(bridge: State<'_, Arc<Bridge>>, preferences: Json) -> Result<(), AppError> {
    *bridge.preferences.write() = Some(preferences);
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn protection_is_assumed_until_the_window_says_otherwise() {
        let bridge = Bridge::default();
        assert!(bridge.refuse_if_protected().is_err());
        *bridge.preferences.write() = Some(json!({ "advanced": { "protectLibrary": true } }));
        assert_eq!(bridge.refuse_if_protected().unwrap_err().code, ScriptError::NOT_MODIFIABLE);
        *bridge.preferences.write() = Some(json!({ "advanced": {} }));
        assert!(bridge.refuse_if_protected().is_err());
        *bridge.preferences.write() = Some(json!({ "advanced": { "protectLibrary": false } }));
        assert!(bridge.refuse_if_protected().is_ok());
    }

    #[test]
    fn a_backend_error_keeps_its_message_and_gets_a_script_error_number() {
        let error = ScriptError::from(AppError::new(ErrorKind::ReadOnly, "rekordbox is running."));
        assert_eq!(error.code, ScriptError::NOT_MODIFIABLE);
        assert_eq!(error.message, "rekordbox is running.");
        assert_eq!(ScriptError::from(AppError::new(ErrorKind::NotFound, "gone")).code, ScriptError::NO_SUCH_OBJECT);
    }
}
