//! Audio placement shared by planning and staging. No media is written here.
use super::{
    analysis_directory, analysis_slot_key, cache, fat_file_name, is_device_gone, path_key,
    source_stamp, CompatibilityFormat, ExportError, Layout, ManifestTrack, Result, SourceTrack,
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub(super) struct PreparedAudio {
    pub place: Layout,
    pub size: u64,
    pub modified: i64,
    pub absent: bool,
    pub conversion: Option<rbl_audio::compatibility::Format>,
}

pub(super) enum AudioPreparation {
    Missing,
    Failed(String),
    Ready(PreparedAudio),
}

/// Reserve the final audio and analysis names in input order. Keep read
/// failures as explicit outcomes so the caller retains its reporting order.
#[allow(
    clippy::too_many_arguments,
    reason = "the existing placement inputs, shared without a second assignment"
)]
pub(super) fn prepare(
    track: &SourceTrack,
    export_id: u32,
    mut place: Layout,
    previous: Option<&ManifestTrack>,
    root_name: &str,
    compatibility: Option<CompatibilityFormat>,
    cache: Option<&cache::PreparationCache>,
    written_analysis_paths: &mut BTreeSet<String>,
    written_audio_paths: &mut BTreeSet<String>,
) -> Result<AudioPreparation> {
    // Read the source before claiming the previous entry: a track whose
    // audio has gone leaves its entry in `stale`, so the copy on the stick
    // is removed rather than orphaned by databases that no longer name it.
    let preserved = track.device.as_ref().is_some_and(|d| d.preserve);
    // A track the stick holds without its audio (deleted by hand, or
    // left by another writer) is carried as the stick has it: its rows,
    // path and analysis stay and nothing is copied. Leaving it out would
    // take another writer's track, its cues and its playlist entries off
    // the stick, which the conflict check refuses, and every later sync
    // to that stick with it (#317). rekordbox's export never revisits a
    // device track it was not given, so it leaves such a row in place.
    let mut absent = false;
    let (size, modified) = match std::fs::metadata(&track.source_path) {
        Ok(meta) => source_stamp(&meta),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && preserved => {
            tracing::warn!(title = %track.title, path = %track.source_path.display(), "device track kept without its audio file");
            absent = true;
            (track.file_size, 0)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(AudioPreparation::Missing);
        }
        Err(e) if is_device_gone(&e) => return Err(ExportError::DeviceGone),
        Err(e) => return Err(e.into()),
    };

    // Reconciliation may carry unrelated device tracks alongside this
    // selection. Their files remain untouched by compatibility conversion.
    let conversion = compatibility
        .filter(|_| !track.device.as_ref().is_some_and(|d| d.preserve))
        .map(CompatibilityFormat::audio);
    let conversion = match conversion {
        Some(target) => match cache.map_or_else(
            || {
                rbl_audio::compatibility::needs_conversion(&track.source_path)
                    .map_err(|e| std::io::Error::other(e.to_string()).into())
            },
            |cache| cache.needs_conversion(&track.source_path),
        ) {
            Ok(true) => Some(target),
            Ok(false) => None,
            Err(e) => {
                return Ok(AudioPreparation::Failed(format!(
                    "Could not read the audio: {e}"
                )));
            }
        },
        None => None,
    };
    if let Some(target) = conversion {
        let stem = Path::new(&place.file_name)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy();
        let name = fat_file_name(&format!(
            "{stem}-rbx-cdj-{export_id}.{}",
            target.extension()
        ));
        let parent = place
            .audio
            .rsplit_once('/')
            .map_or("/Contents", |(parent, _)| parent);
        place.audio = format!("{parent}/{name}");
        place.file_name = name;
    }
    if let Some(old) = previous.filter(|old| path_key(&old.audio) == path_key(&place.audio)) {
        place.audio.clone_from(&old.audio);
        place.file_name = Path::new(&old.audio)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
    }
    if !track.device.as_ref().is_some_and(|d| d.preserve) {
        place.anlz_dir = analysis_directory(&place.audio, root_name);
        let original = PathBuf::from(&place.audio);
        let stem = original.file_stem().unwrap_or_default().to_string_lossy();
        let ext = original
            .extension()
            .map_or(String::new(), |e| format!(".{}", e.to_string_lossy()));
        let parent = original.parent().unwrap_or(Path::new("/Contents"));
        let mut suffix = 0_u32;
        while !written_analysis_paths.insert(analysis_slot_key(&place.anlz_dir, &place.anlz_name)) {
            suffix += 1;
            if suffix > 200_003 {
                return Err(ExportError::Conflict(
                    "Analysis directory space exhausted".into(),
                ));
            }
            place.file_name = format!("{stem}-{export_id}-{suffix}{ext}");
            place.audio = format!("{}/{}", parent.display(), place.file_name);
            place.anlz_dir = analysis_directory(&place.audio, root_name);
        }
    }
    if !written_audio_paths.insert(path_key(&place.audio)) {
        return Err(ExportError::Conflict(format!(
            "Conversion would create duplicate audio path: {}",
            place.audio
        )));
    }

    Ok(AudioPreparation::Ready(PreparedAudio {
        place,
        size,
        modified,
        absent,
        conversion,
    }))
}
