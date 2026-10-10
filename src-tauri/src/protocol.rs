//! The `rbl://` scheme, which serves artwork straight to the webview.
//!
//! Artwork does not go through `invoke`: a JPEG is tens of kilobytes, the IPC
//! cap is 64 KB, and base64 in a JSON response would cost a main-thread decode
//! per row. An `<img src>` lets the webview fetch, decode and cache it off the
//! UI thread, which is the whole point.
//!
//! # Why it takes a track id and not a path
//!
//! The webview names a **track**, and the path is looked up in the index. A
//! scheme that accepted a path would hand anything running in the webview a
//! read of any file the app can reach. The id is resolved against the loaded
//! library, and the resulting path is checked to still sit under the share
//! root — belt and braces, because `ImagePath` comes from the database rather
//! than from us.
//!
//! # Where rekordbox reads it from
//!
//! rekordbox opens the file at `masterDbDirectory + "/share"` joined with
//! `ImagePath` as it is (`getCloudSharedPath`,
//! `rekordboxDBController::getArtworkInfo`, `ArtworkCache::getFromURI`
//! loading `juce::File(path)`) [OBS static, rekordbox 7.2.11 macOS]. It does
//! not resolve links first, so a share whose `Artwork` folder is a link to
//! another disk, or a volume the system cannot give a final path for, still
//! shows its artwork there. The check here is on the stored path's own
//! components for the same reason (issue #271).

use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::http::{Request, Response, StatusCode};

use crate::state::AppState;

/// Artwork, by track id: `rbl://localhost/artwork/<id>`. The only kind of
/// request: audio used to be served here for an `<audio>` element, and
/// playback is the Rust engine now.
///
/// The kind is the first path segment, not the host. On Windows `WebView2` loads
/// no custom scheme, so the page asks for `http://rbl.localhost/…` and `wry`
/// hands it over as `rbl://localhost/…`; the host is `localhost` there, and
/// it is the same on macOS and Linux because the page asks for it there too.
const ARTWORK: &str = "artwork";

/// Refuses anything larger. Real artwork is tens of kilobytes; a file this big
/// is not album art and should not be read into memory to find out.
pub(crate) const MAX_BYTES: u64 = 8 * 1024 * 1024;

/// Answers one `rbl://` request.
pub fn handle(state: &Arc<AppState>, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let Some(track_id) = artwork_id(request.uri().path()) else {
        return status(StatusCode::NOT_FOUND);
    };
    if track_id.is_empty() {
        return status(StatusCode::BAD_REQUEST);
    }

    let Ok(library) = state.library() else {
        // The last visible screen survives a cold start. Its files are
        // trusted app-owned copies keyed only by numeric id, never paths.
        return crate::screen_cache::cached_artwork(track_id)
            .map_or_else(|| status(StatusCode::SERVICE_UNAVAILABLE), image_response);
    };

    let Some(relative) = library.artwork_path_of(track_id) else {
        return status(StatusCode::NOT_FOUND);
    };
    if relative.is_empty() {
        return status(StatusCode::NOT_FOUND);
    }

    let share = state.share_root();
    let Some(path) = resolve_under(&share, relative) else {
        tracing::warn!(%relative, "artwork path escapes the share root; refused");
        return status(StatusCode::FORBIDDEN);
    };

    match std::fs::metadata(&path) {
        Ok(meta) if !meta.is_file() => return status(StatusCode::NOT_FOUND),
        Ok(meta) if meta.len() > MAX_BYTES => return status(StatusCode::PAYLOAD_TOO_LARGE),
        Ok(_) => {}
        Err(error) => {
            note_unreadable(&path, &error);
            return status(StatusCode::NOT_FOUND);
        }
    }
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            note_unreadable(&path, &error);
            return status(StatusCode::NOT_FOUND);
        }
    };

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", content_type(&path))
        .header("Access-Control-Allow-Origin", "*")
        // Artwork for a given track never changes without the library
        // reloading, and the frontend re-requests with a new generation then.
        .header("Cache-Control", "max-age=31536000, immutable")
        .body(bytes)
        .unwrap_or_else(|_| status(StatusCode::INTERNAL_SERVER_ERROR))
}

/// The track id in `/artwork/<id>`, or `None` for a path that asks for
/// anything else. Whatever follows the id is ignored.
fn artwork_id(path: &str) -> Option<&str> {
    let mut parts = path.trim_start_matches('/').split('/');
    (parts.next() == Some(ARTWORK)).then(|| parts.next().unwrap_or(""))
}

