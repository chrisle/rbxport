//! USB-to-library reads. Never infer track identity from a title or USB row id.
use std::{collections::{HashMap, HashSet}, io::Read, path::{Path, PathBuf}, sync::Arc};
use serde::Serialize;
use tauri::{State, Manager};
use crate::{commands::{announce_history, blocking, reload, write_error, wrote_outside_history}, dto::{DeviceLibraryTreeDto, DevicePlaylistNodeDto}, error::{AppError, AppResult, ErrorKind}, state::AppState};

mod ratings;

fn err(e: impl std::fmt::Display) -> AppError {
    let detail = format!("USB import: {e}");
    AppError::new(ErrorKind::Internal, detail.clone()).with_detail(detail)
}
// ANLZ is rendered in memory by the parser and the journal. Refuse a single
// oversized file before reading it, so staging also has a per-track bound.
const MAX_IMPORT_ANLZ_BYTES: u64 = 64 * 1024 * 1024;
const MAX_IMPORT_STAGE_BYTES: u64 = 512 * 1024 * 1024;
const STAGE_PREFIX: &str = "usb-import-v1-";
const STAGE_MARKER: &str = "owner.lock";
const STAGE_MARKER_CONTENT: &str = "rbxport usb import stage v1";

// Read through the handle that holds the lock: Windows byte-range locks reject
// reads from a second handle on the same file. Bounded so a foreign file
// cannot be read whole.
fn marker_has_owner_content(marker: &mut std::fs::File) -> std::io::Result<bool> {
    use std::io::Seek;
    marker.rewind()?;
    let mut bytes = Vec::new();
    marker.by_ref().take(STAGE_MARKER_CONTENT.len() as u64 + 1).read_to_end(&mut bytes)?;
    Ok(bytes == STAGE_MARKER_CONTENT.as_bytes())
}

fn stage_budget_add(used: &mut u64, bytes: usize, limit: u64) -> AppResult<()> {
    *used = used.checked_add(u64::try_from(bytes).map_err(err)?)
        .filter(|total| *total <= limit)
        .ok_or_else(|| err("USB import staging exceeds the 512 MiB aggregate limit"))?;
    Ok(())
}

fn recover_stale_stages(backup_dir: &Path) -> AppResult<()> {
    if !backup_dir.exists() { return Ok(()); }
    let root = backup_dir.canonicalize().map_err(err)?;
    for entry in std::fs::read_dir(backup_dir).map_err(err)? {
        let entry = entry.map_err(err)?;
        if !entry.file_name().to_string_lossy().starts_with(STAGE_PREFIX) { continue; }
        let path = entry.path();
        if !entry.file_type().map_err(err)?.is_dir() || path.canonicalize().map_err(err)?.parent() != Some(root.as_path()) { continue; }
        let marker_path = path.join(STAGE_MARKER);
        if !std::fs::symlink_metadata(&marker_path).is_ok_and(|m| m.file_type().is_file()) { continue; }
        let mut marker = std::fs::OpenOptions::new().read(true).write(true).open(&marker_path).map_err(err)?;
        if fs2::FileExt::try_lock_exclusive(&marker).is_err() { continue; }
        if !marker_has_owner_content(&mut marker).map_err(err)? { continue; }
        // Close the marker before removing its directory on Windows. The
        // exclusive lock proves another importer is not using this stage.
        drop(marker);
        std::fs::remove_dir_all(&path).map_err(err)?;
    }
    Ok(())
}

fn read_import_anlz(path: &Path, stage: &str, extension: &str, id: &str) -> AppResult<rbl_anlz::Anlz> {
    let file = std::fs::File::open(path)
        .map_err(|e| err(format!("{stage} {extension} for track {id}: {e}")))?;
    read_import_anlz_file(file, stage, extension, id)
}

fn read_import_anlz_file(mut file: std::fs::File, stage: &str, extension: &str, id: &str) -> AppResult<rbl_anlz::Anlz> {
    let mut bytes = Vec::new();
    file.by_ref().take(MAX_IMPORT_ANLZ_BYTES + 1).read_to_end(&mut bytes)
        .map_err(|e| err(format!("{stage} {extension} for track {id}: {e}")))?;
    if bytes.len() as u64 > MAX_IMPORT_ANLZ_BYTES {
        return Err(err(format!("{stage} {extension} for track {id}: analysis file exceeds the 64 MiB import limit")));
    }
    rbl_anlz::parse(&bytes).map_err(|e| err(format!("{stage} {extension} for track {id}: {e}")))
}
// Identity is captured from the same handle whose bytes are parsed. A
// failed identity query is not a new failure for the existing full importer.
fn read_import_anlz_identified(path: &Path, stage: &str, extension: &str, id: &str)
    -> AppResult<(rbl_anlz::Anlz, Option<rbl_core::durable::FileIdentity>)> {
    let file = std::fs::File::open(path).map_err(|e| err(format!("{stage} {extension} for track {id}: {e}")))?;
    let identity = rbl_core::durable::file_identity(&file).ok();
    Ok((read_import_anlz_file(file, stage, extension, id)?, identity))
}

fn strict_import_grid(anlz: &rbl_anlz::Anlz) -> AppResult<Option<&rbl_anlz::Section>> {
    let mut grids = anlz.sections.iter().filter(|section| section.tag.0 == *b"PQTZ");
    let Some(grid) = grids.next() else { return Ok(None); };
    if grids.next().is_some() { return Err(err("USB DAT has ambiguous PQTZ sections")); }
    let count = grid.header.get(8..12).and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
        .map(u32::from_be_bytes).ok_or_else(|| err("USB DAT has an incomplete PQTZ header"))?;
    let bytes = usize::try_from(count).ok().and_then(|count| count.checked_mul(8))
        .ok_or_else(|| err("USB DAT PQTZ count exceeds the supported length"))?;
    if grid.payload.len() != bytes { return Err(err("USB DAT PQTZ count does not match its payload")); }
    Ok((count > 0).then_some(grid))
}

fn within(root: &Path, relative: &str) -> AppResult<PathBuf> {
    let path = root.join(relative.trim_start_matches('/')).canonicalize().map_err(err)?;
    if !path.starts_with(root.canonicalize().map_err(err)?) { return Err(err("File lies outside the USB device")); }
    Ok(path)
}

pub fn library_trees(root: &Path) -> AppResult<Vec<DeviceLibraryTreeDto>> {
    let export = rbl_devices::settings::export_root(root);
    let mut libraries = Vec::new();
    let pdb = export.join("rekordbox/export.pdb");
    if pdb.exists() {
        let bytes = std::fs::read(pdb).map_err(err)?;
        let parsed = rbl_pdb::Pdb::parse(&bytes).map_err(err)?;
        let mut nodes = parsed.table(rbl_pdb::PageType::PlaylistTree).map(|t| parsed.playlist_nodes(t)).unwrap_or_default();
        nodes.sort_by_key(|n| (n.parent_id, n.sort_order));
        libraries.push(DeviceLibraryTreeDto { name: "Device Library".into(), nodes: nodes.into_iter().map(|n| DevicePlaylistNodeDto {
            id: n.id.to_string(), parent_id: n.parent_id.to_string(), name: n.name, folder: n.is_folder,
        }).collect() });
    }
    let one = export.join("rekordbox/exportLibrary.db");
    if one.exists() {
        let db = rbl_onelibrary::ExportLibrary::open_read_only(&one).map_err(err)?;
        let mut q = db.connection().prepare("SELECT playlist_id, COALESCE(playlist_id_parent,0), name, attribute FROM playlist ORDER BY sequenceNo").map_err(err)?;
        let nodes = q.query_map([], |r| Ok(DevicePlaylistNodeDto { id: r.get::<_,i64>(0)?.to_string(), parent_id: r.get::<_,i64>(1)?.to_string(), name: r.get(2)?, folder: r.get::<_,i64>(3)? != 0 })).map_err(err)?.collect::<Result<Vec<_>,_>>().map_err(err)?;
        libraries.push(DeviceLibraryTreeDto { name: "OneLibrary".into(), nodes });
    }
    Ok(libraries)
}

/// Where My Settings imported from a stick are kept: beside the state
/// directory `state_dir` (the backups' recovery folder). Export reads the same
/// folder to give them to a stick that has none, so both sides must agree,
/// including under `RBXPORT_STATE_DIR`.
pub(crate) fn settings_stash(state_dir: &Path) -> PathBuf {
    state_dir.parent().unwrap_or(state_dir).join("usb-settings")
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all="camelCase")]
pub struct ImportReport {
    tracks: usize, histories: usize, settings: usize, ratings: usize, skipped: usize,
    /// Tracks whose available import data already matches the library.
    unchanged: usize,
    warnings: Vec<String>,
    #[serde(skip)] changed: Vec<String>,
}

/// Explicit imports and Sync Manager imports share identity checks.
#[tauri::command]
#[allow(clippy::too_many_arguments, reason = "Tauri injects the app and state alongside the independent import choices")]
#[allow(clippy::fn_params_excessive_bools, reason = "independent import choices preserve the existing flat IPC contract")]
pub async fn import_usb<R: tauri::Runtime>(app: tauri::AppHandle<R>, state: State<'_, Arc<AppState>>, path: String, cues: bool, history: bool, settings: bool, ratings: bool) -> AppResult<ImportReport> {
    let state = Arc::clone(&state);
    let worker = Arc::clone(&state);
    if !rbl_devices::list().iter().any(|d| d.mount_point == Path::new(&path)) { return Err(err("Device is no longer connected")); }
    let editor = Arc::clone(&app.state::<Arc<crate::grid::GridEditor>>());
    let log_path = path.clone();
    let result = blocking("import_usb", move || import(&worker, &editor, Path::new(&path), cues, history, settings, ratings)).await;
    match result {
        Ok(result) => {
            if result.tracks > 0 || result.histories > 0 || result.ratings > 0 {
                // Cues, histories and ratings written outside the library
                // history: a Redo must not put back a rating this replaced.
                wrote_outside_history(&state, false);
                reload(app.clone(), Arc::clone(&state)).await?;
                announce_history(&app, &state);
            }
            for id in &result.changed {
                let _ = tauri::Emitter::emit(&app, "grid:changed", id);
                let _ = tauri::Emitter::emit(&app, "cues:changed", id);
            }
            Ok(result)
        }
        Err(e) => {
            tracing::error!(path = %log_path, cues, history, settings, ratings, error = %e, "USB import failed");
            // Part of it may have landed before the failure.
            wrote_outside_history(&state, false);
            let _ = reload(app.clone(), Arc::clone(&state)).await;
            announce_history(&app, &state);
            Err(e)
        }
    }
}

#[allow(clippy::fn_params_excessive_bools, reason = "the same independent choices as import_usb")]
fn import(state: &AppState, editor: &crate::grid::GridEditor, root: &Path, cues: bool, history: bool, settings: bool, ratings: bool) -> AppResult<ImportReport> {
    import_with_stage_budget(state, editor, root, cues, history, settings, ratings, MAX_IMPORT_STAGE_BYTES)
}

