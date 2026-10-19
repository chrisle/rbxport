//! Retain device-only playlists/tracks and history while replacing the selected
//! master-library portion. Device identities are never inferred from titles.
use crate::{
    snapshot::{Library, Snapshot},
    sync_record::SyncRecord,
    DeviceTrack, ExportError, Manifest, Result, SourcePlaylist, SourceTrack,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
pub(crate) const DEVICE_PLAYLIST_ID_BASE: u64 = 1 << 63;
fn sql(e: impl std::fmt::Display) -> ExportError {
    ExportError::OneLibrary(e.to_string())
}

/// Establish one playlist ID namespace before planning or checking removals.
/// [OBS: fixture] the two USB databases can describe the same tree with different
/// IDs. Each sync file is interpreted against its own database, never the other.
/// Existing rbxport baselines already use one namespace and keep their edit checks.
pub(crate) fn playlist_namespace(root: &Path, before: &mut Snapshot, previous: Option<&Manifest>, db_id: u64) -> Option<SyncRecord> {
    let dir = crate::export_root(root).join("rekordbox");
    let read = |name: &str, library: &Library| std::fs::read(dir.join(name)).ok()
        .and_then(|bytes| crate::sync_record::parse(&bytes))
        .filter(|r| r.db_id == db_id && db_id != 0)
        .map(|mut r| {
            let mut counts = BTreeMap::<u32, usize>::new();
            for id in r.device_ids.values() { *counts.entry(*id).or_default() += 1; }
            r.device_ids.retain(|master, id| counts.get(id) == Some(&1) && library.playlists.iter().any(|p| p.id == *id && p.folder == r.folders.contains(master)));
            r
        });
    let legacy_record = before.legacy.as_ref().and_then(|library| read("playlists3.sync", library));
    let plus_record = before.one.as_ref().and_then(|library| read("playlists3Plus.sync", library));
    if previous.and_then(|m| m.baseline.as_ref()).is_some() {
        return plus_record.or(legacy_record);
    }
    let (Some(legacy), Some(one)) = (&mut before.legacy, &before.one) else {
        return plus_record.or(legacy_record);
    };
    let mut pairs = BTreeMap::new();
    if let (Some(left), Some(right)) = (&legacy_record, &plus_record) {
        let unique = |record: &SyncRecord, id: u32| record.device_ids.values().filter(|value| **value == id).count() == 1;
        // Place parents first, so a same-named row in another folder cannot
        // silently stand in for this row. Conflicting contents remain separate.
        loop {
            let count = pairs.len();
            for (master, id) in &left.device_ids {
                let Some(other_id) = right.device_ids.get(master) else { continue };
                if pairs.contains_key(id) || !unique(left, *id) || !unique(right, *other_id) { continue; }
                let Some(a) = legacy.playlists.iter().find(|p| p.id == *id) else { continue };
                let Some(b) = one.playlists.iter().find(|p| p.id == *other_id) else { continue };
                let same_parent = (a.parent == 0 && b.parent == 0) || pairs.get(&a.parent) == Some(&b.parent);
                if same_parent && a.name == b.name && a.folder == b.folder && a.tracks == b.tracks {
                    pairs.insert(*id, *other_id);
                }
            }
            if pairs.len() == count { break; }
        }
    }
    let floor = previous.into_iter().flat_map(|m| &m.playlists).map(|p| p.export_id).max().unwrap_or(0);
    let ids = crate::snapshot::legacy_playlist_ids_with(legacy, one, floor, &pairs);
    for node in &mut legacy.playlists {
        node.id = ids.get(&node.id).copied().unwrap_or(node.id);
        if node.parent != 0 { node.parent = ids.get(&node.parent).copied().unwrap_or(node.parent); }
    }
    plus_record.or_else(|| legacy_record.map(|mut record| {
        record.device_ids = record.device_ids.into_iter().filter_map(|(master, id)| ids.get(&id).map(|mapped| (master, *mapped))).collect();
        record
    }))
}

#[allow(clippy::too_many_arguments, reason = "shared validated playlist record accompanies the snapshot")]
pub fn prepare(
    root: &Path,
    before: &Snapshot,
    previous: Option<&Manifest>,
    tracks: &[SourceTrack],
    playlists: &[SourcePlaylist],
    db_id: u64,
    keep_unlisted: bool,
    record: Option<&SyncRecord>,
) -> Result<Prepared> {
    // The library's entries, by track key, before any track is tied to the
    // device row it came from.
    let keys: Vec<String> = tracks.iter().map(|t| crate::track_key(t.id, &t.source_path.to_string_lossy())).collect();
    let mut tracks = tracks.to_vec();
    let mut playlists = playlists.to_vec();
    // A playlist carried under a new id must not take one an earlier export
    // recorded: that would read as the library's playlist, and be taken off.
    // Nor one the stick held at that export and has since deleted: that
    // would read as a deletion undone (`check_database_changes`).
    let baseline = previous.and_then(|m| m.baseline.as_ref());
    let recorded = previous
        .into_iter()
        .flat_map(|m| m.playlists.iter().map(|p| p.export_id))
        .chain(record.iter().flat_map(|r| r.device_ids.values().copied()))
        .chain(
            baseline
                .into_iter()
                .flat_map(|b| b.legacy.iter().chain(&b.one))
                .flat_map(|l| l.playlists.iter().map(|p| p.id)),
        )
        .max()
        .unwrap_or(0);
    let Some(current) = before.merged_library_above(recorded) else {
        return Ok(Prepared { tracks, playlists, device_additions: BTreeSet::new() });
    };
    let existing_sources = sources(root, before)?;
    identify_tracks(&mut tracks, before, previous, db_id, &existing_sources);
    identify_playlists(&mut playlists, previous, record, before);
    let (mut additions, dropped) = device_additions(before, previous, &keys, &playlists);
    if previous.is_none() {
        additions.extend(rekordbox_device_additions(before, record, &tracks, &playlists));
    }
    let kept: BTreeSet<u32> = additions.values().flatten().flatten().copied().collect();
    retain_device_only(root, &current, &mut tracks, &mut playlists, before, previous, &existing_sources, record, keep_unlisted, &kept, &dropped)?;
    keep_device_additions(&tracks, &mut playlists, &additions)?;
    if keep_unlisted {
        keep_stick_order(&mut playlists, previous);
    }
    Ok(Prepared { tracks, playlists, device_additions: additions.into_keys().collect() })
}

/// What [`prepare`] makes of a selection.
#[derive(Debug)]
pub struct Prepared {
    pub tracks: Vec<SourceTrack>,
    pub playlists: Vec<SourcePlaylist>,
    /// The device ids of the selected playlists the stick holds with
    /// entries added on the stick (see [`device_additions`]): each is
    /// written either as the stick has it or as the library has it, and
    /// either way on purpose, so the check for device edits a sync would
    /// overwrite leaves them out.
    pub device_additions: BTreeSet<u32>,
}

/// The selected playlists whose copy on the stick is the library's, as the
/// last sync wrote it, with entries added on the stick, by device id; and
/// for each, the entries to write: the stick's (`Some`) while the library's
/// playlist is unchanged since that sync, and the library's (`None`) once it
/// has changed.
///
/// That is what rekordbox's sync does with a track added to a synced
/// playlist in its Devices tree [OBS rekordbox 7.2.14.0323 for Windows,
/// chris-win11 2026-10-12, #309, `docs/reference/usb-export-db.md` "What a
/// later sync does to a playlist edited on the device"]: a sync with the
/// library's playlist unchanged keeps the added entry; one after the
/// library's playlist changed rewrites it from the library, and the added
/// track stays on the stick in no playlist. It never adds the entry to the
/// library. Its record keeps the time the library playlist's entries last
/// changed; [ASSUME] it rewrites a playlist when that time has moved. The
/// library's entries are compared here instead, as the last sync recorded
/// them (`ManifestPlaylist::library_entries`). An older record has none; the
/// build that wrote it put every selected playlist on the stick as the
/// library had it, so the stick's copy at that sync stands in.
///
/// Only an addition was observed: the stick's playlist must hold the
/// library's entries in their order, with the same name and place as the
/// last sync left it, and both of the stick's libraries must agree on it.
/// A playlist edited any other way on the stick (an entry of the library's
/// removed or moved, a rename, an edit in one library alone, as a player
/// makes) is left out, and the sync stops on it as before [UNKNOWN: what
/// rekordbox does with those].
///
/// Also returns the device ids of the added entries a playlist rewritten from
/// the library takes out. rekordbox leaves such a track on the stick, in no
/// playlist; so does [`retain_device_only`], even when it is one an earlier
/// export put there for a playlist this sync no longer selects (#471).
fn device_additions(
    before: &Snapshot,
    previous: Option<&Manifest>,
    keys: &[String],
    playlists: &[SourcePlaylist],
) -> (BTreeMap<u32, Option<Vec<u32>>>, BTreeSet<u32>) {
    let mut out = BTreeMap::new();
    let mut dropped = BTreeSet::new();
    let Some((previous, baseline)) = previous.and_then(|m| m.baseline.as_ref().map(|b| (m, b))) else { return (out, dropped) };
    // A library track's device id, as the last sync recorded it: the track
    // it exported before one it only carried.
    let mut export_ids: BTreeMap<String, u32> = BTreeMap::new();
    for carried in [false, true] {
        for t in previous.tracks.iter().filter(|t| (t.preserved == Some(true)) == carried) {
            export_ids.entry(t.key()).or_insert(t.export_id);
        }
    }
    let keys_by_id: BTreeMap<u32, String> = export_ids.iter().map(|(key, id)| (*id, key.clone())).collect();
    for p in playlists.iter().filter(|p| !p.folder && !p.device_only && p.device_id != 0 && p.id != 0 && p.id < DEVICE_PLAYLIST_ID_BASE) {
        let Some(recorded) = previous.playlists.iter().find(|m| m.export_id == p.device_id && m.library_id == p.id && !m.device_only && !m.folder) else { continue };
        let Some((stick, at_sync)) = stick_copy(before, baseline, p.device_id) else { continue };
        let library_then: Option<Vec<String>> = recorded.library_entries.clone()
            .or_else(|| at_sync.iter().map(|id| keys_by_id.get(id).cloned()).collect());
        let Some(library_then) = library_then else { continue };
        // The library's entries at that sync, as the stick numbers them.
        let Some(written) = library_then.iter().map(|key| export_ids.get(key).copied()).collect::<Option<Vec<u32>>>() else { continue };
        if stick == written || !is_subsequence(&written, &stick) {
            continue;
        }
        let library_now: Option<Vec<&String>> = p.track_indices.iter().map(|i| keys.get(*i)).collect();
        let unchanged = library_now.is_some_and(|now| now.len() == library_then.len() && now.iter().zip(&library_then).all(|(a, b)| *a == b));
        if !unchanged {
            dropped.extend(stick.iter().filter(|id| !written.contains(id)));
        }
        out.insert(p.device_id, unchanged.then_some(stick));
    }
    (out, dropped)
}

/// How far apart, in milliseconds, the `Timestamp` rekordbox's sync record
/// keeps for a playlist and the library entries' latest change time
/// (`SourcePlaylist::entries_changed_ms`) may be and still be the same
/// change. rekordbox recorded `1791846371399` for an entry created at
/// `.399` (0 ms) and `1791847083155` for one created at `.154` (1 ms)
/// [OBS #309]: its own clock against the millisecond the row keeps. Any
/// library edit made since is later than the sync that wrote the record,
/// which takes far longer than this.
const SAME_CHANGE_MS: u64 = 2;

/// [`device_additions`] for a stick rekordbox synced and rbxport has not:
/// with no record of its own, it goes by rekordbox's. Its sync record keeps,
/// for each ticked playlist, the time the library playlist's entries last
/// changed [OBS rekordbox 7.2.14, #309]. While the library's time is still
/// that one, the playlist is written as the stick has it, entries added on
/// the stick included, as rekordbox's own sync keeps them; once it has
/// moved, from the library, as before.
///
/// As for a stick rbxport synced, only an addition counts: the stick's
/// playlist must hold the library's entries now, in their order, and both
/// of its libraries must agree on it. Anything else is written from the
/// library, as a first sync from here always has [UNKNOWN: what rekordbox
/// does with a removal, a reorder or an edit in one library].
fn rekordbox_device_additions(
    before: &Snapshot,
    record: Option<&SyncRecord>,
    tracks: &[SourceTrack],
    playlists: &[SourcePlaylist],
) -> BTreeMap<u32, Option<Vec<u32>>> {
    let mut out = BTreeMap::new();
    let Some(record) = record else { return out };
    for p in playlists.iter().filter(|p| !p.folder && !p.device_only && p.device_id != 0 && p.id != 0 && p.id < DEVICE_PLAYLIST_ID_BASE) {
        let (Some(recorded), Some(changed)) = (record.timestamps.get(&p.id), p.entries_changed_ms) else { continue };
        if recorded.abs_diff(changed) > SAME_CHANGE_MS {
            continue;
        }
        let Some(stick) = stick_now(before, p.device_id) else { continue };
        // The library's entries as the stick numbers them; one the stick
        // does not hold means the library's playlist has changed after all.
        let Some(library) = p.track_indices.iter().map(|i| tracks.get(*i).and_then(|t| t.device.as_ref()).map(|d| d.id)).collect::<Option<Vec<u32>>>() else { continue };
        if stick == library || !is_subsequence(&library, &stick) {
            continue;
        }
        out.insert(p.device_id, Some(stick));
    }
    out
}

/// The stick's playlist `id`, when every library the stick holds has it,
/// as a playlist, with the same entries.
fn stick_now(before: &Snapshot, id: u32) -> Option<Vec<u32>> {
    let mut found: Option<Vec<u32>> = None;
    for library in [&before.legacy, &before.one].into_iter().flatten() {
        let now = library.playlists.iter().find(|p| p.id == id)?;
        if now.folder || found.as_ref().is_some_and(|tracks| *tracks != now.tracks) {
            return None;
        }
        found.get_or_insert_with(|| now.tracks.clone());
    }
    found
}

/// The stick's playlist `id` now and as the last sync left it, when every
/// library the stick holds has it with the same entries now, and with the
/// name and place it had then.
fn stick_copy(before: &Snapshot, baseline: &Snapshot, id: u32) -> Option<(Vec<u32>, Vec<u32>)> {
    let mut found: Option<(Vec<u32>, Vec<u32>)> = None;
    for (now, then) in [(&before.legacy, &baseline.legacy), (&before.one, &baseline.one)] {
        let Some(now) = now else { continue };
        let now = now.playlists.iter().find(|p| p.id == id)?;
        let then = then.as_ref()?.playlists.iter().find(|p| p.id == id)?;
        if now.name != then.name || now.parent != then.parent || now.folder || then.folder {
            return None;
        }
        if found.as_ref().is_some_and(|(tracks, _)| *tracks != now.tracks) {
            return None;
        }
        found.get_or_insert_with(|| (now.tracks.clone(), then.tracks.clone()));
    }
    found
}

/// Whether `part`'s entries are all in `whole`, in the same order.
fn is_subsequence(part: &[u32], whole: &[u32]) -> bool {
    let mut rest = whole.iter();
    part.iter().all(|id| rest.any(|other| other == id))
}

/// Points each playlist [`device_additions`] keeps as the stick has it at
/// the stick's entries. [`retain_device_only`] has already carried every
/// track they name.
fn keep_device_additions(
    tracks: &[SourceTrack],
    playlists: &mut [SourcePlaylist],
    additions: &BTreeMap<u32, Option<Vec<u32>>>,
) -> Result<()> {
    let mut positions: BTreeMap<u32, usize> = BTreeMap::new();
    for (i, track) in tracks.iter().enumerate() {
        if let Some(device) = &track.device {
            positions.entry(device.id).or_insert(i);
        }
    }
    for playlist in playlists.iter_mut() {
        let Some(Some(entries)) = additions.get(&playlist.device_id) else { continue };
        playlist.track_indices = entries
            .iter()
            .map(|id| positions.get(id).copied().ok_or_else(|| ExportError::Conflict(format!("Playlist '{}' on the USB names a track it no longer holds", playlist.name))))
            .collect::<Result<_>>()?;
    }
    Ok(())
}

/// The stick's rows [`prepare`] will tie to the selection, and so write afresh
/// from the library: the device tracks `identify_tracks` names by their
/// master ids, and the device playlists `identify_playlists` names. Worked
/// out from the snapshot alone, before any analysis file is read.
pub fn rewritten(
    record: Option<&SyncRecord>,
    before: &Snapshot,
    previous: Option<&Manifest>,
    tracks: &[SourceTrack],
    playlists: &[SourcePlaylist],
    db_id: u64,
) -> crate::snapshot::Rewritten {
    let selected: BTreeSet<u64> = tracks.iter().filter(|t| t.id != 0).map(|t| t.id).collect();
    let mut out = crate::snapshot::Rewritten::default();
    // The first device row per library track, as `identify_tracks` takes it.
    let mut taken = BTreeSet::new();
    for (device, (db, content)) in &before.identity {
        if *db == db_id && selected.contains(content) && taken.insert(*content) {
            out.tracks.insert(*device);
        }
    }
    // A selected track with no such row, that an earlier export recorded.
    if let Some(m) = previous.filter(|m| m.db_id == db_id) {
        let recorded: BTreeMap<String, u32> = m.tracks.iter().map(|t| (t.key(), t.export_id)).collect();
        for track in tracks.iter().filter(|t| t.id == 0 || !taken.contains(&t.id)) {
            if let Some(id) = recorded.get(&crate::track_key(track.id, &track.source_path.to_string_lossy())) {
                out.tracks.insert(*id);
            }
        }
    }
    let mut playlists = playlists.to_vec();
    identify_playlists(&mut playlists, previous, record, before);
    out.playlists = playlists.iter().map(|p| p.device_id).filter(|id| *id != 0).collect();
    out
}

/// Puts the playlists the previous export listed back in the order it listed
/// them, with the ones new to the stick after them in the selection's order:
/// an export that adds to a stick does not move what it already holds.
fn keep_stick_order(playlists: &mut [SourcePlaylist], previous: Option<&Manifest>) {
    let Some(previous) = previous else { return };
    let place: BTreeMap<u32, usize> = previous
        .playlists
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, old)| old.export_id != 0)
        .map(|(i, old)| (old.export_id, i))
        .collect();
    playlists.sort_by_key(|p| place.get(&p.device_id).copied().unwrap_or(usize::MAX));
}

