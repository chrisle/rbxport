//! Tauri shell. Command bodies live in the `rbl-*` crates; everything here is
//! a thin adapter so the backend stays testable without a webview.
//!
//! The commands, their DTOs and the state are public so `tests/` can drive
//! them against a mock app and a fixture library, the way the webview does
//! against the real one.

mod windowfit;
mod file_drop;
mod file_drag;
mod startup;
mod screen_cache;
mod sentry;
pub mod analysis;
pub mod commands;
mod usb_import;
pub mod cues;
pub mod details;
pub mod grid;
mod durable;
mod backups;
mod backup_copy;
mod backup_zip;
mod backup_sizes;
mod backup_restore_scripts;
mod file_journal;
mod new_library;
mod diagnostics;
mod explorer;
mod device_library;
mod link;
mod rx3_link;
mod network_labels;
pub mod logging;
pub mod menu;
pub mod player;
pub mod preview;
mod preferences;
mod browse_settings;
mod protocol;
mod relocate;
mod sync_window;
mod report;
mod scripting;
mod device_settings;
pub mod dto;
mod error;
pub mod state;
mod test_port;
mod elevated_update;
mod update;

pub use error::{AppError, AppResult, ErrorKind};

/// Runs the protected Windows update entry point before Tauri starts.
pub fn run_elevated_update_helper_if_requested() -> Option<i32> {
    update::run_elevated_helper_if_requested()
}

use state::AppState;
use std::sync::Arc;
use tauri::Manager;

/// Loads the library off the UI thread and hands it to the state.
///
/// Read-only always: this application never opens the user's library for
/// writing during startup, and `rbl-db` refuses it while rekordbox runs.
/// Where the library snapshot lives.
///
/// Under the app's own data directory, not the library's: it is derived, it is
/// ours, and nothing outside this app should ever find it next to rekordbox's
/// files.
fn cache_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    use tauri::Manager as _;
    Some(app.path().app_cache_dir().ok()?.join("library.snapshot"))
}

/// The schema version as a plain number, so a library whose schema changed
/// never reads a snapshot built against the old one.
fn schema_key(db_version: Option<i64>) -> u32 {
    db_version.and_then(|v| u32::try_from(v).ok()).unwrap_or(0)
}

