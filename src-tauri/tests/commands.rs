//! The shell's commands, end to end, against a fixture library.
//!
//! What the webview does — `invoke("create_playlist", …)` and then
//! `fetch_rows` to see the result — this does in Rust: the same command
//! functions, the same `AppState`, a mock Tauri app in place of the window,
//! and a library built in a tempdir from the real schema. The deck plays into
//! a sink the test pulls by hand, so nothing here needs an audio device.
//!
//! Nothing here can reach the installed library. The state is pointed at the
//! fixture once, and every open — reader, writer, reload — goes through it.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rbl_db::fixture::{self, playlist_id, track_id, Shape};
use rbl_db::write::ROOT;
use rbl_db::{Library as Db, OpenMode};
use rbl_deck::{NullSink, Sink};
use rbxport_lib::commands;
use rbxport_lib::cues::{self, CueKind};
use rbxport_lib::details;
use rbxport_lib::dto::{RowDto, TrackFilterDto, TrackSourceDto, TreeNodeDto, ViewSpecDto};
use rbxport_lib::player::{Player, TickDto};
use rbxport_lib::preview::{Preview, PreviewStateDto};
use rbxport_lib::state::AppState;
use rbxport_lib::ErrorKind;
use tauri::test::MockRuntime;
use tauri::{AppHandle, Listener, Manager, State};

/// The rate the null sink runs at, so a seek in milliseconds is a known
/// number of frames.
const RATE: u32 = 44_100;

struct Shell {
    _dir: tempfile::TempDir,
    app: tauri::App<MockRuntime>,
    /// The engine's output, once a deck command has opened it.
    sink: Arc<Mutex<Option<Arc<NullSink>>>>,
    /// The preview player's output, once something has been previewed.
    preview_sink: Arc<Mutex<Option<Arc<NullSink>>>>,
    /// Every `library:changed` generation the interface would have seen.
    changes: Arc<Mutex<Vec<u32>>>,
    /// How many `tag-list:changed` the interface would have seen.
    tag_list_changes: Arc<Mutex<usize>>,
}

/// A mock app over a fresh fixture, loaded the way `spawn_library_load`
/// loads the real one.
fn shell() -> Shell {
    shell_with_shape(Shape::default())
}

fn shell_with_shape(shape: Shape) -> Shell {
    let dir = tempfile::tempdir().unwrap();
    let location = fixture::build(dir.path(), shape).expect("build the fixture");

    let state = AppState::with_backups(dir.path().join("backups"));
    let db = Db::open(location.clone(), OpenMode::ReadOnly).expect("open the fixture");
    let (library, _) = rbl_index::load(&db).expect("index the fixture");
    state.set_library(library, false, db.schema().db_version, 0, location);

    let sink: Arc<Mutex<Option<Arc<NullSink>>>> = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&sink);
    let player = Player::with_sink(Box::new(move |render, _device, _wish| {
        let opened = Arc::new(NullSink::new(RATE, render));
        *slot.lock().unwrap() = Some(Arc::clone(&opened));
        Ok(opened as Arc<dyn Sink>)
    }));

    let preview_sink: Arc<Mutex<Option<Arc<NullSink>>>> = Arc::new(Mutex::new(None));
    let preview_slot = Arc::clone(&preview_sink);
    let preview = Preview::with_sink(Box::new(move |render, _device, _wish| {
        let opened = Arc::new(NullSink::new(RATE, render));
        *preview_slot.lock().unwrap() = Some(Arc::clone(&opened));
        Ok(opened as Arc<dyn Sink>)
    }));

    let app = tauri::test::mock_app();
    app.manage(Arc::new(state));
    app.manage(Arc::new(player));
    app.manage(Arc::new(preview));

    let changes: Arc<Mutex<Vec<u32>>> = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&changes);
    app.listen("library:changed", move |event| {
        if let Ok(generation) = serde_json::from_str::<u32>(event.payload()) {
            seen.lock().unwrap().push(generation);
        }
    });

    let tag_list_changes = Arc::new(Mutex::new(0));
    let tagged = Arc::clone(&tag_list_changes);
    app.listen("tag-list:changed", move |_| *tagged.lock().unwrap() += 1);

    Shell { _dir: dir, app, sink, preview_sink, changes, tag_list_changes }
}

/// Runs a command the way the invoke handler does: to completion, on the
/// shell's async runtime.
fn run<T>(fut: impl std::future::Future<Output = T>) -> T {
    tauri::async_runtime::block_on(fut)
}

impl Shell {
    fn handle(&self) -> AppHandle<MockRuntime> {
        self.app.handle().clone()
    }

