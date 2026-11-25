//! Audio placement shared by planning and staging. No media is written here.
use super::{
    analysis_directory, analysis_slot_key, cache, copy_stamp, fat_file_name, file_hash_cancellable,
    files_equal, historical_audio_alias, incremental, is_device_gone, owns_library_file, path_key,
    same_copy, same_file, source_stamp, under, CompatibilityFormat, ExportError, Layout,
    ManifestTrack, Result, SourceTrack,
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

pub(super) enum AudioPreparation<T = PreparedAudio> {
    Missing,
    Failed(String),
    Ready(T),
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

/// Final read/reuse decision. Execution consumes this exact record rather than
/// assigning names or deciding relocation again after the early capacity gate.
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent existing receipt predicates can overlap; preserve their staging and manifest meanings"
)]
pub(super) struct ReadyAudio {
    pub key: String,
    pub placement: PreparedAudio,
    pub in_place: bool,
    pub adopted: bool,
    pub retained_audio: bool,
    pub moved: bool,
    pub moved_copy_stamp: Option<(u64, i64)>,
    pub source_receipt_matches: bool,
    pub copy_now: Option<(u64, i64)>,
    pub source_hash: u64,
    pub existing_audio_hash: Option<u64>,
    pub unchanged: bool,
    pub inspected_copy_stamp: Option<(u64, i64)>,
    pub uncertain: bool,
}

