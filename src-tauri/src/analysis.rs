//! Analysing a track and keeping the result.
//!
//! One command does the whole job: decode, analyse, author the three
//! analysis files into the library's share tree, and register the result
//! on the track's row. The files go first — a row naming files that are not
//! there would show a track as analysed with nothing to draw — and each is
//! written beside its final name and renamed into place, so a crash mid-way
//! leaves the previous files whole.
//!
//! The index is not patched track by track: the columns are shared with
//! every open view, and a reload costs 233 ms on the reference library
//! [OBS]. The frontend draws the new BPM, key and analysed mark from the
//! answer itself, and asks for one reload when its queue drains.

use std::sync::Arc;

use tauri::State;

use crate::commands::{blocking, write_error};
use crate::error::{AppError, AppResult, ErrorKind};
use crate::state::AppState;

/// How much of a file is decoded, in seconds.
///
/// Tempo and key are global properties and three minutes settle them, but
/// the grid and the waveforms have to reach the end of the track — a CDJ
/// draws nothing past where they stop. Thirty minutes covers every track
/// and stops a two-hour mix from becoming a gigabyte of samples; a mix
/// longer than that is analysed up to the cap and drawn that far.
const DECODE_CAP_SECS: f64 = 1800.0;

/// How near an existing memory cue has to be to the first beat to count as
/// one already there. Re-analysing with the same settings places the first
/// beat on the same millisecond, so this only absorbs rounding; a beat that
/// moved further is a new place and gets its own cue.
const FIRST_BEAT_CUE_TOLERANCE_MS: u32 = 5;

/// Per-batch choices from the Analysis Setting dialog. Missing settings
/// preserve the defaults used by imports and older callers.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools, reason = "the dialog's independent checkboxes, sent together")]
pub struct AnalysisSettings {
    pub bpm_grid: bool,
    pub key: bool,
    pub high_precision: bool,
    pub min_bpm: f64,
    pub max_bpm: f64,
    /// Add a memory cue on the first beat of the new grid, unless one is
    /// already there. Only a BPM / Grid pass has a grid to place it on.
    pub first_beat_cue: bool,
}

impl Default for AnalysisSettings {
    fn default() -> Self {
        Self { bpm_grid: true, key: true, high_precision: true, min_bpm: 70.0, max_bpm: 180.0, first_beat_cue: false }
    }
}

impl AnalysisSettings {
    fn options(self, preset: rbl_analysis::AnalysisPreset) -> AppResult<rbl_analysis::AnalysisOptions> {
        if !self.bpm_grid && !self.key {
            return Err(AppError::new(ErrorKind::Malformed, "Select BPM / Grid or KEY to analyze."));
        }
        if !self.min_bpm.is_finite() || !self.max_bpm.is_finite()
            || self.min_bpm < 40.0 || self.max_bpm > 300.0 || self.min_bpm >= self.max_bpm {
            return Err(AppError::new(ErrorKind::Malformed, "Choose a valid BPM range between 40 and 300."));
        }
        let mut options = preset.options();
        options.tempo.min_bpm = self.min_bpm;
        options.tempo.max_bpm = self.max_bpm;
        options.tempo.placement = if self.high_precision {
            rbl_analysis::tempo::Placement::Attack
        } else {
            rbl_analysis::tempo::Placement::Envelope
        };
        Ok(options)
    }
}

/// What analysing one track produced.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisResultDto {
    pub track_id: String,
    pub analysed: u8,
    /// BPM x100, as rekordbox stores it.
    pub bpm_x100: u32,
    pub key: String,
    pub beats: u32,
    /// Peak sample magnitude, 0..=1.
    pub peak: f32,
    pub duration_sec: u32,
    pub elapsed_ms: u64,
    /// Where the analysis files went, share-relative.
    pub analysis_path: String,
    /// Whether this pass added a memory cue on the first beat, so the
    /// command can tell decks showing the track to refetch its cues.
    #[serde(skip)]
    pub added_first_beat_cue: bool,
}