pub(crate) fn spawn_library_load(app: tauri::AppHandle) {
    tauri::async_runtime::spawn_blocking(move || {
        let started = std::time::Instant::now();
        if let Ok(location) = rbl_db::detect() {
            if let Err(e) = backups::recover(app.state::<Arc<state::AppState>>().backup_dir(), &location) {
                report_problem(&app, dto::LibraryProblemDto::Failed { message: e.to_string() });
                return;
            }
        }
        let cache_path = cache_path(&app);
        let snapshot = cache_path.clone().and_then(|path| {
            std::thread::Builder::new().name("startup-snapshot".into())
                .spawn(move || rbl_index::cache::prepare(&path)).ok()
        });
        match rbl_db::Library::open_installed_read_only() {
            Ok(db) => {
                if let Err(e) = file_journal::recover(app.state::<Arc<state::AppState>>().backup_dir(), db.location()) {
                    tracing::error!(error = %e, "analysis recovery failed");
                    report_problem(&app, dto::LibraryProblemDto::Failed { message: e.to_string() });
                    return;
                }
                let db_version = db.schema().db_version;
                let location = db.location().clone();
                let master_db = db.location().master_db.clone();
                // Reading 38,681 rows out of SQLCipher is 543 ms of the 680 ms
                // a start costs, and none of it gets faster — the work is the
                // decryption. A snapshot of the built columns turns the same
                // start into a sequential read.
                // Content rather than file times: rekordbox rewrites the WAL
                // without changing a row, and keying on that refused the
                // snapshot on every start it was running for.
                let content = rbl_index::content_version(&db).ok();
                let fingerprint = cache_path.as_ref().and_then(|_| {
                    rbl_index::cache::Fingerprint::of(&master_db, schema_key(db_version), content?)
                });
                let prepared = snapshot.and_then(|job| job.join().ok()).flatten();
                if let Some(fp) = fingerprint {
                    if let Some(library) = prepared.and_then(|snapshot| snapshot.validated(fp)) {
                        let load_ms =
                            u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                        tracing::debug!(tracks = library.len(), load_ms, "library from cache");
                        let read_only = rbl_db::is_rekordbox_running();
                        app.state::<Arc<AppState>>().set_library(
                            library, read_only, db_version, load_ms, location,
                        );
                        let _ = tauri::Emitter::emit(&app, "library:ready", ());
                        return;
                    }
                }
                // A second read-only handle allows cues to overlap metadata.
                // Failure falls back to the single-connection loader.
                let cue_reader = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).ok();
                match rbl_index::load_with_cue_reader(&db, cue_reader) {
                    Ok((library, stats)) => {
                        let load_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                        tracing::debug!(
                            tracks = stats.tracks,
                            playlists = stats.playlists,
                            heap_mb = stats.heap_bytes / 1_048_576,
                            load_ms,
                            "library loaded"
                        );
                        // Writes are gated on rekordbox not running, which we
                        // re-check per transaction; the banner reflects it now.
                        let read_only = rbl_db::is_rekordbox_running();
                        app.state::<Arc<AppState>>()
                            .set_library(library, read_only, db_version, load_ms, location);
                        let _ = tauri::Emitter::emit(&app, "library:ready", ());

                        // Written after the interface is live, and only if the
                        // database has not moved since the fingerprint was
                        // taken — rekordbox may have written while we read,
                        // and a snapshot of a half-read library keyed to bytes
                        // that no longer exist would be served on a later
                        // start as though it were current.
                        if let (Some(path), Some(before)) = (cache_path.as_ref(), fingerprint) {
                            let after = rbl_index::content_version(&db).ok().and_then(|now| {
                                rbl_index::cache::Fingerprint::of(
                                    &master_db,
                                    schema_key(db_version),
                                    now,
                                )
                            });
                            if after == Some(before) {
                                let held = app.state::<Arc<AppState>>();
                                if let Ok(library) = held.library() {
                                    if let Err(e) =
                                        rbl_index::cache::save(path, &library, before)
                                    {
                                        tracing::warn!(error = %e, "could not write the library cache");
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "could not index the library");
                        report_problem(&app, dto::LibraryProblemDto::Failed { message: e.to_string() });
                    }
                }
            }
            Err(e) => {
                // Nothing to open, as against something that would not open:
                // no library anywhere, or one configured on a drive that is
                // not connected. Both are questions for the window rather
                // than failures.
                if let Some(problem) = rbl_db::locate::locate().ok().as_ref().and_then(new_library::problem_from) {
                    tracing::info!(?problem, error = %e, "no library to open; asking");
                    report_problem(&app, problem);
                    return;
                }
                tracing::error!(error = %e, "could not open the library");
                report_problem(&app, dto::LibraryProblemDto::Failed { message: e.to_string() });
            }
        }
    });
}

/// Keeps why the library did not load, for a window that asks later, and
/// tells a window already listening.
fn report_problem(app: &tauri::AppHandle, problem: dto::LibraryProblemDto) {
    app.state::<Arc<AppState>>().set_library_problem(Some(problem.clone()));
    let _ = tauri::Emitter::emit(app, "library:problem", problem);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Brings the window onto a screen that can hold it, at startup.
///
/// The configured 1800x1130 is larger than a 13-inch laptop's work area, and
/// the window-state plugin restores wherever the window was last — which may
/// be a monitor that is no longer plugged in. Either way the result is a
/// window partly or wholly out of reach, and on macOS a title bar above the
/// menu bar cannot be dragged back.
///
/// Best effort throughout: a screen that cannot be measured is a reason to
/// leave the window alone, not to fail the launch.
/// The label the window in `tauri.conf.json` gets by default.
const MAIN_WINDOW: &str = "main";

/// What the window-state plugin saves and puts back: everything but whether
/// the window is showing. Every window is created hidden and shows itself
/// once its page has rendered (`startup::show_window`); a restore that
/// included visibility called `show()` the moment the window was built, and
/// the window came up as an empty frame until React drew into it.
const WINDOW_STATE: tauri_plugin_window_state::StateFlags =
    tauri_plugin_window_state::StateFlags::all().difference(tauri_plugin_window_state::StateFlags::VISIBLE);

/// How long after the window appears its geometry is still corrected.
///
/// The restored position arrives asynchronously and was measured landing
/// within a second. Two gives that room without reaching as far as anything a
/// person could have done deliberately.
const SETTLE_WINDOW: std::time::Duration = std::time::Duration::from_secs(2);


/// Puts the window back where it was, then makes sure that is on a screen.
///
/// Registered after the window-state plugin, which is told to
/// `skip_initial_state` so that the restore happens here instead — the check
/// has to follow it, and it cannot follow something this does not control.
fn window_geometry() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    use tauri_plugin_window_state::{AppHandleExt, WindowExt};

    tauri::plugin::Builder::<tauri::Wry>::new("windowfit")
        .on_window_ready(|window| {
            if window.label() != MAIN_WINDOW {
                return;
            }
            if let Err(e) = window.restore_state(WINDOW_STATE) {
                tracing::warn!(error = %e, "could not restore the window's geometry");
            }

            // Once now, which covers a first run: no saved geometry to restore,
            // and a configured size larger than the screen it opened on.
            fit_window(&window);

            // And again as the restore lands, which is the case that actually
            // bites. That move is posted to the windowing system and does not
            // arrive for the best part of a second — measured, the window
            // reported x=120 immediately, from a callback queued on the main
            // thread behind the move, and from the first `Moved` event, while
            // its real restored position was x=936 and stayed that way. No
            // single read is trustworthy, so this watches the window's own
            // events for a moment instead.
            //
            // Bounded, because every move after startup is somebody dragging
            // the window, and one that will not stay where it is put is worse
            // than one that hangs off an edge.
            let opened = std::time::Instant::now();
            let subject = window.clone();
            // After the settle window, a move or resize is the user placing the
            // window, so save it then rather than only on exit. The plugin's own
            // save runs on a graceful quit; a kill or a crash runs nothing, which
            // is how a window dragged to a second display kept coming back to the
            // first — the drag was never written. Throttled so a drag does not
            // rewrite the file every frame.
            let last_save = std::sync::Arc::new(std::sync::Mutex::new(opened));
            window.on_window_event(move |event| {
                if !matches!(event, tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_)) {
                    return;
                }
                if opened.elapsed() < SETTLE_WINDOW {
                    fit_window(&subject);
                    return;
                }
                let Ok(mut last) = last_save.lock() else { return };
                if last.elapsed() < std::time::Duration::from_millis(300) {
                    return;
                }
                *last = std::time::Instant::now();
                drop(last);
                if let Err(e) = subject.app_handle().save_window_state(WINDOW_STATE) {
                    tracing::warn!(error = %e, "could not save the window's geometry");
                }
            });
        })
        .build()
}

fn fit_window<R: tauri::Runtime>(window: &tauri::Window<R>) {
    use tauri::{PhysicalPosition, PhysicalSize};

    // The monitor the window is on, or the primary one when it is off every
    // screen and Tauri cannot say which it belongs to.
    let monitor = match window.current_monitor() {
        Ok(Some(monitor)) => Some(monitor),
        _ => window.primary_monitor().ok().flatten(),
    };
    let Some(monitor) = monitor else { return };

    let (Ok(position), Ok(size)) = (window.outer_position(), window.outer_size()) else {
        return;
    };

    // The work area, not the whole monitor: it excludes the menu bar and the
    // Dock, which is what "on the screen" means to someone using it.
    let area = monitor.work_area();
    let available =
        windowfit::Rect::new(area.position.x, area.position.y, area.size.width, area.size.height);
    let current = windowfit::Rect::new(position.x, position.y, size.width, size.height);
    let fitted = windowfit::fit_within(current, available);
    if fitted == current {
        return;
    }

    tracing::debug!(
        from = format!("{}x{} at {},{}", current.width, current.height, current.x, current.y),
        to = format!("{}x{} at {},{}", fitted.width, fitted.height, fitted.x, fitted.y),
        "window did not fit the screen"
    );
    // Size first: moving a window that is still too big only pins it to a
    // corner with the far edge still off.
    if (fitted.width, fitted.height) != (current.width, current.height) {
        let _ = window.set_size(PhysicalSize::new(fitted.width, fitted.height));
    }
    if (fitted.x, fitted.y) != (current.x, current.y) {
        let _ = window.set_position(PhysicalPosition::new(fitted.x, fitted.y));
    }
}

/// Names a port for `WebView2`'s remote debugging, on Windows.
///
/// Set, the webviews accept a Chrome `DevTools` Protocol connection on
/// `127.0.0.1:<port>`, which is how `scripts/e2e-win/` drives the compiled
/// app with Playwright. Unset — every ordinary launch — nothing listens.
/// An environment variable rather than a build flag because the point is to
/// test the build that ships. `WebView2`'s own `WEBVIEW2_ADDITIONAL_BROWSER_
/// ARGUMENTS` is not honoured once the host sets arguments of its own, which
/// wry does, so it has to be passed here.
pub const DEVTOOLS_PORT_ENV: &str = "RBXPORT_DEVTOOLS_PORT";

/// The browser arguments for every webview: wry's defaults, which setting
/// any argument replaces, plus the debugging port when one is asked for.
/// `None` when nothing is asked for, so wry's own defaults stand untouched.
pub fn browser_args() -> Option<String> {
    let port = std::env::var(DEVTOOLS_PORT_ENV).ok()?.parse::<u16>().ok()?;
    // What wry passes when left alone (`wry/src/webview2/mod.rs`), with the
    // autoplay policy this app's audio elements rely on.
    Some(format!(
        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
         --autoplay-policy=no-user-gesture-required --remote-debugging-port={port}"
    ))
}

#[allow(clippy::too_many_lines, reason = "the command list is one line per command, and that is the whole function")]
pub fn run() {
    startup::begin();
    logging::install();
    // Keep the guard alive until the Tauri event loop exits so a panic has a
    // chance to be delivered during shutdown.  The build injects the DSN;
    // without one this intentionally becomes a no-op client.
    let _sentry = sentry::install();

    let mut context = tauri::generate_context!();
    if let Some(args) = browser_args() {
        tracing::debug!(port = %std::env::var(DEVTOOLS_PORT_ENV).unwrap_or_default(), "webview remote debugging on");
        for window in &mut context.config_mut().app.windows {
            window.additional_browser_args = Some(args.clone());
        }
    }

    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        // Puts the window back where it was: size, position, and whether it
        // was maximised. Restored before the window is shown, so it does not
        // appear at the default size and jump.
        // The saved geometry is put back by hand rather than automatically,
        // because it has to be followed by a check that it still fits a screen
        // that is present. Every way of doing that afterwards was tried
        // against a real saved state of 3600x1982 at x=936 on a 3840-wide
        // display — in `setup`, on `RunEvent::Ready`, and from a plugin hook
        // registered after this one — and all three measured the window before
        // the restore had moved it, so all three found nothing wrong.
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(WINDOW_STATE)
                .skip_initial_state(MAIN_WINDOW)
                .build(),
        )
        .plugin(window_geometry())
        .manage(Arc::new(AppState::new()))
        .manage(Arc::new(crate::player::Player::default()))
        .manage(Arc::new(crate::preview::Preview::default()))
        .manage(Arc::new(crate::grid::GridEditor::default()))
        .manage(Arc::new(crate::update::Updates::default()))
        .manage(crate::test_port::TestPort::default())
        .setup(|app| {
            screen_cache::initialize(app.handle());
            // Listens only in a debug build asked to (`RBXPORT_TEST_PORT`).
            crate::test_port::start(app.handle());
            // AppleScript: the bridge to the window, and on macOS the
            // scriptable classes, before any Apple Event can arrive.
            crate::scripting::install(app.handle());
            spawn_library_load(app.handle().clone());
            // Join the network on start: a passive watcher that hears every
            // player and mixer and reports them, so the shell can offer LINK
            // the moment one appears. Nothing is transmitted until LINK is on.
            {
                let handle = app.handle().clone();
                let state = Arc::clone(app.state::<Arc<AppState>>().inner());
                tauri::async_runtime::spawn_blocking(move || {
                    let emitter = handle.clone();
                    let watcher = crate::link::start_watcher(move |peers| {
                        let _ = tauri::Emitter::emit(&emitter, "link:peers", peers);
                    });
                    drop(state.set_watcher(watcher));
                });
            }
            // A stick plugged in or pulled out is noticed within a couple of
            // seconds, focused or not; the panel refreshes itself on the event.
            {
                let emitter = app.handle().clone();
                let mounts = rbl_devices::MountWatcher::start(rbl_devices::mounts::INTERVAL, move || {
                    let _ = tauri::Emitter::emit(&emitter, "devices:changed", ());
                });
                app.manage(mounts);
            }
            app.set_menu(crate::menu::build(app.handle())?)?;
            Ok(())
        })
        .on_menu_event(|app, event| crate::menu::on_event(app, event.id().as_ref()))
        // Closing the main window is quitting, as it is in rekordbox. The
        // Preferences window is a window of its own, and with it still open
        // the process stayed alive showing nothing but Preferences (0.5.1 on
        // Windows, where the main window's close box is the way out).
        .on_window_event(|window, event| {
            if window.label() == MAIN_WINDOW && matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                for label in [crate::preferences::WINDOW, crate::sync_window::WINDOW] {
                    if let Some(extra) = window.app_handle().get_webview_window(label) {
                        let _ = extra.close();
                    }
                }
            }
        })
        // Asynchronous, not the plain form. `wry` calls a synchronous handler
        // straight from the `WKURLSchemeHandler` callback, which is the main
        // thread on macOS, so every artwork read and every audio range would
        // block the UI thread — a fast scroll fires one per row inside the
        // frame loop. The responder lets the read happen on a blocking worker
        // instead, which is the same rule `commands.rs` already follows.
        .register_asynchronous_uri_scheme_protocol("rbl", move |ctx, request, responder| {
            // Artwork goes to the webview as an <img> rather than through
            // invoke: a JPEG blows the 64 KB IPC cap and would cost a
            // main-thread base64 decode per row.
            let state = Arc::clone(ctx.app_handle().state::<Arc<AppState>>().inner());
            tauri::async_runtime::spawn_blocking(move || {
                crate::protocol::serve(&state, &request, |response| responder.respond(response));
            });
        })
        .invoke_handler(tauri::generate_handler![
            browse_settings::rekordbox_browse_settings,
            startup::startup_milestone,
            startup::show_window,
            screen_cache::remember_screen_assets,
            file_drop::dropped_file_paths,
            file_drag::drag_tracks,
            menu::set_history_menu,
            menu::set_menu_labels,
            commands::library_summary,
            commands::disable_read_only,
            new_library::library_problem,
            new_library::create_library,
            new_library::use_default_library,
            new_library::database_drives,
            new_library::switch_library,
            commands::playlist_tree,
            commands::open_view,
            commands::fetch_rows,
            commands::view_ids_in_range,
            commands::track_waveform,
            commands::track_pcm_waveform,
            analysis::analyse_track,
            analysis::edit_phrase,
            commands::reload_library,
            // Editing. Every one of these is refused while rekordbox is
            // running, re-checked immediately before the transaction.
            commands::link_status,
            commands::link_peers,
            commands::start_link_export,
            commands::stop_link_export,
            commands::link_load_track,
            commands::link_set_master,
            commands::link_nudge_master,
            commands::link_take_master_tempo,
            commands::export_playlist,
            commands::export_progress,
            commands::cancel_export,
            commands::list_devices,
            commands::track_beats,
            // The decks. The audio device is not opened until one of these
            // is called, so a window nobody has played anything in holds no
            // device at all.
            commands::deck_load,
            commands::deck_unload,
            commands::deck_play,
            commands::deck_set_loop,
            commands::deck_loop_active,
            commands::deck_clear_loop,
            commands::deck_play_after,
            commands::deck_pause,
            commands::deck_seek,
            commands::deck_move,
            commands::deck_scrub_begin,
            commands::deck_scrub_to,
            commands::deck_scrub_end,
            commands::app_diagnostics,
            commands::app_version,
            commands::open_log,
            commands::reveal_track,
            commands::set_master_level,
            commands::audio_devices,
            commands::set_audio_device,
            commands::master_limiter,
            commands::set_master_limiter,
            update::check_for_update,
            update::ready_update,
            update::download_update,
            update::restart_to_update,
            commands::deck_tempo,
            commands::deck_metronome,
            commands::deck_metronome_grid,
            commands::deck_key_shift,
            commands::set_metronome,
            commands::set_audio_config,
            commands::deck_master_tempo,
            commands::set_channel_band,
            commands::set_channel_kill,
            commands::set_channel_trim,
            commands::set_crossfade,
            commands::set_eq_curve,
            commands::deck_state,
            commands::preview_play,
            commands::preview_stop,
            commands::preview_state,
            commands::track_cues,
            // The GRID panel: every one rewrites the track's analysis files
            // and is refused while rekordbox runs, like the edits above.
            grid::grid_state,
            grid::grid_edit,
            grid::grid_undo,
            grid::grid_redo,
            grid::grid_lock,
            commands::track_phrases,
            commands::track_vocals,
            relocate::missing_tracks,
            relocate::remove_missing_tracks,
            commands::unanalysed_tracks,
            commands::find_duplicates,
            commands::import_files,
            commands::relocate_track,
            relocate::auto_relocate,
            preferences::open_preferences,
            sync_window::open_sync_window,
            report::open_report_window,
            report::report_attachment,
            report::open_report_attachment,
            commands::sync_devices,
            commands::validate_export_files,
            commands::eject_device,
            commands::export_tracks_to_device,
            commands::device_sync_state,
            usb_import::import_usb,
            device_settings::reference_stick_settings,
            commands::create_playlist,
            commands::smart_rule,
            commands::create_smart_playlist,
            commands::set_smart_rule,
            commands::create_folder,
            commands::rename_playlist,
            commands::move_playlist,
            commands::delete_playlist,
            commands::undo_edit,
            commands::redo_edit,
            commands::add_tracks_to_playlist,
            commands::reload_tags,
            commands::add_to_tag_list,
            commands::remove_from_tag_list,
            commands::clear_tag_list,
            commands::remove_tracks_from_playlist,
            commands::export_playlist_file,
            commands::export_loop_wav,
            commands::import_xml,
            commands::import_itunes,
            commands::itunes_default_library,
            commands::itunes_library_at,
            commands::import_itunes_selected,
            commands::export_xml,
            commands::list_backups,
            commands::backup_directory,
            commands::open_backup_directory,
            commands::backup_sizes,
            commands::backup_progress,
            commands::start_backup,
            commands::cancel_backup,
            commands::back_up_library,
            commands::delete_backup,
            commands::set_backup_directory,
            commands::open_url,
            commands::reset_play_count,
            commands::record_play,
            commands::remove_from_history,
            commands::remove_from_collection,
            commands::reorder_playlist,
            commands::set_track_rating,
            commands::set_track_comment,
            commands::set_track_color,
            cues::add_cue,
            cues::add_loop,
            cues::move_cue,
            cues::set_cue_colour,
            cues::delete_cue,
            cues::convert_memory_cues_to_hot,
            commands::filter_values,
            device_settings::device_settings,
            device_settings::write_device_defaults,
            device_settings::save_device_settings,
            device_settings::ensure_device_library,
            explorer::explorer_roots,
            explorer::explorer_children,
            device_library::device_libraries,
            device_library::device_playlist_edit,
            device_library::device_playlist_import,
            // The information panel: one track's full record, the lookup
            // lists its dropdowns offer, and the fields it may write.
            details::track_details,
            details::selection_details,
            details::track_lookups,
            details::set_track_field,
            details::add_artwork,
            details::add_playlist_artwork,
            details::set_my_tags,
            details::clear_artwork,
            // AppleScript's way into the window: it says it is listening,
            // answers what a script asked of it, and mirrors the
            // preferences so a script can read them.
            scripting::script_ready,
            scripting::script_reply,
            scripting::script_preferences,
            // Test-only: the page answering `RBXPORT_TEST_PORT`'s questions.
            test_port::test_eval_result,
        ])
        .build(context);

    match result {
        Ok(app) => {
            crate::update::Updates::clear_stale(app.handle());
            app.run(|handle, event| {
                // An update downloaded this run and waiting for the quit
                // (Windows) is installed now, silently.
                if matches!(event, tauri::RunEvent::Exit) {
                    crate::screen_cache::save(handle);
                    crate::update::on_exit(handle);
                }
            });
        }
        Err(e) => {
            tracing::error!(error = %e, "fatal: could not start the application");
            std::process::exit(1);
        }
    }
}