/// Ties a track already in the selection to the device row it came from, so
/// its file/analysis are kept rather than treated as a fresh import.
fn identify_tracks(
    tracks: &mut [SourceTrack],
    before: &Snapshot,
    previous: Option<&Manifest>,
    db_id: u64,
    existing_sources: &BTreeMap<u32, SourceTrack>,
) {
    let mut identities = BTreeMap::new();
    for (id, (db, content)) in &before.identity {
        if *db == db_id && *content != 0 { identities.entry(*content).or_insert(*id); }
    }
    let recorded: BTreeMap<_, _> = previous.filter(|m| m.db_id == db_id).into_iter().flat_map(|m| &m.tracks)
        .map(|t| (t.key(), t.export_id)).collect();
    let exported: BTreeSet<_> = previous.into_iter().flat_map(|m| &m.tracks).filter(|t| t.preserved != Some(true)).map(|t| t.export_id).collect();
    for track in tracks {
        if track.device.is_some() {
            continue;
        }
        let identity = identities.get(&track.id).copied().or_else(|| recorded.get(&crate::track_key(track.id, &track.source_path.to_string_lossy())).copied());
        if let Some(id) = identity {
            if let Some(source) = existing_sources.get(&id) {
                track.fallback = Some(Box::new(source.clone()));
                let source_changed = previous.and_then(|m| m.generation.as_ref())
                    .and_then(|g| g.inputs.get(&track.source_path.to_string_lossy().into_owned()))
                    .is_some_and(|old| crate::incremental::revision(&track.source_path).is_ok_and(|now| now != *old));
                let unavailable = !track.source_path.is_file() || (source_changed && std::fs::File::open(&track.source_path).is_err());
                if unavailable && source.source_path.is_file() {
                    let master_id = track.id;
                    *track = source.clone();
                    track.id = master_id;
                    track.source_unavailable = true;
                    if let Some(device) = &mut track.device {
                        device.master_db_id = db_id;
                        device.master_content_id = master_id;
                        device.preserve = true;
                        device.exported = exported.contains(&id);
                    }
                    continue;
                }
                let mut device = source.device.clone().unwrap_or_default();
                device.preserve = false;
                device.master_db_id = db_id;
                device.master_content_id = track.id;
                track.device = Some(device);
            }
        }
    }
}

