//! Companion-facing My Tag operations. Reads run off the Cocoa thread;
//! writes share the UI's writer, undo history, and refresh path.

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

use rbl_db::write::MyTagOperation;
use serde_json::json;
use tauri::{AppHandle, Manager, Runtime};

use super::{Bridge, ScriptError};
use super::model::ScriptValue;
use crate::commands::{blocking, recorded_edit, write_error, Touched};
use crate::state::{AppState, LibraryEdit};

/// A versioned JSON catalog keeps category identity intact even when names
/// overlap. IDs belong to this library; a missing DBID is not a global ID.
pub fn catalog_json(library_id: u64, rows: &[rbl_db::export_info::MyTagRow]) -> String {
    let categories: Vec<_> = rows.iter().filter(|row| row.attribute == 1).collect();
    // Tags grouped by their category's place, each category's in `Seq`
    // order (the rows come in `Seq` order and the sort is stable), as
    // rekordbox lists them; tags of no live category last.
    let mut tags: Vec<_> = rows.iter().filter(|row| row.attribute == 0).collect();
    tags.sort_by_key(|tag| categories.iter().position(|category| category.id == tag.parent).unwrap_or(usize::MAX));
    let categories: Vec<_> = categories.into_iter()
        .map(|row| json!({ "id": row.id, "name": row.name })).collect();
    let tags: Vec<_> = tags.into_iter()
        .map(|row| json!({ "id": row.id, "name": row.name, "category_id": row.parent })).collect();
    json!({
        "schema_version": 1,
        "library_id": (library_id != 0).then(|| library_id.to_string()),
        "capabilities": ["my_tags.read", "my_tags.add", "my_tags.remove", "my_tags.reorder", "collection.remove_checked", "waveform.overview.read"],
        "categories": categories,
        "tags": tags,
    }).to_string()
}

pub async fn catalog(state: Arc<AppState>) -> Result<ScriptValue, ScriptError> {
    let text = blocking("script_my_tag_catalog", move || {
        state.read_db(|db| {
            let conn = db.connection();
            Ok(catalog_json(rbl_db::export_info::db_id(conn)?, &rbl_db::export_info::my_tags(conn)?))
        }).map_err(write_error)
    }).await?;
    Ok(ScriptValue::Text(text))
}

pub async fn assigned(state: Arc<AppState>, track: String) -> Result<ScriptValue, ScriptError> {
    let details = blocking("script_read_my_tags", move || {
        state.read_db(|db| db.track_details(&track)).map_err(write_error)
    }).await?;
    let details = details.ok_or_else(|| ScriptError::no_such_object("That track is no longer in the library."))?;
    Ok(ScriptValue::List(details.my_tags.into_iter().map(ScriptValue::Text).collect()))
}

pub fn tag_ids(value: ScriptValue) -> Result<Vec<String>, ScriptError> {
    let ScriptValue::List(values) = value else {
        return Err(ScriptError::wrong_type("Supply a list of existing My Tag IDs as text."));
    };
    values.into_iter().map(|value| match value {
        ScriptValue::Text(id) if !id.is_empty() => Ok(id),
        _ => Err(ScriptError::wrong_type("Each My Tag ID must be nonempty text.")),
    }).collect()
}

pub async fn update<R: Runtime>(
    app: AppHandle<R>,
    track: String,
    tags: Vec<String>,
    operation: MyTagOperation,
) -> Result<ScriptValue, ScriptError> {
    // Also checked under the edit gate below, so a queued request cannot
    // outlive a user turning Library Protection back on.
    let bridge = Arc::clone(app.state::<Arc<Bridge>>().inner());
    bridge.refuse_if_protected()?;
    let changed = Arc::new(AtomicBool::new(false));
    let result = Arc::clone(&changed);
    recorded_edit(app.clone(), app.state(), "script_update_my_tags", Touched::Tracks, "Track Edit", move |writer| {
        bridge.refuse_if_protected().map_err(|error| rbl_db::DbError::WriteRefused(error.message))?;
        let (_, edit) = writer.update_my_tags_with_undo(&track, &tags, operation)?;
        result.store(!edit.is_empty(), Ordering::SeqCst);
        Ok(LibraryEdit::TrackTags { tags: vec![edit], comments: Vec::new() })
    }).await?;
    Ok(ScriptValue::Bool(changed.load(Ordering::SeqCst)))
}

