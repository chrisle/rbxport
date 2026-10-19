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
    /// The selected playlists edited on the stick that this sync writes on
    /// purpose; see [`crate::reconcile::Prepared::device_additions`].
    pub device_additions: std::collections::BTreeSet<u32>,
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
            approximate: false, unknown_size: false, kept_existing: manifest.generation.as_ref().map_or(0, |g| g.kept_existing),
            device_additions: std::collections::BTreeSet::new() });
    }
    let mut before = previous.as_ref().filter(|m| m.generation.as_ref().is_some_and(|g| g.databases == signatures))
        .and_then(|m| m.baseline.clone()).map_or_else(|| Snapshot::read(destination), Ok)?;
    let db_id = options.sync.map_or(0, |s| s.db_id);
    let playlist_record = crate::reconcile::playlist_namespace(destination, &mut before, previous.as_ref(), db_id);
    let rewritten = crate::reconcile::rewritten(playlist_record.as_ref(), &before, previous.as_ref(), tracks, playlists, db_id);
    before.check_baseline_except(previous.as_ref(), db_id, &rewritten)?;
    from_baseline(destination, &before, previous.as_ref(), tracks, playlists, options, playlist_record.as_ref())
}

pub(crate) fn from_baseline(destination: &Path, before: &Snapshot, previous: Option<&Manifest>, tracks: &[SourceTrack], playlists: &[SourcePlaylist], options: &ExportOptions<'_>, record: Option<&crate::sync_record::SyncRecord>) -> Result<SyncPlan> {
    from_baseline_in(destination, before, previous, tracks, playlists, options, record, allocation_unit(destination))
}