/// Ties a playlist already in the selection to the device row it came from.
fn identify_playlists(
    playlists: &mut [SourcePlaylist],
    previous: Option<&Manifest>,
    record: Option<&SyncRecord>,
    before: &Snapshot,
) {
    for p in playlists {
        if p.device_id != 0 {
            continue;
        }
        p.device_id = previous
            .and_then(|m| {
                m.playlists
                    .iter()
                    .find(|old| old.library_id == p.id && p.id != 0 && p.id < DEVICE_PLAYLIST_ID_BASE)
            })
            .map(|p| p.export_id)
            .or_else(|| record.and_then(|r| r.device_ids.get(&p.id).copied()))
            .filter(|id| before.legacy.iter().chain(&before.one).any(|library| library.playlists.iter().any(|node| node.id == *id && node.folder == p.folder)))
            .unwrap_or(0);
    }
}

/// Keeps whatever the device holds that this sync's selection does not name:
/// playlists nobody chose (and their ancestor folders), and the tracks those
/// playlists or the play history still reference.
///
/// A playlist an earlier export put there (`owned`) is taken off unless
/// `keep_unlisted`, in which case it stays as the stick has it and is still
/// recorded as the library's, under the library id it was exported from.
///
/// `kept` names the tracks of selected playlists written as the stick has
/// them ([`device_additions`]), which are carried too. `dropped` names the
/// entries added on the stick that a playlist rewritten from the library
/// takes out: those tracks stay on the stick as the stick's, as rekordbox
/// leaves them, even one an earlier export put there for a playlist no
/// longer selected, which the #317 cleanup would otherwise take off.
#[allow(clippy::too_many_arguments, reason = "everything reconciliation already gathered, passed through")]
fn retain_device_only(
    root: &Path,
    current: &Library,
    tracks: &mut Vec<SourceTrack>,
    playlists: &mut Vec<SourcePlaylist>,
    before: &Snapshot,
    previous: Option<&Manifest>,
    existing_sources: &BTreeMap<u32, SourceTrack>,
    record: Option<&SyncRecord>,
    keep_unlisted: bool,
    kept: &BTreeSet<u32>,
    dropped: &BTreeSet<u32>,
) -> Result<()> {
    // Device playlist id -> the library id it was exported from.
    // A foreign sync record establishes correspondence, not permission to
    // delete unchecked playlists on the first adoption. Only our own baseline
    // establishes ownership; carried playlists remain device-only thereafter.
    let owned: BTreeMap<u32, u64> = previous
        .filter(|m| m.baseline.is_some())
        .into_iter().flat_map(|m| &m.playlists)
        .filter(|p| !p.device_only && p.export_id != 0)
        .map(|p| (p.export_id, p.library_id)).collect();
    // Kept as the library's: owned, with a library id the selection does not
    // already use for another playlist.
    let kept_owned: BTreeMap<u32, u64> = if keep_unlisted {
        owned
            .iter()
            .filter(|(_, library)| **library != 0 && !playlists.iter().any(|n| n.id == **library))
            .map(|(device, library)| (*device, *library))
            .collect()
    } else {
        BTreeMap::new()
    };
    let retained: Vec<_> = current
        .playlists
        .iter()
        .filter(|p| (!owned.contains_key(&p.id) || kept_owned.contains_key(&p.id)) && !playlists.iter().any(|n| n.device_id == p.id))
        .collect();
    // Preserve a known correspondence without granting deletion ownership.
    // This lets a later selection adopt a carried playlist instead of duplicating it.
    let identities: BTreeMap<u32, u64> = record.into_iter().flat_map(|r| &r.device_ids)
        .map(|(master, device)| (*device, *master))
        .chain(previous.into_iter().flat_map(|m| &m.playlists)
            .filter(|p| p.library_id != 0 && p.library_id < DEVICE_PLAYLIST_ID_BASE)
            .map(|p| (p.export_id, p.library_id)))
        .filter(|(_, master)| !playlists.iter().any(|p| p.id == *master)).collect();
    let retained_id = |device: u32| kept_owned.get(&device).or_else(|| identities.get(&device)).copied().unwrap_or(DEVICE_PLAYLIST_ID_BASE + u64::from(device));
    let mut needed: BTreeSet<u32> = retained
        .iter()
        .flat_map(|p| &p.tracks)
        .copied()
        .chain(before.history.iter().flat_map(|h| &h.tracks).copied())
        .chain(kept.iter().copied())
        .chain(dropped.iter().copied())
        .collect();
    // What an earlier export put there itself. A track it only carried
    // (`preserved`) is still another writer's: counting it as ours would keep
    // it for one sync and drop it, unannounced, at the next (#317). An older
    // record has no such flag; `carried_entry` infers it.
    let previously_owned: BTreeSet<u32> = previous
        .map(|m| m.tracks.iter().filter(|t| !crate::carried_entry(root, t)).map(|t| t.export_id).collect())
        .unwrap_or_default();
    // Tracks that arrived through another writer belong to the USB, not to our
    // previous selection. Retain them even when no playlist references them,
    // as rekordbox does: its export only adds and updates the tracks it is
    // given and never revisits the rest of the device library.
    needed.extend(
        current
            .tracks
            .iter()
            .filter(|t| !previously_owned.contains(&t.id))
            .map(|t| t.id),
    );
    let mut positions: BTreeMap<u32, usize> = tracks
        .iter()
        .enumerate()
        .filter_map(|(i, t)| t.device.as_ref().map(|d| (d.id, i)))
        .collect();
    for id in needed {
        if positions.contains_key(&id) {
            continue;
        }
        let mut track = existing_sources
            .get(&id)
            .ok_or_else(|| {
                ExportError::Conflict(format!(
                    "Device playlist/history references missing track {id}"
                ))
            })?
            .clone();
        // A track an earlier export put there that a device playlist or the
        // history still names: carried for now, but still ours to take off
        // once nothing names it. One the stick added to a playlist this sync
        // rewrites is the stick's from now on.
        if let Some(device) = track.device.as_mut() {
            device.exported = previously_owned.contains(&id) && !dropped.contains(&id);
        }
        positions.insert(id, tracks.len());
        tracks.push(track);
    }
    // Preserve ancestor folders of device-only playlists too, including folders
    // shared with master playlists that have just been deselected.
    let mut retained_ids: BTreeSet<_> = retained.iter().map(|p| p.id).collect();
    let mut parents: Vec<_> = retained.iter().map(|p| p.parent).collect();
    while let Some(id) = parents.pop() {
        if id == 0 || playlists.iter().any(|p| p.device_id == id) || !retained_ids.insert(id) {
            continue;
        }
        let p = current
            .playlists
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| ExportError::Conflict("Missing device playlist ancestor".into()))?;
        parents.push(p.parent);
    }
    for p in current
        .playlists
        .iter()
        .filter(|p| retained_ids.contains(&p.id))
    {
        let parent_id = if p.parent == 0 {
            0
        } else {
            playlists
                .iter()
                .find(|n| n.device_id == p.parent)
                .map_or_else(|| retained_id(p.parent), |n| n.id)
        };
        playlists.push(SourcePlaylist {
            id: retained_id(p.id),
            device_id: p.id,
            device_only: !kept_owned.contains_key(&p.id),
            name: p.name.clone(),
            parent_id,
            folder: p.folder,
            track_indices: p
                .tracks
                .iter()
                .map(|id| {
                    positions.get(id).copied().ok_or_else(|| {
                        ExportError::Conflict("Unresolved device playlist track".into())
                    })
                })
                .collect::<Result<_>>()?,
            entries_changed_ms: None,
        });
    }
    Ok(())
}