impl ReadyAudio {
    pub fn writes_audio(&self) -> bool {
        !self.placement.absent
            && !self.retained_audio
            && !self.moved
            && !self.in_place
            && !self.adopted
            && !self.unchanged
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "existing read decision inputs, shared by preview and execution"
)]
pub(super) fn inspect(
    destination: &Path,
    track: &SourceTrack,
    placement: PreparedAudio,
    recorded: Option<&ManifestTrack>,
    previous: Option<&super::Manifest>,
    in_place_path: Option<&str>,
    adopted_path: Option<&str>,
    source_contents_unchanged: &BTreeSet<String>,
    inspected_source_hashes: &std::collections::BTreeMap<String, u64>,
    cache: Option<&cache::PreparationCache>,
    cancelled: &dyn Fn() -> bool,
    preview: bool,
) -> Result<AudioPreparation<ReadyAudio>> {
    let PreparedAudio {
        place,
        size,
        modified,
        absent,
        conversion,
    } = &placement;
    let (size, modified, absent, conversion) = (*size, *modified, *absent, *conversion);
    let source = track.source_path.to_string_lossy().into_owned();
    let key = super::track_key(track.id, &source);
    let preserved = track.device.as_ref().is_some_and(|d| d.preserve);
    // Still the library's own file on the stick, not renamed for a
    // conversion or an analysis collision: point at it, copy nothing.
    let in_place = absent || in_place_path == Some(place.audio.as_str());
    // The stick's own copy, still where its row says and not renamed for
    // a conversion or an analysis collision: kept, nothing copied.
    let adopted = !in_place && adopted_path == Some(place.audio.as_str());
    let profile = conversion.map_or("", rbl_audio::compatibility::Format::profile);
    let audio_dest = under(destination, &place.audio);
    let retained_audio = preserved && same_file(&track.source_path, &audio_dest);
    // Looked at, not yet claimed: a track that fails below must leave
    // its entry in `stale`.
    let previous_entry = recorded;
    let source_receipt_matches = previous
        .and_then(|m| m.generation.as_ref())
        .is_none_or(|g| {
            g.inputs.get(&source).is_none_or(|old| {
                incremental::revision(&track.source_path).is_ok_and(|now| now == *old)
            })
        });
    let moved = previous_entry.filter(|c| {
        c.audio != place.audio
            && c.size == size
            && c.conversion == profile
            && ((c.modified == modified && source_receipt_matches)
                || source_contents_unchanged.contains(&key))
            && !owns_library_file(destination, c)
            && !audio_dest.exists()
            && copy_stamp(&under(destination, &c.audio))
                .is_ok_and(|now| same_copy(c.copy_stamp, now))
    });
    // Unchanged means: same source bytes by size and time, same place on
    // the stick, and still actually there.
    let unchanged_metadata = !in_place
        && previous_entry.is_some_and(|c| {
            (c.audio == place.audio
                || (!c.in_place
                    && c.preserved != Some(true)
                    && c.conversion.is_empty()
                    && historical_audio_alias(destination, &c.audio)
                        .is_some_and(|portable| portable == place.audio)))
                && c.size == size
                && c.conversion == profile
                && ((c.modified == modified && source_receipt_matches)
                    || source_contents_unchanged.contains(&key))
        });
    // The stick's copy as it is now, by size and time only.
    let copy_now = if unchanged_metadata {
        copy_stamp(&audio_dest)?
    } else {
        None
    };
    // The copy is still the one this export recorded: nothing has
    // written to it since, so the bytes `audio_hash` names are the bytes
    // there. Trusting that, as rekordbox trusts its device database for
    // a track already exported, saves reading the whole stick back.
    let copy_unchanged =
        previous_entry.is_some_and(|c| c.audio_hash != 0 && same_copy(c.copy_stamp, copy_now));
    let uncertain = preview && unchanged_metadata && !copy_unchanged;
    let source_hash =
        if let Some(c) = previous_entry.filter(|_| conversion.is_some() && copy_unchanged) {
            c.conversion_source_hash
        } else if conversion.is_some() && !preview {
            match inspected_source_hashes.get(&key).map_or_else(
                || {
                    cache.map_or_else(
                        || file_hash_cancellable(&track.source_path, cancelled),
                        |cache| {
                            cache
                                .source_hash(&track.source_path, cancelled)
                                .map_err(std::io::Error::other)
                        },
                    )
                },
                |hash| Ok(*hash),
            ) {
                Ok(hash) => hash,
                Err(e) if is_device_gone(&e) => return Err(ExportError::DeviceGone),
                Err(e) if e.kind() == std::io::ErrorKind::StorageFull => return Err(e.into()),
                Err(e) => {
                    return Ok(AudioPreparation::Failed(format!(
                        "Could not read the audio: {e}"
                    )));
                }
            }
        } else {
            0
        };
    let carried = recorded;
    // Otherwise the copy was written since this export recorded it, or
    // the record predates `copy_stamp`. Hash the USB copy once against
    // the recorded hash and the source: a same-size rewrite on the stick
    // is caught here, and the copy's new stamp is recorded so the next
    // sync can trust it again.
    let existing_audio_hash = if copy_unchanged {
        carried.map(|c| c.audio_hash)
    } else if unchanged_metadata && !preview {
        match carried.filter(|c| c.audio_hash != 0) {
            Some(_) => match file_hash_cancellable(&audio_dest, cancelled) {
                Ok(hash) => Some(hash),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(e.into()),
            },
            None => None,
        }
    } else {
        None
    };
    let current_source_hash =
        if unchanged_metadata && !copy_unchanged && conversion.is_none() && !preview {
            Some(
                inspected_source_hashes
                    .get(&key)
                    .copied()
                    .map_or_else(|| file_hash_cancellable(&track.source_path, cancelled), Ok)?,
            )
        } else {
            None
        };
    let unchanged = unchanged_metadata
        && !uncertain
        && if let Some(c) = carried {
            if copy_unchanged {
                true
            } else if conversion.is_some() && c.conversion_source_hash != source_hash {
                false
            } else if c.audio_hash != 0 {
                existing_audio_hash == Some(c.audio_hash)
                    && (conversion.is_some() || current_source_hash == existing_audio_hash)
            } else {
                files_equal(&track.source_path, &audio_dest)?
            }
        } else {
            false
        };

    let inspected_copy_stamp = if unchanged { copy_now } else { None };
    let moved_copy_stamp = moved.and_then(|old| old.copy_stamp);
    let moved = moved.is_some();
    Ok(AudioPreparation::Ready(ReadyAudio {
        key,
        placement,
        in_place,
        adopted,
        retained_audio,
        moved,
        moved_copy_stamp,
        source_receipt_matches,
        copy_now,
        source_hash,
        existing_audio_hash,
        unchanged,
        inspected_copy_stamp,
        uncertain,
    }))
}