#[allow(clippy::fn_params_excessive_bools, reason = "the same independent choices as import_usb")]
#[allow(clippy::too_many_arguments, reason = "the stage limit allows disposable budget regression tests")]
#[allow(clippy::too_many_lines, reason = "the existing import sequence keeps its preflight and journal together")]
fn import_with_stage_budget(state: &AppState, editor: &crate::grid::GridEditor, root: &Path, cues: bool, history: bool, settings: bool, ratings: bool, stage_limit: u64) -> AppResult<ImportReport> {
    // The stick first, the library after: an import waiting here for an
    // export to finish writing the stick must not hold the library's edit
    // gate, or every analysis and the next sync's preparation wait behind
    // it (#284). An export never asks for the gate while it holds the stick.
    rbl_devices::settings::recover(root)
        .map_err(|e| err(format!("Could not recover an interrupted export before importing: {e}")))?;
    let _device_read = rbl_core::durable::read_lock(root).map_err(err)?;
    let _gate = state.edit_gate.lock();
    let _files = state.analysis_write.lock();
    let mut report = ImportReport::default();
    let location = state.location()?;
    crate::file_journal::recover(state.backup_dir(), &location)?;
    recover_stale_stages(state.backup_dir())?;
    let export = rbl_devices::settings::export_root(root);
    let one_path = export.join("rekordbox/exportLibrary.db");
    let one = if one_path.exists() { Some(rbl_onelibrary::ExportLibrary::open_read_only(&one_path).map_err(err)?) } else { None };
    let db_id = state.read_db(|db| rbl_db::export_info::db_id(db.connection())).map_err(write_error)?;
    // USB id -> master id, analysis path. masterDbId prevents importing another library's ids.
    let mut tracks: HashMap<u32, (String, String)> = HashMap::new();
    if let Some(db) = &one {
        let mut q = db.connection().prepare("SELECT content_id, masterContentId, COALESCE(analysisDataFilePath,'') FROM content WHERE masterDbId=?1 AND masterContentId>0").map_err(err)?;
        for entry in q.query_map([i64::try_from(db_id).unwrap_or(i64::MAX)], |r| Ok((r.get::<_,u32>(0)?, (r.get::<_,i64>(1)?.to_string(), r.get::<_,String>(2)?)))).map_err(err)? {
            let (id, value) = entry.map_err(err)?; tracks.insert(id, value);
        }
    }
    // Checked before the manifest's entries join: theirs are already checked.
    let gone = retain_live(state, &mut tracks)?;
    // Our manifest also works on legacy-only exports; validate its source path against master.db.
    let manifest = rbl_export::Manifest::load(root).filter(|m| m.db_id == 0 || m.db_id == db_id);
    let mut rating_tracks = HashMap::new();
    if let Some(manifest) = &manifest {
        // The manifest records the path as the index resolved it.
        let drive = state.read_db(|db| Ok(db.drive_mapping())).map_err(write_error)?;
        for t in &manifest.tracks {
            let id = t.library_id.to_string();
            let stored = state.read_db(|db| Ok(db.connection().query_row("SELECT FolderPath FROM djmdContent WHERE ID=?1 AND rb_local_deleted=0", [&id], |r| r.get::<_,String>(0)).ok())).map_err(write_error)?;
            let matched = stored.is_some_and(|p| drive.as_ref().map_or(std::borrow::Cow::Borrowed(p.as_str()), |d| d.apply(&p)) == t.source.as_str());
            if matched {
                rating_tracks.insert(t.export_id, id.clone());
                let analysis = if t.anlz_dir.is_empty() { String::new() } else { format!("{}/ANLZ0000.DAT", t.anlz_dir) };
                tracks.entry(t.export_id).or_insert((id, analysis));
            }
        }
    }
    // Resolve all rating conflicts before any requested imports write to the library.
    if ratings { ratings::import(state, root, manifest.as_ref(), &rating_tracks, db_id, &mut report)?; }
    if cues && tracks.is_empty() { return Err(err("No tracks from this library were found on the device.")); }
    if cues {
        report.skipped += gone;
        // Open the guarded writer even for an empty device; read-only must not look like success.
        state.write(|_| Ok(())).map_err(write_error)?;
        // Parse every eligible ANLZ before publishing any cue/grid changes. A
        // later malformed track must not leave earlier tracks committed.
        // Use the library's on-disk backup volume: /tmp can be tmpfs and
        // would turn a large import back into an unbounded memory consumer.
        std::fs::create_dir_all(state.backup_dir()).map_err(err)?;
        let staging = tempfile::Builder::new().prefix(STAGE_PREFIX)
            .tempdir_in(state.backup_dir()).map_err(err)?;
        let marker_path = staging.path().join(STAGE_MARKER);
        std::fs::write(&marker_path, STAGE_MARKER_CONTENT).map_err(err)?;
        let marker = std::fs::OpenOptions::new().read(true).write(true).open(&marker_path).map_err(err)?;
        fs2::FileExt::lock_exclusive(&marker).map_err(err)?;
        let mut staged_bytes = 0;
        let mut prepared = Vec::new();
        let mut grid_only_tracks = 0usize;
        let mut grid_images = HashMap::new();
        let mut ambiguous_grids = HashSet::new();
        let mut grid_noops = Vec::new();
        let mut full_paths = HashSet::new();
        let mut unknown_full_identity = false;
        let mut unavailable_grid_identity = 0usize;
        let mut ordered: Vec<_> = tracks.values().collect();
        ordered.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        for (id, analysis) in ordered {
            // Read on the open handle. Opening the library, for the lock
            // bit here and the analysis path below, derives its key again:
            // about 155 ms each, two per track, so a stick of 20,000 tracks
            // held the library, and every analysis and sync, for well over
            // an hour (#284).
            if editor.is_locked(id) || state.read_db(|db| crate::grid::locked_in(db, id)).map_err(write_error)? { report.skipped += 1; continue; }
            if analysis.is_empty() { report.skipped += 1; continue; }
            let source_path = root.join(analysis.trim_start_matches('/'));
            match std::fs::metadata(&source_path) {
                Ok(_) => {},
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    report.skipped += 1;
                    report.warnings.push(format!("Track {id}: USB analysis file is missing ({analysis}); cues and grid were left unchanged."));
                    continue;
                }
                Err(e) => return Err(err(e)),
            }
            let source = within(root, analysis)?;
            let source_dat = read_import_anlz(&source, "USB", "DAT", id)?;
            // DAT cue sections are copied to the library even when EXT is the
            // authoritative cue source. Validate their framing as well.
            source_dat.strict_cue_entries().map_err(|e| err(format!("USB DAT for track {id}: {e}")))?;
            // `source` is canonical and `root` need not be (a symlinked mount),
            // so the `.EXT` is found from the relative path, not by stripping.
            let ext_relative = Path::new(analysis).with_extension("EXT");
            let has_source_ext = source.with_extension("EXT").exists();
            let source_cues = if has_source_ext {
                read_import_anlz(&within(root, &ext_relative.to_string_lossy())?, "USB", "EXT", id)?
            } else { source_dat.clone() };
            let grid_only = source_cues.section(b"PCO2").is_none();
            if grid_only {
                if has_source_ext { report.skipped += 1; continue; }
                if !std::fs::symlink_metadata(&source_path).map_err(err)?.file_type().is_file() {
                    return Err(err("Legacy grid import requires a plain DAT file"));
                }
                // An unreadable, linked, mixed-case or ambiguous companion
                // cannot authorize a fallback to the legacy projection.
                match crate::device_library::browser_ext_sibling(&source) {
                    crate::device_library::BrowserExt::Missing => {},
                    crate::device_library::BrowserExt::Present(_) => { report.skipped += 1; continue; },
                    crate::device_library::BrowserExt::Refused => return Err(err(format!("USB EXT for track {id}: companion is unavailable or ambiguous"))),
                }
            }
            let source_grid = strict_import_grid(&source_dat)?;
            if grid_only && source_grid.is_none() { report.skipped += 1; continue; }
            let entries = if grid_only { None } else { Some(source_cues.strict_cue_entries().map_err(|e| {
                let extension = if has_source_ext { "EXT" } else { "DAT" };
                err(format!("USB {extension} for track {id}: {e}"))
            })?) };
            if entries.iter().flatten().any(|c| c.hot_cue > 16 || !matches!(c.kind, 1 | 2) || (c.kind == 2 && c.loop_time_ms <= c.time_ms)) {
                let extension = if has_source_ext { "EXT" } else { "DAT" };
                return Err(err(format!("USB {extension} for track {id}: Invalid USB cue data")));
            }
            let beats = source_dat.beat_grid().unwrap_or_default();
            let previous_bpm = state.read_db(|db| Ok(db.connection().query_row("SELECT COALESCE(BPM,0) FROM djmdContent WHERE ID=?1 AND rb_local_deleted=0", [id], |r| r.get::<_,u32>(0))?)).map_err(write_error)?;
            let bpm = beats.first().map_or(previous_bpm, |b| u32::from(b.tempo_x100));
            // The writer preserves the row BPM for raw zero. Compare that
            // effective change while keeping the raw value for the writer.
            let bpm_changed = bpm > 0 && bpm != previous_bpm;
            let relative = state.read_db(|db| rbl_db::write::analysis_data_path(db.connection(), id)).map_err(write_error)?;
            let target = rbl_anlz::resolve(&location.share_root, &relative);
            if grid_only && !target.with_extension("DAT").exists() { report.skipped += 1; continue; }
            let mut files = Vec::new();
            let mut identities = Vec::new();
            // A stick that carries the cues and grid the library already has
            // is not a change: rewriting it would bump the track's analysis,
            // drop its grid undo history and reload the whole library for
            // nothing, and the next sync would copy it all back (#134).
            let mut differs = false;
            let mut grid_changed = false;
            let mut grid_order_changed = false;
            let mut cues_changed = false;
            for extension in ["DAT", "EXT"] {
                let target = target.with_extension(extension);
                if !target.exists() { continue; }
                let (mut dest, identity) = read_import_anlz_identified(&target, "local library", extension, id)?;
                let src = if extension == "DAT" { &source_dat } else { &source_cues };
                if !grid_only {
                    let changed = !dest.sections.iter().filter(|s| s.is_cue_list()).eq(src.sections.iter().filter(|s| s.is_cue_list()));
                    cues_changed |= changed;
                    differs |= changed;
                    if changed {
                        dest.sections.retain(|s| !s.is_cue_list());
                        dest.sections.extend(src.sections.iter().filter(|s| s.is_cue_list()).cloned());
                    }
                }
                if extension == "DAT" && !beats.is_empty() {
                    grid_changed = !dest.sections.iter().filter(|s| s.as_beat_grid().is_some()).eq(source_dat.section(b"PQTZ"));
                    differs |= grid_changed;
                }
                if extension == "DAT" && !beats.is_empty() {
                    let previous_slot = dest.sections.iter().position(|section| section.tag.0 == *b"PQTZ");
                    dest.sections.retain(|s| s.tag.0 != *b"PQTZ");
                    if let Some(grid) = source_grid {
                        // Stock DAT readers consume PQTZ before waveform and
                        // cue sections. Keep its raw bytes, including when
                        // repairing files reordered by an earlier import.
                        let slot = dest.sections.iter().take_while(|section| matches!(&section.tag.0, b"PPTH" | b"PVBR")).count();
                        grid_order_changed = !grid_changed && previous_slot != Some(slot);
                        dest.sections.insert(slot, grid.clone());
                    }
                    differs |= grid_order_changed;
                }
                let bytes = if extension == "EXT" && grid_only {
                    if !grid_changed { continue; }
                    let Some(cleared) = dest.with_extended_grid_cleared() else { continue; };
                    cleared
                } else if extension == "EXT" && !(grid_order_changed && !cues_changed && !bpm_changed) {
                    dest.with_extended_grid_cleared().unwrap_or_else(|| dest.to_bytes())
                } else { dest.to_bytes() };
                identities.push(identity);
                files.push((target, bytes));
            }
            if files.is_empty() { report.skipped += 1; continue; }
            let identities = identities.into_iter().collect::<Option<Vec<_>>>();
            if !grid_only {
                match &identities {
                    Some(keys) => full_paths.extend(keys.iter().copied()),
                    None => unknown_full_identity = true,
                }
            }
            if grid_only {
                grid_only_tracks += 1;
                let Some(keys) = &identities else {
                    unavailable_grid_identity += 1;
                    report.skipped += 1;
                    continue;
                };
                for (key, (_, bytes)) in keys.iter().zip(&files) {
                    let hash = blake3::hash(bytes);
                    if grid_images.insert(*key, hash).is_some_and(|old| old != hash) {
                        ambiguous_grids.insert(*key);
                    }
                }
            }
            let identities = identities.unwrap_or_default();
            let cues_match = match &entries { Some(entries) => database_cues_match(state, id, entries)?, None => true };
            if !differs && !bpm_changed && cues_match {
                if grid_only { grid_noops.push(identities); }
                report.unchanged += 1; continue;
            }
            // Keep only paths across tracks. The private temporary directory
            // holds validated output and cue input until every track passes.
            let slot = prepared.len();
            // A physical order repair does not replace accepted cue rows.
            let order_only = grid_order_changed && !grid_changed && !cues_changed && !bpm_changed && cues_match;
            let payload = if grid_only {
                ImportPayload::Grid { expected_bpm: previous_bpm, analysis_path: relative }
            } else if order_only {
                ImportPayload::Order { expected_bpm: previous_bpm, analysis_path: relative }
            } else {
                let cue_path = staging.path().join(format!("{slot}.cues"));
                let cue_bytes = source_cues.to_bytes();
                stage_budget_add(&mut staged_bytes, cue_bytes.len(), stage_limit)?;
                std::fs::write(&cue_path, cue_bytes).map_err(err)?;
                ImportPayload::Cues(cue_path)
            };
            for (_, bytes) in &files { stage_budget_add(&mut staged_bytes, bytes.len(), stage_limit)?; }
            let mut staged_files = Vec::new();
            for (index, (target, bytes)) in files.into_iter().enumerate() {
                let staged = staging.path().join(format!("{slot}.{index}"));
                std::fs::write(&staged, bytes).map_err(err)?;
                staged_files.push((target, staged));
            }
            prepared.push((id.clone(), payload, if (grid_only || order_only) && bpm == 0 { previous_bpm } else { bpm }, staged_files, identities));
        }
        // A complete cue import keeps its established authority. Never let
        // a grid-only image built from the old shared DAT undo its cue edit.
        // Conflicting grid-only images have no unique owner either.
        let blocked = |key: &rbl_core::durable::FileIdentity| unknown_full_identity || full_paths.contains(key) || ambiguous_grids.contains(key);
        let before = prepared.len();
        prepared.retain(|(_, payload, _, _, keys)| !matches!(payload, ImportPayload::Grid { .. }) || !keys.iter().any(&blocked));
        let rejected_noops = grid_noops.iter().filter(|paths| paths.iter().any(&blocked)).count();
        let rejected_grids = before - prepared.len() + rejected_noops;
        report.unchanged -= rejected_noops;
        report.skipped += rejected_grids;
        if rejected_grids > 0 {
            let reason = if unknown_full_identity { "complete cue-import file identity is unavailable" } else { "their local analysis files have conflicting import owners" };
            report.warnings.push(format!("Skipped {rejected_grids} DAT-only grid import(s) because {reason}. Complete cue imports were retained."));
        }
        if unavailable_grid_identity > 0 {
            report.warnings.push(format!("Skipped {unavailable_grid_identity} DAT-only grid import(s) because local file identity could not be established."));
        }
        if grid_only_tracks > 0 {
            report.warnings.push(format!("USB EXT is missing for {grid_only_tracks} track(s); their DAT beat grids were considered without importing legacy cue data."));
        }
        // All tracks passed preflight before the first write. Reload only a
        // bounded batch of staged output, sharing its database writer (#794).
        let mut pending: Vec<PendingCues> = Vec::new();
        let mut pending_bytes = 0usize;
        for (id, payload, bpm, staged_files, identities) in prepared {
            let payload = match payload {
                ImportPayload::Cues(cue_path) => ImportPayload::Cues(read_import_anlz(&cue_path, "staged USB", "cue data", &id)?
                    .strict_cue_entries().map_err(|e| err(format!("staged USB cue data for track {id}: {e}")))?),
                ImportPayload::Grid { expected_bpm, analysis_path } => ImportPayload::Grid { expected_bpm, analysis_path },
                ImportPayload::Order { expected_bpm, analysis_path } => ImportPayload::Order { expected_bpm, analysis_path },
            };
            let files = staged_files.into_iter().map(|(target, staged)| {
                std::fs::read(staged).map(|bytes| (target, bytes)).map_err(err)
            }).collect::<AppResult<Vec<_>>>()?;
            let bytes = files.iter().map(|(_, bytes)| bytes.len()).sum::<usize>();
            let item = PendingCues { id, payload, bpm, files, identities };
            // Keep the captured preflight group after an atomic replacement
            // changes its inode. Full/full retains its established path rule.
            if pending_bytes.saturating_add(bytes) > CUE_BATCH_BYTES || pending_overlaps(&pending, &item) {
                apply_cue_batch(state, editor, &location, std::mem::take(&mut pending), &mut report)?;
                pending_bytes = 0;
            }
            pending_bytes += bytes;
            pending.push(item);
            if pending.len() >= CUE_BATCH_TRACKS || pending_bytes >= CUE_BATCH_BYTES {
                apply_cue_batch(state, editor, &location, std::mem::take(&mut pending), &mut report)?;
                pending_bytes = 0;
            }
        }
        apply_cue_batch(state, editor, &location, pending, &mut report)?;
        drop(marker);
    }
    if history {
        let snapshot = rbl_export::snapshot::Snapshot::read(root)
            .map_err(|e| err(format!("Could not read play history: {e}")))?;
        // Resolve one session at a time; only an eligible nonempty session
        // opens a writer. Each import still owns its existing transaction.
        let mut sessions = snapshot.history.iter().filter(|h| !h.folder).filter_map(|session| {
            let matched: Vec<String> = session.tracks.iter().filter_map(|id| tracks.get(id).map(|t| t.0.clone())).collect();
            if matched.len() != session.tracks.len() {
                report.skipped += session.tracks.len() - matched.len();
                report.warnings.push(format!("History '{}' contains tracks that could not be matched to this library; it was left on the USB.", session.name));
                return None;
            }
            if matched.is_empty() { return None; }
            Some((session, matched))
        });
        if let Some(first) = sessions.next() {
            let imported = state.write(|w| {
                let mut imported = 0;
                for (session, matched) in std::iter::once(first).chain(sessions) {
                    // A session keeps one identity as more tracks are appended.
                    // Preserve volume-derived identity and per-session prefix checks.
                    let key = format!("{}:{}:{}", rbl_devices::volume_id(root), session.id, session.name);
                    let hash = rbl_export::manifest::hash(key.as_bytes());
                    let uuid = format!("00000000-0000-4000-8000-{:012x}", hash & 0xffff_ffff_ffff);
                    imported += w.import_usb_history(&format!("{} (USB {:06x})", session.name, hash & 0x00ff_ffff), &uuid, &matched)?;
                }
                Ok(imported)
            }).map_err(write_error)?;
            report.histories += imported;
        }
    }
    if settings {
        let destination = settings_stash(state.backup_dir());
        for name in ["MYSETTING.DAT", "MYSETTING2.DAT", "DJMMYSETTING.DAT"] {
            let source = export.join(name);
            if !source.exists() { continue; }
            let bytes = std::fs::read(within(root, &source.strip_prefix(root).map_err(err)?.to_string_lossy())?).map_err(err)?;
            validate_settings(name, &bytes)
                .map_err(|e| err(format!("Could not import {name}: {e}")))?;
            std::fs::create_dir_all(&destination).map_err(err)?;
            crate::durable::write(&destination.join(name), &bytes).map_err(err)?;
            report.settings += 1;
        }
    }
    Ok(report)
}

/// Most tracks whose cues one writer applies, and the most analysis-file
/// bytes held for them, before the batch is written.
const CUE_BATCH_TRACKS: usize = 32;
const CUE_BATCH_BYTES: usize = 32 << 20;

/// A validated cue or grid change and its analysis files, ready to apply.
enum ImportPayload<C> {
    Cues(C),
    Grid { expected_bpm: u32, analysis_path: String },
    // A full-cue source with matching accepted cues; keep full-source priority
    // while journaling only its physical DAT repair and analysis revision.
    Order { expected_bpm: u32, analysis_path: String },
}

struct PendingCues {
    id: String,
    payload: ImportPayload<Vec<rbl_anlz::CueEntry>>,
    bpm: u32,
    files: Vec<(std::path::PathBuf, Vec<u8>)>,
    identities: Vec<rbl_core::durable::FileIdentity>,
}

fn pending_overlaps(pending: &[PendingCues], incoming: &PendingCues) -> bool {
    pending.iter().any(|previous| {
        previous.files.iter().any(|(path, _)| incoming.files.iter().any(|(other, _)| other == path)) ||
        ((!matches!(previous.payload, ImportPayload::Cues(_)) || !matches!(incoming.payload, ImportPayload::Cues(_))) &&
            previous.identities.iter().any(|key| incoming.identities.contains(key)))
    })
}

/// Applies a batch of planned imports: every track's journal and files first,
/// then all the library edits on one writer, then the journals are committed.
/// The caller holds the edit gate and analysis-write lock, as `import` does,
/// so opening the writer cannot recover these not-yet-committed journals.
/// Each track keeps its own journal and its own database transaction, so a
/// failure part-way leaves each track either fully imported or put back, as
/// `file_journal::recover` decides from its row.
///
/// Every track is planned before any is published, so two tracks sharing one
/// analysis file are both planned against the disk as it was before the batch:
/// the later one can be counted unchanged and the earlier one's version stays.
fn apply_cue_batch(
    state: &AppState, editor: &crate::grid::GridEditor, location: &rbl_db::LibraryLocation,
    batch: Vec<PendingCues>, report: &mut ImportReport,
) -> AppResult<()> {
    if batch.is_empty() { return Ok(()); }
    let mut journals = Vec::with_capacity(batch.len());
    for item in &batch {
        let prepared = state.read_db(|db| Ok(match &item.payload {
            ImportPayload::Cues(_) => crate::file_journal::FileJournal::prepare_cue_import_with(db, state.backup_dir(), location, &item.id, item.bpm, &item.files),
            ImportPayload::Grid { analysis_path, .. } | ImportPayload::Order { analysis_path, .. } => crate::file_journal::FileJournal::prepare_grid_with(db, state.backup_dir(), location, &item.id, item.bpm, analysis_path, &item.files),
        }))
            .map_err(write_error)
            .and_then(|prepared| prepared);
        match prepared {
            Ok(journal) => journals.push(journal),
            Err(e) => { for journal in &journals { journal.rollback()?; } return Err(e); }
        }
    }
    for journal in &journals {
        if let Err(e) = journal.publish() { for journal in &journals { journal.rollback()?; } return Err(e); }
    }
    let written = state.write(|w| {
        for item in &batch {
            match &item.payload {
                ImportPayload::Cues(entries) => w.import_usb_cues(&item.id, entries, item.bpm)?,
                ImportPayload::Grid { expected_bpm, analysis_path } | ImportPayload::Order { expected_bpm, analysis_path } => w.save_grid_revision_if_current(&item.id, item.bpm, *expected_bpm, analysis_path)?,
            }
        }
        Ok(())
    });
    if let Err(e) = written {
        // Rows already edited keep their new files; the rest are put back.
        // Read the persisted USNs rather than guessing from the error: a
        // database call may report failure after its commit reached disk.
        let changed = state.read_db(|db| Ok(journals.iter().map(|journal| journal.row_changed(db)).collect::<AppResult<Vec<_>>>()))
            .map_err(write_error).and_then(|changed| changed)
            .unwrap_or_else(|error| {
                tracing::warn!(%error, "could not inspect failed cue import rows; discarding batch grid history");
                vec![true; batch.len()]
            });
        let reconciled = journals.pop().map_or(Ok(()), |journal| journal.reconcile(location));
        for (item, changed) in batch.iter().zip(changed) {
            if changed { editor.forget_history(&item.id); }
        }
        reconciled?;
        return Err(write_error(e));
    }
    for (journal, item) in journals.into_iter().zip(batch) {
        journal.commit()?;
        editor.forget_history(&item.id);
        report.changed.push(item.id);
        report.tracks += 1;
    }
    Ok(())
}

/// Drops the stick's tracks whose library id names no live `djmdContent`
/// row, and says how many went.
///
/// A stick keeps naming a track by `masterContentId` after the track has
/// left the library: removed from the collection, or the stick written from
/// another copy of the same library (the same `masterDbId`). Such a track is
/// treated as one from another library: its cues are not imported, and a
/// history that plays it stays on the stick with a warning. Trusting the id
/// failed the whole import part-way, with "Query returned no rows" for cues
/// and "USB history contains an unknown track" for history, and so every
/// sync that imports history first (#138).
fn retain_live(state: &AppState, tracks: &mut HashMap<u32, (String, String)>) -> AppResult<usize> {
    if tracks.is_empty() { return Ok(0); }
    let before = tracks.len();
    let ids: Vec<String> = tracks.values().map(|(id, _)| id.clone()).collect();
    let live: HashSet<String> = state.read_db(|db| {
        let mut q = db.connection().prepare("SELECT 1 FROM djmdContent WHERE ID=?1 AND rb_local_deleted=0")?;
        let mut live = HashSet::new();
        for id in ids {
            if q.exists([&id])? { live.insert(id); }
        }
        Ok(live)
    }).map_err(write_error)?;
    tracks.retain(|_, (id, _)| live.contains(id));
    Ok(before - tracks.len())
}