/// Analyses one track and keeps the result: the files in the share tree,
/// the BPM, key, length and analysis path on the row.
///
/// Refused, with the reason, while rekordbox holds the library; nothing is
/// written to the share tree either in that case, since the files are only
/// reachable through the row.
#[tauri::command]
#[allow(clippy::too_many_arguments, reason = "Tauri injects the application states alongside command arguments")]
pub async fn analyse_track<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    editor: State<'_, Arc<crate::grid::GridEditor>>,
    track_id: String,
    mode: Option<String>,
    settings: Option<AnalysisSettings>,
) -> AppResult<AnalysisResultDto> {
    let preset = match mode.as_deref().unwrap_or("rbxport") {
        "rekordbox" => rbl_analysis::AnalysisPreset::Rekordbox,
        "rbxport" => rbl_analysis::AnalysisPreset::Rbxport,
        _ => return Err(AppError::new(ErrorKind::Malformed, "Unknown analysis mode.")),
    };
    let library = state.library()?;
    let share = state.share_root();
    let state = Arc::clone(&state);
    let editor = Arc::clone(&editor);
    let settings = settings.unwrap_or_default();
    let result = blocking("analyse_track", move || {
        analyse_and_save(&state, &library, &share, &track_id, preset, &settings, &editor)
    }).await?;
    // The track's id, well inside the 1 KB event cap: a deck showing the
    // track redraws its waveform and grid from the new files.
    let _ = tauri::Emitter::emit(&app, "analysis:changed", &result.track_id);
    if result.added_first_beat_cue {
        let _ = tauri::Emitter::emit(&app, "cues:changed", &result.track_id);
    }
    Ok(result)
}

