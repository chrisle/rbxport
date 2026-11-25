//! Navigation from Sync Manager to the existing main-owned repair dialog.
use std::sync::Arc;
use tauri::{Emitter, Manager};
use crate::error::{AppError, AppResult, ErrorKind};
use crate::missing_files_request::{Requests, MAIN};

const EVENT: &str = "missing-files:open";

fn unavailable() -> AppError {
    AppError::new(ErrorKind::NotFound, "The main window could not open Display All Missing Files.")
}

#[tauri::command]
pub async fn open_missing_files<R: tauri::Runtime>(
    app: tauri::AppHandle<R>, window: tauri::WebviewWindow<R>,
    requests: tauri::State<'_, Arc<Requests>>,
) -> AppResult<()> {
    let pending = requests.begin(window.label()).map_err(|message| AppError::new(ErrorKind::Busy, message))?;
    let id = pending.id;
    let result = async {
        let main = app.get_webview_window(MAIN).ok_or_else(unavailable)?;
        main.unminimize().map_err(|_| unavailable())?;
        main.show().map_err(|_| unavailable())?;
        main.set_focus().map_err(|_| unavailable())?;
        app.emit_to(MAIN, EVENT, id.to_string()).map_err(|_| unavailable())?;
        let opened = tauri::async_runtime::spawn_blocking(move || pending.answer.recv_timeout(crate::scripting::ANSWER_TIMEOUT))
            .await.map_err(|_| unavailable())?.map_err(|_| unavailable())?;
        if opened { Ok(()) } else { Err(unavailable()) }
    }.await;
    requests.cancel(id);
    if result.is_err() {
        // The main owner may have taken focus before failing to answer. Keep
        // the still-usable originating prompt in front, if it remains open.
        let _ = window.set_focus();
    }
    result
}

#[tauri::command]
pub async fn missing_files_ready<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>, requests: tauri::State<'_, Arc<Requests>>,
    id: String, opened: bool,
) -> AppResult<bool> {
    let id = id.parse().map_err(|_| unavailable())?;
    requests.reply(window.label(), id, opened).map_err(|message| AppError::new(ErrorKind::Busy, message))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "mock-window ownership fixtures")]
mod tests {
    use super::*;
    use tauri::{Listener, WebviewUrl, WebviewWindowBuilder};
    use crate::missing_files_request::SOURCE;

    #[test]
    fn a_missing_main_window_leaves_no_pending_handoff() {
        let requests = Arc::new(Requests::default());
        let app = tauri::test::mock_builder().manage(Arc::clone(&requests))
            .build(tauri::test::mock_context(tauri::test::noop_assets())).unwrap();
        let sync = WebviewWindowBuilder::new(&app, SOURCE, WebviewUrl::App("index.html".into())).build().unwrap();
        let result = tauri::async_runtime::block_on(open_missing_files(app.handle().clone(), sync, app.state()));
        assert!(result.is_err());
        assert!(requests.begin(SOURCE).is_ok(), "unavailable owner cannot strand a pending request");
    }

    #[test]
    fn native_request_reaches_only_main_and_needs_its_acknowledgment() {
        let requests = Arc::new(Requests::default());
        let app = tauri::test::mock_builder().manage(Arc::clone(&requests))
            .build(tauri::test::mock_context(tauri::test::noop_assets())).unwrap();
        let main = WebviewWindowBuilder::new(&app, MAIN, WebviewUrl::App("index.html".into())).build().unwrap();
        let sync = WebviewWindowBuilder::new(&app, SOURCE, WebviewUrl::App("index.html".into())).build().unwrap();
        let wrong = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let seen = Arc::clone(&wrong);
        sync.listen(EVENT, move |_| { seen.store(true, std::sync::atomic::Ordering::SeqCst); });
        let replied = Arc::clone(&requests);
        main.listen(EVENT, move |event| {
            let id: String = serde_json::from_str(event.payload()).unwrap();
            assert!(replied.reply(MAIN, id.parse().unwrap(), true).unwrap());
        });
        tauri::async_runtime::block_on(open_missing_files(app.handle().clone(), sync.clone(), app.state())).unwrap();
        assert!(!wrong.load(std::sync::atomic::Ordering::SeqCst));
        let pending = requests.begin(SOURCE).unwrap();
        let denied = tauri::async_runtime::block_on(missing_files_ready(sync, app.state(), pending.id.to_string(), true));
        assert!(denied.is_err(), "a sync webview cannot impersonate the displayed main dialog");
        assert!(pending.answer.try_recv().is_err());
    }
}