    fn state(&self) -> State<'_, Arc<AppState>> {
        self.app.state::<Arc<AppState>>()
    }

    fn player(&self) -> State<'_, Arc<Player>> {
        self.app.state::<Arc<Player>>()
    }

    fn tree(&self) -> Vec<TreeNodeDto> {
        run(commands::playlist_tree(self.state())).unwrap()
    }

    /// The tree node called `name`, which a test knows it just made.
    fn node(&self, name: &str) -> TreeNodeDto {
        let tree = self.tree();
        tree.iter()
            .find(|n| n.name == name)
            .cloned()
            .unwrap_or_else(|| panic!("no node named {name} in {:?}", names(&tree)))
    }

    fn has_node(&self, name: &str) -> bool {
        self.tree().iter().any(|n| n.name == name)
    }

    /// Opens a view and returns its id and length.
    fn open(&self, spec: ViewSpecDto) -> (u32, u32) {
        let handle = run(commands::open_view(self.state(), spec)).unwrap();
        (handle.view_id, handle.len)
    }

    fn rows(&self, view_id: u32) -> Vec<RowDto> {
        run(commands::fetch_rows(self.state(), view_id, 0, commands::MAX_ROWS, None)).unwrap()
    }

    /// The playlist's rows, in its own order.
    fn playlist_rows(&self, playlist: &str) -> Vec<RowDto> {
        let (view, _) = self.open(playlist_spec(playlist));
        self.rows(view)
    }

    fn deck_state(&self) -> TickDto {
        run(commands::deck_state(self.player())).unwrap()
    }

    fn preview(&self) -> State<'_, Arc<Preview>> {
        self.app.state::<Arc<Preview>>()
    }

    fn preview_state(&self) -> PreviewStateDto {
        run(commands::preview_state(self.preview())).unwrap()
    }

    /// Pulls the preview's output until the condition holds, or gives up.
    fn pull_preview_until(&self, what: &str, mut done: impl FnMut(&PreviewStateDto) -> bool) -> PreviewStateDto {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let state = self.preview_state();
            if done(&state) {
                return state;
            }
            assert!(Instant::now() < deadline, "gave up waiting for {what}: {state:?}");
            if let Some(sink) = self.preview_sink.lock().unwrap().clone() {
                sink.pull(512);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Pulls the sink until the condition holds, or gives up. The engine
    /// decodes on its own thread, so nothing here is instantaneous.
    fn pull_until(&self, what: &str, mut done: impl FnMut(&TickDto) -> bool) -> TickDto {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let tick = self.deck_state();
            if done(&tick) {
                return tick;
            }
            assert!(Instant::now() < deadline, "gave up waiting for {what}: {tick:?}");
            if let Some(sink) = self.sink.lock().unwrap().clone() {
                sink.pull(512);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

fn names(tree: &[TreeNodeDto]) -> Vec<&str> {
    tree.iter().map(|n| n.name.as_str()).collect()
}

fn titles(rows: &[RowDto]) -> Vec<&str> {
    rows.iter().map(|r| r.title.as_str()).collect()
}

fn ids(rows: &[RowDto]) -> Vec<&str> {
    rows.iter().map(|r| r.id.as_str()).collect()
}

fn collection_spec() -> ViewSpecDto {
    ViewSpecDto {
        source: TrackSourceDto::Collection,
        sort: "title".into(),
        descending: false,
        query: String::new(),
        search_field: rbl_index::SearchField::All,
        filter: TrackFilterDto::default(),
    }
}

fn playlist_spec(id: &str) -> ViewSpecDto {
    ViewSpecDto {
        source: TrackSourceDto::Playlist { id: id.to_owned() },
        sort: "trackNo".into(),
        descending: false,
        query: String::new(),
        search_field: rbl_index::SearchField::All,
        filter: TrackFilterDto::default(),
    }
}

fn playlist_folder_spec(id: &str) -> ViewSpecDto {
    ViewSpecDto { source: TrackSourceDto::PlaylistFolder { id: id.to_owned() }, ..playlist_spec(id) }
}

#[test]
fn a_playlist_folder_lists_unique_tracks_from_nested_playlists() {
    let s = shell();
    run(commands::create_folder(s.handle(), s.state(), "Shows".into(), ROOT.into())).unwrap();
    let shows = s.node("Shows");
    run(commands::create_playlist(s.handle(), s.state(), "Friday".into(), shows.id.clone())).unwrap();
    run(commands::create_folder(s.handle(), s.state(), "Weekend".into(), shows.id.clone())).unwrap();
    let weekend = s.node("Weekend");
    run(commands::create_playlist(s.handle(), s.state(), "Saturday".into(), weekend.id.clone())).unwrap();
    run(commands::create_playlist(s.handle(), s.state(), "Outside".into(), ROOT.into())).unwrap();
    let friday = s.node("Friday");
    let saturday = s.node("Saturday");
    let outside = s.node("Outside");
    run(commands::add_tracks_to_playlist(s.handle(), s.state(), friday.id, vec![track_id(3), track_id(1)])).unwrap();
    run(commands::add_tracks_to_playlist(s.handle(), s.state(), saturday.id, vec![track_id(1), track_id(2)])).unwrap();
    run(commands::add_tracks_to_playlist(s.handle(), s.state(), outside.id, vec![track_id(4)])).unwrap();

    let (view, len) = s.open(playlist_folder_spec(&shows.id));
    assert_eq!(len, 3);
    assert_eq!(ids(&s.rows(view)), [track_id(3), track_id(1), track_id(2)]);
    let (view, len) = s.open(playlist_folder_spec(&weekend.id));
    assert_eq!(len, 2);
    assert_eq!(ids(&s.rows(view)), [track_id(1), track_id(2)]);
}

/// A silent stereo WAV the deck can load and the writer can import.
fn write_wav(path: &Path, seconds: u32) -> PathBuf {
    let frames = RATE * seconds;
    let data_len = frames * 4;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16_u32.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes()); // PCM
    out.extend_from_slice(&2_u16.to_le_bytes()); // stereo
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 4).to_le_bytes());
    out.extend_from_slice(&4_u16.to_le_bytes());
    out.extend_from_slice(&16_u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.resize(44 + data_len as usize, 0);
    std::fs::write(path, out).unwrap();
    path.to_path_buf()
}

// ------------------------------------------------------------------ reading

#[test]
fn the_summary_and_the_tree_describe_the_loaded_library() {
    let s = shell();
    let summary = run(commands::library_summary(s.state())).unwrap();
    assert_eq!(summary.track_count, 40);
    assert_eq!(summary.playlist_count, 3);
    assert_eq!(summary.db_version, Some(6000));
    assert!(!summary.read_only, "fixture editing is independent of the installed rekordbox process");

    let tree = s.tree();
    assert_eq!(tree[0].kind, "allTracks");
    assert_eq!(tree[0].child_count, Some(40));
    assert_eq!(tree[1].kind, "collection");
    let playlists: Vec<&TreeNodeDto> = tree.iter().filter(|n| n.kind == "playlist").collect();
    assert_eq!(playlists.len(), 3);
    assert!(playlists.iter().all(|p| p.depth == 1 && p.child_count == Some(5)));
    assert!(tree.iter().any(|n| n.kind == "histories"), "the fixture records sessions");
}

#[test]
fn histories_are_filed_by_year_and_named_month_in_calendar_order() {
    let s = shell();
    let tree = s.tree();
    let at = tree.iter().position(|n| n.kind == "histories").unwrap();
    let section: Vec<(&str, u32, Option<bool>)> = tree[at..]
        .iter()
        .map(|n| (n.name.as_str(), n.depth, n.expanded))
        .collect();
    // The section and its years open, the months closed; the months under
    // their names in calendar order although August's row has the later Seq;
    // the sessions in the order they were played.
    assert_eq!(
        section,
        vec![
            ("Histories", 0, Some(true)),
            ("2026", 1, Some(true)),
            ("August", 2, Some(false)),
            ("September", 2, Some(false)),
            ("HISTORY 2026-09-01", 3, None),
            ("HISTORY 2026-09-02", 3, None),
        ]
    );
}

#[test]
fn a_view_is_a_window_over_rows_the_backend_sorted_and_searched() {
    let s = shell();

    let (view, len) = s.open(collection_spec());
    assert_eq!(len, 40);
    let rows = s.rows(view);
    assert_eq!(rows.len(), 40);
    assert_eq!(rows[0].title, "Track 000");
    assert_eq!(rows[39].title, "Track 039");

    // A window, not the list: the page is what was asked for.
    let page = run(commands::fetch_rows(s.state(), view, 10, 3, None)).unwrap();
    assert_eq!(titles(&page), ["Track 010", "Track 011", "Track 012"]);
    assert_eq!(page[0].track_no, 11, "numbered from where the window starts");

    // Sorted the other way, on another column, by Rust.
    let (view, _) = s.open(ViewSpecDto { sort: "bpm".into(), descending: true, ..collection_spec() });
    let rows = s.rows(view);
    assert_eq!(rows[0].title, "Track 039", "the fixture's BPM climbs with the index");
    assert!(rows.windows(2).all(|w| w[0].bpm_x100 >= w[1].bpm_x100));

    // Searched by Rust: every token a substring, so "01" is Track 001 and
    // Track 010 to 019.
    let (view, len) = s.open(ViewSpecDto { query: "track 01".into(), ..collection_spec() });
    assert_eq!(len, 11);
    assert!(titles(&s.rows(view)).iter().all(|t| t.contains("01")));

    let (_, none) = s.open(ViewSpecDto { query: "no such track".into(), ..collection_spec() });
    assert_eq!(none, 0);
}

#[test]
fn a_page_past_the_cap_is_refused_before_it_is_built() {
    // The 64 KB response cap is kept by never building more than a page.
    let s = shell();
    let (view, _) = s.open(collection_spec());
    let err = run(commands::fetch_rows(s.state(), view, 0, commands::MAX_ROWS + 1, None)).unwrap_err();
    assert_eq!(err.kind, ErrorKind::Malformed);
}

#[test]
fn a_full_page_stays_inside_the_ipc_response_budget() {
    let s = shell_with_shape(Shape { tracks: commands::MAX_ROWS as usize, ..Shape::default() });
    let (view, len) = s.open(collection_spec());
    assert_eq!(len, commands::MAX_ROWS);
    let rows = s.rows(view);
    let bytes = serde_json::to_vec(&rows).unwrap().len();
    let budgets: serde_json::Value =
        serde_json::from_str(include_str!("../../perf-budgets.json")).unwrap();
    let limit_kb = budgets
        .pointer("/gates/ipc/responseKb")
        .and_then(serde_json::Value::as_u64)
        .unwrap();
    assert!(
        bytes <= limit_kb as usize * 1024,
        "a {}-row response is {:.1} KB, above the {limit_kb} KB budget",
        rows.len(),
        bytes as f64 / 1024.0,
    );
}

#[test]
fn visible_detail_columns_are_added_to_browser_pages() {
    let s = shell();
    let (view, _) = s.open(collection_spec());
    let plain = run(commands::fetch_rows(s.state(), view, 0, 1, None)).unwrap();
    assert!(plain[0].extra.is_none());

    let columns = [
        "size", "discNo", "albumArtist", "composer", "lyricist", "fileType", "year",
        "mixName", "remixer", "originalArtist", "sampleRate", "bitrate", "bitDepth",
        "location", "dateCreated", "publishTrackInfo", "message", "color",
        "djPlayCount", "myTag", "trackNumber", "cloud", "unknown",
    ]
        .map(str::to_owned).to_vec();
    let rows = run(commands::fetch_rows(s.state(), view, 0, 1, Some(columns))).unwrap();
    let extra = rows[0].extra.as_ref().unwrap();
    assert_eq!(extra.get("size"), Some(&serde_json::json!(0)));
    assert_eq!(extra.get("publishTrackInfo"), Some(&serde_json::json!(false)));
    assert_eq!(extra.get("myTag"), Some(&serde_json::json!("")));
    assert_eq!(extra.get("cloud"), Some(&serde_json::json!(false)));
    assert_eq!(extra.len(), 22, "every requested browser detail has a value or a blank");
    assert!(!extra.contains_key("unknown"));
}

#[test]
fn a_view_nobody_opened_is_not_found_rather_than_empty() {
    let s = shell();
    let err = run(commands::fetch_rows(s.state(), 999, 0, 10, None)).unwrap_err();
    assert_eq!(err.kind, ErrorKind::NotFound);
}

// -------------------------------------------------------- playlist editing

#[test]
fn a_playlist_is_made_filled_reordered_renamed_moved_and_deleted() {
    let s = shell();
    let before = s.tree().len();

    // A folder at the root, and a playlist inside it.
    run(commands::create_folder(s.handle(), s.state(), "Gigs".into(), ROOT.into())).unwrap();
    let gigs = s.node("Gigs");
    assert_eq!(gigs.depth, 1);
    assert_eq!((gigs.kind, gigs.child_count), ("folder", Some(0)), "a folder before anything is in it");
    run(commands::create_playlist(s.handle(), s.state(), "Friday".into(), gigs.id.clone())).unwrap();
    let friday = s.node("Friday");
    assert_eq!(friday.depth, 2, "inside the folder");
    assert_eq!(friday.kind, "playlist");
    assert_eq!(s.node("Gigs").kind, "folder", "a folder with something in it");
    assert_eq!(s.playlist_rows(&friday.id).len(), 0);

    // Tracks go in, in the order given, and the view numbers them so.
    let (t1, t2, t3) = (track_id(1), track_id(2), track_id(3));
    run(commands::add_tracks_to_playlist(
        s.handle(),
        s.state(),
        friday.id.clone(),
        vec![t3.clone(), t1.clone(), t2.clone()],
    ))
    .unwrap();
    let rows = s.playlist_rows(&friday.id);
    assert_eq!(ids(&rows), [t3.as_str(), t1.as_str(), t2.as_str()]);
    assert_eq!(rows.iter().map(|r| r.track_no).collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(s.node("Friday").child_count, Some(3), "the tree counts them");

    // The `#` column is the playlist's stored TrackNo, not the row's current
    // visible index. Sorting changes which track is on each line, not its
    // place in the set.
    let (sorted, _) = s.open(ViewSpecDto { sort: "title".into(), descending: true, ..playlist_spec(&friday.id) });
    let rows = s.rows(sorted);
    assert_eq!(ids(&rows), [t3.as_str(), t2.as_str(), t1.as_str()]);
    assert_eq!(rows.iter().map(|r| r.track_no).collect::<Vec<_>>(), [1, 3, 2]);

    // Reordered, and one taken out closes the gap.
    run(commands::reorder_playlist(
        s.handle(),
        s.state(),
        friday.id.clone(),
        vec![t1.clone(), t2.clone(), t3.clone()],
    ))
    .unwrap();
    assert_eq!(ids(&s.playlist_rows(&friday.id)), [t1.as_str(), t2.as_str(), t3.as_str()]);
    run(commands::remove_tracks_from_playlist(s.handle(), s.state(), friday.id.clone(), vec![t2]))
        .unwrap();
    let rows = s.playlist_rows(&friday.id);
    assert_eq!(ids(&rows), [t1.as_str(), t3.as_str()]);
    assert_eq!(rows.iter().map(|r| r.track_no).collect::<Vec<_>>(), [1, 2]);

    // Renamed, then moved up to the root.
    run(commands::rename_playlist(s.handle(), s.state(), friday.id.clone(), "Saturday".into())).unwrap();
    assert!(!s.has_node("Friday"));
    assert_eq!(s.node("Saturday").id, friday.id, "the same list under a new name");
    // No index: appended among the root's children, as it was before there
    // was a place to ask for.
    run(commands::move_playlist(s.handle(), s.state(), friday.id.clone(), ROOT.into(), None))
        .unwrap();
    assert_eq!(s.node("Saturday").depth, 1);

    // And with one, it takes that place: first among them.
    run(commands::move_playlist(s.handle(), s.state(), friday.id.clone(), ROOT.into(), Some(0)))
        .unwrap();
    assert_eq!(s.node("Saturday").depth, 1);

    // Deleted, both of them, and the tree is as it was.
    run(commands::delete_playlist(s.handle(), s.state(), gigs.id)).unwrap();
    assert!(!s.has_node("Gigs"));
    assert!(s.has_node("Saturday"), "deleting the folder it left does not take it");
    run(commands::delete_playlist(s.handle(), s.state(), friday.id)).unwrap();
    assert!(!s.has_node("Saturday"));
    assert_eq!(s.tree().len(), before);
}

#[test]
fn a_deleted_playlist_tree_can_be_undone_and_redone() {
    let s = shell();
    run(commands::create_folder(s.handle(), s.state(), "Sets".into(), ROOT.into())).unwrap();
    let folder = s.node("Sets");
    run(commands::create_playlist(s.handle(), s.state(), "Friday".into(), folder.id.clone())).unwrap();
    let playlist = s.node("Friday");
    let tracks = vec![track_id(0), track_id(1)];
    run(commands::add_tracks_to_playlist(
        s.handle(),
        s.state(),
        playlist.id.clone(),
        tracks.clone(),
    )).unwrap();

    let deleted = run(commands::delete_playlist(s.handle(), s.state(), folder.id.clone())).unwrap();
    assert!(deleted.can_undo);
    assert!(!deleted.can_redo);
    assert!(!s.has_node("Sets"));
    assert!(!s.has_node("Friday"));

    let undone = run(commands::undo_edit(s.handle(), s.state())).unwrap();
    assert!(!undone.can_undo);
    assert!(undone.can_redo);
    assert_eq!(s.node("Sets").id, folder.id);
    assert_eq!(s.node("Friday").id, playlist.id);
    assert_eq!(ids(&s.playlist_rows(&playlist.id)), [tracks[0].as_str(), tracks[1].as_str()]);

    let redone = run(commands::redo_edit(s.handle(), s.state())).unwrap();
    assert!(redone.can_undo);
    assert!(!redone.can_redo);
    assert!(!s.has_node("Sets"));
    assert!(!s.has_node("Friday"));
}

#[test]
fn library_history_names_and_reverses_each_supported_edit() {
    let s = shell();
    run(commands::create_folder(s.handle(), s.state(), "Sets".into(), ROOT.into())).unwrap();
    let folder = s.node("Sets");
    run(commands::create_playlist(s.handle(), s.state(), "Friday".into(), ROOT.into())).unwrap();
    let playlist = s.node("Friday");

    let renamed = run(commands::rename_playlist(
        s.handle(), s.state(), playlist.id.clone(), "Saturday".into(),
    )).unwrap();
    assert_eq!(renamed.undo_label.as_deref(), Some("Rename Playlist"));
    run(commands::undo_edit(s.handle(), s.state())).unwrap();
    assert_eq!(s.node("Friday").id, playlist.id);
    run(commands::redo_edit(s.handle(), s.state())).unwrap();
    assert_eq!(s.node("Saturday").id, playlist.id);

    let moved = run(commands::move_playlist(
        s.handle(), s.state(), playlist.id.clone(), folder.id.clone(), Some(0),
    )).unwrap();
    assert_eq!(moved.undo_label.as_deref(), Some("Move Playlist"));
    assert_eq!(s.node("Saturday").depth, 2);
    run(commands::undo_edit(s.handle(), s.state())).unwrap();
    assert_eq!(s.node("Saturday").depth, 1);

    let tracks = vec![track_id(1), track_id(2), track_id(3)];
    run(commands::add_tracks_to_playlist(
        s.handle(), s.state(), playlist.id.clone(), tracks.clone(),
    )).unwrap();
    let removed = run(commands::remove_tracks_from_playlist(
        s.handle(), s.state(), playlist.id.clone(), vec![tracks[1].clone()],
    )).unwrap();
    assert_eq!(removed.undo_label.as_deref(), Some("Remove Tracks from Playlist"));
    assert_eq!(ids(&s.playlist_rows(&playlist.id)), [tracks[0].as_str(), tracks[2].as_str()]);
    run(commands::undo_edit(s.handle(), s.state())).unwrap();
    assert_eq!(ids(&s.playlist_rows(&playlist.id)), [tracks[0].as_str(), tracks[1].as_str(), tracks[2].as_str()]);
    run(commands::redo_edit(s.handle(), s.state())).unwrap();
    assert_eq!(ids(&s.playlist_rows(&playlist.id)), [tracks[0].as_str(), tracks[2].as_str()]);

    let track = track_id(7);
    let edited = run(commands::set_track_rating(s.handle(), s.state(), track.clone(), 4)).unwrap();
    assert_eq!(edited.undo_label.as_deref(), Some("Track Edit"));
    run(commands::undo_edit(s.handle(), s.state())).unwrap();
    assert_eq!(run(details::track_details(s.state(), track.clone())).unwrap().rating, 0);
    run(commands::redo_edit(s.handle(), s.state())).unwrap();
    assert_eq!(run(details::track_details(s.state(), track.clone())).unwrap().rating, 4);

    let field = run(details::set_track_field(
        s.handle(), s.state(), track.clone(), "title".into(), "Seven".into(),
    )).unwrap();
    assert_eq!(field.undo_label.as_deref(), Some("Track Edit"));
    run(commands::undo_edit(s.handle(), s.state())).unwrap();
    assert_eq!(run(details::track_details(s.state(), track.clone())).unwrap().title, "Track 007");
    run(commands::redo_edit(s.handle(), s.state())).unwrap();
    assert_eq!(run(details::track_details(s.state(), track)).unwrap().title, "Seven");
}

#[test]
fn removing_from_collection_is_permanent_and_clears_history() {
    let s = shell();
    let track = track_id(5);
    run(commands::set_track_rating(s.handle(), s.state(), track.clone(), 3)).unwrap();
    run(commands::remove_from_collection(s.handle(), s.state(), vec![track])).unwrap();
    let error = run(commands::undo_edit(s.handle(), s.state())).unwrap_err();
    assert_eq!(error.kind, ErrorKind::NotFound);
}

#[test]
fn every_edit_bumps_the_generation_and_tells_the_interface() {
    let s = shell();
    let (_, _, _, start) = s.state().summary();

    let first = run(commands::create_playlist(s.handle(), s.state(), "One".into(), ROOT.into())).unwrap();
    let second = run(commands::set_track_rating(s.handle(), s.state(), track_id(0), 3)).unwrap();
    assert!(first > start);
    assert!(second.generation > first);
    assert_eq!(s.state().summary().3, second.generation, "the state reports the latest");

    // Both went out as `library:changed`, which is what makes the frontend
    // drop the pages it cached against the old generation.
    let deadline = Instant::now() + Duration::from_secs(2);
    while s.changes.lock().unwrap().len() < 2 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(*s.changes.lock().unwrap(), vec![first, second.generation]);
}

#[test]
fn library_backups_are_manual_only() {
    let s = shell();
    let backups = s._dir.path().join("backups");
    assert!(!backups.exists());

    run(commands::set_track_rating(s.handle(), s.state(), track_id(0), 3)).unwrap();
    assert!(!backups.exists(), "the first edit must not back up");

    // Every kind of edit opens its own writer; none should copy the database.
    run(commands::set_track_comment(s.handle(), s.state(), track_id(0), "x".into())).unwrap();
    run(commands::create_playlist(s.handle(), s.state(), "Later".into(), ROOT.into())).unwrap();
    run(cues::add_cue(s.handle(), s.state(), track_id(0), CueKind::Memory, 1_000)).unwrap();
    run(details::set_track_field(s.handle(), s.state(), track_id(0), "title".into(), "T".into())).unwrap();
    assert!(!backups.exists(), "edits must not back up automatically");

    let path = run(commands::back_up_library(s.state())).unwrap();
    assert!(Path::new(&path).is_file(), "manual backups remain available");
    run(commands::set_track_rating(s.handle(), s.state(), track_id(0), 4)).unwrap();
    assert_eq!(std::fs::read_dir(&backups).unwrap().count(), 1);
}

#[test]
fn an_edit_closes_the_views_that_were_open_over_the_old_library() {
    let s = shell();
    let (view, _) = s.open(playlist_spec(&playlist_id(0)));
    assert_eq!(s.rows(view).len(), 5);

    run(commands::add_tracks_to_playlist(s.handle(), s.state(), playlist_id(0), vec![track_id(30)])).unwrap();

    // The page the frontend held is gone; it reopens against the new tree.
    let err = run(commands::fetch_rows(s.state(), view, 0, 10, None)).unwrap_err();
    assert_eq!(err.kind, ErrorKind::NotFound);
    assert_eq!(s.playlist_rows(&playlist_id(0)).len(), 6);
}

#[test]
fn a_write_the_library_refuses_is_read_only_to_the_interface_and_changes_nothing() {
    let s = shell();
    let (_, _, _, generation) = s.state().summary();

    let err = run(commands::create_playlist(s.handle(), s.state(), "Orphan".into(), "no-such-folder".into()))
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::ReadOnly, "the status bar shows a refusal, not a crash");
    assert!(!s.has_node("Orphan"));
    assert_eq!(s.state().summary().3, generation, "nothing was reloaded");
    assert!(s.changes.lock().unwrap().is_empty());

    let err = run(commands::set_track_rating(s.handle(), s.state(), track_id(0), 9)).unwrap_err();
    assert_eq!(err.kind, ErrorKind::ReadOnly);
}

// ----------------------------------------------------------- the Tag List

/// The Tag List in its own order: `trackNo` is the view's order rather than
/// a column, which for this source is the order the tracks were put on.
fn tag_list_spec() -> ViewSpecDto {
    ViewSpecDto { source: TrackSourceDto::TagList, sort: "trackNo".into(), ..collection_spec() }
}

#[test]
fn the_tag_list_takes_tracks_in_order_ignores_a_repeat_and_empties_on_clear() {
    let s = shell();
    let generation = s.state().summary().3;
    let (collection, _) = s.open(collection_spec());
    let (view, len) = s.open(tag_list_spec());
    assert_eq!(len, 0);
    assert!(s.rows(view).is_empty());

    let (t1, t2, t3) = (track_id(1), track_id(2), track_id(3));
    let first = run(commands::add_to_tag_list(s.handle(), s.state(), vec![t3.clone(), t1.clone()])).unwrap();
    let (view, _) = s.open(tag_list_spec());
    assert_eq!(ids(&s.rows(view)), [t3.as_str(), t1.as_str()], "on the end, in the order given");

    // A track already on the list is not put on twice.
    let second = run(commands::add_to_tag_list(s.handle(), s.state(), vec![t3.clone(), t2.clone()])).unwrap();
    let (view, _) = s.open(tag_list_spec());
    assert_eq!(ids(&s.rows(view)), [t3.as_str(), t1.as_str(), t2.as_str()]);

    // One off the front, and the rest close the gap it left.
    let third = run(commands::remove_from_tag_list(s.handle(), s.state(), vec![t3.clone()])).unwrap();
    let (view, _) = s.open(tag_list_spec());
    assert_eq!(ids(&s.rows(view)), [t1.as_str(), t2.as_str()]);

    // Taking off a track that is not on the list is not a refusal.
    let fourth = run(commands::remove_from_tag_list(s.handle(), s.state(), vec![t3])).unwrap();
    let (view, _) = s.open(tag_list_spec());
    assert_eq!(ids(&s.rows(view)), [t1.as_str(), t2.as_str()]);

    let fifth = run(commands::clear_tag_list(s.handle(), s.state())).unwrap();
    let (view, len) = s.open(tag_list_spec());
    assert_eq!(len, 0);
    assert!(s.rows(view).is_empty());

    // No other list shows Tag List membership, so the generation stays and
    // the list on screen keeps its view: tagging a track, from the menu or
    // from a player, does not reload the playlist being looked at.
    assert_eq!([first, second, third, fourth, fifth], [generation; 5]);
    assert!(!s.rows(collection).is_empty(), "the collection's view is still open");
    let deadline = Instant::now() + Duration::from_secs(2);
    while *s.tag_list_changes.lock().unwrap() < 5 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(*s.tag_list_changes.lock().unwrap(), 5);
    assert!(s.changes.lock().unwrap().is_empty(), "nothing told every list to refetch");
}

#[test]
fn a_tag_list_add_naming_a_track_that_is_not_there_is_read_only_and_adds_none_of_them() {
    let s = shell();
    let track = track_id(4);
    let generation = run(commands::add_to_tag_list(s.handle(), s.state(), vec![track.clone()])).unwrap();

    let err = run(commands::add_to_tag_list(s.handle(), s.state(), vec![track_id(5), "no-such-track".into()]))
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::ReadOnly, "the writer's refusal, as the status bar shows it");

    // The refusal came inside the writer's transaction, so the good track in
    // the same call did not go on either, and nothing was re-read.
    let (view, _) = s.open(tag_list_spec());
    assert_eq!(ids(&s.rows(view)), [track.as_str()]);
    assert_eq!(s.state().summary().3, generation);
    let deadline = Instant::now() + Duration::from_secs(2);
    while *s.tag_list_changes.lock().unwrap() == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(*s.tag_list_changes.lock().unwrap(), 1, "only the add that went through announced");
}

// ----------------------------------------------------------- track editing

#[test]
fn a_rating_a_comment_and_a_colour_show_in_the_rows_after_the_edit() {
    let s = shell();
    let track = track_id(7);

    run(commands::set_track_rating(s.handle(), s.state(), track.clone(), 4)).unwrap();
    run(commands::set_track_comment(s.handle(), s.state(), track.clone(), "opener — long intro".into()))
        .unwrap();
    run(commands::set_track_color(s.handle(), s.state(), track.clone(), Some("pink".into()))).unwrap();

    let (view, _) = s.open(collection_spec());
    let rows = s.rows(view);
    let row = rows.iter().find(|r| r.id == track).expect("the track is still in the collection");
    assert_eq!(row.rating, 4);
    assert_eq!(row.comment, "opener — long intro");

    // And the sort that reads the column sees it too.
    let (view, _) = s.open(ViewSpecDto { sort: "rating".into(), descending: true, ..collection_spec() });
    assert_eq!(s.rows(view)[0].id, track);

    run(commands::set_track_rating(s.handle(), s.state(), track.clone(), 0)).unwrap();
    run(commands::set_track_color(s.handle(), s.state(), track.clone(), None)).unwrap();
    let (view, _) = s.open(collection_spec());
    assert_eq!(s.rows(view).iter().find(|r| r.id == track).unwrap().rating, 0);
}

#[test]
fn the_information_panel_reads_the_record_and_writes_a_field_the_rows_follow() {
    let s = shell();
    let track = track_id(9);

    let record = run(details::track_details(s.state(), track.clone())).unwrap();
    assert_eq!(record.id, track);
    assert_eq!(record.title, "Track 009");
    assert_eq!(record.rating, 0);
    assert_eq!(record.duration_sec, 300);

    run(details::set_track_field(s.handle(), s.state(), track.clone(), "title".into(), "Nine".into())).unwrap();
    run(details::set_track_field(s.handle(), s.state(), track.clone(), "artist".into(), "Somebody".into()))
        .unwrap();
    run(details::set_track_field(s.handle(), s.state(), track.clone(), "year".into(), "2019".into())).unwrap();
    run(commands::set_track_rating(s.handle(), s.state(), track.clone(), 2)).unwrap();

    let record = run(details::track_details(s.state(), track.clone())).unwrap();
    assert_eq!((record.title.as_str(), record.artist.as_str(), record.year, record.rating), ("Nine", "Somebody", 2019, 2));

    // The row in the table, the sort and the search all see the new title.
    let (view, len) = s.open(ViewSpecDto { query: "somebody".into(), ..collection_spec() });
    assert_eq!(len, 1);
    let rows = s.rows(view);
    assert_eq!((rows[0].title.as_str(), rows[0].artist.as_str()), ("Nine", "Somebody"));
    let (view, _) = s.open(ViewSpecDto { sort: "artist".into(), descending: true, ..collection_spec() });
    assert_eq!(s.rows(view)[0].id, track, "the one track with an artist sorts first");

    // A field the writer does not take is refused before anything is opened.
    let err = run(details::set_track_field(s.handle(), s.state(), track.clone(), "bitrate".into(), "320".into()))
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::ReadOnly);
    let err = run(details::set_track_field(s.handle(), s.state(), track, "year".into(), "soon".into())).unwrap_err();
    assert_eq!(err.kind, ErrorKind::ReadOnly);
}

