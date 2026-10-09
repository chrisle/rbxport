//! The webview exposes files dropped from the desktop but withholds their
//! filesystem paths, while HTML5 drag handling stays enabled for the app's own
//! drags. On macOS, read `AppKit`'s drag pasteboard (`dropped_file_paths`). On
//! Windows, the page posts the dropped `File`s to the host through `WebView2`,
//! which gives the host each file's path (`install_webview2_bridge`).

/// The key of a page message asking for the paths of the `File`s posted with
/// it, and of the host's answer. Must match `WEBVIEW2_DROP_KEY` in
/// `src/ipc/client.ts`.
#[cfg(any(windows, test))]
const REQUEST_KEY: &str = "rbxportDroppedFiles";

#[cfg(any(windows, test))]
const UNAVAILABLE: &str = "This platform did not provide the dropped files' locations.";

#[tauri::command]
#[allow(clippy::needless_pass_by_value, reason = "a Tauri command argument is deserialized, so it must be owned")]
pub fn dropped_file_paths(names: Vec<String>) -> Result<Vec<String>, String> {
    #[cfg(target_os = "macos")]
    {
        let paths = macos_paths();
        validate_paths(&names, paths)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = names;
        Err("This platform did not provide the dropped files' locations.".into())
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn macos_paths() -> Vec<String> {
    use objc2_app_kit::{NSPasteboard, NSPasteboardNameDrag};

    // SAFETY: AppKit exports these immutable, process-lifetime NSString constants.
    let board = NSPasteboard::pasteboardWithName(unsafe { NSPasteboardNameDrag });
    pasteboard_paths(&board)
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn pasteboard_paths(board: &objc2_app_kit::NSPasteboard) -> Vec<String> {
    use objc2_app_kit::NSPasteboardTypeFileURL;
    use objc2_foundation::NSURL;

    let Some(items) = board.pasteboardItems() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            // SAFETY: immutable NSString constant exported by AppKit.
            let value = item.stringForType(unsafe { NSPasteboardTypeFileURL })?;
            let url = NSURL::URLWithString(&value)?;
            if !url.isFileURL() {
                return None;
            }
            url.path().map(|path| path.to_string())
        })
        .collect()
}

/// Refuse incomplete or unrelated pasteboard contents instead of importing a
/// subset silently. Compare multisets because the DOM may reorder the files.
#[cfg(any(target_os = "macos", test))]
fn validate_paths(names: &[String], paths: Vec<String>) -> Result<Vec<String>, String> {
    let mut expected: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut actual: Vec<&str> = paths
        .iter()
        .filter_map(|path| std::path::Path::new(path).file_name()?.to_str())
        .collect();
    expected.sort_unstable();
    actual.sort_unstable();
    if expected.is_empty() || names.len() != paths.len() || expected != actual {
        return Err("The dropped files' locations could not be resolved. Please drag them from Finder again.".into());
    }
    Ok(paths)
}

/// The id of a page message asking for dropped files' paths, or `None` for
/// any other message (Tauri's own IPC, for one).
#[cfg(any(windows, test))]
fn request_id(message_json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(message_json).ok()?;
    value.get(REQUEST_KEY)?.as_str().map(str::to_owned)
}

/// The host's answer to request `id`: every path, in the order the page
/// posted the files, or why there are none.
#[cfg(any(windows, test))]
fn reply(id: &str, paths: Result<Vec<String>, String>) -> String {
    let answer = match paths {
        Ok(paths) if !paths.is_empty() && paths.iter().all(|path| !path.is_empty()) => {
            serde_json::json!({ REQUEST_KEY: id, "paths": paths })
        }
        Ok(_) => serde_json::json!({ REQUEST_KEY: id, "error": UNAVAILABLE }),
        Err(error) => serde_json::json!({ REQUEST_KEY: id, "error": error }),
    };
    answer.to_string()
}

/// Answer the page's requests for dropped files' paths on Windows.
///
/// The request is a JSON object, not a string, so wry's own handler (which
/// reads only string messages) skips it and Tauri never sees it.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn install_webview2_bridge(window: &tauri::WebviewWindow) {
    let installed = window.with_webview(|webview| {
        // SAFETY: `with_webview` runs this on the thread that owns the
        // WebView2 controller.
        if let Err(error) = unsafe { webview2::listen(&webview.controller()) } {
            tracing::warn!(%error, "dropped files' paths unavailable: WebView2 refused the message handler");
        }
    });
    if let Err(error) = installed {
        tracing::warn!(%error, "dropped files' paths unavailable: no WebView2 to listen on");
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod webview2 {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Controller, ICoreWebView2File, ICoreWebView2WebMessageReceivedEventArgs,
        ICoreWebView2WebMessageReceivedEventArgs2,
    };
    use webview2_com::{take_pwstr, WebMessageReceivedEventHandler};
    use windows_core::{Interface, HSTRING, PWSTR};

    use super::{reply, request_id, UNAVAILABLE};

    /// # Safety
    /// Must run on the thread that owns `controller`.
    pub(super) unsafe fn listen(controller: &ICoreWebView2Controller) -> windows_core::Result<()> {
        // SAFETY: the caller is on the controller's thread.
        let core = unsafe { controller.CoreWebView2()? };
        let handler = WebMessageReceivedEventHandler::create(Box::new(|sender, args| {
            let (Some(sender), Some(args)) = (sender, args) else {
                return Ok(());
            };
            let mut json = PWSTR::null();
            // SAFETY: WebView2 calls this handler on the controller's thread
            // with live arguments; `take_pwstr` frees the returned string.
            unsafe { args.WebMessageAsJson(&raw mut json)? };
            let Some(id) = request_id(&take_pwstr(json)) else {
                return Ok(());
            };
            // SAFETY: as above.
            let answer = reply(&id, unsafe { dropped_paths(&args) });
            // SAFETY: as above.
            unsafe { sender.PostWebMessageAsJson(&HSTRING::from(answer)) }
        }));
        let mut token = 0_i64;
        // SAFETY: the caller is on the controller's thread; the handler lives
        // as long as the webview, which holds a reference to it.
        unsafe { core.add_WebMessageReceived(&handler, &raw mut token) }
    }

    /// The path of every `File` posted with the message, in posted order.
    ///
    /// # Safety
    /// Must run inside the `WebMessageReceived` handler that received `args`.
    unsafe fn dropped_paths(args: &ICoreWebView2WebMessageReceivedEventArgs) -> Result<Vec<String>, String> {
        let read = || -> windows_core::Result<Vec<String>> {
            // Needs a WebView2 Runtime that supports `AdditionalObjects`.
            let args = args.cast::<ICoreWebView2WebMessageReceivedEventArgs2>()?;
            // SAFETY: the caller is inside the handler; every string WebView2
            // returns is freed by `take_pwstr`.
            unsafe {
                let objects = args.AdditionalObjects()?;
                let mut count = 0_u32;
                objects.Count(&raw mut count)?;
                (0..count)
                    .map(|index| {
                        let file = objects.GetValueAtIndex(index)?.cast::<ICoreWebView2File>()?;
                        let mut path = PWSTR::null();
                        file.Path(&raw mut path)?;
                        Ok(take_pwstr(path))
                    })
                    .collect()
            }
        };
        read().map_err(|error| {
            tracing::warn!(%error, "WebView2 did not give the dropped files' paths");
            UNAVAILABLE.to_owned()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{reply, request_id, validate_paths, REQUEST_KEY, UNAVAILABLE};

    #[test]
    fn answers_only_requests_for_dropped_files() {
        assert_eq!(request_id(r#"{"rbxportDroppedFiles":"7f"}"#), Some("7f".into()));
        // Tauri's own IPC and anything else the page posts are not ours.
        assert_eq!(request_id(r#""{\"cmd\":\"x\"}""#), None);
        assert_eq!(request_id(r#"{"cmd":"dropped_file_paths"}"#), None);
        assert_eq!(request_id(r#"{"rbxportDroppedFiles":7}"#), None);
        assert_eq!(request_id("not json"), None);
    }

    #[test]
    fn replies_with_every_path_in_posted_order() {
        let answer: serde_json::Value = serde_json::from_str(&reply(
            "7f",
            Ok(vec![r"C:\Users\dj\Music\b.mp3".into(), r"C:\Users\dj\Music\é #.mp3".into()]),
        ))
        .unwrap_or_default();
        assert_eq!(
            answer,
            serde_json::json!({
                REQUEST_KEY: "7f",
                "paths": [r"C:\Users\dj\Music\b.mp3", r"C:\Users\dj\Music\é #.mp3"],
            })
        );
    }

    #[test]
    fn replies_with_an_error_when_any_path_is_missing() {
        for paths in [Ok(vec![]), Ok(vec![String::new()]), Err(UNAVAILABLE.to_owned())] {
            let answer: serde_json::Value = serde_json::from_str(&reply("7f", paths)).unwrap_or_default();
            assert_eq!(answer, serde_json::json!({ REQUEST_KEY: "7f", "error": UNAVAILABLE }));
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[allow(unsafe_code)]
    fn reads_native_file_urls_without_touching_the_users_pasteboards() {
        use objc2_app_kit::{NSPasteboard, NSPasteboardTypeFileURL};
        use objc2_foundation::NSString;
        let name = NSString::from_str(&format!("com.rbxport.test.{}", uuid::Uuid::new_v4()));
        let board = NSPasteboard::pasteboardWithName(&name);
        board.clearContents();
        // SAFETY: immutable NSString constant exported by AppKit.
        let file_type = unsafe { NSPasteboardTypeFileURL };
        assert!(board.setString_forType(
            &NSString::from_str("file:///Music/%C3%A9%20%23.mp3"),
            file_type,
        ));
        assert_eq!(super::pasteboard_paths(&board), vec!["/Music/é #.mp3"]);
        board.clearContents();
        assert!(
            board.setString_forType(&NSString::from_str("https://example.com/a.mp3"), file_type)
        );
        assert_eq!(super::pasteboard_paths(&board), Vec::<String>::new());
        board.clearContents();
    }

    #[test]
    fn accepts_reordered_files_and_duplicate_names() {
        let names = vec!["é #.mp3".into(), "b.mp3".into(), "b.mp3".into()];
        let paths = vec!["/one/b.mp3".into(), "/two/b.mp3".into(), "/é #.mp3".into()];
        assert_eq!(validate_paths(&names, paths.clone()), Ok(paths));
    }

    #[test]
    fn refuses_missing_unrelated_and_empty_files() {
        let names = vec!["a.mp3".into(), "b.mp3".into()];
        assert!(validate_paths(&names, vec!["/a.mp3".into()]).is_err());
        assert!(validate_paths(&names, vec!["/a.mp3".into(), "/c.mp3".into()]).is_err());
        assert!(validate_paths(&[], vec![]).is_err());
    }
}