/// [`from_baseline`] for a stick that stores files in `unit`-byte clusters.
#[allow(clippy::too_many_arguments, reason = "from_baseline's inputs and the allocation unit it measured")]
fn from_baseline_in(destination: &Path, before: &Snapshot, previous: Option<&Manifest>, tracks: &[SourceTrack], playlists: &[SourcePlaylist], options: &ExportOptions<'_>, record: Option<&crate::sync_record::SyncRecord>, unit: u64) -> Result<SyncPlan> {
    let on_disk = |bytes: u64| allocation(bytes, unit);
    let db_id = options.sync.map_or(0, |s| s.db_id);
    let crate::reconcile::Prepared { tracks, playlists, device_additions } = crate::reconcile::prepare(destination, before, previous, tracks, playlists, db_id, options.keep_unlisted, record)?;
    let estimate = crate::estimate::estimate_inspected(destination, db_id, &tracks, false, options.compatibility, Some(before), options.cache, unit);
    let trusted = incremental::trusted_paths(destination, previous)?;
    let recorded: std::collections::HashMap<_, _> = previous.into_iter().flat_map(|m| &m.tracks).map(|t| (t.key(), t)).collect();
    let selected: std::collections::HashSet<_> = tracks.iter().map(|t| crate::track_key(t.id, &t.source_path.to_string_lossy())).collect();
    let mut asset_bytes = 0_u64;
    let mut artwork_sources = std::collections::HashSet::new();
    let root = crate::export_root(destination);
    let root_name = root.file_name().map_or_else(|| "PIONEER".to_owned(), |n| n.to_string_lossy().into_owned());
    // The folders the sync writes into. On FAT32 and exFAT a folder takes a
    // cluster of its own, which on a large stick is 128 KiB each.
    let mut folders: std::collections::BTreeSet<std::path::PathBuf> = ["rekordbox", "rbxport"].iter().map(|f| Path::new("/").join(&root_name).join(f)).collect();
    folders.insert(Path::new("/").join(&root_name));
    // The files it writes, each of which macOS gives an AppleDouble
    // companion on FAT32 and exFAT: first the ones every sync writes, its
    // `DATABASE_FILES` and its lock.
    let mut new_files = DATABASE_FILES + 1;
    let mut new_artwork = 0_u64;
    for track in &tracks {
        let key = crate::track_key(track.id, &track.source_path.to_string_lossy());
        let old = recorded.get(&key).copied();
        if old.is_none() && estimate.actions.get(&key).is_some_and(|a| matches!(a, crate::estimate::AudioAction::Copy | crate::estimate::AudioAction::Convert)) {
            let audio = crate::layout(track, 0).audio;
            let album = Path::new(&audio).parent().map(Path::to_path_buf).unwrap_or_default();
            folders.extend(album.ancestors().filter(|f| f.parent().is_some()).map(Path::to_path_buf));
            new_files += 1;
            if track.analysis_source.is_some() || !track.analysis.is_empty() {
                let analysis = std::path::PathBuf::from(crate::analysis_directory(&audio, &root_name));
                folders.extend(analysis.ancestors().filter(|f| f.parent().is_some()).map(Path::to_path_buf));
            }
        }
        let unchanged = old.is_some_and(|c| {
            incremental::analysis_recipe(track, &c.audio).is_ok_and(|r| previous.and_then(|m| m.generation.as_ref()).is_some_and(|g| g.analysis_recipes.get(&key) == Some(&r)))
                && c.analysis_extensions.iter().all(|e| trusted.contains(&format!("{}/ANLZ0000.{e}", c.anlz_dir.trim_start_matches('/'))))
        });
        if !unchanged {
            if let Some(base) = &track.analysis_source {
                for ext in ["DAT", "EXT", "2EX"] {
                    if let Ok(m) = std::fs::metadata(base.with_extension(ext)) { asset_bytes += on_disk(m.len() + 256); new_files += 1; }
                }
            } else {
                asset_bytes += track.analysis.iter().map(|(_, b)| on_disk(b.len() as u64 + 256)).sum::<u64>();
                new_files += track.analysis.len() as u64;
            }
        }
        if let Some(image) = &track.artwork {
            if !artwork_sources.insert(image.clone()) { continue; }
            let source_unchanged = previous.and_then(|m| m.generation.as_ref()).is_some_and(|g| {
                g.inputs.get(&image.to_string_lossy().into_owned()).is_some_and(|old| incremental::revision(image).is_ok_and(|r| r == *old))
                    && old.is_some_and(|c| !c.artwork.is_empty() && trusted.contains(c.artwork.trim_start_matches('/')))
            });
            if !source_unchanged {
                new_artwork += 1;
                for (_, medium) in crate::artwork_names(1) {
                    if let Ok(m) = std::fs::metadata(crate::artwork_source(image, medium)) { asset_bytes += on_disk(m.len()); new_files += 1; }
                }
            }
        }
    }
    if new_artwork > 0 {
        // Numbered from 1, a folder per `ARTWORK_PER_FOLDER` images.
        let artwork = Path::new("/").join(&root_name).join("Artwork");
        folders.extend((1..=new_artwork.div_ceil(u64::from(crate::ARTWORK_PER_FOLDER))).map(|n| artwork.join(format!("{n:05}"))));
        folders.insert(artwork);
    }
    // Folders new to the stick stay; every folder written into is made again
    // inside the publication's staging folder first, and both are there at
    // once until it is published.
    let new_folders = folders.iter().filter(|folder| !crate::under(destination, &folder.to_string_lossy()).is_dir()).count() as u64;
    let staging_folders = folders.len() as u64 + 2;
    asset_bytes += unit * new_folders;
    // macOS writes an AppleDouble `._` companion beside each file and folder
    // it creates on FAT32 and exFAT, a cluster each (#591).
    let companions = if appledouble(destination) { unit } else { 0 };
    asset_bytes += companions * (new_files + new_folders);
    let old_database_bytes: u64 = ["rekordbox/export.pdb", "rekordbox/exportExt.pdb", "rekordbox/exportLibrary.db", "rbxport/manifest.json"].iter()
        .map(|p| std::fs::metadata(root.join(p)).map_or(0, |m| on_disk(m.len()))).sum();
    let entry_count: usize = playlists.iter().map(|p| p.track_indices.len()).sum();
    // Each of the files takes at least a cluster, and part of one more at its end.
    let database_bytes = old_database_bytes.max(256 * 1024 + tracks.len() as u64 * 4096 + entry_count as u64 * 32 + playlists.len() as u64 * 1024)
        .saturating_add(DATABASE_FILES * unit);
    let old_replacements: u64 = previous.into_iter().flat_map(|m| &m.tracks).filter(|c| {
        selected.contains(&c.key()) && estimate.actions.get(&c.key()).is_some_and(|action| matches!(action, crate::estimate::AudioAction::Copy | crate::estimate::AudioAction::Convert))
    }).map(|c| std::fs::metadata(crate::under(destination, &c.audio)).map_or(0, |m| on_disk(m.len()))).sum();
    let staged = estimate.copy_allocated.saturating_add(asset_bytes).saturating_add(database_bytes);
    let reclaimed = estimate.free_allocated.saturating_add(old_database_bytes).saturating_add(old_replacements);
    // The staging folders, the publication's journal and lock files, and a
    // second copy of the databases: each is written through a temporary file
    // or journal, which is there beside it until it is renamed into place.
    let staging = (unit + companions).saturating_mul(staging_folders).saturating_add((64 * 1024).max(16 * unit)).saturating_add(database_bytes);
    let peak_bytes = staged.saturating_add(staging).saturating_add(estimate.uncertain_adoption_bytes);
    let net = i128::from(staged) - i128::from(reclaimed);
    let kept_existing = tracks.iter().filter(|t| t.source_unavailable).count();
    let uncertain_revisions = tracks.iter().any(|t| {
        let key = crate::track_key(t.id, &t.source_path.to_string_lossy());
        recorded.get(&key).is_some_and(|old| estimate.actions.get(&key).is_some_and(|a| matches!(a, crate::estimate::AudioAction::Copy | crate::estimate::AudioAction::Convert))
            && incremental::revision(&t.source_path).is_ok_and(|r| r.is_some_and(|r| r.size == old.size)))
    });
    let unknown_size = uncertain_revisions || estimate.uncertain_adoption_bytes > 0 || (options.compatibility.is_some() && tracks.iter().any(|t| t.duration_sec == 0 && !t.device.as_ref().is_some_and(|d| d.preserve)));
    Ok(SyncPlan { tracks, playlists, approximate: true, estimate, database_bytes, asset_bytes, peak_bytes,
        net_additional_bytes: i64::try_from(net).unwrap_or(if net < 0 { i64::MIN } else { i64::MAX }), unknown_size, kept_existing, device_additions })
}

