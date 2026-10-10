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

pub fn prepare(
    root: &Path,
    before: &Snapshot,
    previous: Option<&Manifest>,
    tracks: &[SourceTrack],
    playlists: &[SourcePlaylist],
    db_id: u64,
    keep_unlisted: bool,
) -> Result<(Vec<SourceTrack>, Vec<SourcePlaylist>)> {
    let mut tracks = tracks.to_vec();
    let mut playlists = playlists.to_vec();
    let Some(current) = before.merged_library() else {
        return Ok((tracks, playlists));
    };
    let existing_sources = sources(root, before)?;
    identify_tracks(&mut tracks, before, previous, db_id, &existing_sources);
    let record = crate::sync_record::read(root).filter(|r| r.db_id == db_id && db_id != 0);
    identify_playlists(&mut playlists, previous, record.as_ref());
    retain_device_only(&current, &mut tracks, &mut playlists, before, previous, &existing_sources, record.as_ref(), keep_unlisted)?;
    if keep_unlisted {
        keep_stick_order(&mut playlists, previous);
    }
    Ok((tracks, playlists))
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
    for track in tracks {
        if track.device.is_some() {
            continue;
        }
        let identity = before
            .identity
            .iter()
            .find(|(_, (db, content))| *db == db_id && *content == track.id && track.id != 0)
            .map(|(id, _)| *id)
            .or_else(|| {
                previous
                    .filter(|m| m.db_id == db_id)
                    .and_then(|m| {
                        m.tracks.iter().find(|t| {
                            t.key()
                                == crate::track_key(track.id, &track.source_path.to_string_lossy())
                        })
                    })
                    .map(|t| t.export_id)
            });
        if let Some(id) = identity {
            if let Some(source) = existing_sources.get(&id) {
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
) {
    for p in playlists {
        if p.device_id != 0 {
            continue;
        }
        p.device_id = previous
            .and_then(|m| {
                m.playlists
                    .iter()
                    .find(|old| old.library_id == p.id && p.id != 0 && !old.device_only)
            })
            .map(|p| p.export_id)
            .or_else(|| record.and_then(|r| r.device_ids.get(&p.id).copied()))
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
#[allow(clippy::too_many_arguments, reason = "everything reconciliation already gathered, passed through")]
fn retain_device_only(
    current: &Library,
    tracks: &mut Vec<SourceTrack>,
    playlists: &mut Vec<SourcePlaylist>,
    before: &Snapshot,
    previous: Option<&Manifest>,
    existing_sources: &BTreeMap<u32, SourceTrack>,
    record: Option<&SyncRecord>,
    keep_unlisted: bool,
) -> Result<()> {
    // Device playlist id -> the library id it was exported from.
    let owned: BTreeMap<u32, u64> = previous.map_or_else(
        || {
            record
                .map(|r| r.device_ids.iter().map(|(library, device)| (*device, *library)).collect())
                .unwrap_or_default()
        },
        |m| {
            m.playlists
                .iter()
                .filter(|p| !p.device_only && p.export_id != 0)
                .map(|p| (p.export_id, p.library_id))
                .collect()
        },
    );
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
    let retained_id = |device: u32| kept_owned.get(&device).copied().unwrap_or(DEVICE_PLAYLIST_ID_BASE + u64::from(device));
    let mut needed: BTreeSet<u32> = retained
        .iter()
        .flat_map(|p| &p.tracks)
        .copied()
        .chain(before.history.iter().flat_map(|h| &h.tracks).copied())
        .collect();
    let previously_owned: BTreeSet<u32> = previous
        .map(|m| m.tracks.iter().map(|t| t.export_id).collect())
        .unwrap_or_default();
    // Tracks that arrived through another writer belong to the USB, not to our
    // previous selection. Retain them even when no playlist references them.
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
        let track = existing_sources
            .get(&id)
            .ok_or_else(|| {
                ExportError::Conflict(format!(
                    "Device playlist/history references missing track {id}"
                ))
            })?
            .clone();
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
        });
    }
    Ok(())
}

/// Read complete track metadata from whichever database exists. Both formats
/// share the audio and analysis files; only the explicit identity map is trusted.
fn sources(root: &Path, before: &Snapshot) -> Result<BTreeMap<u32, SourceTrack>> {
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
                        play_count: r.get(22)?, analysed: r.get(23)?, hot_cue_auto_load: r.get(24)?,
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
    let current = before.merged_library();
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
            for extension in ["DAT", "EXT", "2EX"] {
                let path = base.with_extension(extension);
                match std::fs::read(&path) {
                    Ok(bytes) => track.analysis.push((extension.into(), bytes)),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
            }
        }
        let (master_db_id, master_content_id) = before
            .identity
            .get(&t.id)
            .copied()
            .or_else(|| {
                manifest.and_then(|m| {
                    m.tracks
                        .iter()
                        .find(|old| old.export_id == t.id)
                        .map(|old| (m.db_id, old.library_id))
                })
            })
            .unwrap_or_default();
        track.device = Some(DeviceTrack {
            id: t.id,
            master_db_id,
            master_content_id,
            audio: t.path.clone(),
            analysis_dir,
            preserve: true,
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
            })
        })
        .collect::<Result<_>>()?;
    Ok((tracks, playlists))
}