fn image_response(bytes: Vec<u8>) -> Response<Vec<u8>> {
    let content_type = if bytes.starts_with(b"\x89PNG") { "image/png" }
        else if bytes.starts_with(b"GIF8") { "image/gif" }
        else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") { "image/webp" }
        else { "image/jpeg" };
    Response::builder().status(StatusCode::OK)
        .header("Content-Type", content_type)
        .header("Access-Control-Allow-Origin", "*")
        // This is a startup placeholder. The table remounts once the live
        // library is ready and must be allowed to fetch the authoritative art.
        .header("Cache-Control", "no-store")
        .body(bytes).unwrap_or_else(|_| status(StatusCode::INTERNAL_SERVER_ERROR))
}

/// The first artwork file that could not be read, logged once a session.
///
/// A row asks for its sleeve every time it scrolls into view, so logging each
/// failure would bury the log; one line is enough to say where the app looked
/// and why it got nothing, which is what a report needs.
static LOGGED_UNREADABLE: AtomicBool = AtomicBool::new(false);

fn note_unreadable(path: &Path, error: &std::io::Error) {
    if !LOGGED_UNREADABLE.swap(true, Ordering::Relaxed) {
        tracing::warn!(path = %path.display(), %error, "artwork file could not be read; later failures are not logged");
    }
}

/// Joins a share-relative path onto the root, refusing anything that climbs out.
///
/// `ImagePath` comes from the database, so it is not ours to trust: a value
/// with `..` in it, or a part that names a drive or a root of its own, would
/// otherwise read outside the library. Each part has to be one plain name.
///
/// The check is on the stored path, not on where the file finally lives.
/// rekordbox opens the joined path as it is, and an earlier version here that
/// canonicalised both sides refused every sleeve when the share's `Artwork`
/// folder was a link to another disk, or when the volume had no final path
/// the system would give (some RAM, network and virtual drives on Windows) —
/// while the waveforms and the LINK server, which join the same way rekordbox
/// does, still worked (#271).
pub(crate) fn resolve_under(root: &Path, relative: &str) -> Option<PathBuf> {
    let mut out = root.to_path_buf();
    for part in relative.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return None,
            _ => {
                let mut components = Path::new(part).components();
                match (components.next(), components.next()) {
                    (Some(Component::Normal(_)), None) => out.push(part),
                    _ => return None,
                }
            }
        }
    }
    Some(out)
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        // rekordbox writes .jpg for everything else it caches.
        _ => "image/jpeg",
    }
}

/// Answers one request and hands the response to `respond`, whatever happens.
///
/// The responder is the whole reason this is a function rather than a closure
/// in `lib.rs`: `wry` gives the asynchronous scheme handler one, and a request
/// that never gets a response is a webview waiting for ever — an image that
/// never appears, a track that never starts. So a panic inside `handle`
/// answers 500 rather than escaping, and the panic is logged rather than lost.
pub fn serve<R>(state: &Arc<AppState>, request: &Request<Vec<u8>>, respond: R)
where
    R: FnOnce(Response<Vec<u8>>),
{
    serve_with(|| handle(state, request), respond);
}

/// The guard itself, with the answer as a closure so a test can make it panic.
fn serve_with<A, R>(answer: A, respond: R)
where
    A: FnOnce() -> Response<Vec<u8>>,
    R: FnOnce(Response<Vec<u8>>),
{
    let answered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(answer));
    respond(answered.unwrap_or_else(|_| {
        tracing::error!("the rbl:// handler panicked");
        status(StatusCode::INTERNAL_SERVER_ERROR)
    }));
}