/// The files a sync writes whatever it copies: the three databases, the two
/// playlist sync records, the four player settings files and rbxport's
/// manifest.
const DATABASE_FILES: u64 = 10;

/// The space a file of `bytes` takes on a filesystem that stores files in
/// `unit`-byte clusters. An empty file takes none.
pub(crate) fn allocation(bytes: u64, unit: u64) -> u64 { bytes.div_ceil(unit.max(1)).saturating_mul(unit.max(1)) }

/// The allocation unit (cluster) of the filesystem `destination` is on, at
/// least 4 KiB, which is what the estimate assumed before it asked.
///
/// A stick formatted with a large cluster stores every file in whole
/// clusters: exFAT on a stick over 32 GB uses 128 KiB by default, so each of
/// a track's three analysis files and its artwork takes 128 KiB however small
/// it is. A rekordbox export on a 62 GB FAT32 stick (64 KiB clusters) takes
/// 41.2 GB on disk for 40.5 GB of files [OBS #591, TRIODE2]. Counting 4 KiB
/// let a sync start that could not finish (#591).
pub fn allocation_unit(destination: &Path) -> u64 {
    // exFAT's largest cluster is 32 MiB; anything larger is not a cluster.
    fs2::allocation_granularity(destination).ok()
        .filter(|unit| unit.is_power_of_two() && *unit <= 32 << 20)
        .map_or(4096, |unit| unit.max(4096))
}

/// Whether macOS keeps an `AppleDouble` `._` companion for each file and folder
/// written to `destination`: it does on FAT32 and exFAT, which cannot hold
/// extended attributes, and every file an app writes carries one
/// (`com.apple.provenance`) [OBS macOS 26, #591].
#[cfg(target_os = "macos")]
#[allow(clippy::cast_sign_loss, reason = "a C name's bytes, whichever sign c_char has")]
fn appledouble(destination: &Path) -> bool {
    rustix::fs::statfs(destination).is_ok_and(|stat| {
        let name: Vec<u8> = stat.f_fstypename.iter().take_while(|c| **c != 0).map(|c| *c as u8).collect();
        matches!(name.as_slice(), b"msdos" | b"exfat")
    })
}