pub(super) type PreparedTrack = Result<AudioPreparation<ReadyAudio>>;

#[allow(
    clippy::too_many_arguments,
    reason = "prepare one immutable assignment from the established execution inputs"
)]
pub(super) fn batch(
    destination: &Path,
    tracks: &[SourceTrack],
    ids: &[u32],
    layouts: Vec<Layout>,
    previous: Option<&super::Manifest>,
    root_name: &str,
    in_place_paths: &[Option<String>],
    adopted_paths: &[Option<String>],
    compatibility: Option<CompatibilityFormat>,
    cache: Option<&cache::PreparationCache>,
    source_contents_unchanged: &BTreeSet<String>,
    inspected_source_hashes: &std::collections::BTreeMap<String, u64>,
    cancelled: &dyn Fn() -> bool,
    check_root: &mut dyn FnMut(usize, &SourceTrack) -> Result<()>,
    preview: bool,
) -> Result<Vec<PreparedTrack>> {
    let recorded: std::collections::BTreeMap<_, _> = previous
        .into_iter()
        .flat_map(|m| &m.tracks)
        .map(|t| (t.key(), t))
        .collect();
    let mut written_audio_paths = BTreeSet::new();
    let mut written_analysis_paths = tracks
        .iter()
        .zip(&layouts)
        .filter(|(t, _)| t.device.as_ref().is_some_and(|d| d.preserve))
        .map(|(_, place)| analysis_slot_key(&place.anlz_dir, &place.anlz_name))
        .collect();
    let mut ready = Vec::with_capacity(tracks.len());
    for (index, (track, place)) in tracks.iter().zip(layouts).enumerate() {
        check_root(index, track)?;
        if cancelled() {
            return Err(ExportError::Cancelled);
        }
        let prepared = if let Some(error) = &track.preparation_error {
            Ok(AudioPreparation::Failed(error.clone()))
        } else {
            let old = recorded
                .get(&super::track_key(
                    track.id,
                    &track.source_path.to_string_lossy(),
                ))
                .copied();
            prepare(
                track,
                ids.get(index).copied().unwrap_or(0),
                place,
                old,
                root_name,
                compatibility,
                cache,
                &mut written_analysis_paths,
                &mut written_audio_paths,
            )
            .and_then(|prepared| match prepared {
                AudioPreparation::Missing => Ok(AudioPreparation::Missing),
                AudioPreparation::Failed(error) => Ok(AudioPreparation::Failed(error)),
                AudioPreparation::Ready(placement) => inspect(
                    destination,
                    track,
                    placement,
                    old,
                    previous,
                    in_place_paths.get(index).and_then(Option::as_deref),
                    adopted_paths.get(index).and_then(Option::as_deref),
                    source_contents_unchanged,
                    inspected_source_hashes,
                    cache,
                    cancelled,
                    preview,
                ),
            })
        };
        let stop = prepared.is_err();
        ready.push(prepared);
        if stop {
            break;
        }
    }
    Ok(ready)
}

/// Additional temporary allocation only: a relocation preserves the same
/// permanent audio bytes. Include the same per-file/folder reserve as the late
/// materializer, which still rechecks actual free space before copying.
#[derive(Default, Clone, Copy)]
pub(super) struct OverlapSpace {
    /// Proven minimum, independent of shared folder/companion estimates.
    pub allocated: u64,
    pub temporary: u64,
}