/// The whole job, apart from the event: what the command runs on a worker
/// thread, and what the tests run directly.
fn analyse_and_save(
    state: &AppState,
    library: &rbl_index::Library,
    share: &std::path::Path,
    track_id: &str,
    preset: rbl_analysis::AnalysisPreset,
    settings: &AnalysisSettings,
    editor: &crate::grid::GridEditor,
) -> AppResult<AnalysisResultDto> {
    let options = settings.options(preset)?;
    // By the id map rather than a scan: a queue analyses hundreds of
    // tracks, and each scan is 38,681 comparisons.
    let Some(row) = library.row_of(track_id) else {
        return Err(AppError::new(ErrorKind::NotFound, "That track is not in the library."));
    };
    let row = row as usize;
    ensure_analysis_unlocked(state, editor, track_id)?;
    let path = library.folder_path.get(row);
    if path.is_empty() {
        return Err(AppError::new(ErrorKind::NotFound, "That track has no file path."));
    }
    // Checked before the decode, which is seconds of work that would
    // only end in the same refusal.
    if library_locked(state) {
        return Err(AppError::new(
            ErrorKind::ReadOnly,
            "rekordbox is running. Quit it before analysing.",
        ));
    }

    let started = std::time::Instant::now();
    let audio = rbl_audio::decode_mono(std::path::Path::new(path), Some(DECODE_CAP_SECS)).map_err(|e| {
        AppError::new(ErrorKind::Malformed, "That file could not be decoded.")
            .with_detail(e.to_string())
    })?;
    if !settings.bpm_grid {
        return analyse_key_only(state, library, row, track_id, &audio, editor, started);
    }
    let analysis = rbl_analysis::analyse_with(&audio.samples, audio.sample_rate, options);

    let _edit_guard = state.edit_gate.lock();
    ensure_analysis_unlocked(state, editor, track_id)?;
    let _files_guard = state.analysis_write.lock();
    let location = state.location()?;
    crate::file_journal::recover(state.backup_dir(), &location)?;
    state.write(|writer| writer.import_artwork(track_id)).map_err(write_error)?;
    // The files: where the row already points, or the UUID-derived place
    // rekordbox uses for desktop-library analysis [OBS: rbl-db::write].
    let relative = {
        let current = library.analysis_path.get(row);
        if current.is_empty() {
            state.write(|writer| writer.analysis_data_path_for(track_id)).map_err(write_error)?
        } else {
            current.to_owned()
        }
    };
    let dat = rbl_anlz::resolve(share, &relative);
    let existing = [
        rbl_anlz::Anlz::read(&dat).ok(),
        rbl_anlz::Anlz::read(&rbl_anlz::sibling(&dat, "EXT")).ok(),
        rbl_anlz::Anlz::read(&rbl_anlz::sibling(&dat, "2EX")).ok(),
    ];
    let beats: Vec<rbl_anlz::Beat> = analysis
        .tempo
        .beats
        .iter()
        .map(|b| rbl_anlz::Beat { beat_number: b.beat_number, tempo_x100: b.tempo_x100, time_ms: b.time_ms })
        .collect();
    let columns: Vec<rbl_anlz::BandColumn> = analysis
        .waveform
        .columns
        .iter()
        .map(|c| rbl_anlz::BandColumn { low: c.low, mid: c.mid, high: c.high, peak: c.peak })
        .collect();
    let files = rbl_anlz::author_with_overview(
        path,
        &beats,
        &columns,
        analysis.waveform.overview.as_slice().try_into().ok(),
        rbl_anlz::Existing { dat: existing[0].as_ref(), ext: existing[1].as_ref(), two_ex: existing[2].as_ref() },
    );
    // Clamp before narrowing so the conversion cannot truncate.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "clamped into 0..=u32::MAX on the line above")]
    let to_u32 = |v: f64| v.round().clamp(0.0, f64::from(u32::MAX)) as u32;
    let bpm_x100 = to_u32(analysis.tempo.bpm * 100.0);
    let detected_key = settings.key.then(|| analysis.key.map(|k| k.name)).flatten();
    let key = detected_key.clone().unwrap_or_else(|| library.keys.name(library.key.get(row).copied().unwrap_or(0)).to_owned());
    let duration_sec = to_u32(audio.duration_secs());
    // The length is kept only when the whole file was decoded: a capped
    // decode's length would be the cap, not the track's.
    let length_sec = (audio.duration_secs() < DECODE_CAP_SECS - 1.0).then_some(duration_sec);

    save_analysis_files(state, &location, track_id, &dat, files, bpm_x100, detected_key.as_deref(), &relative, length_sec, editor)?;
    let added_first_beat_cue = match beats.first() {
        Some(first) if settings.first_beat_cue => add_first_beat_cue(state, library, track_id, first.time_ms)?,
        _ => false,
    };

    Ok(AnalysisResultDto {
        track_id: track_id.to_owned(),
        analysed: u8::try_from(rbl_db::write::ANALYSED_BY_THIS_APP).unwrap_or(u8::MAX),
        bpm_x100,
        key,
        beats: u32::try_from(beats.len()).unwrap_or(u32::MAX),
        peak: analysis.peak,
        duration_sec,
        elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        analysis_path: relative,
        added_first_beat_cue,
    })
}

/// Adds a memory cue at `first_beat_ms` unless a memory cue is already
/// there, and re-reads the track's cues into the index. Resolves to whether
/// a cue was added.
///
/// Runs after the analysis is published, so a refusal here leaves the new
/// grid in place; the error says the analysis was saved and only the cue
/// is missing.
fn add_first_beat_cue(
    state: &AppState,
    library: &rbl_index::Library,
    track_id: &str,
    first_beat_ms: u32,
) -> AppResult<bool> {
    // The index is current: every cue edit re-reads its track's cues.
    let Some(row) = library.row_of(track_id) else { return Ok(false) };
    if !needs_first_beat_cue(&library.cues_of(row), first_beat_ms) {
        return Ok(false);
    }
    state
        .write_then(
            |writer| writer.add_cue(track_id, rbl_index::Cue::MEMORY, first_beat_ms),
            |db, _cue| rbl_index::reload_cues_of(db, library, track_id).map_err(rbl_db::DbError::from),
        )
        .map_err(|e| {
            let cause = write_error(e);
            AppError::new(cause.kind, "The analysis was saved, but the first-beat memory cue could not be added.")
                .with_detail(cause.message)
        })?;
    Ok(true)
}

