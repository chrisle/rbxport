//! USB-to-library reads. Never infer track identity from a title or USB row id.
use std::{collections::HashMap, path::{Path, PathBuf}, sync::Arc};
use serde::Serialize;
use tauri::{State, Manager};
use crate::{commands::{blocking, reload, write_error}, dto::{DeviceLibraryTreeDto, DevicePlaylistNodeDto}, error::{AppError, AppResult, ErrorKind}, state::AppState};

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

#[derive(Debug, Default, Serialize)]
#[serde(rename_all="camelCase")]
pub struct ImportReport {
    tracks: usize, histories: usize, settings: usize, skipped: usize,
    warnings: Vec<String>,
    #[serde(skip)] changed: Vec<String>,
}

/// Explicit imports and Sync Manager imports share identity checks.
#[tauri::command]
pub async fn import_usb<R: tauri::Runtime>(app: tauri::AppHandle<R>, state: State<'_, Arc<AppState>>, path: String, cues: bool, history: bool, settings: bool) -> AppResult<ImportReport> {
    let state = Arc::clone(&state);
    let worker = Arc::clone(&state);
    if !rbl_devices::list().iter().any(|d| d.mount_point == Path::new(&path)) { return Err(err("Device is no longer connected")); }
    let editor = Arc::clone(&app.state::<Arc<crate::grid::GridEditor>>());
    let log_path = path.clone();
    let result = blocking("import_usb", move || import(&worker, &editor, Path::new(&path), cues, history, settings)).await;
    match result {
        Ok(result) => {
            if result.tracks > 0 || result.histories > 0 { reload(app.clone(), state).await?; }
            for id in &result.changed {
                let _ = tauri::Emitter::emit(&app, "grid:changed", id);
                let _ = tauri::Emitter::emit(&app, "cues:changed", id);
            }
            Ok(result)
        }
        Err(e) => {
            tracing::error!(path = %log_path, cues, history, settings, error = %e, "USB import failed");
            let _ = reload(app, state).await;
            Err(e)
        }
    }
}

