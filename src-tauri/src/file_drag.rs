//! Native file drags use copy-only URLs. The webview remains an HTML drop
//! destination, so the same gesture can land on a playlist, deck, or Finder.

#[tauri::command]
pub async fn drag_tracks(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, std::sync::Arc<crate::state::AppState>>,
    ids: Vec<String>,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        tracing::debug!(tracks = ids.len(), "preparing native track drag");
        let paths = track_paths(&state, &ids)?;
        let (send, mut receive) = tauri::async_runtime::channel(1);
        let handle = window.clone();
        window
            .run_on_main_thread(move || {
                tracing::debug!(files = paths.len(), "starting native file drag");
                let completed = send.clone();
                let result = crate::error::run_command(
                    "drag_tracks",
                    std::panic::AssertUnwindSafe(|| {
                        drag::start_drag(
                            &handle,
                            drag::DragItem::Files(paths),
                            drag::Image::Raw(include_bytes!("../icons/32x32.png").to_vec()),
                            move |result, _| {
                                tracing::debug!(?result, "native file drag finished");
                                let _ = completed.try_send(Ok(()));
                            },
                            drag::Options {
                                mode: drag::DragMode::Copy,
                                ..Default::default()
                            },
                        )
                        .map_err(|error| crate::AppError::internal(error.to_string()))
                    }),
                );
                if let Err(error) = result {
                    let _ = send.try_send(Err(error.message));
                }
            })
            .map_err(|error| error.to_string())?;
        receive
            .recv()
            .await
            .ok_or_else(|| "The file drag ended unexpectedly.".to_string())?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, state, ids);
        Err("Dragging files out is currently supported on macOS.".into())
    }
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn track_paths(
    state: &crate::state::AppState,
    ids: &[String],
) -> Result<Vec<std::path::PathBuf>, String> {
    let library = state.library().map_err(|error| error.message)?;
    resolve_paths(ids, |id| {
        if let Some(assets) = state.device_assets(id) {
            // The row keeps the database's spelling, while the stick's own
            // file may use another case. Never bypass its plain-path guard.
            return assets.audio_file();
        }
        library.audio_path_of(id).map(std::path::PathBuf::from)
    })
}

#[cfg(any(target_os = "macos", test))]
fn resolve_paths(
    ids: &[String],
    path_of: impl Fn(&str) -> Option<std::path::PathBuf>,
) -> Result<Vec<std::path::PathBuf>, String> {
    if ids.is_empty() {
        return Err("No tracks were selected.".into());
    }
    let mut paths = Vec::with_capacity(ids.len());
    let mut seen = std::collections::HashSet::new();
    for id in ids {
        let path = path_of(id)
            .ok_or_else(|| "A selected track has no audio file.".to_string())?;
        if !path.is_absolute() || !path.is_file() {
            return Err(format!("The audio file is missing: {}", path.display()));
        }
        if seen.insert(path.clone()) {
            paths.push(path);
        }
    }
    Ok(paths)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::resolve_paths;

    #[test]
    fn resolves_all_selected_files_and_deduplicates_paths() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("é #.mp3");
        let b = dir.path().join("b.mp3");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        let ids = vec!["1".into(), "2".into(), "3".into()];
        let result = resolve_paths(&ids, |id| {
            Some(if id == "2" { b.clone() } else { a.clone() })
        });
        assert_eq!(result.unwrap(), vec![a, b]);
    }

    #[test]
    fn refuses_empty_unknown_and_missing_files() {
        assert!(resolve_paths(&[], |_| None).is_err());
        assert!(resolve_paths(&["1".into()], |_| None).is_err());
        assert!(resolve_paths(&["1".into()], |_| Some("/missing/rbxport.mp3".into())).is_err());
        assert!(resolve_paths(&["1".into()], |_| Some("relative.mp3".into())).is_err());
    }
}