pub(super) fn overlap_space(
    destination: &Path,
    previous: Option<&super::Manifest>,
    entries: &[PreparedTrack],
    trusted: &BTreeSet<String>,
    unit: u64,
) -> Result<OverlapSpace> {
    // A prepared fatal error prevents publication. Preserve its original
    // outcome rather than performing additional accounting reads first.
    if entries.iter().any(Result::is_err) {
        return Ok(OverlapSpace::default());
    }
    let ready: Vec<_> = entries
        .iter()
        .filter_map(|entry| match entry {
            Ok(AudioPreparation::Ready(ready)) => Some(ready),
            _ => None,
        })
        .collect();
    if !ready.iter().any(|audio| audio.moved) {
        return Ok(OverlapSpace::default());
    }
    let recorded: std::collections::BTreeMap<_, _> = previous
        .into_iter()
        .flat_map(|m| &m.tracks)
        .map(|t| (t.key(), t))
        .collect();
    let mut moves = Vec::new();
    let mut paths = Vec::new();
    let mut retained = BTreeSet::new();
    let mut relocated = BTreeSet::new();
    for audio in &ready {
        let place = &audio.placement.place;
        retained.insert(place.audio.to_lowercase());
        if audio.writes_audio() || audio.moved {
            paths.push(PathBuf::from(place.audio.trim_start_matches('/')));
        }
        if audio.moved {
            // The recorded key is deliberately retained in the ready record;
            // no lookup by destination or numeric device ID can change owner.
            if let Some(old) = recorded.get(&audio.key) {
                let from = PathBuf::from(old.audio.trim_start_matches('/'));
                relocated.insert(from.clone());
                let stamp = audio.moved_copy_stamp.ok_or_else(|| {
                    ExportError::Conflict("Missing relocation copy receipt".into())
                })?;
                if copy_stamp(&under(destination, &old.audio))? != Some(stamp) {
                    return Err(ExportError::Conflict(
                        "Exported media changed during sync. Retry after closing other writers."
                            .into(),
                    ));
                }
                moves.push((
                    PathBuf::from(place.audio.trim_start_matches('/')),
                    from,
                    stamp.0,
                ));
            }
        }
    }
    if moves.is_empty() {
        return Ok(OverlapSpace::default());
    }
    for old in previous.into_iter().flat_map(|m| &m.tracks) {
        let candidate = old.audio.trim_start_matches('/');
        if old.audio.is_empty()
            || retained.contains(&old.audio.to_lowercase())
            || relocated.contains(Path::new(candidate))
            || super::owns_library_file(destination, old)
            || super::carried_entry(destination, old)
        {
            continue;
        }
        if trusted.contains(candidate)
            || copy_stamp(&under(destination, candidate))
                .is_ok_and(|now| same_copy(old.copy_stamp, now))
        {
            paths.push(PathBuf::from(candidate));
        }
    }
    let publication_paths = rbl_core::durable::RelocationPaths::new(&paths)?;
    let mut bytes = 0_u64;
    let mut count = 0_u64;
    let mut folders = BTreeSet::new();
    for (to, from, size) in moves {
        if !publication_paths.overlaps(destination, None, &from)? {
            continue;
        }
        bytes = bytes.saturating_add(super::plan::allocation(size, unit));
        count += 1;
        folders.extend(
            to.ancestors()
                .skip(1)
                .filter(|p| !p.as_os_str().is_empty())
                .map(Path::to_path_buf),
        );
    }
    if count == 0 {
        return Ok(OverlapSpace::default());
    }
    Ok(OverlapSpace {
        allocated: bytes,
        temporary: bytes.saturating_add(
            count
                .saturating_add((folders.len() as u64).saturating_mul(2))
                .saturating_add(2)
                .saturating_mul(unit),
        ),
    })
}