/// Whether no memory cue (or memory loop) already starts on the first beat.
fn needs_first_beat_cue(cues: &[rbl_index::Cue], first_beat_ms: u32) -> bool {
    !cues.iter().any(|cue| cue.is_memory() && cue.position_ms.abs_diff(first_beat_ms) <= FIRST_BEAT_CUE_TOLERANCE_MS)
}

/// Journals, publishes and registers a fresh analysis — the on-disk half of
/// [`analyse_and_save`], once the analysis itself has been computed.
#[allow(clippy::too_many_arguments, reason = "everything analyse_and_save already computed, passed through")]
fn save_analysis_files(
    state: &AppState,
    location: &rbl_db::LibraryLocation,
    track_id: &str,
    dat: &std::path::Path,
    files: rbl_anlz::AnalysisFiles,
    bpm_x100: u32,
    detected_key: Option<&str>,
    relative: &str,
    length_sec: Option<u32>,
    editor: &crate::grid::GridEditor,
) -> AppResult<()> {
    // A newly created or sparsely imported library has no `djmdKey` rows.
    // Keep the analyser's canonical result instead of silently leaving the
    // track blank just because this is the first occurrence of that key.
    if let Some(key) = detected_key {
        state.write(|writer| writer.ensure_detected_key(key)).map_err(write_error)?;
    }
    let journal = crate::file_journal::FileJournal::prepare(state.backup_dir(), location, track_id,
        bpm_x100, Some(relative.to_owned()), true, &[
            (dat.to_owned(), files.dat), (rbl_anlz::sibling(dat, "EXT"), files.ext),
            (rbl_anlz::sibling(dat, "2EX"), files.two_ex),
        ])?;
    if let Err(e) = journal.publish() {
        journal.rollback()?;
        return Err(e);
    }
    let written = state
        .write(|writer| {
            writer.set_analysis(
                track_id,
                &rbl_db::write::AnalysisWrite {
                    bpm_x100,
                    key: detected_key,
                    analysis_path: relative,
                    length_sec,
                },
            )
        })
        .map_err(write_error);
    if let Err(e) = written {
        journal.reconcile(location)?;
        return Err(e);
    }
    journal.commit()?;
    editor.forget_history(track_id);
    Ok(())
}

fn ensure_analysis_unlocked(state: &AppState, editor: &crate::grid::GridEditor, track: &str) -> AppResult<()> {
    if editor.is_locked(track) || crate::grid::database_locked(&state.location()?, track)? {
        return Err(AppError::new(ErrorKind::ReadOnly, "This track's analysis is locked. Unlock it to analyze."));
    }
    Ok(())
}

/// A key-only pass never authors files or updates grid metadata.
fn analyse_key_only(state: &AppState, library: &rbl_index::Library, row: usize, track_id: &str,
    audio: &rbl_audio::Audio, editor: &crate::grid::GridEditor, started: std::time::Instant) -> AppResult<AnalysisResultDto> {
    let key = rbl_analysis::detect_key(&audio.samples, audio.sample_rate).map(|key| key.name);
    let _edit_guard = state.edit_gate.lock();
    ensure_analysis_unlocked(state, editor, track_id)?;
    // Match full analysis: only use keys already named by this library.
    // A sparse imported library may not contain every recognised key.
    let key = if let Some(key) = key {
        state.write(|writer| {
            writer.ensure_detected_key(&key)?;
            writer.set_field(track_id, rbl_db::write::TrackField::Key, &key)?;
            Ok(Some(key))
        }).map_err(write_error)?
    } else { None };
    Ok(AnalysisResultDto {
        track_id: track_id.to_owned(),
        analysed: library.analysed.get(row).copied().unwrap_or(0),
        bpm_x100: library.bpm_x100.get(row).copied().unwrap_or(0),
        key: key.unwrap_or_else(|| library.keys.name(library.key.get(row).copied().unwrap_or(0)).to_owned()),
        beats: 0,
        peak: audio.samples.iter().fold(0.0_f32, |peak, sample| peak.max(sample.abs())),
        duration_sec: library.length_sec.get(row).copied().unwrap_or(0),
        elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        analysis_path: library.analysis_path.get(row).to_owned(),
        added_first_beat_cue: false,
    })
}