fn import(state: &AppState, editor: &crate::grid::GridEditor, root: &Path, cues: bool, history: bool, settings: bool) -> AppResult<ImportReport> {
    let _gate = state.edit_gate.lock();
    let _files = state.analysis_write.lock();
    rbl_devices::settings::recover(root)
        .map_err(|e| err(format!("Could not recover an interrupted export before importing: {e}")))?;
    let _device_read = rbl_core::durable::read_lock(root).map_err(err)?;
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
    // Our manifest also works on legacy-only exports; validate its source path against master.db.
    if let Some(manifest) = rbl_export::Manifest::load(root).filter(|m| m.db_id == 0 || m.db_id == db_id) {
        // The manifest records the path as the index resolved it.
        let drive = state.read_db(|db| Ok(db.drive_mapping())).map_err(write_error)?;
        for t in manifest.tracks {
            let id = t.library_id.to_string();
            let stored = state.read_db(|db| Ok(db.connection().query_row("SELECT FolderPath FROM djmdContent WHERE ID=?1 AND rb_local_deleted=0", [&id], |r| r.get::<_,String>(0)).ok())).map_err(write_error)?;
            let matched = stored.is_some_and(|p| drive.as_ref().map_or(std::borrow::Cow::Borrowed(p.as_str()), |d| d.apply(&p)) == t.source.as_str());
            if matched {
                let analysis = if t.anlz_dir.is_empty() { String::new() } else { format!("{}/ANLZ0000.DAT", t.anlz_dir) };
                tracks.entry(t.export_id).or_insert((id, analysis));
            }
        }
    }
    if cues && tracks.is_empty() { return Err(err("No tracks from this library were found on the device.")); }
    if cues {
        // Open the guarded writer even for an empty device; read-only must not look like success.
        state.write(|_| Ok(())).map_err(write_error)?;
        for (id, analysis) in tracks.values() {
            if editor.is_locked(id) || crate::grid::database_locked(&state.location()?, id)? { report.skipped += 1; continue; }
            if analysis.is_empty() { report.skipped += 1; continue; }
            let source = within(root, analysis)?;
            let source_dat = rbl_anlz::Anlz::read(&source).map_err(err)?;
            let source_ext = source.with_extension("EXT");
            let source_cues = if source_ext.exists() { rbl_anlz::Anlz::read(&within(root, &source_ext.strip_prefix(root).map_err(err)?.to_string_lossy())?).map_err(err)? } else { source_dat.clone() };
            if source_cues.section(b"PCO2").is_none() { report.skipped += 1; continue; }
            let entries = source_cues.cue_entries();
            let beats = source_dat.beat_grid().unwrap_or_default();
            let previous_bpm = state.read_db(|db| Ok(db.connection().query_row("SELECT COALESCE(BPM,0) FROM djmdContent WHERE ID=?1 AND rb_local_deleted=0", [id], |r| r.get::<_,u32>(0))?)).map_err(write_error)?;
            let bpm = beats.first().map_or(previous_bpm, |b| u32::from(b.tempo_x100));
            let relative = state.write(|w| w.analysis_data_path_for(id)).map_err(write_error)?;
            let target = rbl_anlz::resolve(&location.share_root, &relative);
            let mut files = Vec::new();
            for extension in ["DAT", "EXT"] {
                let target = target.with_extension(extension);
                if !target.exists() { continue; }
                let mut dest = rbl_anlz::Anlz::read(&target).map_err(err)?;
                let src = if extension == "DAT" { &source_dat } else { &source_cues };
                dest.sections.retain(|s| !s.is_cue_list());
                dest.sections.extend(src.sections.iter().filter(|s| s.is_cue_list()).cloned());
                if extension == "DAT" && !beats.is_empty() {
                    dest.sections.retain(|s| s.as_beat_grid().is_none());
                    if let Some(grid) = source_dat.section(b"PQTZ") { dest.sections.push(grid.clone()); }
                }
                let bytes = if extension == "EXT" { dest.with_extended_grid_cleared().unwrap_or_else(|| dest.to_bytes()) } else { dest.to_bytes() };
                files.push((target, bytes));
            }
            if files.is_empty() { report.skipped += 1; continue; }
            let journal = crate::file_journal::FileJournal::prepare(state.backup_dir(), &location, id, bpm, None, true, &files)?;
            if let Err(e) = journal.publish() { journal.rollback()?; return Err(e); }
            if let Err(e) = state.write(|w| w.import_usb_cues(id, &entries, bpm)) { journal.reconcile(&location)?; return Err(write_error(e)); }
            journal.commit()?;
            editor.forget_history(id);
            report.changed.push(id.clone());
            report.tracks += 1;
        }
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
        let destination = state.backup_dir().parent().unwrap_or(state.backup_dir()).join("usb-settings");
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
        rbl_export::Manifest { db_id: 0, baseline: None, version: 1, written: String::new(), playlists: vec![], loose: vec![], tracks: vec![rbl_export::manifest::ManifestTrack { analysis_hashes: std::collections::BTreeMap::new(), analysis_extensions: vec!["DAT".into()], audio_hash: 0,
            export_id: 1, library_id: id.parse().unwrap(), source: source_path, audio: "audio.mp3".into(), anlz_dir: anlz.into(), size: 0, modified: 0, analysis: 0, artwork: String::new(), conversion: String::new(), conversion_source_hash: 0,
        }] }.save(&usb).unwrap();
        let editor = crate::grid::GridEditor::at(state.backup_dir());
        let report = import(&state, &editor, &usb, true, false, false).unwrap();
        assert_eq!(report.tracks, 1);
        let result = rbl_anlz::Anlz::read(&target).unwrap();
        assert_eq!(result.path().as_deref(), Some("/original.mp3"));
        assert_eq!(result.waveform(b"PWAV").unwrap().1, &[1,2,3]);
        assert_eq!(result.beat_grid().unwrap()[0].time_ms, 250);
        assert_eq!(result.beat_grid().unwrap()[0].tempo_x100, 12800);
        editor.set_locked(&id, true).unwrap();
        assert_eq!(import(&state, &editor, &usb, true, false, false).unwrap().skipped, 1);
        editor.set_locked(&id, false).unwrap();
        let mut manifest = rbl_export::Manifest::load(&usb).unwrap();
        manifest.tracks[0].anlz_dir.clear();
        manifest.save(&usb).unwrap();
        // Identity remains available for history even without cue analysis.
        assert_eq!(import(&state, &editor, &usb, true, false, false).unwrap().skipped, 1);
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

        let error = import(&state, &editor, &usb, false, true, false).unwrap_err();
        assert!(error.message.contains("Could not recover an interrupted export"));
        assert!(error.message.contains("missing-track.wav"));
    }
}
