//! One reconciliation and accounting model for previews and execution.
use std::path::Path;
use crate::{incremental, snapshot::Snapshot, ExportOptions, Manifest, SourceTrack, SourcePlaylist, SourceMyTag, Result};

#[derive(Debug)]
pub struct SyncPlan {
    pub tracks: Vec<SourceTrack>,
    pub playlists: Vec<SourcePlaylist>,
    pub estimate: crate::estimate::Estimate,
    pub database_bytes: u64,
    pub asset_bytes: u64,
    pub peak_bytes: u64,
    pub net_additional_bytes: i64,
    pub approximate: bool,
    pub unknown_size: bool,
    pub kept_existing: usize,
}

pub fn preview(destination: &Path, tracks: &[SourceTrack], playlists: &[SourcePlaylist], tags: &[SourceMyTag], options: &ExportOptions<'_>) -> Result<SyncPlan> {
    let _inspection = incremental::Inspection::new();
    let _lease = rbl_core::durable::read_lease(destination)?;
    if destination.join(crate::PUBLICATION).exists() {
        return Err(crate::ExportError::Conflict("This USB requires recovery before its space can be estimated. Sync it to recover.".into()));
    }
    let previous = Manifest::load(destination);
    let signatures = incremental::databases(destination)?;
    if let Some(manifest) = previous.as_ref().filter(|m| m.generation.as_ref().is_some_and(|g| {
        g.request == incremental::request(tracks, playlists, tags, options).unwrap_or_default()
            && g.databases == signatures && incremental::inputs(tracks).is_ok_and(|inputs| inputs == g.inputs)
            && incremental::receipts_match(destination, g).unwrap_or(false)
    }) && options.settings_files.is_empty()) {
        return Ok(SyncPlan { tracks: tracks.to_vec(), playlists: playlists.to_vec(),
            estimate: crate::estimate::Estimate { tracks_kept: manifest.tracks.len(), reuse_bytes: manifest.tracks.iter().map(|t| t.copy_stamp.map_or(t.size, |s| s.0)).sum(), ..Default::default() },
            database_bytes: 0, asset_bytes: 0, peak_bytes: 0, net_additional_bytes: 0,
            approximate: false, unknown_size: false, kept_existing: manifest.generation.as_ref().map_or(0, |g| g.kept_existing) });
    }
    let before = previous.as_ref().filter(|m| m.generation.as_ref().is_some_and(|g| g.databases == signatures))
        .and_then(|m| m.baseline.clone()).map_or_else(|| Snapshot::read(destination), Ok)?;
    let db_id = options.sync.map_or(0, |s| s.db_id);
    let rewritten = crate::reconcile::rewritten(destination, &before, previous.as_ref(), tracks, playlists, db_id);
    before.check_baseline_except(previous.as_ref(), db_id, &rewritten)?;
    from_baseline(destination, &before, previous.as_ref(), tracks, playlists, options)
}