#[test]
fn a_cue_added_through_the_command_is_read_back_and_announced() {
    let s = shell();
    let track = track_id(4);
    let announced: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&announced);
    s.app.listen("cues:changed", move |event| {
        seen.lock().unwrap().push(event.payload().trim_matches('"').to_owned());
    });

    assert!(run(commands::track_cues(s.state(), track.clone())).unwrap().is_empty());

    let hot = run(cues::add_cue(s.handle(), s.state(), track.clone(), CueKind::Hot('B'), 12_000)).unwrap();
    let memory = run(cues::add_cue(s.handle(), s.state(), track.clone(), CueKind::Memory, 30_000)).unwrap();
    let looped = run(cues::add_loop(
        s.handle(),
        s.state(),
        track.clone(),
        CueKind::Memory,
        40_000,
        44_000,
        Some(8),
    ))
    .unwrap();

    let cues = run(commands::track_cues(s.state(), track.clone())).unwrap();
    assert_eq!(cues.len(), 3);
    let by_id = |id: &str| cues.iter().find(|c| c.id == id).unwrap();
    assert_eq!((by_id(&hot).letter.as_str(), by_id(&hot).position_ms, by_id(&hot).memory), ("B", 12_000, false));
    assert!(by_id(&memory).memory);
    assert_eq!((by_id(&looped).position_ms, by_id(&looped).out_ms), (40_000, 44_000));

    // The browser row carries the hot cue's letter without a reload.
    let (view, _) = s.open(collection_spec());
    let rows = s.rows(view);
    let row = rows.iter().find(|r| r.id == track).unwrap();
    assert_eq!(row.hot_cues.iter().map(|c| c.0).collect::<String>(), "B");

    run(cues::move_cue(s.handle(), s.state(), hot.clone(), 15_000)).unwrap();
    run(cues::delete_cue(s.handle(), s.state(), memory.clone())).unwrap();
    let cues = run(commands::track_cues(s.state(), track.clone())).unwrap();
    assert_eq!(cues.len(), 2);
    assert_eq!(cues.iter().find(|c| c.id == hot).unwrap().position_ms, 15_000);
    assert!(cues.iter().all(|c| c.id != memory));

    // Every edit named the track, so the decks showing it refetch.
    let deadline = Instant::now() + Duration::from_secs(2);
    while announced.lock().unwrap().len() < 5 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let announced = announced.lock().unwrap();
    assert_eq!(announced.len(), 5);
    assert!(announced.iter().all(|t| *t == track));
}