/// Puts every category, or every tag of one category, in the order of
/// `order`. Guarded, recorded and refreshed as [`update`] is.
pub async fn reorder<R: Runtime>(app: AppHandle<R>, order: Vec<String>) -> Result<ScriptValue, ScriptError> {
    let bridge = Arc::clone(app.state::<Arc<Bridge>>().inner());
    bridge.refuse_if_protected()?;
    let changed = Arc::new(AtomicBool::new(false));
    let result = Arc::clone(&changed);
    recorded_edit(app.clone(), app.state(), "script_reorder_my_tags", Touched::Tracks, "Reorder My Tags", move |writer| {
        bridge.refuse_if_protected().map_err(|error| rbl_db::DbError::WriteRefused(error.message))?;
        let (_, edit) = writer.reorder_my_tags_with_undo(&order)?;
        result.store(!edit.is_empty(), Ordering::SeqCst);
        Ok(LibraryEdit::MyTagOrder(edit))
    }).await?;
    Ok(ScriptValue::Bool(changed.load(Ordering::SeqCst)))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn catalog_retains_ids_and_category_relationships_with_duplicate_names() {
        let row = |id: &str, name: &str, parent: &str, attribute| rbl_db::export_info::MyTagRow {
            id: id.into(), name: name.into(), parent: parent.into(), attribute, seq: 1,
        };
        let rows = vec![row("1", "Mood", "root", 1), row("2", "Texture", "root", 1),
            row("3", "deep", "1", 0), row("4", "deep", "2", 0)];
        let catalog: serde_json::Value = serde_json::from_str(&catalog_json(42, &rows)).unwrap();
        assert_eq!(catalog["library_id"], "42");
        assert_eq!(catalog["schema_version"], 1);
        for capability in ["my_tags.read", "my_tags.add", "my_tags.remove", "my_tags.reorder", "collection.remove_checked", "waveform.overview.read"] {
            assert!(catalog["capabilities"].as_array().unwrap().contains(&json!(capability)));
        }
        assert_eq!(catalog["tags"][0]["id"], "3");
        assert_eq!(catalog["tags"][1]["category_id"], "2");
        // Tags follow their category's place, each category's in Seq order.
        let seq_row = |id: &str, parent: &str, attribute, seq| rbl_db::export_info::MyTagRow {
            id: id.into(), name: id.into(), parent: parent.into(), attribute, seq,
        };
        let rows = vec![seq_row("B", "root", 1, 1), seq_row("A", "root", 1, 2),
            seq_row("a1", "A", 0, 1), seq_row("b1", "B", 0, 1), seq_row("b2", "B", 0, 2), seq_row("x", "gone", 0, 1)];
        let catalog: serde_json::Value = serde_json::from_str(&catalog_json(1, &rows)).unwrap();
        let order: Vec<_> = catalog["tags"].as_array().unwrap().iter().map(|tag| tag["id"].as_str().unwrap()).collect();
        assert_eq!(order, ["b1", "b2", "a1", "x"]);
        assert_eq!(catalog["categories"].as_array().unwrap().len(), 2);
        let missing: serde_json::Value = serde_json::from_str(&catalog_json(0, &[])).unwrap();
        assert!(missing["library_id"].is_null());
    }

    #[test]
    fn tag_ids_require_a_list_of_nonempty_text_ids() {
        assert_eq!(tag_ids(ScriptValue::List(vec![ScriptValue::Text("4294967295".into())])).unwrap(), ["4294967295"]);
        assert_eq!(tag_ids(ScriptValue::List(vec![])).unwrap(), Vec::<String>::new());
        for value in [ScriptValue::Text("1".into()), ScriptValue::List(vec![ScriptValue::Int(1)]),
            ScriptValue::List(vec![ScriptValue::Text(String::new())])] {
            assert_eq!(tag_ids(value).unwrap_err().code, ScriptError::WRONG_TYPE);
        }
    }

    #[test]
    fn script_updates_honor_protection_refresh_and_undo() {
        use rbl_db::{fixture::{self, track_id, Shape, MY_TAG_PEAK}, Library, OpenMode};
        use tauri::Listener;
        let dir = tempfile::tempdir().unwrap();
        let location = fixture::build(dir.path(), Shape::default()).unwrap();
        let db = Library::open(location.clone(), OpenMode::ReadOnly).unwrap();
        let (library, _) = rbl_index::load(&db).unwrap();
        let state = Arc::new(AppState::with_backups(dir.path().join("backups")));
        state.set_library(library, false, db.schema().db_version, 0, location);
        let app = tauri::test::mock_app();
        app.manage(Arc::clone(&state));
        let bridge = Arc::new(Bridge::default());
        app.manage(Arc::clone(&bridge));
        let refreshed = Arc::new(AtomicBool::new(false));
        let seen = Arc::clone(&refreshed);
        app.listen("library:changed", move |_| { seen.store(true, Ordering::SeqCst); });
        let run = |operation| tauri::async_runtime::block_on(update(
            app.handle().clone(), track_id(2), vec![MY_TAG_PEAK.into()], operation,
        ));
        assert!(run(MyTagOperation::Add).is_err(), "unknown protection must refuse");
        *bridge.preferences.write() = Some(json!({"advanced": {"protectLibrary": true}}));
        assert_eq!(run(MyTagOperation::Add).unwrap_err().code, ScriptError::NOT_MODIFIABLE);
        *bridge.preferences.write() = Some(json!({"advanced": {"protectLibrary": false}}));
        assert_eq!(run(MyTagOperation::Add).unwrap(), ScriptValue::Bool(true));
        assert!(refreshed.load(Ordering::SeqCst));
        assert_eq!(run(MyTagOperation::Add).unwrap(), ScriptValue::Bool(false));
        let read = || tauri::async_runtime::block_on(assigned(Arc::clone(&state), track_id(2))).unwrap();
        assert_eq!(read(), ScriptValue::List(vec![ScriptValue::Text(MY_TAG_PEAK.into())]));
        tauri::async_runtime::block_on(crate::commands::undo_edit(app.handle().clone(), app.state())).unwrap();
        assert_eq!(read(), ScriptValue::List(vec![]));
        tauri::async_runtime::block_on(crate::commands::redo_edit(app.handle().clone(), app.state())).unwrap();
        assert_eq!(read(), ScriptValue::List(vec![ScriptValue::Text(MY_TAG_PEAK.into())]));
        assert_eq!(run(MyTagOperation::Remove).unwrap(), ScriptValue::Bool(true));
        assert_eq!(run(MyTagOperation::Remove).unwrap(), ScriptValue::Bool(false));
    }

    #[test]
    fn script_reorder_honors_protection_refresh_undo_and_refuses_bad_orders() {
        use rbl_db::{fixture::{self, Shape, MY_TAG_CATEGORY, MY_TAG_PEAK, MY_TAG_WARM_UP}, Library, OpenMode};
        use tauri::Listener;
        let dir = tempfile::tempdir().unwrap();
        let location = fixture::build(dir.path(), Shape::default()).unwrap();
        let db = Library::open(location.clone(), OpenMode::ReadOnly).unwrap();
        let (library, _) = rbl_index::load(&db).unwrap();
        let state = Arc::new(AppState::with_backups(dir.path().join("backups")));
        state.set_library(library, false, db.schema().db_version, 0, location);
        let app = tauri::test::mock_app();
        app.manage(Arc::clone(&state));
        let bridge = Arc::new(Bridge::default());
        app.manage(Arc::clone(&bridge));
        let refreshed = Arc::new(AtomicBool::new(false));
        let seen = Arc::clone(&refreshed);
        app.listen("library:changed", move |_| { seen.store(true, Ordering::SeqCst); });
        let run = |order: &[&str]| tauri::async_runtime::block_on(reorder(
            app.handle().clone(), order.iter().map(|&id| id.to_owned()).collect(),
        ));
        // The order the index holds, as the panel and filters show it.
        let shown = || state.library().unwrap().my_tags().iter()
            .find(|category| category.name == "Situation").unwrap().tags.clone();
        let catalog = || {
            let text = match tauri::async_runtime::block_on(catalog(Arc::clone(&state))).unwrap() {
                ScriptValue::Text(text) => text,
                other => panic!("catalog is text, not {other:?}"),
            };
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            value["tags"].as_array().unwrap().iter().map(|tag| tag["id"].as_str().unwrap().to_owned()).collect::<Vec<_>>()
        };
        let swapped = [MY_TAG_WARM_UP, MY_TAG_PEAK];

        assert!(run(&swapped).is_err(), "unknown protection must refuse");
        *bridge.preferences.write() = Some(json!({"advanced": {"protectLibrary": true}}));
        assert_eq!(run(&swapped).unwrap_err().code, ScriptError::NOT_MODIFIABLE);
        assert_eq!(shown(), ["Peak", "Warm-up"]);
        *bridge.preferences.write() = Some(json!({"advanced": {"protectLibrary": false}}));
        for bad in [&[][..], &[MY_TAG_WARM_UP][..], &[MY_TAG_WARM_UP, MY_TAG_PEAK, MY_TAG_CATEGORY][..],
            &[MY_TAG_WARM_UP, "unknown"][..]] {
            assert!(run(bad).is_err(), "{bad:?} must be refused");
        }
        assert!(!refreshed.load(Ordering::SeqCst));
        assert_eq!(shown(), ["Peak", "Warm-up"]);

        assert_eq!(run(&swapped).unwrap(), ScriptValue::Bool(true));
        assert!(refreshed.load(Ordering::SeqCst));
        assert_eq!(shown(), ["Warm-up", "Peak"]);
        assert_eq!(catalog(), [MY_TAG_WARM_UP, MY_TAG_PEAK]);
        assert_eq!(run(&swapped).unwrap(), ScriptValue::Bool(false));
        assert_eq!(run(&[MY_TAG_CATEGORY]).unwrap(), ScriptValue::Bool(false));

        tauri::async_runtime::block_on(crate::commands::undo_edit(app.handle().clone(), app.state())).unwrap();
        assert_eq!(shown(), ["Peak", "Warm-up"]);
        assert_eq!(catalog(), [MY_TAG_PEAK, MY_TAG_WARM_UP]);
        tauri::async_runtime::block_on(crate::commands::redo_edit(app.handle().clone(), app.state())).unwrap();
        assert_eq!(shown(), ["Warm-up", "Peak"]);
    }
}