/// Read complete track metadata from whichever database exists. Both formats
/// share the audio and analysis files; only the explicit identity map is trusted.
fn sources(root: &Path, before: &Snapshot) -> Result<BTreeMap<u32, SourceTrack>> {
    let manifest = Manifest::load(root);
    if let Some(generation) = manifest.as_ref().and_then(|m| m.generation.as_ref())
        .filter(|g| !g.device_sources.is_empty() && crate::incremental::databases(root).is_ok_and(|signatures| signatures == g.databases)) {
        let mut sources = generation.device_sources.clone();
        for source in sources.values_mut() {
            source.source_path = crate::checked_under(root, &source.source_path.to_string_lossy())?;
            if let Some(path) = &source.analysis_source { source.analysis_source = Some(crate::checked_under(root, &path.to_string_lossy())?); }
            if let Some(path) = &source.artwork { source.artwork = Some(crate::checked_under(root, &path.to_string_lossy())?); }
        }
        return Ok(sources);
    }
    read_sources(root, before)
}

/// Normalized metadata from the independently parsed staged generation.
/// Persist relative paths so a volume rename cannot strand its cached rows.
pub(crate) fn capture_sources(root: &Path, before: &Snapshot) -> Result<BTreeMap<u32, SourceTrack>> {
    let mut sources = read_sources(root, before)?;
    for source in sources.values_mut() {
        source.source_path = source.source_path.strip_prefix(root).map_err(std::io::Error::other)?.to_owned();
        if let Some(path) = &source.analysis_source { source.analysis_source = Some(path.strip_prefix(root).map_err(std::io::Error::other)?.to_owned()); }
        if let Some(path) = &source.artwork { source.artwork = Some(path.strip_prefix(root).map_err(std::io::Error::other)?.to_owned()); }
    }
    Ok(sources)
}