/// A drive library's Location, in the Info panel and the browser column, is
/// the path rekordbox shows: the stored `FolderPath` with `BaseDBDrive`
/// swapped for `CurrentDBDrive` (`replaceDrivePath`), not the raw column.
#[test]
fn a_drive_librarys_location_reads_under_the_drives_current_mount() {
    let s = shell();
    let track = track_id(3);
    let location = s.state().location().unwrap();
    fixture::point_at_audio(&location, 3, "/Volumes/Music/Tracks/a.mp3", 300).unwrap();
    let writer = rbl_db::write::Writer::open(location, s._dir.path().join("drive-backups")).unwrap();
    writer
        .library()
        .connection()
        .execute("UPDATE djmdProperty SET BaseDBDrive = '/Volumes/Music/', CurrentDBDrive = '/Volumes/Music 1/'", [])
        .unwrap();
    drop(writer);

    // On Windows a fixture folder off the default sits on a lettered drive,
    // which rekordbox takes as the current drive instead.
    let expected = if cfg!(windows) {
        format!("{}/Tracks/a.mp3", &s._dir.path().to_string_lossy()[..2])
    } else {
        "/Volumes/Music 1/Tracks/a.mp3".to_owned()
    };
    let record = run(details::track_details(s.state(), track.clone())).unwrap();
    assert_eq!(record.path, expected);

    let (view, _) = s.open(collection_spec());
    let rows = run(commands::fetch_rows(s.state(), view, 0, commands::MAX_ROWS, Some(vec!["location".into()]))).unwrap();
    let row = rows.iter().find(|r| r.id == track).unwrap();
    assert_eq!(row.extra.as_ref().unwrap()["location"], expected.as_str());
}