pub(crate) fn from_baseline(destination: &Path, before: &Snapshot, previous: Option<&Manifest>, tracks: &[SourceTrack], playlists: &[SourcePlaylist], options: &ExportOptions<'_>) -> Result<SyncPlan> {
    let db_id = options.sync.map_or(0, |s| s.db_id);
    let (tracks, playlists) = crate::reconcile::prepare(destination, before, previous, tracks, playlists, db_id, options.keep_unlisted)?;
    let estimate = crate::estimate::estimate_inspected(destination, db_id, &tracks, false, options.compatibility, Some(before), options.cache);
    let trusted = incremental::trusted_paths(destination, previous)?;
    let recorded: std::collections::HashMap<_, _> = previous.into_iter().flat_map(|m| &m.tracks).map(|t| (t.key(), t)).collect();
    let selected: std::collections::HashSet<_> = tracks.iter().map(|t| crate::track_key(t.id, &t.source_path.to_string_lossy())).collect();
    let mut asset_bytes = 0_u64;
    let mut artwork_sources = std::collections::HashSet::new();
    for track in &tracks {
        let key = crate::track_key(track.id, &track.source_path.to_string_lossy());
        let old = recorded.get(&key).copied();
        let unchanged = old.is_some_and(|c| {
            incremental::analysis_recipe(track, &c.audio).is_ok_and(|r| previous.and_then(|m| m.generation.as_ref()).is_some_and(|g| g.analysis_recipes.get(&key) == Some(&r)))
                && c.analysis_extensions.iter().all(|e| trusted.contains(&format!("{}/ANLZ0000.{e}", c.anlz_dir.trim_start_matches('/'))))
        });
        if !unchanged {
            if let Some(base) = &track.analysis_source {
                for ext in ["DAT", "EXT", "2EX"] {
                    asset_bytes += std::fs::metadata(base.with_extension(ext)).map_or(0, |m| allocation(m.len() + 256));
                }
            } else { asset_bytes += track.analysis.iter().map(|(_, b)| allocation(b.len() as u64 + 256)).sum::<u64>(); }
        }
        if let Some(image) = &track.artwork {
            if !artwork_sources.insert(image.clone()) { continue; }
            let source_unchanged = previous.and_then(|m| m.generation.as_ref()).is_some_and(|g| {
                g.inputs.get(&image.to_string_lossy().into_owned()).is_some_and(|old| incremental::revision(image).is_ok_and(|r| r == *old))
                    && old.is_some_and(|c| !c.artwork.is_empty() && trusted.contains(c.artwork.trim_start_matches('/')))
            });
            if !source_unchanged {
                for (_, medium) in crate::artwork_names(1) {
                    asset_bytes += std::fs::metadata(crate::artwork_source(image, medium)).map_or(0, |m| allocation(m.len()));
                }
            }
        }
    }
    let root = crate::export_root(destination);
    let old_database_bytes: u64 = ["rekordbox/export.pdb", "rekordbox/exportExt.pdb", "rekordbox/exportLibrary.db", "rbxport/manifest.json"].iter()
        .map(|p| std::fs::metadata(root.join(p)).map_or(0, |m| allocation(m.len()))).sum();
    let entry_count: usize = playlists.iter().map(|p| p.track_indices.len()).sum();
    let database_bytes = old_database_bytes.max(256 * 1024 + tracks.len() as u64 * 4096 + entry_count as u64 * 32 + playlists.len() as u64 * 1024);
    let old_replacements: u64 = previous.into_iter().flat_map(|m| &m.tracks).filter(|c| {
        selected.contains(&c.key()) && estimate.actions.get(&c.key()).is_some_and(|action| matches!(action, crate::estimate::AudioAction::Copy | crate::estimate::AudioAction::Convert))
    }).map(|c| std::fs::metadata(crate::under(destination, &c.audio)).map_or(0, |m| allocation(m.len()))).sum();
    let staged = estimate.copy_bytes.saturating_add(asset_bytes).saturating_add(database_bytes);
    let reclaimed = estimate.free_bytes.saturating_add(old_database_bytes).saturating_add(old_replacements);
    let peak_bytes = staged.saturating_add(64 * 1024).saturating_add(estimate.uncertain_adoption_bytes);
    let net = i128::from(staged) - i128::from(reclaimed);
    let kept_existing = tracks.iter().filter(|t| t.source_unavailable).count();
    let uncertain_revisions = tracks.iter().any(|t| {
        let key = crate::track_key(t.id, &t.source_path.to_string_lossy());
        recorded.get(&key).is_some_and(|old| estimate.actions.get(&key).is_some_and(|a| matches!(a, crate::estimate::AudioAction::Copy | crate::estimate::AudioAction::Convert))
            && incremental::revision(&t.source_path).is_ok_and(|r| r.is_some_and(|r| r.size == old.size)))
    });
    let unknown_size = uncertain_revisions || estimate.uncertain_adoption_bytes > 0 || (options.compatibility.is_some() && tracks.iter().any(|t| t.duration_sec == 0 && !t.device.as_ref().is_some_and(|d| d.preserve)));
    Ok(SyncPlan { tracks, playlists, approximate: true, estimate, database_bytes, asset_bytes, peak_bytes,
        net_additional_bytes: i64::try_from(net).unwrap_or(if net < 0 { i64::MIN } else { i64::MAX }), unknown_size, kept_existing })
}

fn allocation(bytes: u64) -> u64 { bytes.div_ceil(4096).saturating_mul(4096) }