fn read_sources(root: &Path, before: &Snapshot) -> Result<BTreeMap<u32, SourceTrack>> {
    let mut out = BTreeMap::new();
    let manifest = Manifest::load(root);
    let dir = crate::export_root(root).join("rekordbox");
    if before.one.is_some() {
        sources_from_one(root, &dir, &mut out)?;
    }
    if before.legacy.is_some() {
        sources_from_legacy(root, &dir, &mut out)?;
    }
    fill_source_details(root, before, manifest.as_ref(), &mut out)?;
    Ok(out)
}

/// A track's `analysedBits` as the unsigned flags byte this crate writes.
///
/// `export.pdb` keeps the same flags in one byte at row offset `0x56` [OBS:
/// docs/reference/usb-export-db.md, a rekordbox 7 stick]. Users' sticks hold
/// negative values here [OBS: #545, #583 "Integer -23 out of range at index
/// 23", index 23 being this column], taken to be that byte stored signed,
/// -23 for `0xE9` [ASSUME]. Reading it as `u32` stopped the whole sync
/// before it changed anything, so keep the byte instead.
fn analysed_bits(value: i64) -> u32 {
    u32::try_from(value).unwrap_or_else(|_| u32::from(value.to_le_bytes()[0]))
}

/// `OneLibrary`'s share of [`sources`]: every track it has metadata for.
fn sources_from_one(root: &Path, dir: &Path, out: &mut BTreeMap<u32, SourceTrack>) -> Result<()> {
    let db = rbl_onelibrary::ExportLibrary::open_read_only(&dir.join("exportLibrary.db"))
        .map_err(sql)?;
    let mut q = db.connection().prepare("SELECT c.content_id, COALESCE(c.title,''), COALESCE(a.name,''), COALESCE(al.name,''), COALESCE(g.name,''), COALESCE(l.name,''), COALESCE(k.name,''), COALESCE(c.djComment,''), COALESCE(c.dateAdded,''), COALESCE(c.releaseDate,''), COALESCE(c.bpmx100,0), COALESCE(c.length,0), COALESCE(c.rating,0), COALESCE(c.color_id,0), COALESCE(c.releaseYear,0), COALESCE(c.bitrate,0), COALESCE(c.samplingRate,0), COALESCE(c.fileSize,0), COALESCE(i.path,''), COALESCE(c.trackNo,0), COALESCE(c.discNo,0), COALESCE(c.bitDepth,0), COALESCE(c.djPlayCount,0), COALESCE(c.analysedBits,0), COALESCE(c.isHotCueAutoLoadOn,0), COALESCE(c.dateCreated,''), COALESCE(c.isrc,'') FROM content c LEFT JOIN artist a ON a.artist_id=c.artist_id_artist LEFT JOIN album al ON al.album_id=c.album_id LEFT JOIN genre g ON g.genre_id=c.genre_id LEFT JOIN label l ON l.label_id=c.label_id LEFT JOIN key k ON k.key_id=c.key_id LEFT JOIN image i ON i.image_id=c.image_id").map_err(sql)?;
    let rows = q
        .query_map([], |r| {
            Ok((
                r.get::<_, u32>(0)?,
                SourceTrack {
                    metadata: rbl_core::ExportMetadata {
                        track_number: r.get(19)?, disc_number: r.get(20)?, bit_depth: r.get(21)?,
                        play_count: r.get(22)?, analysed: analysed_bits(r.get(23)?), hot_cue_auto_load: r.get(24)?,
                        date_created: r.get(25)?, isrc: r.get(26)?,
                    },
                    title: r.get(1)?,
                    artist: r.get(2)?,
                    album: r.get(3)?,
                    genre: r.get(4)?,
                    label: r.get(5)?,
                    key: r.get(6)?,
                    comment: r.get(7)?,
                    date_added: r.get(8)?,
                    release_date: r.get(9)?,
                    bpm_x100: r.get(10)?,
                    duration_sec: r.get(11)?,
                    rating: crate::snapshot::onelibrary_stars(r.get(12)?),
                    color_id: r.get(13)?,
                    year: r.get(14)?,
                    bitrate: r.get(15)?,
                    sample_rate: r.get(16)?,
                    file_size: u64::try_from(r.get::<_, i64>(17)?).unwrap_or(0),
                    artwork: None,
                    ..SourceTrack::default()
                },
                r.get::<_, String>(18)?,
            ))
        })
        .map_err(sql)?;
    for row in rows {
        let (id, mut track, artwork) = row.map_err(sql)?;
        if !artwork.is_empty() {
            track.artwork = Some(crate::checked_under(root, &artwork)?);
        }
        out.insert(id, track);
    }
    Ok(())
}

