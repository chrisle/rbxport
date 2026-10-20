//! Bounded read-only overview for companion applications; never starts analysis.
use std::sync::Arc;
use serde::Deserialize;
use serde_json::json;
use super::{ScriptError, model::ScriptValue};
use crate::state::AppState;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request { library_id: String, track_id: String }

pub async fn overview(state: Arc<AppState>, request: String) -> Result<ScriptValue, ScriptError> {
    if request.len() > 4096 { return Err(ScriptError::wrong_type("Waveform request too large.")); }
    let request: Request = serde_json::from_str(&request)
        .map_err(|_| ScriptError::wrong_type("Supply library_id and track_id as JSON strings."))?;
    if request.track_id.parse::<u64>().is_err() {
        return Err(ScriptError::wrong_type("Invalid track ID."));
    }
    let text = crate::commands::blocking("script_waveform_overview", move || {
        let id = state.read_db(|db| rbl_db::export_info::db_id(db.connection()))
            .map_err(crate::commands::write_error)?;
        if id == 0 || id.to_string() != request.library_id {
            return Err(crate::error::AppError::new(crate::error::ErrorKind::Malformed, "Library identity changed. Reconnect first."));
        }
        let library = state.library()?;
        let bytes = crate::commands::waveform_bytes(&state, &library, &state.share_root(), &request.track_id, "bands", Some(0), Some(1200))?;
        Ok(json!({"schema_version":1,"library_id":request.library_id,"track_id":request.track_id,"kind":"bands","data":bytes}).to_string())
    }).await?;
    Ok(ScriptValue::Text(text))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use rbl_db::{fixture::{self, Shape, track_id}, Library, OpenMode};

    #[test]
    fn rejects_malformed_or_unbounded_requests_before_reading_a_library() {
        let state = Arc::new(AppState::with_backups(std::env::temp_dir().join("unused-waveform-backup")));
        for request in ["{".to_owned(), "{}".to_owned(), "x".repeat(4097),
            json!({"library_id":"1","track_id":"../file"}).to_string(),
            json!({"library_id":"1","track_id":"1","path":"/tmp/audio"}).to_string()] {
            let error = tauri::async_runtime::block_on(overview(Arc::clone(&state), request)).unwrap_err();
            assert_eq!(error.code, ScriptError::WRONG_TYPE);
        }
    }

    #[test]
    fn overview_is_identity_checked_bounded_and_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let location = fixture::build(dir.path(), Shape::default()).unwrap();
        fixture::set_analysis_path(&location, 1, "/waveform/ANLZ0000.DAT").unwrap();
        let dat = rbl_anlz::resolve(&location.share_root, "/waveform/ANLZ0000.DAT");
        std::fs::create_dir_all(dat.parent().unwrap()).unwrap();
        let mut file = rbl_anlz::write::AnlzBuilder::new();
        file.waveform_scroll(b"PWV6", 3, &vec![42; 4500]);
        let bytes = file.finish();
        std::fs::write(dat.with_extension("2EX"), &bytes).unwrap();
        let db = Library::open(location.clone(), OpenMode::ReadOnly).unwrap();
        let library_id = rbl_db::export_info::db_id(db.connection()).unwrap().to_string();
        let (library, _) = rbl_index::load(&db).unwrap();
        let state = Arc::new(AppState::with_backups(dir.path().join("backups")));
        state.set_library(library, true, db.schema().db_version, 0, location.clone());
        let before = std::fs::read(&location.master_db).unwrap();
        let run = |library_id: &str, track: String| tauri::async_runtime::block_on(overview(
            Arc::clone(&state), json!({"library_id":library_id,"track_id":track}).to_string(),
        ));
        assert!(run("wrong-library", track_id(1)).is_err());
        let ScriptValue::Text(text) = run(&library_id, track_id(1)).unwrap() else { panic!("expected JSON"); };
        let response: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(response["schema_version"], 1);
        assert_eq!(response["library_id"], library_id);
        assert_eq!(response["track_id"], track_id(1));
        assert_eq!(response["kind"], "bands");
        assert_eq!(response["data"].as_array().unwrap().len(), 3600);
        assert!(response["data"].as_array().unwrap().iter().all(|v| v == 42));
        std::fs::remove_file(dat.with_extension("2EX")).unwrap();
        let ScriptValue::Text(text) = run(&library_id, track_id(1)).unwrap() else { panic!("expected JSON"); };
        assert_eq!(serde_json::from_str::<serde_json::Value>(&text).unwrap()["data"], json!([]));
        assert!(!dat.with_extension("2EX").exists());
        assert_eq!(std::fs::read(&location.master_db).unwrap(), before);
    }
}
