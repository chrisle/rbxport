//! Ordinary create's additive exact-row result, through the public commands.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use tauri::Manager;

use super::{create_playlist, create_playlist_with_result, playlist_tree_snapshot, redo_edit, undo_edit};
use crate::state::AppState;

fn fixture() -> (tempfile::TempDir, tauri::App<tauri::test::MockRuntime>) {
    let dir = tempfile::tempdir().unwrap();
    let location = rbl_db::fixture::build(dir.path(), rbl_db::fixture::Shape::default()).unwrap();
    let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
    let (library, _) = rbl_index::load(&db).unwrap();
    let state = Arc::new(AppState::with_backups(dir.path().join("backups")));
    state.set_library(library, false, db.schema().db_version, 0, location);
    let app = tauri::test::mock_app();
    app.manage(state);
    (dir, app)
}

fn run<T>(future: impl std::future::Future<Output = T>) -> T {
    tauri::async_runtime::block_on(future)
}

#[test]
fn ordinary_create_returns_exact_identity_without_changing_legacy_or_undo() {
    let (_dir, app) = fixture();
    let state = || app.state::<Arc<AppState>>();
    let legacy = run(create_playlist(
        app.handle().clone(),
        state(),
        "Same name".into(),
        "root".into(),
        Some(0),
    ))
    .unwrap();
    let made = run(create_playlist_with_result(
        app.handle().clone(),
        state(),
        "Same name".into(),
        "root".into(),
        Some(1),
        state().playlist_deletion_context().unwrap(),
    ))
    .unwrap();
    let next = run(create_playlist_with_result(
        app.handle().clone(),
        state(),
        "Same name".into(),
        "root".into(),
        Some(2),
        state().playlist_deletion_context().unwrap(),
    ))
    .unwrap();
    assert!(legacy < made.generation && made.generation < next.generation);
    assert_ne!(made.id, next.id);
    let saved = state()
        .read_db(|db| {
            Ok(db.connection().query_row(
                "SELECT Name FROM djmdPlaylist WHERE ID = ?1",
                [&made.id],
                |row| row.get::<_, String>(0),
            )?)
        })
        .unwrap();
    assert_eq!(saved, "Same name");
    let ids = state().read_db(|db| {
        let mut query = db.connection().prepare("SELECT ID FROM djmdPlaylist WHERE ParentID = 'root' AND Name = 'Same name' ORDER BY Seq")?;
        let rows = query.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }).unwrap();
    assert_eq!(ids.len(), 3);
    assert_eq!(ids[1], made.id);
    assert_eq!(ids[2], next.id);
    run(undo_edit(app.handle().clone(), state())).unwrap();
    let library = state().library().unwrap();
    assert!(library
        .playlists()
        .index_of(made.id.parse().unwrap())
        .is_some());
    assert!(library
        .playlists()
        .index_of(next.id.parse().unwrap())
        .is_none());
    drop(library);
    run(redo_edit(app.handle().clone(), state())).unwrap();
    assert!(state()
        .library()
        .unwrap()
        .playlists()
        .index_of(next.id.parse().unwrap())
        .is_some());
    assert!(serde_json::to_value(legacy).unwrap().is_number());
    assert_eq!(
        serde_json::to_value(&made).unwrap(),
        serde_json::json!({ "id": made.id, "generation": made.generation })
    );
}

#[test]
fn ordinary_create_result_preserves_pending_restore_refusal_and_history() {
    let (_dir, app) = fixture();
    let state = || app.state::<Arc<AppState>>();
    let before = state().summary().3;
    let opens = state().writer_opens();
    std::fs::create_dir_all(state().backup_dir()).unwrap();
    let journal = state().backup_dir().join(rbl_backup::journal::NAME);
    std::fs::write(&journal, b"owned unfinished restore").unwrap();
    let old = tauri::async_runtime::block_on(create_playlist(
        app.handle().clone(),
        state(),
        "Refused old".into(),
        "root".into(),
        None,
    ));
    let new = tauri::async_runtime::block_on(create_playlist_with_result(
        app.handle().clone(),
        state(),
        "Refused new".into(),
        "root".into(),
        None,
        state().playlist_deletion_context().unwrap(),
    ));
    assert!(old.is_err() && new.is_err());
    assert_eq!(old.unwrap_err().message, new.unwrap_err().message);
    assert_eq!(state().summary().3, before);
    assert_eq!(state().writer_opens(), opens);
    assert_eq!(
        std::fs::read(&journal).unwrap(),
        b"owned unfinished restore"
    );
    assert!(!super::current_history(&state()).can_undo);
}

#[test]
fn ordinary_create_refuses_rows_from_a_replaced_library() {
    let (_old, app) = fixture();
    let state = || app.state::<Arc<AppState>>();
    let old = run(playlist_tree_snapshot(state())).unwrap();
    let new_dir = tempfile::tempdir().unwrap();
    let location = rbl_db::fixture::build(new_dir.path(), rbl_db::fixture::Shape::default()).unwrap();
    let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
    let (library, _) = rbl_index::load(&db).unwrap();
    state().set_library(library, false, db.schema().db_version, 0, location);
    let new = run(playlist_tree_snapshot(state())).unwrap();
    assert!(old.nodes.iter().map(|node| &node.id).eq(new.nodes.iter().map(|node| &node.id)));
    assert_ne!(old.context, new.context);

    let refused = run(create_playlist_with_result(
        app.handle().clone(), state(), "Old selection".into(), "root".into(), None, old.context.unwrap(),
    )).unwrap_err();
    assert!(refused.message.contains("the selected library has changed"));
    assert_eq!(run(playlist_tree_snapshot(state())).unwrap().nodes.len(), new.nodes.len());
}