/// The legacy database's share of [`sources`]: whatever `OneLibrary` did not
/// already provide (`or_insert`), since both name the same tracks.
fn sources_from_legacy(root: &Path, dir: &Path, out: &mut BTreeMap<u32, SourceTrack>) -> Result<()> {
    use rbl_pdb::PageType;
    let bytes = std::fs::read(dir.join("export.pdb"))?;
    let pdb = rbl_pdb::Pdb::parse(&bytes).map_err(sql)?;
    let names = |kind| -> BTreeMap<u32, String> {
        pdb.table(kind)
            .map(|t| {
                pdb.named_rows(t)
                    .into_iter()
                    .map(|n| (n.id, n.name))
                    .collect()
            })
            .unwrap_or_default()
    };
    let artists = names(PageType::Artists);
    let albums = names(PageType::Albums);
    let genres = names(PageType::Genres);
    let labels = names(PageType::Labels);
    let keys = names(PageType::Keys);
    let art = names(PageType::Artwork);
    for t in pdb
        .table(PageType::Tracks)
        .map(|t| pdb.track_rows(t))
        .unwrap_or_default()
    {
        out.entry(t.id).or_insert(SourceTrack {
            metadata: rbl_core::ExportMetadata { track_number: t.track_number, play_count: u32::from(t.play_count), bit_depth: t.sample_depth, disc_number: t.disc_number, hot_cue_auto_load: t.hot_cue_auto_load, ..Default::default() },
            title: t.title,
            artist: artists.get(&t.artist_id).cloned().unwrap_or_default(),
            album: albums.get(&t.album_id).cloned().unwrap_or_default(),
            genre: genres.get(&t.genre_id).cloned().unwrap_or_default(),
            label: labels.get(&t.label_id).cloned().unwrap_or_default(),
            key: keys.get(&t.key_id).cloned().unwrap_or_default(),
            comment: t.comment,
            date_added: t.date_added,
            release_date: t.release_date,
            bpm_x100: t.tempo_x100,
            duration_sec: t.duration_sec,
            rating: t.rating,
            color_id: t.color_id,
            year: t.year,
            bitrate: t.bitrate,
            sample_rate: t.sample_rate,
            file_size: u64::from(t.file_size),
            artwork: art
                .get(&t.artwork_id)
                .map(|p| crate::checked_under(root, p))
                .transpose()?,
            ..SourceTrack::default()
        });
    }
    Ok(())
}