fn status(code: StatusCode) -> Response<Vec<u8>> {
    Response::builder()
        .status(code)
        .body(Vec::new())
        .unwrap_or_else(|_| Response::new(Vec::new()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// Counts how often the responder was called, which is the thing `wry`
    /// cares about: twice is a panic inside the webview, never is a request
    /// that hangs for ever.
    fn responses_of<A>(answer: A) -> Vec<Response<Vec<u8>>>
    where
        A: FnOnce() -> Response<Vec<u8>>,
    {
        let mut out = Vec::new();
        serve_with(answer, |response| out.push(response));
        out
    }

    #[test]
    fn an_answer_reaches_the_responder_once() {
        let out = responses_of(|| status(StatusCode::OK));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].status(), StatusCode::OK);
    }

    #[test]
    fn a_panicking_answer_becomes_a_500_rather_than_a_request_that_never_returns() {
        // The whole point of the guard: `wry` hands the asynchronous handler a
        // responder, and an image whose request is never answered is a webview
        // waiting for ever.
        let out = responses_of(|| panic!("the analysis file was truncated"));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn a_request_before_the_library_is_loaded_is_answered_too() {
        // `serve` on the real handler, off the main thread in production: a
        // request that arrives during the load must come back, not hang.
        let state = Arc::new(AppState::new());
        let request = Request::builder()
            .uri("rbl://localhost/artwork/12345")
            .body(Vec::new())
            .unwrap();
        let mut out = Vec::new();
        serve(&state, &request, |response| out.push(response));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn a_request_that_is_not_for_artwork_is_refused() {
        let state = Arc::new(AppState::new());
        for uri in ["rbl://localhost/etc/passwd", "rbl://etc/passwd", "rbl://localhost/"] {
            let request = Request::builder().uri(uri).body(Vec::new()).unwrap();
            let mut out = Vec::new();
            serve(&state, &request, |response| out.push(response));
            assert_eq!(out[0].status(), StatusCode::NOT_FOUND, "{uri}");
        }
    }

    #[test]
    fn the_track_id_is_read_from_the_path_whatever_the_host() {
        // macOS and Linux ask for `rbl://localhost/artwork/7`; Windows asks
        // for `http://rbl.localhost/artwork/7`, which `wry` hands over as
        // the same `rbl://localhost/artwork/7`.
        assert_eq!(artwork_id("/artwork/7"), Some("7"));
        assert_eq!(artwork_id("/artwork/7/anything"), Some("7"));
        assert_eq!(artwork_id("/artwork/"), Some(""));
        assert_eq!(artwork_id("/artwork"), Some(""));
        assert_eq!(artwork_id("/7"), None);
        assert_eq!(artwork_id("/"), None);
    }

    #[test]
    fn a_relative_path_resolves_under_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("PIONEER/Artwork/abc");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("artwork.jpg"), b"x").unwrap();

        let got = resolve_under(dir.path(), "/PIONEER/Artwork/abc/artwork.jpg");
        assert!(got.is_some());
        assert!(got.unwrap().ends_with("artwork.jpg"));
    }

    #[test]
    fn a_path_that_climbs_out_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("share")).unwrap();
        for attempt in [
            "../../../etc/passwd",
            "/PIONEER/../../etc/passwd",
            "..\\..\\Windows\\System32",
        ] {
            assert!(resolve_under(&dir.path().join("share"), attempt).is_none(), "{attempt}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_artwork_folder_linked_to_another_disk_is_still_read() {
        // A share whose `Artwork` folder was moved to a bigger disk and left
        // behind as a link: rekordbox opens the joined path as it is and
        // shows the sleeve, so this must too (#271).
        let dir = tempfile::tempdir().unwrap();
        let share = dir.path().join("share");
        let elsewhere = dir.path().join("other-disk/Artwork");
        std::fs::create_dir_all(share.join("PIONEER")).unwrap();
        std::fs::create_dir_all(elsewhere.join("abc/uuid")).unwrap();
        std::fs::write(elsewhere.join("abc/uuid/artwork.jpg"), b"jpeg").unwrap();
        std::os::unix::fs::symlink(&elsewhere, share.join("PIONEER/Artwork")).unwrap();

        let got = resolve_under(&share, "/PIONEER/Artwork/abc/uuid/artwork.jpg").unwrap();
        assert_eq!(got, share.join("PIONEER/Artwork/abc/uuid/artwork.jpg"));
        assert_eq!(std::fs::read(got).unwrap(), b"jpeg");
    }

    #[test]
    fn the_path_is_joined_without_asking_the_volume_for_its_final_path() {
        // Some volumes cannot say where a file finally lives (Windows RAM,
        // network and virtual drives), and asking refused every sleeve on
        // them. The stored path is checked on its own parts instead.
        let root = Path::new("/no/such/volume/share");
        assert_eq!(
            resolve_under(root, "/PIONEER/Artwork/00a/uuid/artwork.jpg"),
            Some(root.join("PIONEER").join("Artwork").join("00a").join("uuid").join("artwork.jpg")),
        );
        assert_eq!(
            resolve_under(root, "\\PIONEER\\Artwork\\00a\\uuid\\artwork.jpg"),
            Some(root.join("PIONEER").join("Artwork").join("00a").join("uuid").join("artwork.jpg")),
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_part_that_names_a_drive_is_refused() {
        let root = Path::new("C:\\share");
        for attempt in ["/PIONEER/C:/Windows/win.ini", "D:\\secret.jpg", "/PIONEER/D:"] {
            assert!(resolve_under(root, attempt).is_none(), "{attempt}");
        }
    }

    #[test]
    fn content_type_follows_the_extension() {
        assert_eq!(content_type(Path::new("a/b.png")), "image/png");
        assert_eq!(content_type(Path::new("a/b.PNG")), "image/png");
        assert_eq!(content_type(Path::new("a/b.jpg")), "image/jpeg");
        // rekordbox writes .jpg for everything else it caches.
        assert_eq!(content_type(Path::new("a/b")), "image/jpeg");
    }
}