#[test]
fn a_cue_added_outside_the_app_appears_without_reloading_the_library() {
    let s = shell();
    let track = track_id(4);
    assert!(run(commands::track_cues(s.state(), track.clone())).unwrap().is_empty());

    // Rekordbox can write a cue while RBX still holds its startup snapshot.
    let mut external = rbl_db::write::Writer::open(s.state().location().unwrap(), s._dir.path().join("external-backups")).unwrap();
    external.add_cue(&track, 7, 61).unwrap(); // Hot Cue F
    drop(external);

    let cues = run(commands::track_cues(s.state(), track)).unwrap();
    assert_eq!(cues.len(), 1);
    assert_eq!((cues[0].letter.as_str(), cues[0].position_ms), ("F", 61));
}

/// Collects every `cues:changed` the command emits, and waits for them: the
/// emit is the last thing each edit does and the listener runs off-thread.
fn cue_announcements(s: &Shell) -> impl Fn(usize) -> Vec<String> {
    let announced: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&announced);
    s.app.listen("cues:changed", move |event| {
        seen.lock().unwrap().push(event.payload().trim_matches('"').to_owned());
    });
    move |count: usize| {
        let deadline = Instant::now() + Duration::from_secs(2);
        while announced.lock().unwrap().len() < count && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        announced.lock().unwrap().clone()
    }
}

#[test]
fn memory_cues_are_converted_into_the_free_hot_slots_in_position_order_and_each_one_is_announced() {
    let s = shell();
    let track = track_id(5);
    let announced = cue_announcements(&s);

    // B is taken before the conversion, so the three memory cues take the
    // letters left: A, C and D, in order of position rather than of adding.
    run(cues::add_cue(s.handle(), s.state(), track.clone(), CueKind::Hot('B'), 1_000)).unwrap();
    run(cues::add_cue(s.handle(), s.state(), track.clone(), CueKind::Memory, 30_000)).unwrap();
    run(cues::add_loop(s.handle(), s.state(), track.clone(), CueKind::Memory, 10_000, 14_000, Some(8))).unwrap();
    run(cues::add_cue(s.handle(), s.state(), track.clone(), CueKind::Memory, 50_000)).unwrap();
    assert_eq!(announced(4).len(), 4, "the four cues that set the track up");

    let made = run(cues::convert_memory_cues_to_hot(s.handle(), s.state(), track.clone())).unwrap();
    assert_eq!(made, 3);

    let cues = run(commands::track_cues(s.state(), track.clone())).unwrap();
    assert_eq!(cues.len(), 7, "the memory cues are kept beside the hot cues they became");
    let mut hot: Vec<(String, u32, u32)> = cues
        .iter()
        .filter(|c| !c.memory)
        .map(|c| (c.letter.clone(), c.position_ms, c.out_ms))
        .collect();
    hot.sort();
    assert_eq!(
        hot,
        vec![
            ("A".to_owned(), 10_000, 14_000),
            ("B".to_owned(), 1_000, 0),
            ("C".to_owned(), 30_000, 0),
            ("D".to_owned(), 50_000, 0),
        ],
        "the loop stayed a loop when it became a hot cue"
    );

    // One `cues:changed` per cue written, so a deck showing the track
    // refetches after each of them rather than only at the end.
    let announced = announced(7);
    assert_eq!(announced.len(), 7);
    assert!(announced.iter().all(|t| *t == track));
}

