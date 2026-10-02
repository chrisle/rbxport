//! `WKWebView` exposes dropped files but withholds their filesystem paths.
//! Read `AppKit`'s drag pasteboard while leaving HTML5 drag handling enabled.

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

#[cfg(test)]
mod tests {
    use super::validate_paths;

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