/// Fills in what neither database's row alone carries: the checked source
/// path, My Tags, the analysis file bytes, and the device identity — for
/// every track the merged library actually has.
fn fill_source_details(
    root: &Path,
    before: &Snapshot,
    manifest: Option<&Manifest>,
    out: &mut BTreeMap<u32, SourceTrack>,
) -> Result<()> {
    // Tracks only. Its playlist ids use no recorded floor, unlike
    // `prepare`'s `merged_library_above`: do not rely on them here.
    let current = before.merged_library();
    let recorded: BTreeMap<_, _> = manifest.into_iter().flat_map(|m| m.tracks.iter().map(|t| (t.export_id, (m.db_id, t.library_id)))).collect();
    for t in current.as_ref().into_iter().flat_map(|l| &l.tracks) {
        let track = out
            .get_mut(&t.id)
            .ok_or_else(|| ExportError::Conflict("Missing USB track metadata".into()))?;
        track.source_path = crate::checked_under(root, &t.path)?;
        track.my_tags = before
            .tag_memberships
            .get(&t.id)
            .cloned()
            .unwrap_or_default();
        let analysis_dir = Path::new(&t.analysis)
            .parent()
            .map_or(String::new(), |p| p.to_string_lossy().into_owned());
        if !t.analysis.is_empty() {
            let base = crate::checked_under(root, &t.analysis)?;
            track.analysis_source = Some(base);
        }
        let (master_db_id, master_content_id) = before
            .identity
            .get(&t.id)
            .copied()
            .or_else(|| {
                recorded.get(&t.id).copied()
            })
            .unwrap_or_default();
        track.device = Some(DeviceTrack {
            id: t.id,
            master_db_id,
            master_content_id,
            audio: t.path.clone(),
            analysis_dir,
            preserve: true,
            exported: false,
        });
    }
    Ok(())
}