/// Whether the library's cue rows for `id` are the ones `import_usb_cues`
/// would write for `entries`. Compared as a set: the order rows were
/// inserted in carries no meaning.
fn database_cues_match(state: &AppState, id: &str, entries: &[rbl_anlz::CueEntry]) -> AppResult<bool> {
    // A memory cue's colour is its `Color`, 0-7; any other value is none,
    // as rekordbox reads it, so 255 and -1 compare as one.
    type Row = (i64, Option<i64>, i64, Option<i64>, i64, String);
    let mut wanted: Vec<Row> = entries.iter().map(|cue| {
        let end = (cue.kind == 2).then_some(i64::from(cue.loop_time_ms));
        let memory = Some(cue.library_color()).filter(|c| (0..=7).contains(c) && cue.hot_cue == 0);
        (i64::from(cue.time_ms), end, i64::from(cue.library_kind()), memory, i64::from(cue.library_color_table_index()), cue.comment.clone().unwrap_or_default())
    }).collect();
    let mut stored: Vec<Row> = state.read_db(|db| {
        let mut q = db.connection().prepare(
            "SELECT COALESCE(InMsec,0), OutMsec, COALESCE(Kind,0),
                    CASE WHEN Kind IN (0,4) AND Color BETWEEN 0 AND 7 THEN Color END,
                    COALESCE(ColorTableIndex,0), COALESCE(Comment,'')
             FROM djmdCue WHERE ContentID=?1 AND rb_local_deleted=0",
        )?;
        let rows = q.query_map([id], |r| Ok((r.get(0)?, r.get::<_, Option<i64>>(1)?.filter(|end| *end >= 0), r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?
            .collect::<Result<Vec<Row>, _>>()?;
        Ok(rows)
    }).map_err(write_error)?;
    wanted.sort();
    stored.sort();
    Ok(wanted == stored)
}

fn validate_settings(name: &str, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() < 108 || bytes.len() > 4096 { return Err(format!("Invalid {name}")); }
    let size = u32::from_le_bytes(bytes[100..104].try_into().map_err(|e: std::array::TryFromSliceError| e.to_string())?) as usize;
    if size + 108 != bytes.len() || bytes[0..4] != [96, 0, 0, 0] { return Err(format!("Invalid {name} length")); }
    let end = 104 + size;
    let mut crc = 0u16;
    for byte in &bytes[if name == "DJMMYSETTING.DAT" { 0 } else { 104 }..end] {
        crc ^= u16::from(*byte) << 8;
        for _ in 0..8 { crc = if crc & 0x8000 == 0 { crc << 1 } else { (crc << 1) ^ 0x1021 }; }
    }
    let stored = u16::from_le_bytes([bytes[end], bytes[end+1]]);
    if stored != crc { return Err(format!("Invalid {name} checksum")); }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::io::Seek;
    #[test]
    fn oversized_analysis_is_rejected_before_reading() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ANLZ0000.DAT");
        std::fs::File::create(&file).unwrap().set_len(MAX_IMPORT_ANLZ_BYTES + 1).unwrap();
        let error = read_import_anlz(&file, "USB", "DAT", "123").unwrap_err().to_string();
        assert!(error.contains("USB DAT for track 123"), "{error}");
        assert!(error.contains("64 MiB"), "{error}");
    }
    #[cfg(unix)]
    #[test]
    fn open_handle_is_bounded_even_if_path_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ANLZ0000.DAT");
        let mut valid = rbl_anlz::write::AnlzBuilder::new();
        valid.path("/original.mp3");
        std::fs::write(&path, valid.finish()).unwrap();
        let handle = std::fs::File::open(&path).unwrap();
        // Renaming keeps the original handle on its inode while the path now
        // names an oversized replacement (on platforms allowing the rename).
        let original = dir.path().join("original.DAT");
        std::fs::rename(&path, &original).unwrap();
        std::fs::File::create(&path).unwrap().set_len(MAX_IMPORT_ANLZ_BYTES + 1).unwrap();
        assert_eq!(read_import_anlz_file(handle, "USB", "DAT", "123").unwrap().path().as_deref(), Some("/original.mp3"));
        assert!(read_import_anlz(&path, "USB", "DAT", "123").unwrap_err().to_string().contains("64 MiB"));
    }
    #[test]
    fn stage_recovery_only_removes_unlocked_owned_directories() {
        let dir = tempfile::tempdir().unwrap();
        let backup = dir.path();
        let stale = backup.join(format!("{STAGE_PREFIX}stale"));
        let active = backup.join(format!("{STAGE_PREFIX}active"));
        let lookalike = backup.join(format!("{STAGE_PREFIX}unmarked"));
        for path in [&stale, &active, &lookalike] { std::fs::create_dir(path).unwrap(); }
        for path in [&stale, &active] {
            std::fs::write(path.join(STAGE_MARKER), "rbxport usb import stage v1").unwrap();
        }
        let live = std::fs::OpenOptions::new().read(true).write(true).open(active.join(STAGE_MARKER)).unwrap();
        fs2::FileExt::lock_exclusive(&live).unwrap();
        recover_stale_stages(backup).unwrap();
        assert!(!stale.exists());
        assert!(active.exists());
        assert!(lookalike.exists());
        drop(live);
        recover_stale_stages(backup).unwrap();
        assert!(!active.exists());
    }
    #[test]
    fn marker_content_is_read_through_the_locking_handle() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(STAGE_MARKER);
        std::fs::write(&path, STAGE_MARKER_CONTENT).unwrap();
        let mut held = std::fs::OpenOptions::new().read(true).write(true).open(&path).unwrap();
        fs2::FileExt::lock_exclusive(&held).unwrap();
        // Position must not matter: the helper rewinds before reading.
        held.seek(std::io::SeekFrom::End(0)).unwrap();
        assert!(marker_has_owner_content(&mut held).unwrap());
        for bad in ["", "rbxport usb import stage v2", "rbxport usb import stage v1 trailing"] {
            held.set_len(0).unwrap();
            held.rewind().unwrap();
            std::io::Write::write_all(&mut held, bad.as_bytes()).unwrap();
            assert!(!marker_has_owner_content(&mut held).unwrap(), "{bad:?}");
        }
    }
    #[test]
    fn aggregate_stage_limit_prevents_any_track_commit_and_cleans_up() {
        let mut used = 0;
        stage_budget_add(&mut used, 400, 1000).unwrap();
        stage_budget_add(&mut used, 400, 1000).unwrap();
        assert!(stage_budget_add(&mut used, 201, 1000).is_err());
        assert_eq!(used, 800);
        let dir = tempfile::tempdir().unwrap();
        let (state, location, usb, _) = library_and_stick(dir.path());
        let ids = [rbl_db::fixture::track_id(0), rbl_db::fixture::track_id(1)];
        let mut targets = Vec::new();
        for id in &ids {
            let relative = state.write(|w| w.analysis_data_path_for(id)).unwrap();
            state.write(|w| w.register_analysis(id, &rbl_db::write::AnalysisRegistration {
                bpm_x100: 12000, key: None, analysis_data_path: &relative,
            })).unwrap();
            let target = rbl_anlz::resolve(&location.share_root, &relative);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            let mut dat = rbl_anlz::write::AnlzBuilder::new();
            dat.path("/original.mp3").cue_lists(false);
            std::fs::write(&target, dat.finish()).unwrap();
            let mut ext = rbl_anlz::write::AnlzBuilder::new();
            ext.path("/original.mp3").cue_lists(true);
            std::fs::write(target.with_extension("EXT"), ext.finish()).unwrap();
            targets.push(target);
        }
        let before = targets.iter().map(|target| (std::fs::read(target).unwrap(), std::fs::read(target.with_extension("EXT")).unwrap())).collect::<Vec<_>>();
        let snapshot = || ids.iter().map(|id| state.read_db(|db| {
            let track = db.connection().query_row("SELECT BPM, AnalysisUpdated, rb_local_usn FROM djmdContent WHERE ID=?1", [id],
                |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<i64>>(2)?)))?;
            let mut q = db.connection().prepare("SELECT InMsec, OutMsec, Kind, ColorTableIndex, Comment FROM djmdCue WHERE ContentID=?1 AND rb_local_deleted=0 ORDER BY InMsec")?;
            let cues = q.query_map([id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, Option<String>>(4)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok((track, cues))
        }).unwrap()).collect::<Vec<_>>();
        let before_db = snapshot();
        // The first track's complete staged payload is the boundary: it must
        // fit, while the next track must cross the aggregate limit.
        let first_stick = usb.join("PIONEER/USBANLZ/P000/00000001/ANLZ0000.DAT");
        let source_dat = rbl_anlz::Anlz::read(&first_stick).unwrap();
        let source_ext = rbl_anlz::Anlz::read(&first_stick.with_extension("EXT")).unwrap();
        let mut first_stage_bytes = source_ext.to_bytes().len();
        for extension in ["DAT", "EXT"] {
            let mut dest = rbl_anlz::Anlz::read(&targets[0].with_extension(extension)).unwrap();
            let src = if extension == "DAT" { &source_dat } else { &source_ext };
            dest.sections.retain(|s| !s.is_cue_list());
            dest.sections.extend(src.sections.iter().filter(|s| s.is_cue_list()).cloned());
            if extension == "DAT" {
                dest.sections.retain(|s| s.as_beat_grid().is_none());
                if let Some(grid) = source_dat.section(b"PQTZ") { dest.sections.push(grid.clone()); }
            }
            first_stage_bytes += if extension == "EXT" {
                dest.with_extended_grid_cleared().unwrap_or_else(|| dest.to_bytes()).len()
            } else { dest.to_bytes().len() };
        }
        assert!(first_stage_bytes > 1);
        // A fresh fixture with only the first track eligible must import at
        // this exact limit. Otherwise the two-track error below could come
        // from rejecting the first track before the second is considered.
        let single_dir = tempfile::tempdir().unwrap();
        let (single_state, single_location, single_usb, _) = library_and_stick(single_dir.path());
        let first_id = &ids[0];
        let relative = single_state.write(|w| w.analysis_data_path_for(first_id)).unwrap();
        single_state.write(|w| w.register_analysis(first_id, &rbl_db::write::AnalysisRegistration {
            bpm_x100: 12000, key: None, analysis_data_path: &relative,
        })).unwrap();
        let single_target = rbl_anlz::resolve(&single_location.share_root, &relative);
        std::fs::create_dir_all(single_target.parent().unwrap()).unwrap();
        let mut dat = rbl_anlz::write::AnlzBuilder::new();
        dat.path("/original.mp3").cue_lists(false);
        std::fs::write(&single_target, dat.finish()).unwrap();
        let mut ext = rbl_anlz::write::AnlzBuilder::new();
        ext.path("/original.mp3").cue_lists(true);
        std::fs::write(single_target.with_extension("EXT"), ext.finish()).unwrap();
        let single_editor = crate::grid::GridEditor::at(single_state.backup_dir());
        let admitted = import_with_stage_budget(&single_state, &single_editor, &single_usb, true, false, false, false, first_stage_bytes as u64).unwrap();
        assert_eq!(admitted.tracks, 1, "the first staged track must fit the exact aggregate limit");
        assert_eq!(admitted.changed, vec![first_id.clone()]);

        let editor = crate::grid::GridEditor::at(state.backup_dir());
        let error = import_with_stage_budget(&state, &editor, &usb, true, false, false, false, first_stage_bytes as u64).unwrap_err().to_string();
        assert!(error.contains("aggregate limit"), "{error}");
        assert_eq!(targets.iter().map(|target| (std::fs::read(target).unwrap(), std::fs::read(target.with_extension("EXT")).unwrap())).collect::<Vec<_>>(), before);
        assert_eq!(snapshot(), before_db, "aggregate preflight changed local cues or track metadata");
        assert!(!std::fs::read_dir(state.backup_dir()).unwrap().any(|entry| entry.unwrap().file_name().to_string_lossy().starts_with(STAGE_PREFIX)));
    }
    #[test]
    fn malformed_later_cue_frame_preserves_both_tracks() {
        for damage in ["marker", "short length", "overrun", "trailing", "count 1/0", "count 2/1", "DAT count 2/1"] {
            let dir = tempfile::tempdir().unwrap();
            let (state, location, usb, _) = library_and_stick(dir.path());
            let ids = [rbl_db::fixture::track_id(0), rbl_db::fixture::track_id(1)];
            let mut targets = Vec::new();
            for (index, id) in ids.iter().enumerate() {
                let relative = state.write(|w| w.analysis_data_path_for(id)).unwrap();
                state.write(|w| w.register_analysis(id, &rbl_db::write::AnalysisRegistration {
                    bpm_x100: 12000, key: None, analysis_data_path: &relative,
                })).unwrap();
                let target = rbl_anlz::resolve(&location.share_root, &relative);
                std::fs::create_dir_all(target.parent().unwrap()).unwrap();
                let mut dat = rbl_anlz::write::AnlzBuilder::new();
                dat.path("/original.mp3").cue_lists(false);
                std::fs::write(&target, dat.finish()).unwrap();
                let mut ext = rbl_anlz::write::AnlzBuilder::new();
                ext.path("/original.mp3").cue_lists(true);
                std::fs::write(target.with_extension("EXT"), ext.finish()).unwrap();
                let stick = usb.join(format!("PIONEER/USBANLZ/P000/{:08}/", index + 1));
                write_stick(&stick, &[stick_cue(1, 1000, None, 22, "original")]);
                targets.push(target);
            }
            let editor = crate::grid::GridEditor::at(state.backup_dir());
            assert_eq!(import(&state, &editor, &usb, true, false, false, false).unwrap().tracks, 2);
            for index in 0..2 {
                let stick = usb.join(format!("PIONEER/USBANLZ/P000/{:08}/", index + 1));
                write_stick(&stick, &[stick_cue(1, 2000, None, 22, "changed")]);
            }
            let bad = usb.join("PIONEER/USBANLZ/P000/00000002/ANLZ0000.DAT")
                .with_extension(if damage == "DAT count 2/1" { "DAT" } else { "EXT" });
            let mut anlz = rbl_anlz::Anlz::read(&bad).unwrap();
            if damage == "DAT count 2/1" {
                let ext = rbl_anlz::Anlz::read(&bad.with_extension("EXT")).unwrap();
                anlz.sections.extend(ext.sections.into_iter().filter(|s| s.tag.0 == *b"PCO2"));
            }
            let cue = anlz.sections.iter_mut().find(|s| s.tag.0 == *b"PCO2" && !s.payload.is_empty()).unwrap();
            match damage {
                "marker" => cue.payload[..4].copy_from_slice(b"BAD!"),
                "short length" => cue.payload[8..12].copy_from_slice(&1_u32.to_be_bytes()),
                "overrun" => cue.payload[8..12].copy_from_slice(&u32::MAX.to_be_bytes()),
                "trailing" => cue.payload.push(0),
                "count 1/0" => cue.payload.clear(),
                "count 2/1" | "DAT count 2/1" => cue.header[4..6].copy_from_slice(&2_u16.to_be_bytes()),
                _ => unreachable!(),
            }
            std::fs::write(&bad, anlz.to_bytes()).unwrap();
            let snapshot = || ids.iter().zip(&targets).map(|(id, target)| {
                let db = state.read_db(|db| {
                    let track = db.connection().query_row("SELECT BPM, AnalysisUpdated, rb_local_usn FROM djmdContent WHERE ID=?1", [id],
                        |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<i64>>(2)?)))?;
                    let mut q = db.connection().prepare("SELECT InMsec, OutMsec, Kind, ColorTableIndex, Comment FROM djmdCue WHERE ContentID=?1 AND rb_local_deleted=0 ORDER BY InMsec")?;
                    let cues = q.query_map([id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, Option<String>>(4)?)))?
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok((track, cues))
                }).unwrap();
                (db, std::fs::read(target).unwrap(), std::fs::read(target.with_extension("EXT")).unwrap())
            }).collect::<Vec<_>>();
            let before = snapshot();
            assert!(before.iter().all(|(db, _, _)| !db.1.is_empty()), "fixture must hold real local cues");
            let error = import(&state, &editor, &usb, true, false, false, false).unwrap_err().to_string();
            let extension = if damage == "DAT count 2/1" { "DAT" } else { "EXT" };
            assert!(error.contains(&format!("USB {extension} for track {}", ids[1])), "{error}");
            assert_eq!(snapshot(), before, "{damage} changed local state");
            assert_eq!(std::fs::read(&bad).unwrap(), anlz.to_bytes());
        }
    }
    #[test]
    fn malformed_anlz_preflight_leaves_all_cues_and_grids_untouched() {
        // The valid lower-ID track precedes the malformed one in the sorted
        // write order. Without preflight, it would commit first.
        for (stage, extension) in [
            ("USB", "DAT"), ("USB", "EXT"),
            ("local library", "DAT"), ("local library", "EXT"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let (state, location, usb, _) = library_and_stick(dir.path());
            let ids = [rbl_db::fixture::track_id(0), rbl_db::fixture::track_id(1)];
            let mut targets = Vec::new();
            for id in &ids {
                let relative = state.write(|w| w.analysis_data_path_for(id)).unwrap();
                state.write(|w| w.register_analysis(id, &rbl_db::write::AnalysisRegistration {
                    bpm_x100: 12000, key: None, analysis_data_path: &relative,
                })).unwrap();
                let target = rbl_anlz::resolve(&location.share_root, &relative);
                std::fs::create_dir_all(target.parent().unwrap()).unwrap();
                let mut dat = rbl_anlz::write::AnlzBuilder::new();
                dat.path("/original.mp3").cue_lists(false);
                std::fs::write(&target, dat.finish()).unwrap();
                let mut ext = rbl_anlz::write::AnlzBuilder::new();
                ext.path("/original.mp3").cue_lists(true);
                std::fs::write(target.with_extension("EXT"), ext.finish()).unwrap();
                targets.push(target);
            }
            let bad = if stage == "USB" {
                usb.join("PIONEER/USBANLZ/P000/00000002/ANLZ0000.DAT").with_extension(extension)
            } else {
                targets[1].with_extension(extension)
            };
            std::fs::write(&bad, b"NOPE00000000").unwrap();
            let snapshot = || ids.iter().zip(&targets).map(|(id, target)| {
                let db = state.read_db(|db| {
                    let track = db.connection().query_row(
                        "SELECT BPM, AnalysisUpdated, rb_local_usn FROM djmdContent WHERE ID=?1", [id],
                        |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<i64>>(2)?)),
                    )?;
                    let cues = db.connection().query_row(
                        "SELECT COUNT(*) FROM djmdCue WHERE ContentID=?1 AND rb_local_deleted=0", [id],
                        |r| r.get::<_, i64>(0),
                    )?;
                    Ok((track, cues))
                }).unwrap();
                (db, std::fs::read(target).unwrap(), std::fs::read(target.with_extension("EXT")).unwrap())
            }).collect::<Vec<_>>();
            let before = snapshot();
            let editor = crate::grid::GridEditor::at(state.backup_dir());
            let error = import(&state, &editor, &usb, true, false, false, false).unwrap_err().to_string();
            assert!(error.contains(&format!("{stage} {extension} for track {}", ids[1])), "{error}");
            assert!(error.contains("PMAI"), "{error}");
            assert!(!error.contains(&dir.path().to_string_lossy().to_string()), "path leaked: {error}");
            assert_eq!(snapshot(), before, "{stage} {extension} changed another track");
            assert_eq!(std::fs::read(&bad).unwrap(), b"NOPE00000000", "malformed input was rewritten");
        }
    }
    #[test]
    fn parseable_invalid_cues_prevent_a_prior_track_from_committing() {
        for (invalid_field, extension) in [
            ("kind", "EXT"), ("hot cue", "EXT"), ("loop end", "EXT"),
            ("kind", "DAT"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let (state, location, usb, _) = library_and_stick(dir.path());
            let ids = [rbl_db::fixture::track_id(0), rbl_db::fixture::track_id(1)];
            let mut targets = Vec::new();
            for id in &ids {
                let relative = state.write(|w| w.analysis_data_path_for(id)).unwrap();
                state.write(|w| w.register_analysis(id, &rbl_db::write::AnalysisRegistration {
                    bpm_x100: 12000, key: None, analysis_data_path: &relative,
                })).unwrap();
                let target = rbl_anlz::resolve(&location.share_root, &relative);
                std::fs::create_dir_all(target.parent().unwrap()).unwrap();
                let mut dat = rbl_anlz::write::AnlzBuilder::new();
                dat.path("/original.mp3").cue_lists(false);
                std::fs::write(&target, dat.finish()).unwrap();
                let mut ext = rbl_anlz::write::AnlzBuilder::new();
                ext.path("/original.mp3").cue_lists(true);
                std::fs::write(target.with_extension("EXT"), ext.finish()).unwrap();
                targets.push(target);
            }
            let bad = usb.join("PIONEER/USBANLZ/P000/00000002/ANLZ0000.DAT").with_extension(extension);
            let mut anlz = rbl_anlz::Anlz::read(&bad).unwrap();
            if extension == "DAT" {
                let ext = bad.with_extension("EXT");
                let extended = rbl_anlz::Anlz::read(&ext).unwrap();
                anlz.sections.extend(extended.sections.into_iter().filter(|s| s.tag.0 == *b"PCO2"));
                std::fs::remove_file(ext).unwrap();
            }
            let cue = anlz.sections.iter_mut().find(|s| s.tag.0 == *b"PCO2").unwrap();
            // PCP2: slot at 12, kind at 16, start at 20, end at 24.
            match invalid_field {
                "kind" => cue.payload[16] = 0,
                "hot cue" => cue.payload[12..16].copy_from_slice(&17_u32.to_be_bytes()),
                "loop end" => {
                    cue.payload[16] = 2;
                    cue.payload[24..28].copy_from_slice(&0_u32.to_be_bytes());
                }
                _ => unreachable!(),
            }
            std::fs::write(&bad, anlz.to_bytes()).unwrap();
            let parsed = rbl_anlz::Anlz::read(&bad).unwrap().cue_entries();
            assert!(!parsed.is_empty(), "invalid cue must be parseable");
            assert!(parsed.iter().any(|c| c.hot_cue > 16 || !matches!(c.kind, 1 | 2) || (c.kind == 2 && c.loop_time_ms <= c.time_ms)));
            let snapshot = || ids.iter().zip(&targets).map(|(id, target)| {
                let db = state.read_db(|db| {
                    let row = db.connection().query_row(
                        "SELECT BPM, AnalysisUpdated, rb_local_usn FROM djmdContent WHERE ID=?1", [id],
                        |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<i64>>(2)?)),
                    )?;
                    let cues = db.connection().query_row(
                        "SELECT COUNT(*) FROM djmdCue WHERE ContentID=?1 AND rb_local_deleted=0", [id],
                        |r| r.get::<_, i64>(0),
                    )?;
                    Ok((row, cues))
                }).unwrap();
                (db, std::fs::read(target).unwrap(), std::fs::read(target.with_extension("EXT")).unwrap())
            }).collect::<Vec<_>>();
            let before = snapshot();
            let editor = crate::grid::GridEditor::at(state.backup_dir());
            let error = import(&state, &editor, &usb, true, false, false, false).unwrap_err().to_string();
            assert!(error.contains(&format!("USB {extension} for track {}", ids[1])), "{error}");
            assert!(error.contains("Invalid USB cue data"), "{error}");
            assert_eq!(snapshot(), before, "{invalid_field} in {extension} partially wrote cues or grid");
        }
    }
    #[test]
    fn many_changed_tracks_import_without_retaining_all_replacements() {
        let dir = tempfile::tempdir().unwrap();
        let (state, location, usb, tracks) = library_and_stick(dir.path());
        assert!(tracks >= 40);
        for index in 0..tracks {
            let id = rbl_db::fixture::track_id(index);
            let relative = state.write(|w| w.analysis_data_path_for(&id)).unwrap();
            state.write(|w| w.register_analysis(&id, &rbl_db::write::AnalysisRegistration {
                bpm_x100: 12000, key: None, analysis_data_path: &relative,
            })).unwrap();
            let target = rbl_anlz::resolve(&location.share_root, &relative);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            let mut dat = rbl_anlz::write::AnlzBuilder::new();
            dat.path("/original.mp3").waveform_preview(b"PWAV", &vec![0x55; 256 * 1024]).cue_lists(false);
            std::fs::write(&target, dat.finish()).unwrap();
            let mut ext = rbl_anlz::write::AnlzBuilder::new();
            ext.path("/original.mp3").cue_lists(true);
            std::fs::write(target.with_extension("EXT"), ext.finish()).unwrap();
        }
        let editor = crate::grid::GridEditor::at(state.backup_dir());
        let first = import(&state, &editor, &usb, true, false, false, false).unwrap();
        assert_eq!(first.tracks, tracks);
        let again = import(&state, &editor, &usb, true, false, false, false).unwrap();
        assert_eq!((again.tracks, again.unchanged), (0, tracks));
        assert!(!std::fs::read_dir(state.backup_dir()).unwrap().any(|entry| {
            entry.unwrap().file_name().to_string_lossy().starts_with("usb-import-")
        }), "successful imports clean up disk staging");
    }
    #[test]
    fn imports_only_cues_and_grid_preserving_local_path_and_waveform() {
        let dir = tempfile::tempdir().unwrap();
        let location = rbl_db::fixture::build(dir.path(), rbl_db::fixture::Shape::default()).unwrap();
        let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
        let (library, _) = rbl_index::load(&db).unwrap();
        let state = AppState::with_backups(dir.path().join("backups"));
        state.set_library(library, false, db.schema().db_version, 0, location.clone());
        let id = rbl_db::fixture::track_id(0);
        let source_path: String = db.connection().query_row("SELECT FolderPath FROM djmdContent WHERE ID=?1", [&id], |r| r.get(0)).unwrap();
        drop(db);
        let relative = state.write(|w| w.analysis_data_path_for(&id)).unwrap();
        state.write(|w| w.register_analysis(&id, &rbl_db::write::AnalysisRegistration { bpm_x100: 12000, key: None, analysis_data_path: &relative })).unwrap();
        let target = rbl_anlz::resolve(&location.share_root, &relative);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        let mut original = rbl_anlz::write::AnlzBuilder::new();
        original.path("/original.mp3").waveform_preview(b"PWAV", &[1,2,3]).beat_grid(&[rbl_anlz::Beat { beat_number: 1, tempo_x100: 12000, time_ms: 0 }]).cue_lists(true);
        std::fs::write(&target, original.finish()).unwrap();
        let usb = dir.path().join("usb");
        let anlz = "PIONEER/USBANLZ/test";
        std::fs::create_dir_all(usb.join(anlz)).unwrap();
        let mut changed = rbl_anlz::write::AnlzBuilder::new();
        changed.path("/usb.mp3").beat_grid(&[rbl_anlz::Beat { beat_number: 1, tempo_x100: 12800, time_ms: 250 }]).cue_lists(true);
        std::fs::write(usb.join(anlz).join("ANLZ0000.DAT"), changed.finish()).unwrap();
        rbl_export::Manifest { generation: None, db_id: 0, baseline: None, version: 1, written: String::new(), playlists: vec![], loose: vec![], tracks: vec![rbl_export::manifest::ManifestTrack { analysis_hashes: std::collections::BTreeMap::new(), analysis_extensions: vec!["DAT".into()], audio_hash: 0,
            export_id: 1, library_id: id.parse().unwrap(), source: source_path, audio: "audio.mp3".into(), anlz_dir: anlz.into(), size: 0, modified: 0, analysis: 0, artwork: String::new(), conversion: String::new(), conversion_source_hash: 0, in_place: false, preserved: Some(false), copy_stamp: None,
        }] }.save(&usb).unwrap();
        let editor = crate::grid::GridEditor::at(state.backup_dir());
        let report = import(&state, &editor, &usb, true, false, false, false).unwrap();
        assert_eq!(report.tracks, 1);
        let result = rbl_anlz::Anlz::read(&target).unwrap();
        assert_eq!(result.path().as_deref(), Some("/original.mp3"));
        assert_eq!(result.waveform(b"PWAV").unwrap().1, &[1,2,3]);
        assert_eq!(result.beat_grid().unwrap()[0].time_ms, 250);
        assert_eq!(result.beat_grid().unwrap()[0].tempo_x100, 12800);
        // The same stick again carries nothing new: no file, row or analysis
        // counter is touched, so nothing reloads and the next sync has nothing
        // to copy back (#134).
        let stamp = |state: &AppState| state.read_db(|db| Ok(db.connection().query_row(
            "SELECT AnalysisUpdated, rb_local_usn FROM djmdContent WHERE ID=?1", [&id],
            |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<i64>>(1)?)))?)).unwrap();
        let before = (stamp(&state), std::fs::read(&target).unwrap());
        let again = import(&state, &editor, &usb, true, false, false, false).unwrap();
        assert_eq!((again.tracks, again.unchanged, again.skipped), (0, 1, 0));
        assert_eq!(again.changed, Vec::<String>::new());
        assert_eq!((stamp(&state), std::fs::read(&target).unwrap()), before);
        editor.set_locked(&id, true).unwrap();
        assert_eq!(import(&state, &editor, &usb, true, false, false, false).unwrap().skipped, 1);
        editor.set_locked(&id, false).unwrap();
        // A USB database can still name an analysis file that has been
        // removed. Preserve the other USB bytes and the local library, then
        // make the same attempt twice: the missing track must not abort sync.
        let usb_dat = usb.join(anlz).join("ANLZ0000.DAT");
        std::fs::remove_file(&usb_dat).unwrap();
        let marker = usb.join("unrelated.bin");
        std::fs::write(&marker, b"keep this USB file").unwrap();
        let before = (stamp(&state), std::fs::read(&target).unwrap(), std::fs::read(&marker).unwrap());
        for _ in 0..2 {
            let missing = import(&state, &editor, &usb, true, false, false, false).unwrap();
            assert_eq!((missing.tracks, missing.skipped), (0, 1));
            assert_eq!(missing.warnings.len(), 1);
            assert!(missing.warnings[0].contains("analysis file is missing"));
            assert_eq!((stamp(&state), std::fs::read(&target).unwrap(), std::fs::read(&marker).unwrap()), before);
        }
        let mut manifest = rbl_export::Manifest::load(&usb).unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("ANLZ0000.DAT"), std::fs::read(&target).unwrap()).unwrap();
        manifest.tracks[0].anlz_dir = "../outside".into();
        manifest.save(&usb).unwrap();
        assert!(import(&state, &editor, &usb, true, false, false, false).is_err(), "an analysis path outside the USB is rejected");
        assert_eq!((stamp(&state), std::fs::read(&target).unwrap(), std::fs::read(&marker).unwrap()), before, "failed import leaves library and unrelated USB bytes intact");
        manifest.tracks[0].anlz_dir.clear();
        manifest.save(&usb).unwrap();
        // Identity remains available for history even without cue analysis.
        assert_eq!(import(&state, &editor, &usb, true, false, false, false).unwrap().skipped, 1);
    }

    trait HasEntries { fn exists_with_entries(&self) -> bool; }
    impl HasEntries for std::path::PathBuf {
        fn exists_with_entries(&self) -> bool { std::fs::read_dir(self).is_ok_and(|mut entries| entries.next().is_some()) }
    }

    /// A cue as the master database holds it (`Kind` 0 memory, 1-3 A-C,
    /// 5-17 D-P), for the stick's analysis files.
    fn stick_cue(kind: u8, time_ms: u32, loop_end: Option<u32>, color_code: u8, comment: &str) -> rbl_anlz::cues::ExportCue {
        rbl_anlz::cues::ExportCue { kind, time_ms, loop_time_ms: loop_end, color_code, comment: comment.into(), ..Default::default() }
    }

    /// Writes a stick's `.DAT` and `.EXT` the way an export lays them out:
    /// memory cues and A-C in `PCOB`, D-P and the full lists in `PCO2`, over
    /// one fixed grid.
    fn write_stick(dir: &Path, cues: &[rbl_anlz::cues::ExportCue]) {
        let grid = [rbl_anlz::Beat { beat_number: 1, tempo_x100: 12800, time_ms: 250 }];
        let mut dat = rbl_anlz::write::AnlzBuilder::new();
        dat.path("/usb.mp3").beat_grid(&grid);
        for section in rbl_anlz::cues::sections(cues, false) { dat.copy_section(&section); }
        std::fs::write(dir.join("ANLZ0000.DAT"), dat.finish()).unwrap();
        let mut ext = rbl_anlz::write::AnlzBuilder::new();
        ext.path("/usb.mp3");
        for section in rbl_anlz::cues::sections(cues, true) { ext.copy_section(&section); }
        std::fs::write(dir.join("ANLZ0000.EXT"), ext.finish()).unwrap();
    }

    /// The "unchanged" check on a track with real cues: every mapping from a
    /// stick entry to a `djmdCue` row (slot E and later shifted by one, a
    /// cue's missing end, the colour index, an empty comment) has to agree,
    /// or every real track would be rewritten again on each import (#134);
    /// and a stick or library that differs only in its cues must still be
    /// written.
    #[test]
    fn unchanged_check_compares_real_cues_both_ways() {
        let dir = tempfile::tempdir().unwrap();
        let location = rbl_db::fixture::build(dir.path(), rbl_db::fixture::Shape::default()).unwrap();
        let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
        let (library, _) = rbl_index::load(&db).unwrap();
        let state = AppState::with_backups(dir.path().join("backups"));
        state.set_library(library, false, db.schema().db_version, 0, location.clone());
        let id = rbl_db::fixture::track_id(0);
        let source_path: String = db.connection().query_row("SELECT FolderPath FROM djmdContent WHERE ID=?1", [&id], |r| r.get(0)).unwrap();
        drop(db);
        let relative = state.write(|w| w.analysis_data_path_for(&id)).unwrap();
        state.write(|w| w.register_analysis(&id, &rbl_db::write::AnalysisRegistration { bpm_x100: 12800, key: None, analysis_data_path: &relative })).unwrap();
        let target = rbl_anlz::resolve(&location.share_root, &relative);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        let mut dat = rbl_anlz::write::AnlzBuilder::new();
        dat.path("/original.mp3").waveform_preview(b"PWAV", &[1, 2, 3]).beat_grid(&[rbl_anlz::Beat { beat_number: 1, tempo_x100: 12800, time_ms: 250 }]).cue_lists(false);
        std::fs::write(&target, dat.finish()).unwrap();
        let mut ext = rbl_anlz::write::AnlzBuilder::new();
        ext.path("/original.mp3").waveform_preview(b"PWV3", &[4, 5, 6]).cue_lists(true);
        std::fs::write(target.with_extension("EXT"), ext.finish()).unwrap();

        let usb = dir.path().join("usb");
        let anlz = "PIONEER/USBANLZ/test";
        std::fs::create_dir_all(usb.join(anlz)).unwrap();
        let mut cues = vec![
            stick_cue(0, 1000, None, 0, ""),               // a memory cue
            stick_cue(1, 2000, None, 22, "Drop"),          // hot cue A, coloured, with a comment
            stick_cue(6, 3000, None, 1, ""),               // hot cue E: Kind 6, slot 5
            stick_cue(0, 4000, Some(8000), 0, ""),         // a memory loop
        ];
        write_stick(&usb.join(anlz), &cues);
        rbl_export::Manifest { generation: None, db_id: 0, baseline: None, version: 1, written: String::new(), playlists: vec![], loose: vec![], tracks: vec![rbl_export::manifest::ManifestTrack { analysis_hashes: std::collections::BTreeMap::new(), analysis_extensions: vec!["DAT".into(), "EXT".into()], audio_hash: 0,
            export_id: 1, library_id: id.parse().unwrap(), source: source_path, audio: "audio.mp3".into(), anlz_dir: anlz.into(), size: 0, modified: 0, analysis: 0, artwork: String::new(), conversion: String::new(), conversion_source_hash: 0, in_place: false, preserved: Some(false), copy_stamp: None,
        }] }.save(&usb).unwrap();
        let editor = crate::grid::GridEditor::at(state.backup_dir());
        let rows = |state: &AppState| state.read_db(|db| {
            let mut q = db.connection().prepare("SELECT InMsec, OutMsec, Kind, ColorTableIndex, Comment FROM djmdCue WHERE ContentID=?1 AND rb_local_deleted=0 ORDER BY InMsec")?;
            let rows = q.query_map([&id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, Option<String>>(4)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        }).unwrap();
        let snapshot = |state: &AppState| (
            state.read_db(|db| Ok(db.connection().query_row(
                "SELECT AnalysisUpdated, rb_local_usn FROM djmdContent WHERE ID=?1", [&id],
                |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<i64>>(1)?)))?)).unwrap(),
            std::fs::read(&target).unwrap(),
            std::fs::read(target.with_extension("EXT")).unwrap(),
            rows(state),
        );
        let import_cues = || import(&state, &editor, &usb, true, false, false, false).unwrap();
        // Imports once, then asserts the same stick is a no-op that touches
        // no file, row or counter.
        let import_then_unchanged = |why: &str| {
            let first = import_cues();
            assert_eq!((first.tracks, first.unchanged, first.skipped), (1, 0, 0), "{why}: the change is written");
            let before = snapshot(&state);
            let again = import_cues();
            assert_eq!((again.tracks, again.unchanged, again.skipped), (0, 1, 0), "{why}: the same stick again is unchanged");
            assert_eq!(again.changed, Vec::<String>::new());
            assert_eq!(snapshot(&state), before, "{why}: nothing is touched");
        };

        // (a) and (b): the first import writes the cues, the second is a no-op.
        import_then_unchanged("first import");
        assert_eq!(rows(&state), vec![
            (1000, None, 0, 0, Some(String::new())),
            (2000, None, 1, 22, Some("Drop".into())),
            (3000, None, 6, 1, Some(String::new())),
            (4000, Some(8000), 0, 0, Some(String::new())),
        ]);
        let written = rbl_anlz::Anlz::read(&target).unwrap();
        assert_eq!(written.waveform(b"PWAV").unwrap().1, &[1, 2, 3]);

        // (c): one cue's time, colour or comment changes on the stick, the
        // grid does not; each is written, and then settles.
        cues[2].time_ms = 3500;
        write_stick(&usb.join(anlz), &cues);
        import_then_unchanged("hot cue E moved");
        cues[1].color_code = 46;
        write_stick(&usb.join(anlz), &cues);
        import_then_unchanged("hot cue A recoloured");
        cues[1].comment = "Break".into();
        write_stick(&usb.join(anlz), &cues);
        import_then_unchanged("hot cue A renamed");
        // A memory cue's colour row is stored one down in `Color`, as
        // rekordbox reads it back (#617): row 3 is Orange, 2.
        let memory_colour = |state: &AppState| state.read_db(|db| Ok(db.connection().query_row(
            "SELECT Color FROM djmdCue WHERE ContentID=?1 AND rb_local_deleted=0 AND InMsec=1000", [&id], |r| r.get::<_, i64>(0))?)).unwrap();
        assert_eq!(memory_colour(&state), 255, "no colour row, no colour");
        cues[0].color_id = 3;
        write_stick(&usb.join(anlz), &cues);
        import_then_unchanged("memory cue colour row changed");
        assert_eq!(memory_colour(&state), 2, "colour row 3 is Orange");
        // The library's colour alone differing is a change too: the stick's
        // is written back.
        let memory: String = state.read_db(|db| Ok(db.connection().query_row(
            "SELECT ID FROM djmdCue WHERE ContentID=?1 AND rb_local_deleted=0 AND InMsec=1000", [&id], |r| r.get(0))?)).unwrap();
        state.write(|w| w.set_cue_colour(&memory, Some(6))).unwrap();
        import_then_unchanged("memory cue recoloured in the library");
        assert_eq!(memory_colour(&state), 2);
        assert!(rows(&state).contains(&(2000, None, 1, 46, Some("Break".into()))));
        assert!(rows(&state).contains(&(3500, None, 6, 1, Some(String::new()))));

        // A row rekordbox wrote keeps a cue's end as -1 and may leave its
        // comment NULL: that is the same cue, not a change.
        let writable = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadWrite).unwrap();
        assert_eq!(writable.connection().execute("UPDATE djmdCue SET OutMsec=-1 WHERE ContentID=?1 AND rb_local_deleted=0 AND OutMsec IS NULL", [&id]).unwrap(), 3);
        assert_eq!(writable.connection().execute("UPDATE djmdCue SET Comment=NULL WHERE ContentID=?1 AND rb_local_deleted=0 AND Comment=''", [&id]).unwrap(), 3);
        drop(writable);
        let again = import_cues();
        assert_eq!((again.tracks, again.unchanged), (0, 1), "rekordbox's -1 end and NULL comment match the stick");

        // (d): only the library's cue row changes, the analysis files are
        // identical; the stick's cue is written back.
        let memory: String = state.read_db(|db| Ok(db.connection().query_row(
            "SELECT ID FROM djmdCue WHERE ContentID=?1 AND rb_local_deleted=0 AND InMsec=1000", [&id], |r| r.get(0))?)).unwrap();
        state.write(|w| w.move_cue(&memory, 1200)).unwrap();
        let files = (std::fs::read(&target).unwrap(), std::fs::read(target.with_extension("EXT")).unwrap());
        import_then_unchanged("library cue moved");
        assert!(rows(&state).contains(&(1000, None, 0, 0, Some(String::new()))));
        assert_eq!((std::fs::read(&target).unwrap(), std::fs::read(target.with_extension("EXT")).unwrap()), files);
    }

    /// A stick rekordbox wrote to an HFS+ drive (`.PIONEER`, `OneLibrary`
    /// identity only) still names tracks the library no longer has: one
    /// whose id is gone, one removed (`rb_local_deleted`). Both used to fail
    /// the whole import (#138); now they are skipped and the rest comes in.
    #[test]
    fn tracks_the_library_no_longer_has_are_skipped_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let location = rbl_db::fixture::build(dir.path(), rbl_db::fixture::Shape::default()).unwrap();
        let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
        let (library, _) = rbl_index::load(&db).unwrap();
        let db_id = rbl_db::export_info::db_id(db.connection()).unwrap();
        let state = AppState::with_backups(dir.path().join("backups"));
        state.set_library(library, false, db.schema().db_version, 0, location.clone());
        drop(db);
        let live = rbl_db::fixture::track_id(0);
        let removed = rbl_db::fixture::track_id(1);
        let relative = state.write(|w| w.analysis_data_path_for(&live)).unwrap();
        state.write(|w| w.register_analysis(&live, &rbl_db::write::AnalysisRegistration { bpm_x100: 12800, key: None, analysis_data_path: &relative })).unwrap();
        let target = rbl_anlz::resolve(&location.share_root, &relative);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        let mut original = rbl_anlz::write::AnlzBuilder::new();
        original.path("/original.mp3").beat_grid(&[rbl_anlz::Beat { beat_number: 1, tempo_x100: 12800, time_ms: 250 }]).cue_lists(true);
        std::fs::write(&target, original.finish()).unwrap();
        let writable = rbl_db::Library::open(location, rbl_db::OpenMode::ReadWrite).unwrap();
        assert_eq!(writable.connection().execute("UPDATE djmdContent SET rb_local_deleted=1 WHERE ID=?1", [&removed]).unwrap(), 1);
        drop(writable);

        let usb = dir.path().join("usb");
        let one = usb.join(".PIONEER/rekordbox/exportLibrary.db");
        std::fs::create_dir_all(one.parent().unwrap()).unwrap();
        let mut builder = rbl_onelibrary::build::Builder::create(&one).unwrap();
        // Stick id 1 is in the library; 2 never was (or was deleted for
        // good); 3 is in it but removed.
        for (content_id, master) in [(1, live.as_str()), (2, "99999"), (3, removed.as_str())] {
            let analysis = format!("/.PIONEER/USBANLZ/P000/{content_id:08X}/ANLZ0000.DAT");
            std::fs::create_dir_all(usb.join(analysis.trim_start_matches('/')).parent().unwrap()).unwrap();
            write_stick(usb.join(analysis.trim_start_matches('/')).parent().unwrap(), &[stick_cue(1, 2000, None, 22, "")]);
            builder.add_track(&rbl_onelibrary::build::Track {
                content_id, master_db_id: i64::try_from(db_id).unwrap(), master_content_id: master.parse().unwrap(),
                title: format!("Track {content_id}"), path: format!("/Contents/{content_id}.mp3"), analysis_path: analysis,
                ..Default::default()
            }).unwrap();
        }
        builder.add_history(1, "Only ours", 0, 1, false).unwrap();
        builder.add_history_track(1, 1, 1).unwrap();
        builder.add_history(2, "With a gone track", 0, 2, false).unwrap();
        for (seq, content) in [1, 2, 3].into_iter().enumerate() {
            builder.add_history_track(2, content, i64::try_from(seq + 1).unwrap()).unwrap();
        }
        builder.finish("Playlist 1 HFS+", "2026-10-09", 0).unwrap();

        let editor = crate::grid::GridEditor::at(state.backup_dir());
        let cues = import(&state, &editor, &usb, true, false, false, false).unwrap();
        assert_eq!((cues.tracks, cues.skipped), (1, 2), "the live track's cues come in; the other two are skipped");
        let local_before = std::fs::read(&target).unwrap();
        let usb_db_before = std::fs::read(&one).unwrap();
        std::fs::remove_file(usb.join(".PIONEER/USBANLZ/P000/00000001/ANLZ0000.DAT")).unwrap();
        let missing = import(&state, &editor, &usb, true, false, false, false).unwrap();
        assert_eq!((missing.tracks, missing.skipped), (0, 3));
        assert_eq!(missing.warnings.len(), 1);
        assert!(missing.warnings[0].contains("analysis file is missing"));
        assert_eq!(std::fs::read(&target).unwrap(), local_before);
        assert_eq!(std::fs::read(&one).unwrap(), usb_db_before);
        let history = import(&state, &editor, &usb, false, true, false, false).unwrap();
        assert_eq!(history.histories, 1, "the session of library tracks comes in");
        assert_eq!(history.skipped, 2);
        assert_eq!(history.warnings, vec!["History 'With a gone track' contains tracks that could not be matched to this library; it was left on the USB.".to_owned()]);
        let imported: Vec<String> = state.read_db(|db| {
            let mut q = db.connection().prepare("SELECT s.ContentID FROM djmdSongHistory s JOIN djmdHistory h ON h.ID=s.HistoryID WHERE h.Name LIKE 'Only ours (USB %' AND s.rb_local_deleted=0")?;
            let rows = q.query_map([], |r| r.get(0))?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        }).unwrap();
        assert_eq!(imported, vec![live]);
    }

    fn history_import_fixture(dir: &Path) -> (AppState, std::path::PathBuf) {
        let location = rbl_db::fixture::build(dir, rbl_db::fixture::Shape {
            tracks: 3, playlists: 0, tracks_per_playlist: 0, history_sessions: 0,
            ..Default::default()
        }).unwrap();
        let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
        let (library, _) = rbl_index::load(&db).unwrap();
        let state = AppState::with_backups(dir.join("backups"));
        state.set_library(library, false, db.schema().db_version, 0, location);
        drop(db);
        (state, dir.join("usb"))
    }

    fn write_history_sessions(state: &AppState, usb: &Path, sessions: &[(i64, &str, &[i64])]) -> std::path::PathBuf {
        let db_id = state.read_db(|db| rbl_db::export_info::db_id(db.connection())).unwrap();
        let one = usb.join("PIONEER/rekordbox/exportLibrary.db");
        std::fs::create_dir_all(one.parent().unwrap()).unwrap();
        if one.exists() { std::fs::remove_file(&one).unwrap(); }
        let mut builder = rbl_onelibrary::build::Builder::create(&one).unwrap();
        for index in 0..4 {
            builder.add_track(&rbl_onelibrary::build::Track {
                content_id: i64::try_from(index + 1).unwrap(),
                master_db_id: i64::try_from(db_id).unwrap(),
                master_content_id: if index < 3 { rbl_db::fixture::track_id(index).parse().unwrap() } else { 99999 },
                title: format!("Track {index}"), path: format!("/Contents/{index}.mp3"),
                ..Default::default()
            }).unwrap();
        }
        builder.add_history(10000, "Folder", 0, 0, true).unwrap();
        for &(id, name, tracks) in sessions {
            builder.add_history(id, name, 0, id, false).unwrap();
            for (sequence, &content) in tracks.iter().enumerate() {
                builder.add_history_track(id, content, i64::try_from(sequence + 1).unwrap()).unwrap();
            }
        }
        builder.finish("Disposable history writer fixture", "2026-11-25", 0).unwrap();
        one
    }

    fn imported_history_rows(state: &AppState, name: &str) -> Vec<(String, String, i64, String, i64)> {
        state.read_db(|db| {
            let mut q = db.connection().prepare(
                "SELECT h.UUID, s.ContentID, s.TrackNo, s.UUID, s.rb_local_usn
                 FROM djmdSongHistory s JOIN djmdHistory h ON h.ID=s.HistoryID
                 WHERE h.Name LIKE ?1 AND s.rb_local_deleted=0
                 ORDER BY h.Name, s.TrackNo")?;
            let values = q.query_map([format!("{name} (USB %")], |r| Ok((r.get::<_,String>(0)?, r.get::<_,String>(1)?,
                r.get::<_,i64>(2)?, r.get::<_,String>(3)?, r.get::<_,i64>(4)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(values)
        }).unwrap()
    }

    /// Connection lifetime must not scale with the number of history sessions.
    /// Each session keeps its existing transaction, order and repeated plays.
    #[test]
    fn history_import_reuses_one_writer_for_all_eligible_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let (state, usb) = history_import_fixture(dir.path());
        let names: Vec<String> = (1..=33).map(|id| format!("Writer probe {id:02}")).collect();
        let sessions: Vec<_> = names.iter().enumerate().map(|(index, name)|
            (i64::try_from(index + 1).unwrap(), name.as_str(), &[1, 2, 1][..])).collect();
        let one = write_history_sessions(&state, &usb, &sessions);
        let usb_before = std::fs::read(&one).unwrap();
        let editor = crate::grid::GridEditor::at(state.backup_dir());
        let opens = state.writer_opens();
        let report = import(&state, &editor, &usb, false, true, false, false).unwrap();
        let first_opens = state.writer_opens() - opens;
        assert_eq!((report.histories, report.skipped), (99, 0));
        let rows = || imported_history_rows(&state, "Writer probe %");
        let before_repeat = rows();
        assert_eq!(before_repeat.len(), 99);
        for session in before_repeat.chunks_exact(3) {
            assert_eq!(session.iter().map(|r| (r.1.clone(), r.2)).collect::<Vec<_>>(),
                vec![(rbl_db::fixture::track_id(0), 1), (rbl_db::fixture::track_id(1), 2), (rbl_db::fixture::track_id(0), 3)]);
        }
        let opens = state.writer_opens();
        let repeat = import(&state, &editor, &usb, false, true, false, false).unwrap();
        let repeat_opens = state.writer_opens() - opens;
        assert_eq!((repeat.histories, repeat.skipped), (0, 0));
        assert_eq!(rows(), before_repeat, "exact repeat preserves row identities, order and USNs");
        assert_eq!(std::fs::read(&one).unwrap(), usb_before, "import never rewrites its source USB");
        assert!(first_opens <= 1 && repeat_opens <= 1,
            "33 eligible sessions opened {first_opens} writers on first import and {repeat_opens} on exact repeat");
    }

    #[test]
    fn history_import_with_no_eligible_session_opens_no_writer() {
        let dir = tempfile::tempdir().unwrap();
        let (state, usb) = history_import_fixture(dir.path());
        let one = write_history_sessions(&state, &usb, &[(1, "Empty", &[]), (2, "Mixed", &[1, 4])]);
        let before = std::fs::read(&one).unwrap();
        let editor = crate::grid::GridEditor::at(state.backup_dir());
        let opens = state.writer_opens();
        let report = import(&state, &editor, &usb, false, true, false, false).unwrap();
        assert_eq!(state.writer_opens(), opens);
        assert_eq!((report.histories, report.skipped), (0, 1));
        assert_eq!(report.warnings, vec!["History 'Mixed' contains tracks that could not be matched to this library; it was left on the USB.".to_owned()]);
        assert_eq!(imported_history_rows(&state, "%"), []);
        assert_eq!(std::fs::read(one).unwrap(), before);
    }

    #[test]
    fn shared_history_writer_preserves_append_repeat_and_committed_prefix_on_later_refusal() {
        let dir = tempfile::tempdir().unwrap();
        let (state, usb) = history_import_fixture(dir.path());
        write_history_sessions(&state, &usb, &[(1, "Alpha", &[1, 2, 1]), (2, "Beta", &[2])]);
        let editor = crate::grid::GridEditor::at(state.backup_dir());
        assert_eq!(import(&state, &editor, &usb, false, true, false, false).unwrap().histories, 4);
        let alpha_initial = imported_history_rows(&state, "Alpha");
        let beta_initial = imported_history_rows(&state, "Beta");
        let one = write_history_sessions(&state, &usb, &[(1, "Alpha", &[1, 2, 1, 3]), (2, "Beta", &[2, 3])]);
        let source_before = std::fs::read(&one).unwrap();
        let opens = state.writer_opens();
        assert_eq!(import(&state, &editor, &usb, false, true, false, false).unwrap().histories, 2);
        assert_eq!(state.writer_opens() - opens, 1);
        let alpha_appended = imported_history_rows(&state, "Alpha");
        let beta_appended = imported_history_rows(&state, "Beta");
        assert_eq!(&alpha_appended[..3], alpha_initial.as_slice());
        assert_eq!(&beta_appended[..1], beta_initial.as_slice());
        assert_eq!(import(&state, &editor, &usb, false, true, false, false).unwrap().histories, 0);
        assert_eq!(imported_history_rows(&state, "Alpha"), alpha_appended);
        assert_eq!(imported_history_rows(&state, "Beta"), beta_appended);
        assert_eq!(std::fs::read(&one).unwrap(), source_before);
        // Alpha commits before Beta's changed prefix refuses. Sharing the
        // connection must not turn the sessions into one atomic transaction.
        write_history_sessions(&state, &usb, &[(1, "Alpha", &[1, 2, 1, 3, 2]), (2, "Beta", &[1, 3])]);
        let source_before = std::fs::read(&one).unwrap();
        let opens = state.writer_opens();
        let error = import(&state, &editor, &usb, false, true, false, false).unwrap_err();
        assert!(error.message.contains("history changed"));
        assert_eq!(state.writer_opens() - opens, 1);
        let alpha_after = imported_history_rows(&state, "Alpha");
        assert_eq!(alpha_after.len(), 5);
        assert_eq!(&alpha_after[..4], alpha_appended.as_slice());
        assert_eq!(alpha_after[4].1, rbl_db::fixture::track_id(1));
        assert_eq!(imported_history_rows(&state, "Beta"), beta_appended);
        assert_eq!(std::fs::read(one).unwrap(), source_before);
    }

    /// A library and a stick rekordbox wrote from it: every fixture track on
    /// the stick, with cues, under `OneLibrary` identity.
    fn library_and_stick(dir: &Path) -> (AppState, rbl_db::LibraryLocation, std::path::PathBuf, usize) {
        let location = rbl_db::fixture::build(dir, rbl_db::fixture::Shape::default()).unwrap();
        let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
        let (library, _) = rbl_index::load(&db).unwrap();
        let db_id = rbl_db::export_info::db_id(db.connection()).unwrap();
        let tracks = library.len();
        let state = AppState::with_backups(dir.join("backups"));
        state.set_library(library, false, db.schema().db_version, 0, location.clone());
        drop(db);
        let usb = dir.join("usb");
        let one = usb.join("PIONEER/rekordbox/exportLibrary.db");
        std::fs::create_dir_all(one.parent().unwrap()).unwrap();
        let mut builder = rbl_onelibrary::build::Builder::create(&one).unwrap();
        for index in 0..tracks {
            let content_id = i64::try_from(index + 1).unwrap();
            let analysis = format!("/PIONEER/USBANLZ/P000/{content_id:08X}/ANLZ0000.DAT");
            let folder = usb.join(analysis.trim_start_matches('/')).parent().unwrap().to_path_buf();
            std::fs::create_dir_all(&folder).unwrap();
            write_stick(&folder, &[stick_cue(1, 2000, None, 22, "")]);
            builder.add_track(&rbl_onelibrary::build::Track {
                content_id, master_db_id: i64::try_from(db_id).unwrap(),
                master_content_id: rbl_db::fixture::track_id(index).parse().unwrap(),
                title: format!("Track {content_id}"), path: format!("/Contents/{content_id}.mp3"), analysis_path: analysis,
                ..Default::default()
            }).unwrap();
        }
        builder.finish("Playlist 1 HFS+", "2026-10-09", 0).unwrap();
        (state, location, usb, tracks)
    }

    #[test]
    fn a_missing_usb_analysis_file_does_not_block_other_cue_imports() {
        let dir = tempfile::tempdir().unwrap();
        let (state, location, usb, tracks) = library_and_stick(dir.path());
        let mut local = Vec::new();
        for index in 0..2 {
            let id = rbl_db::fixture::track_id(index);
            let relative = state.write(|w| w.analysis_data_path_for(&id)).unwrap();
            state.write(|w| w.register_analysis(&id, &rbl_db::write::AnalysisRegistration {
                bpm_x100: 12000, key: None, analysis_data_path: &relative,
            })).unwrap();
            let target = rbl_anlz::resolve(&location.share_root, &relative);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            let mut original = rbl_anlz::write::AnlzBuilder::new();
            original.path("/original.mp3").beat_grid(&[rbl_anlz::Beat { beat_number: 1, tempo_x100: 12000, time_ms: 0 }]).cue_lists(true);
            std::fs::write(&target, original.finish()).unwrap();
            local.push(target);
        }
        let missing = usb.join("PIONEER/USBANLZ/P000/00000001/ANLZ0000.DAT");
        std::fs::remove_file(&missing).unwrap();
        let before_missing = std::fs::read(&local[0]).unwrap();
        let usb_db = usb.join("PIONEER/rekordbox/exportLibrary.db");
        let before_usb_db = std::fs::read(&usb_db).unwrap();
        let editor = crate::grid::GridEditor::at(state.backup_dir());
        let report = import(&state, &editor, &usb, true, false, false, false).unwrap();
        assert_eq!((report.tracks, report.skipped), (1, tracks - 1));
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("analysis file is missing"));
        assert_eq!(std::fs::read(&local[0]).unwrap(), before_missing);
        assert_eq!(rbl_anlz::Anlz::read(&local[1]).unwrap().beat_grid().unwrap()[0].time_ms, 250);
        assert_eq!(std::fs::read(&usb_db).unwrap(), before_usb_db);
        let repeat = import(&state, &editor, &usb, true, false, false, false).unwrap();
        assert_eq!((repeat.tracks, repeat.unchanged, repeat.skipped), (0, 1, tracks - 1));
    }

    /// Importing cues reads two things from the library for each track on
    /// the stick, its analysis lock and its analysis path. Each used to open
    /// the library again, deriving its key every time (about 155 ms), so a
    /// stick of 20,000 tracks held the library's edit gate, and with it
    /// every analysis and every sync's preparation, for well over an hour
    /// (#284). Measured against one open on this machine, the whole import
    /// of the stick must cost a fraction of an open per track.
    #[test]
    fn importing_cues_does_not_reopen_the_library_for_every_track() {
        let dir = tempfile::tempdir().unwrap();
        let (state, location, usb, tracks) = library_and_stick(dir.path());
        assert!(tracks >= 40);
        let editor = crate::grid::GridEditor::at(state.backup_dir());
        // Warm the shared handle and the writer, as an app that has loaded
        // the library has.
        import(&state, &editor, &usb, true, false, false, false).unwrap();
        let started = std::time::Instant::now();
        for _ in 0..3 {
            drop(rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).unwrap());
        }
        let one_open = started.elapsed() / 3;
        let started = std::time::Instant::now();
        let report = import(&state, &editor, &usb, true, false, false, false).unwrap();
        let elapsed = started.elapsed();
        // No local analysis files: each track is looked up, then skipped.
        assert_eq!((report.tracks, report.skipped), (0, tracks));
        assert!(
            elapsed < one_open * u32::try_from(tracks / 4).unwrap(),
            "{tracks} tracks took {elapsed:?}; one open of the library takes {one_open:?}",
        );
    }

    /// Gives each of the first `count` fixture tracks a local analysis file
    /// whose cues differ from the stick's, so importing has to rewrite it.
    fn local_analysis_for(state: &AppState, location: &rbl_db::LibraryLocation, count: usize) -> Vec<std::path::PathBuf> {
        (0..count).map(|index| {
            let id = rbl_db::fixture::track_id(index);
            let relative = state.write(|w| w.analysis_data_path_for(&id)).unwrap();
            state.write(|w| w.register_analysis(&id, &rbl_db::write::AnalysisRegistration {
                bpm_x100: 12000, key: None, analysis_data_path: &relative,
            })).unwrap();
            let target = rbl_anlz::resolve(&location.share_root, &relative);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            let mut original = rbl_anlz::write::AnlzBuilder::new();
            original.path("/original.mp3").beat_grid(&[rbl_anlz::Beat { beat_number: 1, tempo_x100: 12000, time_ms: 0 }]).cue_lists(true);
            std::fs::write(&target, original.finish()).unwrap();
            target
        }).collect()
    }

    /// A stick whose tracks all differ from the library used to cost two
    /// key derivations each (the writer's open and the journal's read), at
    /// about 60 ms apiece, and a journal's fsyncs on top: a stick of 14,000
    /// such tracks held the library for most of an hour and an import never
    /// finished (#794). The writer is opened once for each batch instead,
    /// and every track is still imported through its own journal.
    #[test]
    fn importing_changed_cues_opens_the_writer_once_per_batch() {
        let dir = tempfile::tempdir().unwrap();
        let (state, location, usb, tracks) = library_and_stick(dir.path());
        assert!(tracks >= 40);
        let local = local_analysis_for(&state, &location, tracks);
        let editor = crate::grid::GridEditor::at(state.backup_dir());
        let opens = state.writer_opens();
        let report = import(&state, &editor, &usb, true, false, false, false).unwrap();
        assert_eq!((report.tracks, report.skipped, report.unchanged), (tracks, 0, 0));
        let opened = state.writer_opens() - opens;
        // One to check the library is writable, then one for each batch.
        assert!(opened <= 1 + u64::try_from(tracks.div_ceil(CUE_BATCH_TRACKS)).unwrap(), "{tracks} tracks opened the writer {opened} times");
        for path in &local {
            assert_eq!(rbl_anlz::Anlz::read(path).unwrap().beat_grid().unwrap()[0].time_ms, 250);
        }
        assert!(!state.backup_dir().join("analysis-journal").exists_with_entries(), "every journal was committed");
        let repeat = import(&state, &editor, &usb, true, false, false, false).unwrap();
        assert_eq!((repeat.tracks, repeat.unchanged), (0, tracks), "what the batches wrote matches the stick");
    }

    /// A track the library refuses part-way through a batch leaves each
    /// track whole: those edited before it keep their new files, it and the
    /// rest are put back, and no journal is left behind.
    #[test]
    fn malformed_later_track_prevents_every_batch_from_writing() {
        let dir = tempfile::tempdir().unwrap();
        let (state, location, usb, tracks) = library_and_stick(dir.path());
        assert!(tracks >= 40);
        let local = local_analysis_for(&state, &location, tracks);
        // A loop that ends before it starts is data the library refuses.
        let invalid = tracks - 5;
        write_stick(
            &usb.join(format!("PIONEER/USBANLZ/P000/{:08X}", invalid + 1)),
            &[stick_cue(2, 3000, Some(1000), 22, "")],
        );
        let before: Vec<Vec<u8>> = local.iter().map(|p| std::fs::read(p).unwrap()).collect();
        let editor = crate::grid::GridEditor::at(state.backup_dir());
        // Grid edits made before the import must survive a preflight refusal
        // because no batch has changed any track yet.
        for index in 0..tracks { editor.seed_history(&rbl_db::fixture::track_id(index)); }
        assert!(import(&state, &editor, &usb, true, false, false, false).is_err());
        let mut imported = 0;
        for (index, path) in local.iter().enumerate() {
            let now = std::fs::read(path).unwrap();
            let id = rbl_db::fixture::track_id(index);
            let stick_cues = rbl_anlz::Anlz::read(&usb.join(format!("PIONEER/USBANLZ/P000/{:08X}/ANLZ0000.EXT", index + 1))).unwrap().cue_entries();
            let in_library = database_cues_match(&state, &id, &stick_cues).unwrap();
            // The file and the row agree: both imported, or both as they were.
            assert_eq!(stick_cues.len(), 1);
            assert_eq!(now != before[index], in_library, "track {index} has its file and its cues from different sides");
            // An imported track holds the stick's grid, so Undo over it is gone;
            // one the import never reached keeps its history.
            if in_library { assert!(!editor.has_history(&id), "a failed batch leaves no Undo over imported track {index}'s grid"); }
            imported += usize::from(in_library);
        }
        assert_eq!(imported, 0, "all tracks must pass preflight before any batch writes");
        assert!(!state.backup_dir().join("analysis-journal").exists_with_entries(), "no journal is left behind");
    }

    #[test]
    fn a_database_refusal_reconciles_every_journal_in_a_batch() {
        let dir = tempfile::tempdir().unwrap();
        let (state, location, usb, _) = library_and_stick(dir.path());
        let local = local_analysis_for(&state, &location, 3);
        let before: Vec<_> = local.iter().map(|p| std::fs::read(p).unwrap()).collect();
        let editor = crate::grid::GridEditor::at(state.backup_dir());
        // Match the import caller: the writer's automatic recovery must not
        // run between publishing these journals and committing their rows.
        let _gate = state.edit_gate.lock();
        let _files = state.analysis_write.lock();
        let mut batch = Vec::new();
        for (index, target) in local.iter().enumerate() {
            let id = rbl_db::fixture::track_id(index);
            editor.seed_history(&id);
            let source = usb.join(format!("PIONEER/USBANLZ/P000/{:08X}/ANLZ0000", index + 1));
            let mut entries = rbl_anlz::Anlz::read(&source.with_extension("EXT")).unwrap().cue_entries();
            if index == 1 {
                entries[0].kind = 2;
                entries[0].time_ms = 3000;
                entries[0].loop_time_ms = 1000;
            }
            batch.push(PendingCues { id, identities: Vec::new(), payload: ImportPayload::Cues(entries), bpm: 12800,
                files: vec![(target.clone(), std::fs::read(source.with_extension("DAT")).unwrap())] });
        }
        assert!(apply_cue_batch(&state, &editor, &location, batch, &mut ImportReport::default()).is_err());
        assert_ne!(std::fs::read(&local[0]).unwrap(), before[0]);
        for (path, bytes) in local.iter().zip(&before).skip(1) { assert_eq!(&std::fs::read(path).unwrap(), bytes); }
        assert!(!editor.has_history(&rbl_db::fixture::track_id(0)));
        for index in 1..3 {
            assert!(editor.has_history(&rbl_db::fixture::track_id(index)), "a refused or unreached track keeps its prior Undo");
        }
        assert!(!state.backup_dir().join("analysis-journal").exists_with_entries());
    }

    /// An import that has to wait for the stick, while an export holds it,
    /// waits without the library's edit gate or analysis lock, so analysis
    /// and the next sync's preparation go on (#284).
    #[test]
    fn an_import_waiting_for_the_stick_holds_no_library_lock() {
        let dir = tempfile::tempdir().unwrap();
        let (state, _location, usb, _tracks) = library_and_stick(dir.path());
        let state = std::sync::Arc::new(state);
        let editor = std::sync::Arc::new(crate::grid::GridEditor::at(state.backup_dir()));
        // What an export holds for its whole run: the stick's write lock.
        let export = rbl_core::durable::Publication::new(&usb, ".rbxport-publication-284").unwrap();
        let (sent, finished) = std::sync::mpsc::channel();
        let worker = {
            let (state, editor, usb) = (std::sync::Arc::clone(&state), std::sync::Arc::clone(&editor), usb.clone());
            std::thread::spawn(move || {
                let result = import(&state, &editor, &usb, false, true, false, false);
                sent.send(()).unwrap();
                result
            })
        };
        // Long enough for the import to have reached the stick's lock.
        assert!(finished.recv_timeout(std::time::Duration::from_millis(300)).is_err(), "the import waits for the export");
        let other = {
            let state = std::sync::Arc::clone(&state);
            std::thread::spawn(move || state.edit_gate.try_lock().is_some() && state.analysis_write.try_lock().is_some())
        };
        assert!(other.join().unwrap(), "an analysis can take the library's locks while the import waits");
        drop(export);
        finished.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
        worker.join().unwrap().unwrap();
    }

    /// A player's `MYSETTING.DAT`: the 104-byte header, a 40-byte body and
    /// CRC-16/XMODEM over the body, as a rekordbox stick carries it.
    fn my_setting(body_byte: u8) -> Vec<u8> {
        let mut bytes = vec![0u8; 104];
        bytes[0] = 96;
        bytes[4..14].copy_from_slice(b"PIONEER DJ");
        bytes[100] = 40;
        bytes.extend([body_byte; 40]);
        let mut crc = 0u16;
        for byte in &bytes[104..] {
            crc ^= u16::from(*byte) << 8;
            for _ in 0..8 { crc = if crc & 0x8000 == 0 { crc << 1 } else { (crc << 1) ^ 0x1021 }; }
        }
        bytes.extend(crc.to_le_bytes());
        bytes.extend([0, 0]);
        bytes
    }

    #[test]
    fn imported_settings_land_where_export_reads_them() {
        let dir = tempfile::tempdir().unwrap();
        let location = rbl_db::fixture::build(dir.path(), rbl_db::fixture::Shape::default()).unwrap();
        let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
        let (library, _) = rbl_index::load(&db).unwrap();
        drop(db);
        let state = AppState::with_backups(dir.path().join("state/backups"));
        state.set_library(library, false, None, 0, location);
        let usb = dir.path().join("usb");
        std::fs::create_dir_all(usb.join("PIONEER")).unwrap();
        let file = my_setting(7);
        std::fs::write(usb.join("PIONEER/MYSETTING.DAT"), &file).unwrap();
        let editor = crate::grid::GridEditor::at(state.backup_dir());
        let report = import(&state, &editor, &usb, false, false, true, false).unwrap();
        assert_eq!(report.settings, 1);
        // The folder the export reads from, for the same state directory.
        assert_eq!(settings_stash(state.backup_dir()), dir.path().join("state/usb-settings"));
        assert_eq!(std::fs::read(settings_stash(state.backup_dir()).join("MYSETTING.DAT")).unwrap(), file);
    }

    #[test]
    fn onelibrary_tree_is_read_without_a_legacy_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("PIONEER/rekordbox/exportLibrary.db");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut builder = rbl_onelibrary::build::Builder::create(&path).unwrap();
        builder.add_playlist(1, "Set", 0, 0).unwrap();
        builder.finish("USB", "2026-09-21", 0).unwrap();
        let trees = library_trees(dir.path()).unwrap();
        assert_eq!(trees.len(), 1);
        assert_eq!(trees[0].name, "OneLibrary");
        assert_eq!(trees[0].nodes[0].name, "Set");
    }

    #[test]
    fn invalid_settings_and_escaped_paths_are_rejected() {
        assert!(validate_settings("MYSETTING.DAT", &[0; 148]).is_err());
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("usb")).unwrap();
        std::fs::write(dir.path().join("outside"), b"x").unwrap();
        assert!(within(&dir.path().join("usb"), "../outside").is_err());
    }

    #[test]
    fn import_explains_an_interrupted_export_that_cannot_be_recovered() {
        let dir = tempfile::tempdir().unwrap();
        let usb = dir.path().join("usb");
        let journal = usb.join(".rbxport-publication");
        std::fs::create_dir_all(&journal).unwrap();
        std::fs::write(
            journal.join("publication.json"),
            br#"[{"path":"missing-track.wav","present":true}]"#,
        )
        .unwrap();
        let state = AppState::with_backups(dir.path().join("backups"));
        let editor = crate::grid::GridEditor::at(state.backup_dir());

        let error = import(&state, &editor, &usb, false, true, false, false).unwrap_err();
        assert!(error.message.contains("Could not recover an interrupted export"));
        assert!(error.message.contains("missing-track.wav"));
    }

    #[test]
    fn preserved_zero_bpm_recovers_a_committed_prefix_after_later_import_refusal() {
        for requested_bpm in [12800_u16, 0] {
            let dir = tempfile::tempdir().unwrap();
            let (state, location, usb, _) = library_and_stick(dir.path());
            let local = local_analysis_for(&state, &location, 3);
            let before: Vec<_> = local.iter().map(|p| std::fs::read(p).unwrap()).collect();
            let editor = crate::grid::GridEditor::at(state.backup_dir());
            let _gate = state.edit_gate.lock();
            let _files = state.analysis_write.lock();
            let mut batch = Vec::new();
            let mut source_images = Vec::new();
            for (index, target) in local.iter().enumerate() {
                let id = rbl_db::fixture::track_id(index);
                editor.seed_history(&id);
                let source = usb.join(format!("PIONEER/USBANLZ/P000/{:08X}/ANLZ0000", index + 1));
                let mut dat = rbl_anlz::Anlz::read(&source.with_extension("DAT")).unwrap();
                if index == 0 {
                    let grid = dat.sections.iter_mut().find(|section| section.tag.0 == *b"PQTZ").unwrap();
                    grid.payload[2..4].copy_from_slice(&requested_bpm.to_be_bytes());
                    std::fs::write(source.with_extension("DAT"), dat.to_bytes()).unwrap();
                }
                let entries_file = rbl_anlz::Anlz::read(&source.with_extension("EXT")).unwrap();
                let mut entries = entries_file.cue_entries();
                if index == 1 {
                    // The existing later-row refusal leaves the first row
                    // committed before the production caller reconciles.
                    entries[0].kind = 2;
                    entries[0].time_ms = 3000;
                    entries[0].loop_time_ms = 1000;
                }
                for extension in ["DAT", "EXT"] {
                    let path = source.with_extension(extension);
                    source_images.push((path.clone(), std::fs::read(path).unwrap()));
                }
                batch.push(PendingCues { id, identities: Vec::new(), payload: ImportPayload::Cues(entries),
                    bpm: u32::from(dat.beat_grid().unwrap()[0].tempo_x100),
                    files: vec![(target.clone(), dat.to_bytes())] });
            }
            let error = apply_cue_batch(&state, &editor, &location, batch, &mut ImportReport::default()).unwrap_err();
            assert!(!error.full_message().contains("row changed outside"), "raw BPM {requested_bpm}: {}", error.full_message());
            assert!(error.full_message().contains("Invalid USB cue data"), "the original later-row refusal is retained");
            assert!(!state.backup_dir().join("analysis-journal").exists_with_entries(), "raw BPM {requested_bpm}: a committed prefix must reconcile: {error}");
            crate::file_journal::recover(state.backup_dir(), &location).unwrap();
            let expected_bpm = if requested_bpm == 0 { 12000 } else { u32::from(requested_bpm) };
            let actual_bpm: u32 = state.read_db(|db| Ok(db.connection().query_row(
                "SELECT BPM FROM djmdContent WHERE ID=?1", [rbl_db::fixture::track_id(0)], |row| row.get(0))?)).unwrap();
            assert_eq!(actual_bpm, expected_bpm, "the writer's zero-preserve behavior is unchanged");
            let first_entries = rbl_anlz::Anlz::read(&usb.join("PIONEER/USBANLZ/P000/00000001/ANLZ0000.EXT")).unwrap().cue_entries();
            assert!(database_cues_match(&state, &rbl_db::fixture::track_id(0), &first_entries).unwrap());
            assert_ne!(std::fs::read(&local[0]).unwrap(), before[0]);
            assert_eq!(rbl_anlz::Anlz::read(&local[0]).unwrap().beat_grid().unwrap()[0].tempo_x100, requested_bpm);
            for (path, image) in local.iter().zip(&before).skip(1) { assert_eq!(&std::fs::read(path).unwrap(), image); }
            assert!(!editor.has_history(&rbl_db::fixture::track_id(0)));
            for index in 1..3 { assert!(editor.has_history(&rbl_db::fixture::track_id(index))); }
            for (path, image) in source_images { assert_eq!(std::fs::read(path).unwrap(), image); }
        }
    }

    #[test]
    fn identical_zero_tempo_import_preserves_row_revision_and_grid_undo() {
        let dir = tempfile::tempdir().unwrap();
        let (state, location, usb, _) = library_and_stick(dir.path());
        let local = local_analysis_for(&state, &location, 1);
        let id = rbl_db::fixture::track_id(0);
        let source = usb.join("PIONEER/USBANLZ/P000/00000001/ANLZ0000.DAT");
        let mut dat = rbl_anlz::Anlz::read(&source).unwrap();
        let grid = dat.sections.iter_mut().find(|section| section.tag.0 == *b"PQTZ").unwrap();
        grid.payload[2..4].copy_from_slice(&0_u16.to_be_bytes());
        std::fs::write(&source, dat.to_bytes()).unwrap();
        let source_images = [source.clone(), source.with_extension("EXT")]
            .map(|path| (path.clone(), std::fs::read(path).unwrap()));
        let editor = crate::grid::GridEditor::at(state.backup_dir());
        let import_cues = || import(&state, &editor, &usb, true, false, false, false).unwrap();
        let first = import_cues();
        assert_eq!(first.tracks, 1, "a changed grid and cues still import");
        let snapshot = || state.read_db(|db| Ok(db.connection().query_row(
            "SELECT BPM, COALESCE(rb_local_usn,0), CAST(COALESCE(AnalysisUpdated,'') AS TEXT) FROM djmdContent WHERE ID=?1",
            [&id], |row| Ok((row.get::<_, u32>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?)))?)).unwrap();
        let before = snapshot();
        assert_eq!(before.0, 12000, "raw zero retains the writer's existing positive BPM");
        assert_eq!(rbl_anlz::Anlz::read(&local[0]).unwrap().beat_grid().unwrap()[0].tempo_x100, 0,
            "accepted zero-tempo source data is unchanged by this comparison fix");
        let local_images = [local[0].clone(), local[0].with_extension("EXT")]
            .map(|path| (path.clone(), std::fs::read(path).ok()));
        editor.seed_history(&id);
        let repeated = import_cues();
        assert_eq!((repeated.tracks, repeated.unchanged), (0, 1),
            "identical source must be a no-op: before={before:?}, after={:?}, Undo kept={}, files unchanged={}",
            snapshot(), editor.has_history(&id), local_images.iter().all(|(path, image)| std::fs::read(path).ok() == *image));
        assert_eq!(repeated.changed, Vec::<String>::new());
        assert_eq!(snapshot(), before, "the retained BPM does not create a row change");
        assert!(editor.has_history(&id), "an unchanged import preserves grid Undo");
        for (path, image) in local_images { assert_eq!(std::fs::read(path).ok(), image); }
        for (path, image) in source_images { assert_eq!(std::fs::read(path).unwrap(), image); }

        // A real grid change still imports with raw zero, then settles again.
        let old_time = dat.beat_grid().unwrap()[0].time_ms;
        dat.sections.iter_mut().find(|section| section.tag.0 == *b"PQTZ").unwrap()
            .payload[4..8].copy_from_slice(&(old_time + 125).to_be_bytes());
        let changed_source = dat.to_bytes();
        std::fs::write(&source, &changed_source).unwrap();
        let changed = import_cues();
        assert_eq!(changed.tracks, 1);
        assert_eq!(snapshot().0, before.0, "the positive BPM is still retained");
        assert!(!editor.has_history(&id), "a changed grid still retires its old Undo");
        assert_eq!(rbl_anlz::Anlz::read(&local[0]).unwrap().beat_grid().unwrap()[0].time_ms, old_time + 125);
        let changed_row = snapshot();
        editor.seed_history(&id);
        let settled = import_cues();
        assert_eq!((settled.tracks, settled.unchanged), (0, 1));
        assert_eq!(snapshot(), changed_row);
        assert!(editor.has_history(&id));
        assert_eq!(std::fs::read(source).unwrap(), changed_source);
    }

    // A valid legacy DAT is independent grid authority, not authority to
    // delete or reconstruct richer local cues omitted from that projection.
    fn legacy_dat_grid_only_case(raw_tempo: u16) {
        let dir = tempfile::tempdir().unwrap();
        let (state, location, usb, tracks) = library_and_stick(dir.path());
        let id = rbl_db::fixture::track_id(0);
        let relative = state.write(|w| w.analysis_data_path_for(&id)).unwrap();
        state.write(|w| w.register_analysis(&id, &rbl_db::write::AnalysisRegistration {
            bpm_x100: 12800, key: None, analysis_data_path: &relative,
        })).unwrap();
        let target = rbl_anlz::resolve(&location.share_root, &relative);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        let mut dat = rbl_anlz::AnlzBuilder::new();
        dat.path("/local-fixture.mp3").waveform_preview(b"PWAV", &[1, 2, 3])
            .beat_grid(&[rbl_anlz::Beat { beat_number: 1, tempo_x100: 12800, time_ms: 0 }]).cue_lists(false);
        std::fs::write(&target, dat.finish()).unwrap();
        let mut ext = rbl_anlz::AnlzBuilder::new();
        ext.path("/local-fixture.mp3").waveform_preview(b"PWV3", &[4, 5, 6]).cue_lists(true);
        std::fs::write(target.with_extension("EXT"), ext.finish()).unwrap();
        let source = usb.join("PIONEER/USBANLZ/P000/00000001/ANLZ0000.DAT");
        write_stick(source.parent().unwrap(), &[
            stick_cue(0, 1000, None, 0, "memory label"),
            stick_cue(1, 2000, None, 22, "A label"),
            stick_cue(6, 5000, Some(9000), 46, "E loop label"),
        ]);
        let editor = crate::grid::GridEditor::at(state.backup_dir());
        let full = import(&state, &editor, &usb, true, false, false, false).unwrap();
        assert_eq!((full.tracks, full.skipped), (1, tracks - 1));
        let cue_rows = || state.write(|w| w.cue_rows(&id)).unwrap();
        let cues_before = cue_rows();
        let dat_before = rbl_anlz::Anlz::read(&target).unwrap();
        let ext_before = std::fs::read(target.with_extension("EXT")).unwrap();
        let snapshot = || state.read_db(|db| Ok(db.connection().query_row(
            "SELECT BPM, COALESCE(rb_local_usn,0), CAST(COALESCE(AnalysisUpdated,'') AS TEXT) FROM djmdContent WHERE ID=?1",
            [&id], |row| Ok((row.get::<_, u32>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?)))?)).unwrap();
        let row_before = snapshot();
        std::fs::remove_file(source.with_extension("EXT")).unwrap();
        let mut legacy = rbl_anlz::Anlz::read(&source).unwrap();
        let grid = legacy.sections.iter_mut().find(|section| section.tag.0 == *b"PQTZ").unwrap();
        grid.payload[2..4].copy_from_slice(&raw_tempo.to_be_bytes());
        grid.payload[4..8].copy_from_slice(&500_u32.to_be_bytes());
        grid.header[6..8].copy_from_slice(&(-37_i16).to_be_bytes());
        let legacy_bytes = legacy.to_bytes();
        std::fs::write(&source, &legacy_bytes).unwrap();
        assert!(legacy.section(b"PCO2").is_none());
        assert_eq!(legacy.legacy_cue_entries().len(), 2);
        let usb_database = usb.join("PIONEER/rekordbox/exportLibrary.db");
        let database_before = std::fs::read(&usb_database).unwrap();
        editor.seed_history(&id);
        let imported = import(&state, &editor, &usb, true, false, false, false).unwrap();
        assert_eq!(cue_rows(), cues_before, "every local cue ID/value must survive grid-only import");
        assert_eq!((imported.tracks, imported.skipped), (1, tracks - 1),
            "valid legacy DAT grid must not be skipped merely because EXT is absent");
        let after = rbl_anlz::Anlz::read(&target).unwrap();
        assert_eq!(after.section(b"PQTZ"), legacy.section(b"PQTZ"), "copy the exact grid including its signed offset");
        assert_eq!(after.sections.iter().filter(|s| s.is_cue_list()).collect::<Vec<_>>(),
            dat_before.sections.iter().filter(|s| s.is_cue_list()).collect::<Vec<_>>());
        assert_eq!(std::fs::read(target.with_extension("EXT")).unwrap(), ext_before);
        assert_eq!(snapshot().0, if raw_tempo == 0 { row_before.0 } else { u32::from(raw_tempo) });
        assert!(!editor.has_history(&id), "a changed grid retires its old Undo");
        assert!(!imported.warnings.is_empty(), "partial cue availability must be explicit");
        let changed_row = snapshot();
        let changed_dat = std::fs::read(&target).unwrap();
        editor.seed_history(&id);
        let repeated = import(&state, &editor, &usb, true, false, false, false).unwrap();
        assert_eq!((repeated.tracks, repeated.unchanged), (0, 1));
        assert!(editor.has_history(&id), "an identical import preserves Undo");
        assert_eq!(snapshot(), changed_row);
        assert_eq!(cue_rows(), cues_before);
        assert_eq!(std::fs::read(&target).unwrap(), changed_dat);
        assert_eq!(std::fs::read(target.with_extension("EXT")).unwrap(), ext_before);
        assert_eq!(std::fs::read(&source).unwrap(), legacy_bytes);
        assert_eq!(std::fs::read(&usb_database).unwrap(), database_before);
        assert!(!source.with_extension("EXT").exists(), "import never creates files on USB");
    }

    #[test]
    fn legacy_dat_grid_only_preserves_all_cue_identity_and_offset() {
        legacy_dat_grid_only_case(13000);
    }

    #[test]
    fn legacy_dat_grid_only_preserves_zero_tempo_and_noop() {
        legacy_dat_grid_only_case(0);
    }

    #[allow(clippy::too_many_lines, reason = "one generated import checks framing, ordered publication, repair and repeat on the same library")]
    fn modern_import_grid_framing_case(incomplete: bool, raw_tempo: u16) {
        let dir = tempfile::tempdir().unwrap();
        let (state, location, usb, _) = library_and_stick(dir.path());
        let id = rbl_db::fixture::track_id(0);
        let target = local_analysis_for(&state, &location, 1).remove(0);
        let relative = state.write(|writer| writer.analysis_data_path_for(&id)).unwrap();
        state
            .write(|writer| {
                writer.register_analysis(
                    &id,
                    &rbl_db::write::AnalysisRegistration {
                        bpm_x100: 10830,
                        key: None,
                        analysis_data_path: &relative,
                    },
                )?;
                writer.add_cue(&id, 0, 3000).map(|_| ())
            })
            .unwrap();
        let beats: Vec<_> = (0..4)
            .map(|index| rbl_anlz::Beat {
                beat_number: index + 1,
                tempo_x100: 10830,
                time_ms: u32::from(index) * 554,
            })
            .collect();
        let local_cues = [stick_cue(0, 3000, None, 1, "local memory")];
        let mut dat = rbl_anlz::AnlzBuilder::new();
        dat.path("/original.mp3")
            .copy_section(&rbl_anlz::write::vbr_table_zero_section())
            .beat_grid(&beats)
            .waveform_preview(b"PWAV", &[1, 2, 3, 4])
            .waveform_preview(b"PWV2", &[9, 10]);
        for section in rbl_anlz::cues::sections(&local_cues, false) {
            dat.copy_section(&section);
        }
        std::fs::write(&target, dat.finish()).unwrap();
        let mut ext = rbl_anlz::AnlzBuilder::new();
        ext.path("/original.mp3").waveform_scroll(b"PWV3", 1, &[5, 6, 7, 8]);
        for section in rbl_anlz::cues::sections(&local_cues, true) {
            ext.copy_section(&section);
        }
        std::fs::write(target.with_extension("EXT"), ext.finish()).unwrap();
        let before_dat = std::fs::read(&target).unwrap();
        let before_ext = std::fs::read(target.with_extension("EXT")).unwrap();
        let before_cues = state.write(|writer| writer.cue_rows(&id)).unwrap();
        let source = usb.join("PIONEER/USBANLZ/P000/00000001/ANLZ0000.DAT");
        let mut source_dat = rbl_anlz::Anlz::read(&source).unwrap();
        let grid = source_dat
            .sections
            .iter_mut()
            .find(|section| section.tag.0 == *b"PQTZ")
            .unwrap();
        grid.payload[2..4].copy_from_slice(&raw_tempo.to_be_bytes());
        if incomplete {
            grid.header[8..12].copy_from_slice(&2_u32.to_be_bytes());
        }
        std::fs::write(&source, source_dat.to_bytes()).unwrap();
        let source_bytes = std::fs::read(&source).unwrap();
        let source_ext = std::fs::read(source.with_extension("EXT")).unwrap();
        assert!(
            rbl_anlz::parse(&source_ext).unwrap().section(b"PCO2").is_some(),
            "exercise complete modern cue import, not DAT-only fallback"
        );
        assert_eq!(
            source_dat.beat_grid().unwrap().len(),
            1,
            "the tolerant reader still decodes one positive-tempo beat"
        );
        assert_eq!(strict_import_grid(&source_dat).is_err(), incomplete);
        let editor = crate::grid::GridEditor::at(state.backup_dir());
        let result = import(&state, &editor, &usb, true, false, false, false);
        let after_dat = rbl_anlz::Anlz::read(&target).unwrap();
        let after_ext = rbl_anlz::Anlz::read(&target.with_extension("EXT")).unwrap();
        let bpm = state
            .read_db(|db| {
                Ok(db
                    .connection()
                    .query_row("SELECT BPM FROM djmdContent WHERE ID=?1", [&id], |row| {
                        row.get::<_, u32>(0)
                    })?)
            })
            .unwrap();
        assert_eq!(bpm, 10830, "the positive collection BPM is independent of file framing");
        assert_eq!(
            after_dat.section(b"PWAV"),
            rbl_anlz::parse(&before_dat).unwrap().section(b"PWAV")
        );
        assert_eq!(
            after_ext.section(b"PWV3"),
            rbl_anlz::parse(&before_ext).unwrap().section(b"PWV3")
        );
        assert_eq!(std::fs::read(&source).unwrap(), source_bytes);
        assert_eq!(std::fs::read(source.with_extension("EXT")).unwrap(), source_ext);
        let tags = |file: &rbl_anlz::Anlz| {
            file.sections
                .iter()
                .map(|section| String::from_utf8_lossy(&section.tag.0).into_owned())
                .collect::<Vec<_>>()
        };
        if incomplete {
            assert!(
                result.is_err(),
                "a structurally incomplete modern USB grid must refuse before replacing a valid local grid"
            );
            assert_eq!(std::fs::read(&target).unwrap(), before_dat);
            assert_eq!(std::fs::read(target.with_extension("EXT")).unwrap(), before_ext);
            assert_eq!(state.write(|writer| writer.cue_rows(&id)).unwrap(), before_cues);
        } else {
            assert_eq!(result.unwrap().tracks, 1);
            assert_eq!(
                tags(&after_dat),
                tags(&rbl_anlz::parse(&before_dat).unwrap()),
                "normal cue import must retain canonical DAT section order"
            );
            assert_eq!(after_dat.section(b"PQTZ"), source_dat.section(b"PQTZ"));
            assert_ne!(state.write(|writer| writer.cue_rows(&id)).unwrap(), [] as [rbl_db::write::CueRow; 0]);
            // Simulate a file emitted by the previous importer: only PQTZ is
            // out of place; all current source cues/grid already match.
            let mut reordered = after_dat.clone();
            let slot = reordered
                .sections
                .iter()
                .position(|section| section.tag.0 == *b"PQTZ")
                .unwrap();
            let grid = reordered.sections.remove(slot);
            reordered.sections.push(grid);
            std::fs::write(&target, reordered.to_bytes()).unwrap();
            let mut timing = rbl_anlz::write::extended_grid_empty_section();
            timing.header[12..16].copy_from_slice(&((1_u32 << 16) | 0x2a4e).to_be_bytes());
            timing.header[16..20].copy_from_slice(&250_u32.to_be_bytes());
            timing.header[20..24].copy_from_slice(&((1_u32 << 16) | 0x2a4e).to_be_bytes());
            timing.header[24..28].copy_from_slice(&250_u32.to_be_bytes());
            timing.header[28..32].copy_from_slice(&1_u32.to_be_bytes());
            timing.header[32..36].copy_from_slice(&13051_u32.to_be_bytes());
            timing.payload = vec![0x12, 0x34];
            let mut preserved_ext = after_ext;
            preserved_ext.sections.push(timing);
            let preserved_ext = preserved_ext.to_bytes();
            std::fs::write(target.with_extension("EXT"), &preserved_ext).unwrap();
            let rows = state.write(|writer| writer.cue_rows(&id)).unwrap();
            assert_eq!(
                import(&state, &editor, &usb, true, false, false, false).unwrap().tracks,
                1,
                "an otherwise identical reimport repairs the prior DAT order"
            );
            assert_eq!(std::fs::read(&target).unwrap(), after_dat.to_bytes());
            assert_eq!(
                std::fs::read(target.with_extension("EXT")).unwrap(),
                preserved_ext,
                "physical DAT order repair must retain existing extended timing"
            );
            assert_eq!(state.write(|writer| writer.cue_rows(&id)).unwrap(), rows);
            assert_eq!(
                state
                    .read_db(|db| Ok(db.connection().query_row(
                        "SELECT BPM FROM djmdContent WHERE ID=?1",
                        [&id],
                        |row| row.get::<_, u32>(0),
                    )?))
                    .unwrap(),
                10830
            );
            let repeated = import(&state, &editor, &usb, true, false, false, false).unwrap();
            assert_eq!((repeated.tracks, repeated.unchanged), (0, 1));
            assert_eq!(std::fs::read(&target).unwrap(), after_dat.to_bytes());
            assert_eq!(std::fs::read(target.with_extension("EXT")).unwrap(), preserved_ext);
            assert_eq!(std::fs::read(&source).unwrap(), source_bytes);
            assert_eq!(std::fs::read(source.with_extension("EXT")).unwrap(), source_ext);
        }
    }

    #[test]
    fn modern_import_grid_framing_rejects_incomplete_source_before_writes() {
        modern_import_grid_framing_case(true, 10830);
    }

    #[test]
    fn modern_import_grid_framing_complete_source_still_imports() {
        modern_import_grid_framing_case(false, 10830);
    }

    #[test]
    fn modern_import_grid_framing_zero_tempo_order_repair_keeps_collection_bpm() {
        modern_import_grid_framing_case(false, 0);
    }
    fn legacy_grid_control_fixture(dir: &Path) -> (AppState, rbl_db::LibraryLocation, PathBuf, PathBuf, PathBuf) {
        let (state, location, usb, _) = library_and_stick(dir);
        let local = local_analysis_for(&state, &location, 1).remove(0);
        let editor = crate::grid::GridEditor::at(state.backup_dir());
        assert_eq!(import(&state, &editor, &usb, true, false, false, false).unwrap().tracks, 1);
        let source = usb.join("PIONEER/USBANLZ/P000/00000001/ANLZ0000.DAT");
        std::fs::remove_file(source.with_extension("EXT")).unwrap();
        let mut dat = rbl_anlz::Anlz::read(&source).unwrap();
        dat.sections.iter_mut().find(|section| section.tag.0 == *b"PQTZ").unwrap().payload[4..8].copy_from_slice(&500_u32.to_be_bytes());
        std::fs::write(&source, dat.to_bytes()).unwrap();
        (state, location, usb, local, source)
    }

    #[test]
    fn legacy_dat_grid_only_rejects_malformed_before_any_track_writes() {
        for malformed in ["short_header", "short_payload", "trailing_payload", "duplicate", "overflow"] {
            let dir = tempfile::tempdir().unwrap();
            let (state, location, usb, _) = library_and_stick(dir.path());
            let local = local_analysis_for(&state, &location, 2);
            let before: Vec<_> = local.iter().map(|path| std::fs::read(path).unwrap()).collect();
            let editor = crate::grid::GridEditor::at(state.backup_dir());
            for index in 0..2 {
                editor.seed_history(&rbl_db::fixture::track_id(index));
                let source = usb.join(format!("PIONEER/USBANLZ/P000/{:08X}/ANLZ0000.DAT", index + 1));
                std::fs::remove_file(source.with_extension("EXT")).unwrap();
                if index == 0 { continue; }
                let mut dat = rbl_anlz::Anlz::read(&source).unwrap();
                let grid = dat.sections.iter_mut().find(|section| section.tag.0 == *b"PQTZ").unwrap();
                match malformed {
                    "short_header" => grid.header.truncate(8),
                    "short_payload" => { grid.payload.pop(); },
                    "trailing_payload" => grid.payload.push(0),
                    "overflow" => grid.header[8..12].copy_from_slice(&u32::MAX.to_be_bytes()),
                    "duplicate" => { let duplicate = grid.clone(); dat.sections.push(duplicate); },
                    _ => unreachable!(),
                }
                std::fs::write(source, dat.to_bytes()).unwrap();
            }
            let cues: Vec<_> = (0..2).map(|index| state.write(|w| w.cue_rows(&rbl_db::fixture::track_id(index))).unwrap()).collect();
            assert!(import(&state, &editor, &usb, true, false, false, false).is_err(), "{malformed}");
            for (index, (path, bytes)) in local.iter().zip(&before).enumerate() {
                assert_eq!(&std::fs::read(path).unwrap(), bytes, "{malformed}");
                assert_eq!(state.write(|w| w.cue_rows(&rbl_db::fixture::track_id(index))).unwrap(), cues[index]);
                assert!(editor.has_history(&rbl_db::fixture::track_id(index)));
            }
            assert!(!state.backup_dir().join("analysis-journal").exists_with_entries());
        }
    }

    #[test]
    fn legacy_dat_grid_only_does_not_fallback_for_present_companions() {
        for companion in ["empty", "corrupt", "directory", "mixed_case", "ambiguous"] {
            let dir = tempfile::tempdir().unwrap();
            let (state, _location, usb, local, source) = legacy_grid_control_fixture(dir.path());
            let before = std::fs::read(&local).unwrap();
            let id = rbl_db::fixture::track_id(0);
            let cues = state.write(|w| w.cue_rows(&id)).unwrap();
            let mut empty = rbl_anlz::AnlzBuilder::new(); empty.path("/usb.mp3");
            let empty = empty.finish();
            match companion {
                "empty" => std::fs::write(source.with_extension("EXT"), &empty).unwrap(),
                "corrupt" => std::fs::write(source.with_extension("EXT"), b"invalid").unwrap(),
                "directory" => std::fs::create_dir(source.with_extension("EXT")).unwrap(),
                "mixed_case" => std::fs::write(source.with_extension("eXt"), &empty).unwrap(),
                "ambiguous" => {
                    std::fs::write(source.with_extension("ext"), &empty).unwrap();
                    std::fs::write(source.with_extension("eXt"), &empty).unwrap();
                },
                _ => unreachable!(),
            }
            let editor = crate::grid::GridEditor::at(state.backup_dir());
            let result = import(&state, &editor, &usb, true, false, false, false);
            assert!(result.is_err() || result.unwrap().tracks == 0, "{companion}");
            assert_eq!(std::fs::read(&local).unwrap(), before, "{companion}");
            assert_eq!(state.write(|w| w.cue_rows(&id)).unwrap(), cues);
        }
    }

    #[cfg(unix)]
    #[test]
    fn legacy_dat_grid_only_refuses_named_dat_alias_and_dangling_ext() {
        for alias in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let (state, _location, usb, local, source) = legacy_grid_control_fixture(dir.path());
            let before = std::fs::read(&local).unwrap();
            if alias {
                let actual = source.with_file_name("actual.DAT");
                std::fs::rename(&source, &actual).unwrap();
                std::os::unix::fs::symlink(&actual, &source).unwrap();
                // The named source has a present companion, but the actual
                // target's sibling is absent. Never infer absence from it.
                let mut ext = rbl_anlz::AnlzBuilder::new(); ext.path("/usb.mp3");
                std::fs::write(source.with_extension("EXT"), ext.finish()).unwrap();
            } else {
                std::os::unix::fs::symlink(source.with_file_name("missing.EXT"), source.with_extension("EXT")).unwrap();
            }
            let editor = crate::grid::GridEditor::at(state.backup_dir());
            let result = import(&state, &editor, &usb, true, false, false, false);
            assert!(result.is_err() || result.unwrap().tracks == 0);
            assert_eq!(std::fs::read(&local).unwrap(), before);
        }
    }

    #[test]
    fn legacy_dat_grid_only_honors_lock_and_exact_stage_budget() {
        let dir = tempfile::tempdir().unwrap();
        let (state, _location, usb, local, _source) = legacy_grid_control_fixture(dir.path());
        let id = rbl_db::fixture::track_id(0);
        let before = std::fs::read(&local).unwrap();
        let cues = state.write(|w| w.cue_rows(&id)).unwrap();
        let editor = crate::grid::GridEditor::at(state.backup_dir());
        state.write(|w| w.set_analysis_lock(&id, true)).unwrap();
        assert_eq!(import(&state, &editor, &usb, true, false, false, false).unwrap().tracks, 0);
        assert_eq!(std::fs::read(&local).unwrap(), before);
        state.write(|w| w.set_analysis_lock(&id, false)).unwrap();
        assert!(import_with_stage_budget(&state, &editor, &usb, true, false, false, false, 0).is_err());
        assert_eq!(std::fs::read(&local).unwrap(), before);
        assert_eq!(state.write(|w| w.cue_rows(&id)).unwrap(), cues);
        // No cue input or unchanged EXT is charged or staged for grid-only.
        let report = import_with_stage_budget(&state, &editor, &usb, true, false, false, false, before.len() as u64).unwrap();
        assert_eq!(report.tracks, 1);
        assert_eq!(state.write(|w| w.cue_rows(&id)).unwrap(), cues);
    }

    #[test]
    fn legacy_dat_grid_only_cue_changes_are_not_import_authority() {
        let dir = tempfile::tempdir().unwrap();
        let (state, _location, usb, local, source) = legacy_grid_control_fixture(dir.path());
        let editor = crate::grid::GridEditor::at(state.backup_dir());
        assert_eq!(import(&state, &editor, &usb, true, false, false, false).unwrap().tracks, 1);
        let before = std::fs::read(&local).unwrap();
        let id = rbl_db::fixture::track_id(0);
        let cues = state.write(|w| w.cue_rows(&id)).unwrap();
        let grid = rbl_anlz::Anlz::read(&source).unwrap().section(b"PQTZ").unwrap().clone();
        write_stick(source.parent().unwrap(), &[stick_cue(1, 9000, None, 0, "ignored legacy cue")]);
        std::fs::remove_file(source.with_extension("EXT")).unwrap();
        let mut changed = rbl_anlz::Anlz::read(&source).unwrap();
        changed.sections.retain(|section| section.tag.0 != *b"PQTZ"); changed.sections.push(grid);
        std::fs::write(source, changed.to_bytes()).unwrap();
        editor.seed_history(&id);
        let report = import(&state, &editor, &usb, true, false, false, false).unwrap();
        assert_eq!((report.tracks, report.unchanged), (0, 1));
        assert_eq!(std::fs::read(&local).unwrap(), before);
        assert_eq!(state.write(|w| w.cue_rows(&id)).unwrap(), cues);
        assert!(editor.has_history(&id));
    }

    #[test]
    fn legacy_dat_grid_only_yields_shared_analysis_to_complete_cue_import() {
        for grid_index in 0..2 {
            for grid_changed in [false, true] {
              for full_changed in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                let (state, location, usb, _) = library_and_stick(dir.path());
                let local = local_analysis_for(&state, &location, 2);
                let editor = crate::grid::GridEditor::at(state.backup_dir());
                assert_eq!(import(&state, &editor, &usb, true, false, false, false).unwrap().tracks, 2);
                let relative = state.write(|w| w.analysis_data_path_for(&rbl_db::fixture::track_id(0))).unwrap();
                state.write(|w| w.register_analysis(&rbl_db::fixture::track_id(1), &rbl_db::write::AnalysisRegistration {
                    bpm_x100: 12800, key: None, analysis_data_path: &relative,
                })).unwrap();
                let full_index = 1 - grid_index;
                let full_source = usb.join(format!("PIONEER/USBANLZ/P000/{:08X}/ANLZ0000.DAT", full_index + 1));
                if full_changed { write_stick(full_source.parent().unwrap(), &[stick_cue(1, 9000, None, 22, "full import wins")]); }
                let mut full = rbl_anlz::Anlz::read(&full_source).unwrap();
                if full_changed {
                    full.sections.iter_mut().find(|section| section.tag.0 == *b"PQTZ").unwrap().payload[4..8].copy_from_slice(&700_u32.to_be_bytes());
                    std::fs::write(&full_source, full.to_bytes()).unwrap();
                }
                let grid_source = usb.join(format!("PIONEER/USBANLZ/P000/{:08X}/ANLZ0000.DAT", grid_index + 1));
                std::fs::remove_file(grid_source.with_extension("EXT")).unwrap();
                if grid_changed {
                    let mut grid = rbl_anlz::Anlz::read(&grid_source).unwrap();
                    grid.sections.iter_mut().find(|section| section.tag.0 == *b"PQTZ").unwrap().payload[4..8].copy_from_slice(&500_u32.to_be_bytes());
                    std::fs::write(grid_source, grid.to_bytes()).unwrap();
                }
                let grid_id = rbl_db::fixture::track_id(grid_index);
                let cues = state.write(|w| w.cue_rows(&grid_id)).unwrap();
                let report = import(&state, &editor, &usb, true, false, false, false).unwrap();
                assert_eq!((report.tracks, report.unchanged), if full_changed { (1, 0) } else { (0, 1) });
                let shared = rbl_anlz::Anlz::read(&local[0]).unwrap();
                assert_eq!(shared.section(b"PQTZ"), full.section(b"PQTZ"));
                assert_eq!(shared.legacy_cue_entries(), full.legacy_cue_entries());
                assert_eq!(state.write(|w| w.cue_rows(&grid_id)).unwrap(), cues);
                assert!(report.warnings.iter().any(|warning| warning.contains("conflicting import owners")));
              }
            }
        }
    }

    #[test]
    fn legacy_dat_grid_only_shared_grids_require_identical_images() {
        for identical in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let (state, location, usb, _) = library_and_stick(dir.path());
            let local = local_analysis_for(&state, &location, 2);
            let editor = crate::grid::GridEditor::at(state.backup_dir());
            assert_eq!(import(&state, &editor, &usb, true, false, false, false).unwrap().tracks, 2);
            let relative = state.write(|w| w.analysis_data_path_for(&rbl_db::fixture::track_id(0))).unwrap();
            state.write(|w| w.register_analysis(&rbl_db::fixture::track_id(1), &rbl_db::write::AnalysisRegistration {
                bpm_x100: 12800, key: None, analysis_data_path: &relative,
            })).unwrap();
            let before = std::fs::read(&local[0]).unwrap();
            let cues: Vec<_> = (0..2).map(|index| state.write(|w| w.cue_rows(&rbl_db::fixture::track_id(index))).unwrap()).collect();
            for index in 0..2 {
                let id = rbl_db::fixture::track_id(index);
                state.write(|w| w.save_grid_revision(&id, 12800)).unwrap();
                let source = usb.join(format!("PIONEER/USBANLZ/P000/{:08X}/ANLZ0000.DAT", index + 1));
                std::fs::remove_file(source.with_extension("EXT")).unwrap();
                let mut dat = rbl_anlz::Anlz::read(&source).unwrap();
                let grid = dat.sections.iter_mut().find(|section| section.tag.0 == *b"PQTZ").unwrap();
                grid.payload[2..4].copy_from_slice(&13000_u16.to_be_bytes());
                grid.payload[4..8].copy_from_slice(&(if identical || index == 0 {500_u32} else {700_u32}).to_be_bytes());
                std::fs::write(source, dat.to_bytes()).unwrap();
            }
            let revisions = || (0..2).map(|index| state.read_db(|db| Ok(db.connection().query_row(
                "SELECT CAST(AnalysisUpdated AS TEXT) FROM djmdContent WHERE ID=?1", [rbl_db::fixture::track_id(index)], |row| row.get::<_,String>(0))?)).unwrap()).collect::<Vec<_>>();
            let before_revisions = revisions();
            let report = import(&state, &editor, &usb, true, false, false, false).unwrap();
            assert_eq!(report.tracks, if identical {2} else {0});
            for (before, after) in before_revisions.iter().zip(revisions()) {
                if identical { assert_eq!(after.encode_utf16().next(), before.encode_utf16().next().map(|unit| unit.wrapping_add(1))); }
                else { assert_eq!(*before, after); }
            }
            for (index, cue) in cues.iter().enumerate() {
                let id = rbl_db::fixture::track_id(index);
                assert_eq!(state.write(|w| w.cue_rows(&id)).unwrap(), *cue);
                let bpm = state.read_db(|db| Ok(db.connection().query_row("SELECT BPM FROM djmdContent WHERE ID=?1", [&id], |row| row.get::<_,u32>(0))?)).unwrap();
                assert_eq!(bpm, if identical {13000} else {12800});
            }
            if !identical { assert_eq!(std::fs::read(&local[0]).unwrap(), before); }
            assert!(!state.backup_dir().join("analysis-journal").exists_with_entries());
        }
    }

    #[test]
    fn legacy_dat_grid_only_journal_preserves_committed_prefix_and_nonnumeric_revision() {
        let dir = tempfile::tempdir().unwrap();
        let (state, location, _, _) = library_and_stick(dir.path());
        let local = local_analysis_for(&state, &location, 2);
        let editor = crate::grid::GridEditor::at(state.backup_dir());
        let _gate = state.edit_gate.lock();
        let _files = state.analysis_write.lock();
        let id = rbl_db::fixture::track_id(0);
        // The actual grid writer advances the UTF-16 revision, not a decimal.
        for _ in 0..12 { state.write(|w| w.save_grid_revision(&id, 12000)).unwrap(); }
        let revision = || state.read_db(|db| Ok(db.connection().query_row("SELECT AnalysisUpdated FROM djmdContent WHERE ID=?1", [&id], |row| row.get::<_,String>(0))?)).unwrap();
        let previous_revision = revision();
        assert!(previous_revision.parse::<u64>().is_err());
        let relative = state.write(|w| w.analysis_data_path_for(&id)).unwrap();
        let old = std::fs::read(&local[0]).unwrap();
        let mut image = rbl_anlz::Anlz::read(&local[0]).unwrap();
        image.sections.iter_mut().find(|section| section.tag.0 == *b"PQTZ").unwrap().payload[4..8].copy_from_slice(&500_u32.to_be_bytes());
        image.sections.iter_mut().find(|section| section.tag.0 == *b"PQTZ").unwrap().payload[2..4].copy_from_slice(&0_u16.to_be_bytes());
        let image = image.to_bytes();
        editor.seed_history(&id);
        let first = PendingCues { id: id.clone(), identities: Vec::new(), payload: ImportPayload::Grid { expected_bpm: 12000, analysis_path: relative.clone() }, bpm: 12000, files: vec![(local[0].clone(), image.clone())] };
        let bad = rbl_anlz::CueEntry { hot_cue: 1, active_loop: false, kind: 2, time_ms: 3000, loop_time_ms: 1000, color_id: 0, comment: None, color_code: None, rgb: None };
        let second_id = rbl_db::fixture::track_id(1);
        state.write(|w| w.register_analysis(&second_id, &rbl_db::write::AnalysisRegistration {
            bpm_x100: 12000, key: None, analysis_data_path: &relative,
        })).unwrap();
        editor.seed_history(&second_id);
        let second_before = std::fs::read(&local[1]).unwrap();
        let second = PendingCues { id: second_id.clone(), identities: Vec::new(), payload: ImportPayload::Cues(vec![bad]), bpm: 12800, files: vec![(local[1].clone(), image.clone())] };
        let mut report = ImportReport::default();
        assert!(apply_cue_batch(&state, &editor, &location, vec![first, second], &mut report).is_err());
        assert_ne!(old, image);
        assert_eq!(std::fs::read(&local[0]).unwrap(), image);
        assert_ne!(revision(), previous_revision);
        assert_eq!(rbl_anlz::Anlz::read(&local[0]).unwrap().beat_grid().unwrap()[0].tempo_x100, 0);
        assert_eq!(state.read_db(|db| Ok(db.connection().query_row("SELECT BPM FROM djmdContent WHERE ID=?1", [&id], |row| row.get::<_,u32>(0))?)).unwrap(), 12000);
        assert_eq!(std::fs::read(&local[1]).unwrap(), second_before);
        assert!(!editor.has_history(&id));
        assert!(editor.has_history(&second_id));
        // The next same-file journal starts AFTER the committed first image,
        // as the caller's shared-path batch flush does. A rejected row CAS
        // restores that image, never the earlier generation.
        let conflict = PendingCues { id: second_id, identities: Vec::new(), payload: ImportPayload::Grid { expected_bpm: 9999, analysis_path: relative }, bpm: 12000, files: vec![(local[0].clone(), image.clone())] };
        assert!(apply_cue_batch(&state, &editor, &location, vec![conflict], &mut report).is_err());
        assert_eq!(std::fs::read(&local[0]).unwrap(), image);
        assert!(!state.backup_dir().join("analysis-journal").exists_with_entries());
    }

    #[test]
    fn legacy_dat_grid_only_clears_stale_pqt2_but_keeps_noop_extended_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let (state, _location, usb, local, _source) = legacy_grid_control_fixture(dir.path());
        let mut builder = rbl_anlz::AnlzBuilder::new();
        builder.path("/original.mp3").extended_grid_empty();
        for section in rbl_anlz::cues::sections(&[stick_cue(1, 2000, None, 22, "")], true) { builder.copy_section(&section); }
        let mut ext = rbl_anlz::parse(&builder.finish()).unwrap();
        let grid = ext.sections.iter_mut().find(|section| section.tag.0 == *b"PQT2").unwrap();
        grid.header[12..16].copy_from_slice(&((1_u32 << 16) | 0x3200).to_be_bytes());
        grid.header[16..20].copy_from_slice(&250_u32.to_be_bytes());
        grid.header[20..24].copy_from_slice(&((1_u32 << 16) | 0x3200).to_be_bytes());
        grid.header[24..28].copy_from_slice(&250_u32.to_be_bytes());
        grid.header[28..32].copy_from_slice(&1_u32.to_be_bytes());
        grid.header[32..36].copy_from_slice(&13051_u32.to_be_bytes());
        grid.payload = vec![0x12, 0x34];
        let before = ext.to_bytes();
        let path = local.with_extension("EXT");
        std::fs::write(&path, &before).unwrap();
        let id = rbl_db::fixture::track_id(0);
        let cues = state.write(|w| w.cue_rows(&id)).unwrap();
        let editor = crate::grid::GridEditor::at(state.backup_dir());
        assert_eq!(import(&state, &editor, &usb, true, false, false, false).unwrap().tracks, 1);
        let cleared = rbl_anlz::Anlz::read(&path).unwrap();
        assert_eq!(cleared.section(b"PQT2").unwrap().payload, [] as [u8; 0]);
        assert_eq!(cleared.sections.iter().filter(|section| section.is_cue_list()).collect::<Vec<_>>(),
            ext.sections.iter().filter(|section| section.is_cue_list()).collect::<Vec<_>>());
        assert_eq!(state.write(|w| w.cue_rows(&id)).unwrap(), cues);
        // An identical DAT never empties a subsequently present local PQT2.
        std::fs::write(&path, &before).unwrap();
        editor.seed_history(&id);
        let repeat = import(&state, &editor, &usb, true, false, false, false).unwrap();
        assert_eq!((repeat.tracks, repeat.unchanged), (0, 1));
        assert_eq!(std::fs::read(path).unwrap(), before);
        assert!(editor.has_history(&id));
    }

    #[cfg(unix)]
    #[test]
    fn legacy_dat_grid_only_shared_filesystem_alias_preserves_complete_cues() {
        use std::os::unix::fs::MetadataExt;
        for directory in [true, false] {
            for grid_index in [1, 0] {
              for full_changed in [true, false] {
                let dir = tempfile::tempdir().unwrap();
                let (state, location, usb, _) = library_and_stick(dir.path());
                let local = local_analysis_for(&state, &location, 2);
                let editor = crate::grid::GridEditor::at(state.backup_dir());
                assert_eq!(import(&state, &editor, &usb, true, false, false, false).unwrap().tracks, 2);
                let real_relative = state.write(|w| w.analysis_data_path_for(&rbl_db::fixture::track_id(0))).unwrap();
                let alias = if directory {
                    let parent = location.share_root.join("grid-alias");
                    std::os::unix::fs::symlink(local[0].parent().unwrap(), &parent).unwrap();
                    parent.join(local[0].file_name().unwrap())
                } else {
                    let alias = location.share_root.join("grid-alias.DAT");
                    std::fs::hard_link(&local[0], &alias).unwrap();
                    alias
                };
                assert!(alias.canonicalize().unwrap().starts_with(location.share_root.canonicalize().unwrap()));
                let real_meta = std::fs::metadata(&local[0]).unwrap();
                let alias_meta = std::fs::metadata(&alias).unwrap();
                assert_eq!((real_meta.dev(), real_meta.ino()), (alias_meta.dev(), alias_meta.ino()));
                let alias_relative = format!("/{}", alias.strip_prefix(&location.share_root).unwrap().to_str().unwrap());
                let full_index = 1 - grid_index;
                for (index, relative) in [(full_index, &real_relative), (grid_index, &alias_relative)] {
                    state.write(|w| w.register_analysis(&rbl_db::fixture::track_id(index), &rbl_db::write::AnalysisRegistration {
                        bpm_x100: 12800, key: None, analysis_data_path: relative,
                    })).unwrap();
                }
                let full_source = usb.join(format!("PIONEER/USBANLZ/P000/{:08X}/ANLZ0000.DAT", full_index + 1));
                if full_changed { write_stick(full_source.parent().unwrap(), &[stick_cue(1, 9000, None, 22, "complete cue owner")]); }
                let full = rbl_anlz::Anlz::read(&full_source).unwrap();
                let grid_source = usb.join(format!("PIONEER/USBANLZ/P000/{:08X}/ANLZ0000.DAT", grid_index + 1));
                std::fs::remove_file(grid_source.with_extension("EXT")).unwrap();
                let mut grid = rbl_anlz::Anlz::read(&grid_source).unwrap();
                grid.sections.iter_mut().find(|section| section.tag.0 == *b"PQTZ").unwrap().payload[4..8].copy_from_slice(&500_u32.to_be_bytes());
                std::fs::write(grid_source, grid.to_bytes()).unwrap();
                let report = import(&state, &editor, &usb, true, false, false, false).unwrap();
                let actual = rbl_anlz::Anlz::read(&local[0]).unwrap();
                assert_eq!(actual.legacy_cue_entries(), full.legacy_cue_entries(), "complete cues must win through filesystem aliases");
                assert_eq!(actual.section(b"PQTZ"), full.section(b"PQTZ"));
                assert_eq!((report.tracks, report.unchanged), if full_changed { (1, 0) } else { (0, 1) });
              }
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn legacy_dat_grid_only_alias_journals_flush_with_captured_preflight_identity() {
        let dir = tempfile::tempdir().unwrap();
        let (state, location, _, _) = library_and_stick(dir.path());
        let local = local_analysis_for(&state, &location, 2);
        let first_id = rbl_db::fixture::track_id(0);
        let second_id = rbl_db::fixture::track_id(1);
        let relative = state.write(|w| w.analysis_data_path_for(&first_id)).unwrap();
        let alias_parent = location.share_root.join("journal-alias");
        std::os::unix::fs::symlink(local[0].parent().unwrap(), &alias_parent).unwrap();
        let alias = alias_parent.join(local[0].file_name().unwrap());
        let alias_relative = format!("/{}", alias.strip_prefix(&location.share_root).unwrap().to_str().unwrap());
        state.write(|w| w.register_analysis(&second_id, &rbl_db::write::AnalysisRegistration {
            bpm_x100: 12000, key: None, analysis_data_path: &alias_relative,
        })).unwrap();
        let key = |path: &Path| rbl_core::durable::file_identity(&std::fs::File::open(path).unwrap()).unwrap();
        let first_key = key(&local[0]);
        let second_key = key(&alias);
        assert_eq!(first_key, second_key);
        let mut image = rbl_anlz::Anlz::read(&local[0]).unwrap();
        image.sections.iter_mut().find(|section| section.tag.0 == *b"PQTZ").unwrap().payload[4..8].copy_from_slice(&500_u32.to_be_bytes());
        let image = image.to_bytes();
        let editor = crate::grid::GridEditor::at(state.backup_dir());
        editor.seed_history(&first_id); editor.seed_history(&second_id);
        let _gate = state.edit_gate.lock();
        let _files = state.analysis_write.lock();
        let first = PendingCues { id: first_id.clone(), identities: vec![first_key], payload: ImportPayload::Grid { expected_bpm: 12000, analysis_path: relative }, bpm: 12000, files: vec![(local[0].clone(), image.clone())] };
        let second = PendingCues { id: second_id.clone(), identities: vec![second_key], payload: ImportPayload::Grid { expected_bpm: 9999, analysis_path: alias_relative }, bpm: 12000, files: vec![(alias.clone(), image.clone())] };
        let mut pending = vec![first];
        assert!(pending_overlaps(&pending, &second), "actual alias identity must trigger the caller's batch flush");
        let mut report = ImportReport::default();
        apply_cue_batch(&state, &editor, &location, std::mem::take(&mut pending), &mut report).unwrap();
        assert_ne!(key(&alias), first_key, "atomic replacement changes the inode; do not recapture it for grouping");
        pending.push(second);
        assert!(apply_cue_batch(&state, &editor, &location, pending, &mut report).is_err());
        assert_eq!(std::fs::read(&local[0]).unwrap(), image);
        assert_eq!(std::fs::read(&alias).unwrap(), image);
        assert_eq!(report.changed, vec![first_id.clone()]);
        assert!(!editor.has_history(&first_id));
        assert!(editor.has_history(&second_id));
        assert!(!state.backup_dir().join("analysis-journal").exists_with_entries());
    }

}