/// PHRASE EDIT: CUT splits the phrase under `beat`, CLEAR takes it out.
/// The track's EXT file is rewritten in place with its other sections as
/// they were; nothing in the database changes. Resolves to whether anything
/// changed — a cut on a phrase's first beat, or no phrase under the beat,
/// is nothing.
#[tauri::command]
pub async fn edit_phrase<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Arc<AppState>>,
    track_id: String,
    beat: u16,
    action: String,
) -> AppResult<bool> {
    let library = state.library()?;
    let share = state.share_root();
    let state = Arc::clone(&state);
    let edit = match action.as_str() {
        "cut" => rbl_anlz::PhraseEdit::Cut { beat },
        "clear" => rbl_anlz::PhraseEdit::Clear { beat },
        other => return Err(AppError::new(ErrorKind::Malformed, format!("{other:?} is not a phrase edit."))),
    };
    let reported = track_id.clone();
    let changed = blocking("edit_phrase", move || {
        if library_locked(&state) {
            return Err(AppError::new(ErrorKind::ReadOnly, "rekordbox is running. Quit it before editing phrases."));
        }
        let Some(row) = library.row_of(&track_id) else {
            return Err(AppError::new(ErrorKind::NotFound, "That track is not in the library."));
        };
        let relative = library.analysis_path.get(row as usize);
        if relative.is_empty() {
            return Err(AppError::new(ErrorKind::NotFound, "That track has no analysis."));
        }
        let ext = rbl_anlz::sibling(&rbl_anlz::resolve(&share, relative), "EXT");
        let _edit_guard = state.edit_gate.lock();
        let _files = state.analysis_write.lock();
        let file = rbl_anlz::Anlz::read(&ext).map_err(|e| {
            AppError::new(ErrorKind::NotFound, "That track's analysis file could not be read.").with_detail(e.to_string())
        })?;
        let Some(bytes) = file.with_phrase_edit(edit) else { return Ok(false) };
        crate::durable::write(&ext, &bytes).map_err(|e| {
            AppError::new(ErrorKind::Internal, "The analysis file could not be written.").with_detail(e.to_string())
        })?;
        Ok(true)
    })
    .await?;
    if changed {
        let _ = tauri::Emitter::emit(&app, "analysis:changed", &reported);
    }
    Ok(changed)
}

/// Whether the library cannot be written right now: rekordbox holds the
/// installed library's file. A fixture is never held.
fn library_locked(state: &AppState) -> bool {
    state.location().is_ok_and(|location| location.is_real_install) && rbl_db::is_rekordbox_running()
}