#[test]
fn a_conversion_stops_at_the_last_free_hot_slot_and_a_track_with_no_memory_cues_is_a_no_op() {
    let s = shell();
    let track = track_id(6);
    let announced = cue_announcements(&s);

    // Fifteen of the sixteen slots taken, and two memory cues for the one
    // that is left.
    for letter in 'A'..='O' {
        run(cues::add_cue(s.handle(), s.state(), track.clone(), CueKind::Hot(letter), 1_000)).unwrap();
    }
    run(cues::add_cue(s.handle(), s.state(), track.clone(), CueKind::Memory, 20_000)).unwrap();
    run(cues::add_cue(s.handle(), s.state(), track.clone(), CueKind::Memory, 40_000)).unwrap();

    assert_eq!(run(cues::convert_memory_cues_to_hot(s.handle(), s.state(), track.clone())).unwrap(), 1);
    let cues = run(commands::track_cues(s.state(), track.clone())).unwrap();
    assert_eq!(cues.len(), 18, "one cue written, and the memory cue with nowhere to go is untouched");
    assert_eq!(
        cues.iter().find(|c| c.letter == "P").unwrap().position_ms,
        20_000,
        "the earlier memory cue took the last slot"
    );

    // Every slot is taken now, so a second conversion writes nothing at all.
    assert_eq!(run(cues::convert_memory_cues_to_hot(s.handle(), s.state(), track.clone())).unwrap(), 0);
    assert_eq!(run(commands::track_cues(s.state(), track.clone())).unwrap().len(), 18);

    // A track with no memory cues is the same no-op.
    let other = track_id(7);
    assert_eq!(run(cues::convert_memory_cues_to_hot(s.handle(), s.state(), other.clone())).unwrap(), 0);
    assert!(run(commands::track_cues(s.state(), other)).unwrap().is_empty());

    // Seventeen for the setup and one for the conversion: neither no-op
    // announced anything, because neither wrote anything.
    let announced = announced(18);
    assert_eq!(announced.len(), 18);
    assert!(announced.iter().all(|t| *t == track));
}

// ---------------------------------------------------------------- playing

#[test]
fn an_imported_file_goes_into_a_playlist_and_plays_on_a_deck() {
    let s = shell();
    let audio = s._dir.path().join("Silent Two Seconds.wav");
    write_wav(&audio, 2);

    // Imported: the library grows by one, and the row is the file's.
    let report = run(commands::import_files(s.handle(), s.state(), vec![audio.display().to_string()])).unwrap();
    assert_eq!(report.imported, 1);
    assert!(report.skipped.is_empty());
    let id = report.tracks[0].id.clone();
    assert_eq!(run(commands::library_summary(s.state())).unwrap().track_count, 41);
    let (view, _) = s.open(ViewSpecDto { query: "silent".into(), ..collection_spec() });
    let rows = s.rows(view);
    assert_eq!(ids(&rows), [id.as_str()]);
    assert_eq!(rows[0].duration_sec, 2);

    // Into a playlist, and loaded from there.
    run(commands::add_tracks_to_playlist(s.handle(), s.state(), playlist_id(1), vec![id.clone()])).unwrap();
    assert_eq!(s.playlist_rows(&playlist_id(1)).len(), 6);

    // Nothing has opened the audio output yet: a window nobody played in
    // holds no device.
    assert!(s.sink.lock().unwrap().is_none());
    assert!(!s.deck_state().a.loaded);

    run(commands::deck_load(s.handle(), s.state(), s.player(), "a".into(), id.clone(), 1)).unwrap();
    let loaded = s.pull_until("the deck to load", |t| t.a.loaded);
    assert_eq!(loaded.sample_rate, RATE);
    assert_eq!(loaded.a.total_frames, u64::from(RATE) * 2);
    assert_eq!(loaded.a.load_id, 1);
    assert!(!loaded.a.playing);
    assert_eq!(loaded.a.frames, 0);
    assert!(!loaded.b.loaded, "the other deck is untouched");

    // Playing moves the clock; pausing stops it where it is.
    run(commands::deck_play(s.handle(), s.player(), "a".into())).unwrap();
    let playing = s.pull_until("the playhead to move", |t| t.a.frames > 4_096);
    assert!(playing.a.playing);
    run(commands::deck_pause(s.player(), "a".into())).unwrap();
    s.pull_until("the deck to pause", |t| !t.a.playing);
    // A pause is a fade, and the clock runs to the end of it; after that
    // the deck stays put however much the device pulls.
    let sink = s.sink.lock().unwrap().clone().unwrap();
    for _ in 0..8 {
        sink.pull(512);
    }
    let at = s.deck_state().a.frames;
    for _ in 0..8 {
        sink.pull(512);
    }
    assert_eq!(s.deck_state().a.frames, at, "a paused deck does not drift");

    // A seek lands where it was asked, in frames, whether paused or not.
    run(commands::deck_seek(s.handle(), s.player(), "a".into(), 500.0)).unwrap();
    let sought = s.pull_until("the seek to land", |t| t.a.frames == i64::from(RATE) / 2);
    assert!(sought.a.generation > loaded.a.generation, "the interface snaps rather than eases");

    // Unloaded: the deck is empty again.
    run(commands::deck_unload(s.player(), "a".into())).unwrap();
    let empty = s.pull_until("the deck to unload", |t| !t.a.loaded && t.a.frames == 0);
    assert_eq!(empty.a.frames, 0);
}

#[test]
fn a_track_whose_file_is_gone_is_refused_at_load_rather_than_failing_later() {
    let s = shell();
    // The fixture's tracks point at files that do not exist. That is caught
    // when the deck is asked for one, not by the engine mid-play.
    let err = run(commands::deck_load(s.handle(), s.state(), s.player(), "a".into(), "no-such-track".into(), 1))
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::NotFound);
    assert!(s.sink.lock().unwrap().is_none(), "the audio output was not opened for it");
}

#[test]
fn the_two_decks_play_independently_and_the_master_level_is_the_engine_s() {
    let s = shell();
    let a = write_wav(&s._dir.path().join("a.wav"), 1);
    let b = write_wav(&s._dir.path().join("b.wav"), 3);
    let report = run(commands::import_files(
        s.handle(),
        s.state(),
        vec![a.display().to_string(), b.display().to_string()],
    ))
    .unwrap();
    assert_eq!(report.imported, 2);
    let (id_a, id_b) = (report.tracks[0].id.clone(), report.tracks[1].id.clone());

    run(commands::deck_load(s.handle(), s.state(), s.player(), "a".into(), id_a, 1)).unwrap();
    run(commands::deck_load(s.handle(), s.state(), s.player(), "b".into(), id_b, 2)).unwrap();
    s.pull_until("both decks to load", |t| t.a.loaded && t.b.loaded);

    run(commands::set_master_level(s.handle(), s.player(), 0.5)).unwrap();
    assert!((s.deck_state().master - 0.5).abs() < 1e-6);

    run(commands::deck_play(s.handle(), s.player(), "b".into())).unwrap();
    let tick = s.pull_until("deck B to move", |t| t.b.frames > 4_096);
    assert!(tick.b.playing);
    assert!(!tick.a.playing);
    assert_eq!(tick.a.frames, 0, "deck A stays put while B plays");

    // Deck A's one second runs out; deck B is still going.
    run(commands::deck_play(s.handle(), s.player(), "a".into())).unwrap();
    let ended = s.pull_until("deck A to reach its end", |t| !t.a.playing && t.a.frames > 0);
    assert!(ended.b.playing);
    assert!(ended.b.frames > ended.a.frames);
}

#[test]
fn a_waveform_click_previews_the_track_without_loading_a_deck() {
    let s = shell();
    let a = write_wav(&s._dir.path().join("deck.wav"), 3);
    let b = write_wav(&s._dir.path().join("preview.wav"), 4);
    let report = run(commands::import_files(
        s.handle(),
        s.state(),
        vec![a.display().to_string(), b.display().to_string()],
    ))
    .unwrap();
    let (on_deck, previewed) = (report.tracks[0].id.clone(), report.tracks[1].id.clone());

    // Nothing previewed yet: no preview output opened, and an idle state.
    assert_eq!(s.preview_state(), PreviewStateDto { track: None, playing: false, position_ms: 0.0, duration_ms: 0.0 });
    assert!(s.preview_sink.lock().unwrap().is_none());

    // Deck A is playing something else.
    run(commands::deck_load(s.handle(), s.state(), s.player(), "a".into(), on_deck.clone(), 1)).unwrap();
    s.pull_until("deck A to load", |t| t.a.loaded);
    run(commands::deck_play(s.handle(), s.player(), "a".into())).unwrap();
    s.pull_until("deck A to play", |t| t.a.playing && t.a.frames > 0);

    // A click halfway across the second track's waveform.
    run(commands::preview_play(s.handle(), s.state(), s.player(), s.preview(), previewed.clone(), 2_000.0)).unwrap();
    // rekordbox outside PERFORMANCE mode pauses the decks for a preview.
    let decks = s.pull_until("deck A to pause", |t| !t.a.playing);
    assert_eq!(decks.a.load_id, 1, "the deck keeps its own track; the preview did not load onto it");
    let playing = s.pull_preview_until("the preview to move past the click", |p| p.playing && p.position_ms > 2_050.0);
    assert_eq!(playing.track.as_deref(), Some(previewed.as_str()));
    assert!((playing.duration_ms - 4_000.0).abs() < 1.0);
    assert!(playing.position_ms < 3_000.0, "started at the click, not the top: {playing:?}");

    // A click on the same track moves it rather than reloading it.
    run(commands::preview_play(s.handle(), s.state(), s.player(), s.preview(), previewed.clone(), 500.0)).unwrap();
    let moved = s.pull_preview_until("the preview to move back", |p| p.playing && p.position_ms < 1_500.0);
    assert!(moved.position_ms >= 500.0);

    // Stopped where it is.
    run(commands::preview_stop(s.preview())).unwrap();
    s.pull_preview_until("the preview to stop", |p| !p.playing);

    // A track whose file is not there is refused, as rekordbox refuses it.
    std::fs::remove_file(&a).unwrap();
    let err = run(commands::preview_play(s.handle(), s.state(), s.player(), s.preview(), on_deck, 0.0)).unwrap_err();
    assert_eq!(err.kind, ErrorKind::NotFound);
}