/// Cheap preview shares naming and receipt-based decisions. Adoption needs a
/// byte comparison and can reserve different names, so that assignment remains
/// explicitly uncertain until execution has verified it.
#[allow(
    clippy::too_many_arguments,
    reason = "the plan's existing inputs and measured allocation unit"
)]
pub(super) fn preview_overlap(
    destination: &Path,
    tracks: &[SourceTrack],
    previous: Option<&super::Manifest>,
    root_name: &str,
    estimate: &super::estimate::Estimate,
    options: &super::ExportOptions<'_>,
    trusted: &BTreeSet<String>,
    unit: u64,
) -> Result<(u64, bool)> {
    if estimate.uncertain_adoption_bytes > 0 {
        return Ok((0, true));
    }
    let previous_in_place = super::previous_in_place(destination, previous);
    let in_place = super::in_place_paths(destination, tracks, &previous_in_place);
    let adopted = vec![None; tracks.len()];
    let ids = super::assign_ids(tracks, previous);
    let layouts = super::layouts(
        destination,
        tracks,
        &ids,
        root_name,
        previous,
        &in_place,
        &previous_in_place,
    );
    let entries = batch(
        destination,
        tracks,
        &ids,
        layouts,
        previous,
        root_name,
        &in_place,
        &adopted,
        options.compatibility,
        options.cache,
        &BTreeSet::new(),
        &std::collections::BTreeMap::new(),
        &|| false,
        &mut |_, _| Ok(()),
        true,
    )?;
    let uncertain = entries.len() != tracks.len()
        || entries.iter().any(|entry| match entry {
            Ok(AudioPreparation::Ready(audio)) => audio.uncertain,
            Err(_) => true,
            _ => false,
        });
    Ok((
        overlap_space(destination, previous, &entries, trusted, unit)?.temporary,
        uncertain,
    ))
}

/// Enforce the fully bound budget, or only the verified overlap minimum when
/// the rest of the estimate is uncertain. Keep an already prepared fatal error's
/// original outcome: it must not become a new capacity error.
pub(super) fn preflight(
    entries: &[PreparedTrack],
    original_peak: u64,
    estimated_overlap: u64,
    resolved_overlap: OverlapSpace,
    unknown: bool,
    available: impl FnOnce() -> std::io::Result<u64>,
) -> Result<()> {
    if entries.iter().any(Result::is_err) {
        return Ok(());
    }
    let required = if unknown {
        resolved_overlap.allocated
    } else {
        original_peak.max(
            original_peak
                .saturating_sub(estimated_overlap)
                .saturating_add(resolved_overlap.temporary),
        )
    };
    if required > 0 && available()? < required {
        return Err(ExportError::Conflict(format!("Not enough temporary space to sync safely: approximately {required} bytes required while the previous library is retained")));
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    fn overlap(bytes: u64) -> OverlapSpace {
        OverlapSpace {
            allocated: bytes,
            temporary: bytes,
        }
    }

    #[test]
    fn preflight_checks_known_overlap_even_with_uncertain_base() {
        assert!(preflight(&[], 1_000_000, 0, overlap(8192), true, || Ok(8191)).is_err());
        preflight(
            &[],
            1_000_000,
            0,
            OverlapSpace {
                allocated: 8192,
                temporary: 16384,
            },
            true,
            || Ok(8192),
        )
        .unwrap();
        preflight(&[], 1_000_000, 0, overlap(0), true, || {
            panic!("no known requirement")
        })
        .unwrap();
    }

    #[test]
    fn preflight_preserves_original_and_saturated_peaks() {
        assert!(preflight(&[], u64::MAX, u64::MAX, overlap(1), false, || Ok(
            u64::MAX - 1
        ))
        .is_err());
        assert!(preflight(&[], 100, 90, overlap(1), false, || Ok(99)).is_err());
        assert!(preflight(&[], 100, 0, overlap(50), false, || Ok(149)).is_err());
        preflight(&[], 100, 0, overlap(50), false, || Ok(150)).unwrap();
    }

    #[test]
    fn fatal_prepared_errors_are_not_masked_but_skipped_tracks_keep_the_overlap_floor() {
        let fatal = [Err(ExportError::Cancelled)];
        preflight(&fatal, u64::MAX, 0, overlap(8192), false, || {
            panic!("retain the prepared fatal error")
        })
        .unwrap();
        let skipped = [Ok(AudioPreparation::Failed("read failed".into()))];
        assert!(preflight(&skipped, u64::MAX, 0, overlap(8192), true, || Ok(8191)).is_err());
        preflight(&skipped, u64::MAX, 0, overlap(8192), true, || Ok(8192)).unwrap();
    }
}
