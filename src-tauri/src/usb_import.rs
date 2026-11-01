//! USB-to-library reads. Never infer track identity from a title or USB row id.
use std::{collections::{HashMap, HashSet}, path::{Path, PathBuf}, sync::Arc};
use serde::Serialize;
use tauri::{State, Manager};
use crate::{commands::{announce_history, blocking, reload, write_error, wrote_outside_history}, dto::{DeviceLibraryTreeDto, DevicePlaylistNodeDto}, error::{AppError, AppResult, ErrorKind}, state::AppState};

mod ratings;

fn err(e: impl std::fmt::Display) -> AppError {
    let detail = format!("USB import: {e}");
    AppError::new(ErrorKind::Internal, detail.clone()).with_detail(detail)
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
    /// Tracks whose cues and grid on the stick already match the library.
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
        // Edits apply in batches, one writer per batch. A writer per track
        // meant two key derivations for each (the writer and the journal's
        // read), about 60 ms apiece, so a stick of thousands of changed
        // tracks held the library for many minutes (#794).
        let mut pending: Vec<PendingCues> = Vec::new();
        let mut pending_bytes = 0usize;
        for (id, analysis) in tracks.values() {
            let planned = match plan_cue_import(state, editor, root, &location, id, analysis, &mut report) {
                Ok(planned) => planned,
                Err(e) => { apply_cue_batch(state, editor, &location, std::mem::take(&mut pending), &mut report)?; return Err(e); }
            };
            let Some(planned) = planned else { continue };
            // Two tracks naming one analysis file must not share a batch: the
            // second's journal would capture the file before the first's edit.
            if pending.iter().any(|p| p.files.iter().any(|(path, _)| planned.files.iter().any(|(other, _)| other == path))) {
                apply_cue_batch(state, editor, &location, std::mem::take(&mut pending), &mut report)?;
                pending_bytes = 0;
            }
            pending_bytes += planned.files.iter().map(|(_, bytes)| bytes.len()).sum::<usize>();
            pending.push(planned);
            if pending.len() >= CUE_BATCH_TRACKS || pending_bytes >= CUE_BATCH_BYTES {
                apply_cue_batch(state, editor, &location, std::mem::take(&mut pending), &mut report)?;
                pending_bytes = 0;
            }
        }
        apply_cue_batch(state, editor, &location, pending, &mut report)?;
    }
    if history {
        let snapshot = rbl_export::snapshot::Snapshot::read(root)
            .map_err(|e| err(format!("Could not read play history: {e}")))?;
        for session in snapshot.history.iter().filter(|h| !h.folder) {
            let matched: Vec<String> = session.tracks.iter().filter_map(|id| tracks.get(id).map(|t| t.0.clone())).collect();
            if matched.len() != session.tracks.len() {
                report.skipped += session.tracks.len() - matched.len();
                report.warnings.push(format!("History '{}' contains tracks that could not be matched to this library; it was left on the USB.", session.name));
                continue;
            }
            if matched.is_empty() { continue; }
            // A session keeps one identity as more tracks are appended. The
            // writer rejects a changed prefix instead of silently duplicating it.
            let key = format!("{}:{}:{}", rbl_devices::volume_id(root), session.id, session.name);
            let hash = rbl_export::manifest::hash(key.as_bytes());
            let uuid = format!("00000000-0000-4000-8000-{:012x}", hash & 0xffff_ffff_ffff);
            report.histories += state.write(|w| w.import_usb_history(&format!("{} (USB {:06x})", session.name, hash & 0x00ff_ffff), &uuid, &matched)).map_err(write_error)?;
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

/// One track's cues and the analysis files that carry them, ready to apply.
struct PendingCues {
    id: String,
    entries: Vec<rbl_anlz::CueEntry>,
    bpm: u32,
    files: Vec<(std::path::PathBuf, Vec<u8>)>,
}

/// Reads one track's USB analysis and works out what importing it changes,
/// counting it as skipped or unchanged when it changes nothing. Writes
/// nothing: `None` is a track with nothing to apply.
fn plan_cue_import(
    state: &AppState, editor: &crate::grid::GridEditor, root: &Path, location: &rbl_db::LibraryLocation,
    id: &str, analysis: &str, report: &mut ImportReport,
) -> AppResult<Option<PendingCues>> {
    // Read on the open handle. Opening the library, for the lock
    // bit here and the analysis path below, derives its key again:
    // about 155 ms each, two per track, so a stick of 20,000 tracks
    // held the library, and every analysis and sync, for well over
    // an hour (#284).
    if editor.is_locked(id) || state.read_db(|db| crate::grid::locked_in(db, id)).map_err(write_error)? { report.skipped += 1; return Ok(None); }
    if analysis.is_empty() { report.skipped += 1; return Ok(None); }
    let source_path = root.join(analysis.trim_start_matches('/'));
    match std::fs::metadata(&source_path) {
        Ok(_) => {},
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            report.skipped += 1;
            report.warnings.push(format!("Track {id}: USB analysis file is missing ({analysis}); cues and grid were left unchanged."));
            return Ok(None);
        }
        Err(e) => return Err(err(e)),
    }
    let source = within(root, analysis)?;
    let source_dat = rbl_anlz::Anlz::read(&source).map_err(err)?;
    // `source` is canonical and `root` need not be (a symlinked mount),
    // so the `.EXT` is found from the relative path, not by stripping.
    let ext_relative = Path::new(analysis).with_extension("EXT");
    let source_cues = if source.with_extension("EXT").exists() { rbl_anlz::Anlz::read(&within(root, &ext_relative.to_string_lossy())?).map_err(err)? } else { source_dat.clone() };
    if source_cues.section(b"PCO2").is_none() { report.skipped += 1; return Ok(None); }
    let entries = source_cues.cue_entries();
    let beats = source_dat.beat_grid().unwrap_or_default();
    let previous_bpm = state.read_db(|db| Ok(db.connection().query_row("SELECT COALESCE(BPM,0) FROM djmdContent WHERE ID=?1 AND rb_local_deleted=0", [id], |r| r.get::<_,u32>(0))?)).map_err(write_error)?;
    let bpm = beats.first().map_or(previous_bpm, |b| u32::from(b.tempo_x100));
    let bpm_changed = bpm != previous_bpm;
    let relative = state.read_db(|db| rbl_db::write::analysis_data_path(db.connection(), id)).map_err(write_error)?;
    let target = rbl_anlz::resolve(&location.share_root, &relative);
    let mut files = Vec::new();
    // A stick that carries the cues and grid the library already has
    // is not a change: rewriting it would bump the track's analysis,
    // drop its grid undo history and reload the whole library for
    // nothing, and the next sync would copy it all back (#134).
    let mut differs = false;
    for extension in ["DAT", "EXT"] {
        let target = target.with_extension(extension);
        if !target.exists() { continue; }
        let mut dest = rbl_anlz::Anlz::read(&target).map_err(err)?;
        let src = if extension == "DAT" { &source_dat } else { &source_cues };
        differs |= !dest.sections.iter().filter(|s| s.is_cue_list()).eq(src.sections.iter().filter(|s| s.is_cue_list()));
        if extension == "DAT" && !beats.is_empty() {
            differs |= !dest.sections.iter().filter(|s| s.as_beat_grid().is_some()).eq(source_dat.section(b"PQTZ"));
        }
        dest.sections.retain(|s| !s.is_cue_list());
        dest.sections.extend(src.sections.iter().filter(|s| s.is_cue_list()).cloned());
        if extension == "DAT" && !beats.is_empty() {
            dest.sections.retain(|s| s.as_beat_grid().is_none());
            if let Some(grid) = source_dat.section(b"PQTZ") { dest.sections.push(grid.clone()); }
        }
        let bytes = if extension == "EXT" { dest.with_extended_grid_cleared().unwrap_or_else(|| dest.to_bytes()) } else { dest.to_bytes() };
        files.push((target, bytes));
    }
    if files.is_empty() { report.skipped += 1; return Ok(None); }
    if !differs && !bpm_changed && database_cues_match(state, id, &entries)? { report.unchanged += 1; return Ok(None); }
    Ok(Some(PendingCues { id: id.to_owned(), entries, bpm, files }))
}

/// Applies a batch of planned imports: every track's journal and files first,
/// then all the library edits on one writer, then the journals are committed.
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
        let prepared = state.read_db(|db| Ok(crate::file_journal::FileJournal::prepare_with(db, state.backup_dir(), location, &item.id, item.bpm, None, true, &item.files)))
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
        for item in &batch { w.import_usb_cues(&item.id, &item.entries, item.bpm)?; }
        Ok(())
    });
    if let Err(e) = written {
        // Rows already edited keep their new files; the rest are put back.
        let reconciled = journals.pop().map_or(Ok(()), |journal| journal.reconcile(location));
        // Tracks edited before the refusal now hold the stick's grid; Undo
        // must not write the pre-import grid back over it. A refused track
        // was never changed, so forgetting its history too loses nothing.
        for item in &batch { editor.forget_history(&item.id); }
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
    fn a_failed_edit_in_a_batch_leaves_every_track_whole() {
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
        // Grid edits made before the import, on a track imported ahead of the refusal.
        let first = rbl_db::fixture::track_id(0);
        editor.seed_history(&first);
        assert!(import(&state, &editor, &usb, true, false, false, false).is_err());
        assert!(!editor.has_history(&first), "a failed batch leaves no Undo over the imported grid");
        let mut imported = 0;
        for (index, path) in local.iter().enumerate() {
            let now = std::fs::read(path).unwrap();
            let id = rbl_db::fixture::track_id(index);
            let stick_cues = rbl_anlz::Anlz::read(&usb.join(format!("PIONEER/USBANLZ/P000/{:08X}/ANLZ0000.EXT", index + 1))).unwrap().cue_entries();
            let in_library = database_cues_match(&state, &id, &stick_cues).unwrap();
            // The file and the row agree: both imported, or both as they were.
            assert_eq!(stick_cues.len(), 1);
            assert_eq!(now != before[index], in_library, "track {index} has its file and its cues from different sides");
            imported += usize::from(in_library);
        }
        let first_cues = rbl_anlz::Anlz::read(&usb.join("PIONEER/USBANLZ/P000/00000001/ANLZ0000.EXT")).unwrap().cue_entries();
        assert!(database_cues_match(&state, &first, &first_cues).unwrap(), "the seeded track was imported before the refusal");
        assert!(imported < tracks, "{imported} of {tracks} tracks imported before the refusal");
        assert!(!state.backup_dir().join("analysis-journal").exists_with_entries(), "no journal is left behind");
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
}