#[test]
fn usb_export_preflight_reports_missing_source_audio() {
    let s = shell();
    let audio = s._dir.path().join("Will disappear.wav");
    write_wav(&audio, 2);
    let imported = run(commands::import_files(
        s.handle(),
        s.state(),
        vec![audio.display().to_string()],
    ))
    .unwrap();
    run(commands::add_tracks_to_playlist(
        s.handle(),
        s.state(),
        playlist_id(1),
        vec![imported.tracks[0].id.clone()],
    ))
    .unwrap();
    std::fs::remove_file(&audio).unwrap();

    let missing = run(commands::validate_export_files(
        s.state(),
        vec![playlist_id(1)],
    ))
    .unwrap();
    assert!(missing
        .iter()
        .any(|file| file.path == audio.display().to_string()));
}

#[test]
fn export_track_puts_a_track_on_a_stick_by_itself_and_a_sync_keeps_it_there() {
    let s = shell();
    let one = s._dir.path().join("One.wav");
    let two = s._dir.path().join("Two.wav");
    write_wav(&one, 2);
    write_wav(&two, 2);
    let report = run(commands::import_files(
        s.handle(),
        s.state(),
        vec![one.display().to_string(), two.display().to_string()],
    ))
    .unwrap();
    let (first, second) = (report.tracks[0].id.clone(), report.tracks[1].id.clone());
    run(commands::add_tracks_to_playlist(s.handle(), s.state(), playlist_id(1), vec![first.clone()])).unwrap();

    // The second track goes on a blank stick on its own: one track, no
    // playlist, and the record says it is loose.
    let stick = tempfile::tempdir().unwrap();
    let written = run(commands::export_tracks_to_device(
        s.handle(),
        s.state(),
        vec![second.clone()],
        stick.path().display().to_string(),
        None,
        None,
    ))
    .unwrap();
    assert_eq!((written.tracks, written.playlists), (1, 0));
    let record = rbl_export::Manifest::load(stick.path()).expect("a record");
    assert_eq!(record.loose, vec![second.parse::<u64>().unwrap()]);

    // A sync of playlist 1 to the same stick keeps the loose track beside
    // the playlist's, and the record still says which is which.
    let reports = run(commands::sync_devices(
        s.handle(),
        s.state(),
        vec![playlist_id(1)],
        vec![stick.path().display().to_string()],
        None,
        None,
        None,
        None,
        None,
    ))
    .unwrap();
    let synced = reports[0].report.as_ref().expect("written");
    assert_eq!((synced.tracks, synced.playlists, synced.removed), (2, 1, 0));
    let record = rbl_export::Manifest::load(stick.path()).expect("a record");
    assert_eq!(record.loose, vec![second.parse::<u64>().unwrap()]);
    assert_eq!(record.playlists.len(), 1);

    // Exporting the first track loose too, on top of the synced playlist:
    // it is in the playlist already, so it is not loose, and nothing is
    // removed.
    let again = run(commands::export_tracks_to_device(
        s.handle(),
        s.state(),
        vec![first],
        stick.path().display().to_string(),
        None,
        None,
    ))
    .unwrap();
    assert_eq!((again.tracks, again.playlists, again.removed), (2, 1, 0));

    // Both playlist entry points honor cleanup. Only recorded USB copies
    // may disappear, never the source or an unrelated file on the stick.
    let unrelated = stick.path().join("Keep me.wav");
    write_wav(&unrelated, 1);
    let original_one = std::fs::read(&one).unwrap();
    let original_two = std::fs::read(&two).unwrap();
    let unrelated_bytes = std::fs::read(&unrelated).unwrap();
    for single_playlist in [false, true] {
        run(commands::export_tracks_to_device(
            s.handle(), s.state(), vec![second.clone()], stick.path().display().to_string(), None, None,
        )).unwrap();
        let before = rbl_export::Manifest::load(stick.path()).unwrap();
        let loose = before.tracks.iter().find(|t| t.library_id.to_string() == second).unwrap();
        let loose_audio = stick.path().join(loose.audio.trim_start_matches('/'));
        let loose_analysis = stick.path().join(loose.anlz_dir.trim_start_matches('/'));
        assert!(loose_audio.exists());
        let cleaned = if single_playlist {
            run(commands::export_playlist(
                s.handle(), s.state(), playlist_id(1), stick.path().display().to_string(), None, Some(true), None,
            )).unwrap()
        } else {
            run(commands::sync_devices(
                s.handle(), s.state(), vec![playlist_id(1)], vec![stick.path().display().to_string()],
                None, None, None, Some(true), None,
            )).unwrap().remove(0).report.unwrap()
        };
        assert_eq!((cleaned.tracks, cleaned.playlists, cleaned.removed), (1, 1, 1));
        assert!(!loose_audio.exists());
        if !loose.anlz_dir.is_empty() { assert!(!loose_analysis.exists()); }
        let after = rbl_export::Manifest::load(stick.path()).unwrap();
        assert!(after.loose.is_empty());
        assert_eq!(after.tracks.len(), 1);
        assert!(stick.path().join(after.tracks[0].audio.trim_start_matches('/')).exists());
        assert_eq!(std::fs::read(&one).unwrap(), original_one);
        assert_eq!(std::fs::read(&two).unwrap(), original_two);
        assert_eq!(std::fs::read(&unrelated).unwrap(), unrelated_bytes);
    }
}

#[test]
fn a_sync_writes_the_same_playlists_to_every_stick_and_each_stick_remembers_them() {
    let s = shell();
    // One real file in playlist 1, so there is something to copy; the
    // fixture's other rows point at audio that does not exist and are
    // skipped, which is a stick's ordinary condition, not a failure.
    let audio = s._dir.path().join("Silent Two Seconds.wav");
    write_wav(&audio, 2);
    let report = run(commands::import_files(s.handle(), s.state(), vec![audio.display().to_string()])).unwrap();
    let id = report.tracks[0].id.clone();
    run(commands::add_tracks_to_playlist(s.handle(), s.state(), playlist_id(1), vec![id])).unwrap();

    let progress: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&progress);
    s.app.listen("sync:progress", move |event| {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(event.payload()) {
            seen.lock().unwrap().push((
                value["path"].as_str().unwrap_or_default().to_owned(),
                value["state"].as_str().unwrap_or_default().to_owned(),
            ));
        }
    });

    // A stick that is not there says nothing about what it holds.
    let gone = s._dir.path().join("gone");
    let err = run(commands::device_sync_state(s.state(), gone.display().to_string())).unwrap_err();
    assert_eq!(err.kind, ErrorKind::NotFound);

    // A stick with nothing on it: no selection, nothing on the device.
    let stick_a = tempfile::tempdir().unwrap();
    let stick_b = tempfile::tempdir().unwrap();
    let fresh = run(commands::device_sync_state(s.state(), stick_a.path().display().to_string())).unwrap();
    assert!(fresh.selected.is_empty());
    assert!(fresh.on_device.is_empty());

    // Two sticks and one that was pulled: the two are written, the third
    // reports its error, and the run says which is which as it goes.
    let reports = run(commands::sync_devices(
        s.handle(),
        s.state(),
        vec![playlist_id(1)],
        vec![
            stick_a.path().display().to_string(),
            gone.display().to_string(),
            stick_b.path().display().to_string(),
        ],
        None,
        Some(true),
        Some(true),
        None,
        None,
    ))
    .unwrap();
    assert_eq!(reports.len(), 3);
    let written_a = reports[0].report.as_ref().expect("stick A written");
    assert!(reports[0].error.is_none());
    assert!(!reports[0].ejected);
    assert!(reports[0].eject_error.as_deref().unwrap().contains("incomplete"), "skipped tracks keep the device mounted");
    assert_eq!(written_a.playlists, 1);
    assert_eq!(written_a.tracks, 1);
    assert!(written_a.verified);
    assert!(reports[1].report.is_none());
    assert_eq!(reports[1].error.as_deref(), Some("That device is no longer connected. It may have been unplugged or renamed."));
    let written_b = reports[2].report.as_ref().expect("stick B written");
    assert_eq!(written_b.tracks, written_a.tracks);
    // The workers run independently, so event ordering is deliberately not
    // part of the protocol. Each destination still gets a start and exactly
    // one terminal state.
    let progress = progress.lock().unwrap();
    for (path, terminal) in [
        (stick_a.path().display().to_string(), "done"),
        (gone.display().to_string(), "failed"),
        (stick_b.path().display().to_string(), "done"),
    ] {
        let states = progress.iter().filter(|(seen, _)| seen == &path).map(|(_, state)| state.as_str()).collect::<Vec<_>>();
        assert_eq!(states.first(), Some(&"writing"));
        assert_eq!(states.last(), Some(&terminal));
        assert_eq!(states.len(), 2);
    }

    // Each stick remembers the selection it was given, by the tree's id,
    // and shows the playlist it holds.
    for stick in [&stick_a, &stick_b] {
        let state = run(commands::device_sync_state(s.state(), stick.path().display().to_string())).unwrap();
        assert_eq!(state.selected.len(), 1);
        assert_eq!(state.selected[0].library_id, playlist_id(1));
        assert_eq!(state.on_device, vec![state.selected[0].name.clone()]);
        assert!(state.automatic, "the record asked for automatic sync");
        // The sync record rekordbox reads is there, naming this library and
        // the playlist by its id, and it alone carries the selection once
        // our manifest is gone.
        let record = rbl_export::sync_record::read(stick.path()).expect("a sync record");
        assert!(record.automatic);
        assert_eq!(record.ticked, vec![playlist_id(1).parse::<u64>().unwrap()]);
        std::fs::remove_file(stick.path().join("PIONEER/rbxport/manifest.json")).unwrap();
        let from_record = run(commands::device_sync_state(s.state(), stick.path().display().to_string())).unwrap();
        assert_eq!(from_record.selected.len(), 1);
        assert_eq!(from_record.selected[0].library_id, playlist_id(1));
    }
}