/// Writes the three files beside `dat`, each through a temporary name so a
/// reader never sees a half-written one.
///
/// Only ever called from inside `analyse_track`'s `blocking` closure.
#[cfg(test)]
fn write_analysis_files(dat: &std::path::Path, files: &rbl_anlz::AnalysisFiles) -> std::io::Result<()> {
    if let Some(dir) = dat.parent() {
        // perf-ok: runs inside the command's spawn_blocking closure
        std::fs::create_dir_all(dir)?;
    }
    for (path, bytes) in [
        (dat.to_path_buf(), &files.dat),
        (rbl_anlz::sibling(dat, "EXT"), &files.ext),
        (rbl_anlz::sibling(dat, "2EX"), &files.two_ex),
    ] {
        crate::durable::write(&path, bytes)?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
mod tests {
    use super::*;

    #[test]
    fn the_files_land_beside_the_dat_with_no_temporaries_left() {
        let dir = tempfile::tempdir().unwrap();
        let dat = dir.path().join("PIONEER/USBANLZ/P001/00000001/ANLZ0000.DAT");
        let files = rbl_anlz::AnalysisFiles { dat: b"PMAI".to_vec(), ext: b"PMAI!".to_vec(), two_ex: b"PMAI!!".to_vec() };
        write_analysis_files(&dat, &files).unwrap();
        assert_eq!(std::fs::read(&dat).unwrap(), b"PMAI");
        assert_eq!(std::fs::read(rbl_anlz::sibling(&dat, "EXT")).unwrap(), b"PMAI!");
        assert_eq!(std::fs::read(rbl_anlz::sibling(&dat, "2EX")).unwrap(), b"PMAI!!");
        let names: Vec<String> = std::fs::read_dir(dat.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().all(|n| std::path::Path::new(n).extension() != Some("tmp".as_ref())), "{names:?}");
    }

    /// A mono 16-bit WAV of a click every `beat_secs`, loud enough for the
    /// tempo detector and long enough for a few hundred waveform columns.
    fn write_click_wav(path: &std::path::Path, seconds: u32, beat_secs: f32) {
        let rate = 22_050_u32;
        let frames = rate * seconds;
        let mut data = Vec::with_capacity(frames as usize * 2);
        let period = (beat_secs * rate as f32) as u32;
        for i in 0..frames {
            let since = i % period.max(1);
            let sample: i16 = if since < 400 { (20_000.0 * (1.0 - since as f32 / 400.0)) as i16 } else { 0 };
            data.extend_from_slice(&sample.to_le_bytes());
        }
        let data_len = u32::try_from(data.len()).unwrap();
        let mut out = Vec::with_capacity(44 + data.len());
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16_u32.to_le_bytes());
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2).to_le_bytes());
        out.extend_from_slice(&2_u16.to_le_bytes());
        out.extend_from_slice(&16_u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        out.extend_from_slice(&data);
        std::fs::write(path, out).unwrap();
    }

    #[test]
    fn analysing_a_fixture_track_writes_its_files_and_registers_them() {
        use rbl_db::fixture::{self, track_id, Shape};
        use rbl_db::{Library as Db, OpenMode};

        let dir = tempfile::tempdir().unwrap();
        let location = fixture::build(dir.path(), Shape::default()).expect("build the fixture");
        let audio = dir.path().join("click.wav");
        write_click_wav(&audio, 20, 0.5);
        fixture::point_at_audio(&location, 0, audio.to_str().unwrap(), 20).unwrap();
        {
            let db = Db::open(location.clone(), OpenMode::ReadWrite).unwrap();
            db.connection()
                .execute("UPDATE djmdContent SET Analysed = 0, AnalysisDataPath = '', ImagePath = '', ContentLink = NULL WHERE ID = ?1", [track_id(0)])
                .unwrap();
        }

        let db = Db::open(location.clone(), OpenMode::ReadOnly).unwrap();
        let (library, _) = rbl_index::load(&db).unwrap();
        let state = AppState::with_backups(dir.path().join("backups"));
        let share = location.share_root.clone();
        state.set_library(library, false, None, 0, location.clone());
        crate::backups::create(&state).unwrap();
        let library = state.library().unwrap();

        let editor = crate::grid::GridEditor::at(&dir.path().join("editor"));
        let settings = AnalysisSettings::default();
        let result = analyse_and_save(&state, &library, &share, &track_id(0), rbl_analysis::AnalysisPreset::Rbxport, &settings, &editor).expect("analysed");
        assert_eq!(result.track_id, track_id(0));
        assert_eq!(result.analysed, 105);
        assert_eq!(
            result.analysis_path,
            "/PIONEER/USBANLZ/fix/ture-content-00000000-0000-4000-8000-000000000000/ANLZ0000.DAT",
            "new desktop analyses use the row UUID just as rekordbox does"
        );
        assert!(result.bpm_x100 > 0, "a click track has a tempo");
        assert_eq!(result.duration_sec, 20);

        // The three files, parseable, with the grid and every waveform.
        let dat_path = rbl_anlz::resolve(&share, &result.analysis_path);
        let dat = rbl_anlz::Anlz::read(&dat_path).expect("the DAT is there");
        assert_eq!(dat.path().unwrap(), audio.to_str().unwrap());
        assert_eq!(dat.beat_grid().unwrap().len() as u32, result.beats);
        assert_eq!(dat.waveform(b"PWAV").unwrap().1.len(), 400);
        let ext = rbl_anlz::Anlz::read(&rbl_anlz::sibling(&dat_path, "EXT")).expect("the EXT is there");
        assert_eq!(ext.waveform(b"PWV5").unwrap().0, 2);
        let two = rbl_anlz::Anlz::read(&rbl_anlz::sibling(&dat_path, "2EX")).expect("the 2EX is there");
        assert_eq!(two.waveform(b"PWV6").unwrap().1.len(), 3600);

        // And the row says so.
        let (bpm, path, analysed, length, updated): (i64, String, i64, i64, String) = db
            .connection()
            .query_row(
                "SELECT BPM, AnalysisDataPath, Analysed, Length, AnalysisUpdated FROM djmdContent WHERE ID = ?1",
                [track_id(0)],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(bpm, i64::from(result.bpm_x100));
        assert_eq!(path, result.analysis_path);
        assert_eq!(analysed, rbl_db::write::ANALYSED_BY_THIS_APP);
        assert_eq!(length, 20);
        assert_eq!(updated, "1");
        let link: i64 = db.connection().query_row("SELECT ContentLink FROM djmdContent WHERE ID = ?1", [track_id(0)], |r| r.get(0)).unwrap();
        assert_eq!(link, 0x002c_0600, "rekordbox needs the track registration to display its preview");

        // A second analysis lands in the same place, files rewritten in place.
        let again = analyse_and_save(&state, &library, &share, &track_id(0), rbl_analysis::AnalysisPreset::Rbxport, &settings, &editor).expect("analysed again");
        assert_eq!(again.analysis_path, result.analysis_path);
        let names: Vec<String> = std::fs::read_dir(dat_path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 3, "{names:?}");

        // A key-only pass preserves every analysis file and all grid metadata.
        let (fresh, _) = rbl_index::load(&db).unwrap();
        let saved_files: Vec<_> = [dat_path.clone(), rbl_anlz::sibling(&dat_path, "EXT"), rbl_anlz::sibling(&dat_path, "2EX")]
            .into_iter().map(|path| { let bytes = std::fs::read(&path).unwrap(); (path, bytes) }).collect();
        let metadata = || db.connection().query_row(
            "SELECT BPM, AnalysisDataPath, Analysed, Length, AnalysisUpdated FROM djmdContent WHERE ID = ?1", [track_id(0)],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, String>(4)?)),
        ).unwrap();
        let before = metadata();
        let key_only = AnalysisSettings { bpm_grid: false, ..settings };
        let result = analyse_and_save(&state, &fresh, &share, &track_id(0), rbl_analysis::AnalysisPreset::Rbxport, &key_only, &editor).unwrap();
        assert_eq!(metadata(), before);
        assert_eq!(result.bpm_x100, again.bpm_x100);
        for (path, bytes) in &saved_files { assert_eq!(std::fs::read(path).unwrap(), *bytes); }

        // Unchecking KEY must leave its database reference untouched.
        let key_before: Option<String> = db.connection().query_row("SELECT KeyID FROM djmdContent WHERE ID = ?1", [track_id(0)], |r| r.get(0)).unwrap();
        let grid_only = AnalysisSettings { key: false, ..settings };
        analyse_and_save(&state, &fresh, &share, &track_id(0), rbl_analysis::AnalysisPreset::Rbxport, &grid_only, &editor).unwrap();
        let key_after: Option<String> = db.connection().query_row("SELECT KeyID FROM djmdContent WHERE ID = ?1", [track_id(0)], |r| r.get(0)).unwrap();
        assert_eq!(key_after, key_before);

        // No cue unless asked for: the passes above left the track's cues alone.
        let cues = |library: &rbl_index::Library| library.cues_of(library.row_of(&track_id(0)).unwrap());
        assert!(cues(&fresh).iter().all(|cue| !cue.is_memory()));

        // Asked for, a memory cue lands on the grid's first beat, once: a
        // second pass finds it there and adds nothing.
        let first_beat = rbl_anlz::Anlz::read(&dat_path).unwrap().beat_grid().unwrap()[0].time_ms;
        let with_cue = AnalysisSettings { first_beat_cue: true, ..settings };
        let result = analyse_and_save(&state, &fresh, &share, &track_id(0), rbl_analysis::AnalysisPreset::Rbxport, &with_cue, &editor).unwrap();
        assert!(result.added_first_beat_cue);
        let memory: Vec<u32> = cues(&fresh).iter().filter(|cue| cue.is_memory()).map(|cue| cue.position_ms).collect();
        assert_eq!(memory, [first_beat], "the index follows without a reload");
        let again = analyse_and_save(&state, &fresh, &share, &track_id(0), rbl_analysis::AnalysisPreset::Rbxport, &with_cue, &editor).unwrap();
        assert!(!again.added_first_beat_cue);
        let stored: i64 = db.connection().query_row(
            "SELECT COUNT(*) FROM djmdCue WHERE ContentID = ?1 AND Kind = 0 AND rb_local_deleted = 0", [track_id(0)], |r| r.get(0),
        ).unwrap();
        assert_eq!(stored, 1);
        // Key-only has no grid to place it on.
        let key_with_cue = AnalysisSettings { bpm_grid: false, first_beat_cue: true, ..settings };
        assert!(!analyse_and_save(&state, &fresh, &share, &track_id(0), rbl_analysis::AnalysisPreset::Rbxport, &key_with_cue, &editor).unwrap().added_first_beat_cue);

        // Locks refuse both kinds of analysis before any files are rewritten.
        editor.set_locked(&track_id(0), true).unwrap();
        assert!(analyse_and_save(&state, &fresh, &share, &track_id(0), rbl_analysis::AnalysisPreset::Rbxport, &settings, &editor).is_err());
        assert!(analyse_and_save(&state, &fresh, &share, &track_id(0), rbl_analysis::AnalysisPreset::Rbxport, &key_only, &editor).is_err());
    }

    #[test]
    fn a_memory_cue_near_the_first_beat_counts_and_hot_cues_do_not() {
        let cue = |kind: u8, position_ms: u32, out_ms: u32| rbl_index::Cue { id: 1, position_ms, out_ms, kind, colour: 0 };
        assert!(needs_first_beat_cue(&[], 120));
        assert!(!needs_first_beat_cue(&[cue(0, 120, 0)], 120));
        assert!(!needs_first_beat_cue(&[cue(0, 125, 0)], 120), "within the tolerance");
        assert!(!needs_first_beat_cue(&[cue(0, 120, 4_000)], 120), "a memory loop marks the beat too");
        assert!(needs_first_beat_cue(&[cue(0, 126, 0)], 120));
        assert!(needs_first_beat_cue(&[cue(1, 120, 0)], 120), "a hot cue is not a memory cue");
    }

    #[test]
    fn analysis_choices_validate_and_reach_the_tempo_options() {
        let preset = rbl_analysis::AnalysisPreset::Rbxport;
        let defaults = AnalysisSettings::default();
        let chosen = AnalysisSettings { high_precision: false, min_bpm: 98.0, max_bpm: 195.0, ..defaults };
        let options = chosen.options(preset).unwrap();
        assert_eq!(options.tempo.placement, rbl_analysis::tempo::Placement::Envelope);
        assert!((options.tempo.min_bpm - 98.0).abs() < f64::EPSILON);
        assert!((options.tempo.max_bpm - 195.0).abs() < f64::EPSILON);
        assert!(AnalysisSettings { bpm_grid: false, key: false, ..defaults }.options(preset).is_err());
        for (min_bpm, max_bpm) in [(180.0, 70.0), (70.0, 70.0), (0.0, 180.0), (70.0, 301.0), (f64::NAN, 180.0)] {
            assert!(AnalysisSettings { min_bpm, max_bpm, ..defaults }.options(preset).is_err());
        }
    }
}
