//! Explicit checked collection removal for companions; audio is never touched.
use std::sync::Arc;
use serde::Deserialize;
use tauri::{AppHandle, Manager, Runtime};
use super::{Bridge, ScriptError, model::ScriptValue};
use crate::commands::{permanent_edit, Touched};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request { library_id: String, tracks: Vec<Track> }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Track { id: String, path: String }

pub async fn remove<R: Runtime>(app: AppHandle<R>, json: &str) -> Result<ScriptValue, ScriptError> {
    if json.len() > 262_144 { return Err(ScriptError::wrong_type("Removal request too large.")); }
    let request: Request = serde_json::from_str(json).map_err(|_| ScriptError::wrong_type("Invalid removal JSON."))?;
    if request.tracks.is_empty() || request.tracks.len() > 200 {
        return Err(ScriptError::wrong_type("Remove 1–200 checked tracks at a time."));
    }
    let bridge = Arc::clone(app.state::<Arc<Bridge>>().inner());
    bridge.refuse_if_protected()?;
    let tracks: Vec<_> = request.tracks.into_iter().map(|t| (t.id, t.path)).collect();
    permanent_edit(app.clone(), app.state(), "script_remove_collection", Touched::Tracks, move |writer| {
        bridge.refuse_if_protected().map_err(|e| rbl_db::DbError::WriteRefused(e.message))?;
        writer.delete_tracks_checked(&request.library_id, &tracks).map(|_| ())
    }).await?;
    Ok(ScriptValue::Bool(true))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::state::AppState;
    #[test]
    fn collection_removal_honors_protection_and_refreshes_the_library() {
        let dir = tempfile::tempdir().unwrap();
        let location = rbl_db::fixture::build(dir.path(), rbl_db::fixture::Shape::default()).unwrap();
        let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
        let id = rbl_db::fixture::track_id(0);
        let path: String = db.connection().query_row("SELECT FolderPath FROM djmdContent WHERE ID=?1", [&id], |r| r.get(0)).unwrap();
        let library_id = rbl_db::export_info::db_id(db.connection()).unwrap().to_string();
        let request = serde_json::json!({"library_id":library_id,"tracks":[{"id":id,"path":path}]}).to_string();
        let (library, _) = rbl_index::load(&db).unwrap();
        let state = Arc::new(AppState::with_backups(dir.path().join("backups")));
        state.set_library(library, false, db.schema().db_version, 0, location);
        let app = tauri::test::mock_app();app.manage(Arc::clone(&state));
        let bridge = Arc::new(Bridge::default());app.manage(Arc::clone(&bridge));
        let run = || tauri::async_runtime::block_on(remove(app.handle().clone(), &request));
        assert!(run().is_err());
        *bridge.preferences.write() = Some(serde_json::json!({"advanced":{"protectLibrary":false}}));
        assert_eq!(run().unwrap(), ScriptValue::Bool(true));
        assert!(state.read_db(|db| db.track_details(&id)).unwrap().is_none());
        assert_eq!(run().unwrap(), ScriptValue::Bool(true));
    }
}