#[test]
fn exporting_a_folder_writes_the_folder_with_its_playlists_inside() {
    use rbxport_lib::dto::{SmartConditionDto, SmartRuleDto};
    let s = shell();
    let audio = s._dir.path().join("Folder Song.wav");
    write_wav(&audio, 2);
    let report = run(commands::import_files(s.handle(), s.state(), vec![audio.display().to_string()])).unwrap();
    let song = report.tracks[0].id.clone();

    run(commands::create_folder(s.handle(), s.state(), "Set".into(), ROOT.into())).unwrap();
    let set = s.node("Set");
    run(commands::create_playlist(s.handle(), s.state(), "Inside".into(), set.id.clone())).unwrap();
    run(commands::add_tracks_to_playlist(s.handle(), s.state(), s.node("Inside").id, vec![song])).unwrap();
    run(commands::create_folder(s.handle(), s.state(), "Later".into(), set.id.clone())).unwrap();
    let rule = SmartRuleDto {
        logic: "all".to_owned(),
        conditions: vec![SmartConditionDto {
            property: "name".to_owned(), operator: "11".to_owned(), left: "Folder Song".to_owned(), right: String::new(), unit: String::new(),
        }],
    };
    run(commands::create_smart_playlist(s.handle(), s.state(), "Songs".into(), set.id.clone(), rule)).unwrap();

    let stick = tempfile::tempdir().unwrap();
    let written = run(commands::export_playlist(
        s.handle(), s.state(), set.id.clone(), stick.path().display().to_string(), None, None, None,
    ))
    .unwrap();
    assert_eq!(written.tracks, 1, "one track, however many playlists hold it");

    let snapshot = rbl_export::snapshot::Snapshot::read(stick.path()).unwrap();
    for library in [snapshot.one.expect("exportLibrary.db"), snapshot.legacy.expect("export.pdb")] {
        let named = |name: &str| library.playlists.iter().find(|p| p.name == name).unwrap_or_else(|| panic!("no {name} in {:?}", library.playlists));
        let set = named("Set");
        assert!(set.folder, "the folder is a folder on the stick, not an empty playlist");
        assert_eq!(set.parent, 0);
        let later = named("Later");
        assert!(later.folder && later.parent == set.id, "an empty folder under it keeps its place");
        for name in ["Inside", "Songs"] {
            let playlist = named(name);
            assert!(!playlist.folder);
            assert_eq!(playlist.parent, set.id);
            assert_eq!(playlist.tracks.len(), 1, "{name}");
        }
        assert_eq!(library.playlists.len(), 4);
    }
}

#[test]
fn an_intelligent_playlist_is_made_from_a_rule_and_its_rule_is_edited() {
    use rbxport_lib::dto::{SmartConditionDto, SmartRuleDto};
    let s = shell();
    let condition = |property: &str, operator: &str, left: &str, right: &str| SmartConditionDto {
        property: property.to_owned(),
        operator: operator.to_owned(),
        left: left.to_owned(),
        right: right.to_owned(),
        unit: String::new(),
    };
    // Track titles are "Track 000" to "Track 039": the ones ending in 7.
    let rule = SmartRuleDto { logic: "all".to_owned(), conditions: vec![condition("name", "11", "7", "")] };
    run(commands::create_smart_playlist(s.handle(), s.state(), "Sevens".to_owned(), ROOT.to_owned(), rule)).unwrap();
    let node = s.node("Sevens");
    assert_eq!(node.kind, "smartPlaylist");
    assert_eq!(s.playlist_rows(&node.id).len(), 4, "007, 017, 027, 037");

    // The rule reads back as it was given, and a new one takes effect.
    let read = run(commands::smart_rule(s.state(), node.id.clone())).unwrap();
    assert_eq!(read.logic, "all");
    assert_eq!(read.conditions.len(), 1);
    assert_eq!((read.conditions[0].property.as_str(), read.conditions[0].operator.as_str(), read.conditions[0].left.as_str()), ("name", "11", "7"));
    let wider = SmartRuleDto {
        logic: "any".to_owned(),
        conditions: vec![condition("name", "11", "7", ""), condition("name", "11", "8", "")],
    };
    run(commands::set_smart_rule(s.handle(), s.state(), node.id.clone(), wider)).unwrap();
    assert_eq!(s.playlist_rows(&node.id).len(), 8);
    // Not on a plain playlist, and not with a property this cannot answer.
    assert!(run(commands::set_smart_rule(
        s.handle(),
        s.state(),
        playlist_id(1),
        SmartRuleDto { logic: "all".to_owned(), conditions: vec![] }
    ))
    .is_err());
    assert!(run(commands::create_smart_playlist(
        s.handle(),
        s.state(),
        "Nope".to_owned(),
        ROOT.to_owned(),
        SmartRuleDto { logic: "all".to_owned(), conditions: vec![condition("hotCueCount", "1", "x", "")] }
    ))
    .is_err());
}

#[test]
fn an_intelligent_playlist_on_a_my_tag_holds_the_tracks_carrying_it() {
    // Issue #84: a rule on a My Tag opened empty and read back with no
    // property, which the editor drew as "Album artist".
    use rbl_db::fixture::MY_TAG_PEAK;
    use rbxport_lib::dto::{SmartConditionDto, SmartRuleDto};
    let s = shell();
    run(details::set_my_tags(s.handle(), s.state(), track_id(3), vec![MY_TAG_PEAK.to_owned()])).unwrap();
    run(details::set_my_tags(s.handle(), s.state(), track_id(5), vec![MY_TAG_PEAK.to_owned()])).unwrap();
    let rule = SmartRuleDto {
        logic: "all".to_owned(),
        conditions: vec![SmartConditionDto {
            property: "myTag".to_owned(),
            operator: "8".to_owned(),
            left: MY_TAG_PEAK.to_owned(),
            right: String::new(),
            unit: String::new(),
        }],
    };
    run(commands::create_smart_playlist(s.handle(), s.state(), "Peak".to_owned(), ROOT.to_owned(), rule)).unwrap();
    let node = s.node("Peak");
    assert_eq!(s.playlist_rows(&node.id).len(), 2);
    let read = run(commands::smart_rule(s.state(), node.id.clone())).unwrap();
    assert_eq!(
        (read.conditions[0].property.as_str(), read.conditions[0].operator.as_str(), read.conditions[0].left.as_str()),
        ("myTag", "8", MY_TAG_PEAK)
    );
}

#[test]
fn export_selection_materializes_an_intelligent_playlist_from_its_rule() {
    use rbxport_lib::dto::{SmartConditionDto, SmartRuleDto};

    let s = shell();
    let audio = write_wav(&s._dir.path().join("Smart Export Match.wav"), 1);
    let imported = run(commands::import_files(
        s.handle(),
        s.state(),
        vec![audio.display().to_string()],
    ))
    .unwrap();
    assert_eq!(imported.imported, 1);

    let rule = SmartRuleDto {
        logic: "all".to_owned(),
        conditions: vec![SmartConditionDto {
            property: "name".to_owned(),
            operator: "11".to_owned(),
            left: "Smart Export Match".to_owned(),
            right: String::new(),
            unit: String::new(),
        }],
    };
    run(commands::create_smart_playlist(
        s.handle(),
        s.state(),
        "Rule Export".to_owned(),
        ROOT.to_owned(),
        rule,
    ))
    .unwrap();
    let node = s.node("Rule Export");
    assert_eq!(node.kind, "smartPlaylist");
    assert_eq!(s.playlist_rows(&node.id).len(), 1);
    let missing = run(commands::validate_export_files(s.state(), vec![node.id.clone()]))
        .unwrap_or_else(|error| panic!("selection failed: {:?}", error.detail));
    assert!(missing.is_empty());
    std::fs::remove_file(&audio).unwrap();
    let missing = run(commands::validate_export_files(s.state(), vec![node.id]))
        .unwrap_or_else(|error| panic!("selection failed: {:?}", error.detail));
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].title, "Smart Export Match");
    assert_eq!(missing[0].path, audio.display().to_string());
}