#[cfg(not(target_os = "macos"))]
fn appledouble(_: &Path) -> bool { false }

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn a_file_takes_whole_clusters() {
        assert_eq!(allocation(0, 131_072), 0);
        assert_eq!(allocation(1, 131_072), 131_072);
        assert_eq!(allocation(131_072, 131_072), 131_072);
        assert_eq!(allocation(131_073, 131_072), 262_144);
        assert_eq!(allocation(5000, 4096), 8192);
    }

    #[test]
    fn the_unit_is_the_filesystems_and_at_least_4_kib() {
        let dir = tempfile::tempdir().unwrap();
        let unit = allocation_unit(dir.path());
        assert!(unit >= 4096 && unit.is_power_of_two(), "{unit}");
        assert_eq!(allocation_unit(&dir.path().join("not there")), 4096);
    }

    fn track(root: &Path, id: u64, audio: usize) -> SourceTrack {
        let source = root.join(format!("track-{id}.mp3"));
        std::fs::write(&source, vec![1_u8; audio]).unwrap();
        let base = root.join(format!("analysis-{id}/ANLZ0000.DAT"));
        std::fs::create_dir_all(base.parent().unwrap()).unwrap();
        std::fs::write(&base, vec![2_u8; 3000]).unwrap();
        std::fs::write(base.with_extension("EXT"), vec![3_u8; 20_000]).unwrap();
        std::fs::write(base.with_extension("2EX"), vec![4_u8; 150_000]).unwrap();
        SourceTrack { id, source_path: source, title: format!("Track {id}"), artist: "Artist".into(), album: "Album".into(),
            duration_sec: 120, sample_rate: 44_100, analysis_source: Some(base), ..Default::default() }
    }

    /// What a fresh sync of two tracks takes on an exFAT stick with 128 KiB
    /// clusters, worked out by hand from the cluster rule: the stick holds the
    /// whole of every cluster a file starts.
    #[test]
    fn a_large_cluster_stick_counts_every_file_in_whole_clusters() {
        const UNIT: u64 = 128 * 1024;
        let source = tempfile::tempdir().unwrap();
        let usb = tempfile::tempdir().unwrap();
        let tracks = vec![track(source.path(), 1, 3_000_000), track(source.path(), 2, 5_000_001)];
        let playlists = vec![SourcePlaylist { id: 10, name: "Playlist".into(), track_indices: vec![0, 1], ..Default::default() }];
        let sync = crate::SyncSource { db_id: 123, ..Default::default() };
        let options = ExportOptions { sync: Some(&sync), ..Default::default() };
        let before = Snapshot::read(usb.path()).unwrap();
        let at = |unit| from_baseline_in(usb.path(), &before, None, &tracks, &playlists, &options, None, unit).unwrap();
        let (small, large) = (at(4096), at(UNIT));

        // Audio: 3,000,000 B is 23 clusters, 5,000,001 B is 39.
        assert_eq!(large.estimate.copy_bytes, 8_000_001);
        assert_eq!(large.estimate.copy_allocated, (23 + 39) * UNIT);
        assert_eq!(small.estimate.copy_allocated, (733 + 1221) * 4096);
        // Analysis: DAT, EXT and 2EX (each 256 bytes longer once written for
        // the stick's path) are 1 + 1 + 2 clusters a track. Folders: PIONEER
        // with its rekordbox, rbxport and USBANLZ; Contents with the artist's
        // and the album's; each track's analysis folder and the shard folder
        // above it.
        let shards = tracks.iter().map(|t| {
            let audio = crate::layout(t, 0).audio;
            Path::new(&crate::analysis_directory(&audio, "PIONEER")).parent().unwrap().to_path_buf()
        }).collect::<std::collections::BTreeSet<_>>().len() as u64;
        let folders = 4 + 3 + shards + 2;
        assert_eq!(large.asset_bytes, (2 * (1 + 1 + 2) + folders) * UNIT);
        assert_eq!(small.asset_bytes, 2 * (1 + 5 + 37) * 4096 + folders * 4096);
        // The databases' estimate, plus a cluster for each of their 10 files.
        assert_eq!(large.database_bytes - small.database_bytes, DATABASE_FILES * (UNIT - 4096));
        let staged = (23 + 39 + 8 + folders) * UNIT + large.database_bytes;
        assert_eq!(large.net_additional_bytes, i64::try_from(staged).unwrap());
        // While staging: every folder again inside the staging folder, which
        // with the journal is two more; 16 clusters for the journal's files;
        // the databases' temporary copies.
        assert_eq!(large.peak_bytes, staged + (folders + 2) * UNIT + 16 * UNIT + large.database_bytes);
        assert!(large.peak_bytes > small.peak_bytes);
    }
}