/// Convert a missing sibling database without changing the device selection.
pub fn all(
    root: &Path,
    before: &Snapshot,
    manifest: Option<&Manifest>,
) -> Result<(Vec<SourceTrack>, Vec<SourcePlaylist>)> {
    // Runs only when one database is missing, so no playlist is re-id'd and
    // the floor `prepare` passes to `merged_library_above` cannot matter.
    let current = before.merged_library();
    let sources = sources(root, before)?;
    let positions: BTreeMap<u32, usize> =
        sources.keys().enumerate().map(|(i, id)| (*id, i)).collect();
    let tracks = sources.into_values().collect();
    let ids: BTreeMap<u32, u64> = current
        .as_ref()
        .into_iter()
        .flat_map(|l| &l.playlists)
        .map(|p| {
            let id = manifest
                .and_then(|m| m.playlists.iter().find(|n| n.export_id == p.id))
                .map_or(DEVICE_PLAYLIST_ID_BASE + u64::from(p.id), |p| p.library_id);
            (p.id, id)
        })
        .collect();
    let playlists = current
        .as_ref()
        .into_iter()
        .flat_map(|l| &l.playlists)
        .map(|p| {
            Ok(SourcePlaylist {
                id: ids[&p.id],
                device_id: p.id,
                device_only: manifest
                    .and_then(|m| m.playlists.iter().find(|n| n.export_id == p.id))
                    .is_none_or(|p| p.device_only),
                parent_id: ids.get(&p.parent).copied().unwrap_or(0),
                folder: p.folder,
                name: p.name.clone(),
                track_indices: p
                    .tracks
                    .iter()
                    .map(|id| {
                        positions.get(id).copied().ok_or_else(|| {
                            ExportError::Conflict("Missing track in existing playlist".into())
                        })
                    })
                    .collect::<Result<_>>()?,
                entries_changed_ms: None,
            })
        })
        .collect::<Result<_>>()?;
    Ok((tracks, playlists))
}
