//! Cbm participates in the same retained publication as Device Library/OneLibrary.
use crate::{cbm, incremental, ExportError, Manifest, OneLibraryTrack, Result, SourcePlaylist};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
type Files = BTreeMap<String, String>;
fn conflict(s: impl std::fmt::Display) -> ExportError {
    ExportError::Conflict(format!("XDJ-R1: {s}"))
}
fn read_limited(path: &Path, limit: usize) -> Result<Vec<u8>> {
    use std::io::Read;
    let failed = |e| conflict(format!("cannot read {}: {e}", path.display()));
    let source = std::fs::File::open(path).map_err(failed)?;
    if source.metadata().map_err(failed)?.len() > limit as u64 {
        return Err(conflict(format!(
            "{} exceeds the safe R1 allocation limit",
            path.display()
        )));
    }
    // A concurrent grow after metadata cannot bypass the allocation bound.
    let mut data = Vec::new();
    source
        .take(limit as u64 + 1)
        .read_to_end(&mut data)
        .map_err(failed)?;
    if data.len() > limit {
        return Err(conflict(format!(
            "{} grew beyond the safe R1 allocation limit",
            path.display()
        )));
    }
    Ok(data)
}
fn read_set(root: &Path, name: &str) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut set = BTreeMap::new();
    let mut total = 0;
    for file in cbm::FILES {
        let limit = cbm::MAX_FILE_BYTES.min(cbm::MAX_SET_BYTES - total);
        let data = read_limited(&root.join(name).join("DBH").join(file), limit)?;
        total += data.len();
        set.insert(file.into(), data);
    }
    Ok(set)
}
/// Fail closed for foreign, incomplete, malformed or changed libraries, even with
/// support OFF. Blocking also protects all audio a foreign Cbm library may name.
pub(crate) fn protect(root: &Path, previous: Option<&Manifest>) -> Result<()> {
    let owned = previous
        .and_then(|m| m.generation.as_ref())
        .map(|g| &g.r1_files);
    for name in ["PIONEER", ".PIONEER"] {
        let dir = root.join(name).join("DBH");
        if !dir.is_dir() {
            continue;
        }
        let entries = std::fs::read_dir(&dir)?.collect::<std::io::Result<Vec<_>>>()?;
        if !entries.iter().any(|e| {
            e.file_name()
                .to_string_lossy()
                .to_ascii_lowercase()
                .starts_with("cbm")
        }) {
            continue;
        }
        for entry in &entries {
            let filename = entry.file_name().to_string_lossy().into_owned();
            if filename.to_ascii_lowercase().starts_with("cbm")
                && !cbm::FILES.contains(&filename.as_str())
            {
                return Err(conflict("unknown or temporary Cbm files require manual reconciliation; USB left unchanged"));
            }
        }
        let set = read_set(root, name)?;
        let parsed = cbm::parse_contents(&set[cbm::FILES[0]])?;
        for file in &cbm::FILES[1..] {
            if cbm::parse_category(&set[*file], &parsed.records)?.pending_edits {
                return Err(conflict(
                    "pending device playlist edits; preserve/import them before syncing",
                ));
            }
        }
        for file in cbm::FILES {
            let path = format!("{name}/DBH/{file}");
            if owned.and_then(|o| o.get(&path)).is_none() {
                return Err(conflict(
                    "existing Cbm library is not owned by this export; preserve it before syncing",
                ));
            }
        }
    }
    if let Some(files) = owned {
        check_files(root, files)?;
    }
    Ok(())
}
fn check_files(root: &Path, files: &Files) -> Result<()> {
    for (path, hash) in files {
        let file = crate::checked_under(root, path)?;
        if incremental::content_digest(&file)?.as_ref() != Some(hash) {
            return Err(conflict(format!(
                "R1 library or asset changed: {path}; USB left unchanged"
            )));
        }
    }
    Ok(())
}
pub(crate) fn verify(root: &Path) -> Result<()> {
    if let Some(m) = Manifest::load(root) {
        if let Some(g) = m.generation {
            if !g.r1_files.is_empty() {
                check_files(root, &g.r1_files)?;
                let set = read_set(root, "PIONEER")?;
                cbm::validate(&set)?;
            }
        }
    }
    Ok(())
}
fn resolved(stage: &Path, existing: &Path, path: &str) -> Result<std::path::PathBuf> {
    let staged = crate::checked_under(stage, path)?;
    if staged.is_file() {
        Ok(staged)
    } else {
        crate::checked_under(existing, path)
    }
}
fn store(stage: &Path, path: &str, data: &[u8], files: &mut Files) -> Result<()> {
    let target = crate::checked_under(stage, path)?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    rbl_core::durable::write(&target, data)?;
    files.insert(
        path.trim_start_matches('/').into(),
        blake3::hash(data).to_hex().to_string(),
    );
    Ok(())
}
fn dat_path(bytes: &[u8]) -> Result<String> {
    let dat = rbl_anlz::parse(bytes).map_err(conflict)?;
    if dat.sections.iter().filter(|s| &s.tag.0 == b"PPTH").count() != 1 {
        return Err(conflict("base DAT must contain exactly one PPTH"));
    }
    dat.path()
        .ok_or_else(|| conflict("base DAT has no valid PPTH"))
}
#[allow(clippy::too_many_arguments, reason = "one immutable export generation")]
#[allow(
    clippy::too_many_lines,
    reason = "ordered format construction and validation; preserve evidence traceability"
)]
pub(crate) fn stage(
    stage: &Path,
    existing: &Path,
    name: &str,
    tracks: &[OneLibraryTrack],
    sources: &[crate::SourceTrack],
    artists: &crate::Intern,
    albums: &crate::Intern,
    playlists: &[SourcePlaylist],
    playlist_ids: &[u32],
    export_ids: &[Option<u32>],
    previous: Option<&Manifest>,
    warnings: &mut Vec<crate::ExportWarning>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Files> {
    // Native R1 lookup names PIONEER. The other libraries retain their own
    // existing root; only these additional DBH/analysis/artwork assets use it.
    let normal_root = name;
    let name = "PIONEER";
    if tracks.len() > cbm::MAX_TRACKS {
        return Err(conflict(
            "source Device Library exceeds the 10,000-track Cbm writer bound",
        ));
    }
    let mut files = Files::new();
    let mut cbm_tracks = Vec::new();
    for t in tracks {
        if cancelled() {
            return Err(ExportError::Cancelled);
        }
        let mut audio = resolved(stage, existing, &t.audio_path)?;
        // Journaled relocations still live under their old name until commit.
        if !audio.is_file() {
            if let Some(old) = previous
                .into_iter()
                .flat_map(|m| &m.tracks)
                .find(|old| old.export_id == t.export_id)
            {
                audio = crate::checked_under(existing, &old.audio)?;
            }
        }
        let properties = rbl_audio::legacy::inspect(&audio, cancelled).map_err(|e| {
            if cancelled() {
                ExportError::Cancelled
            } else {
                conflict(format!("{}: {e}", t.title))
            }
        })?;
        let Some(info) = properties else {
            warnings.push(crate::ExportWarning {
                track_id: t.library_id,
                title: t.title.clone(),
                reason: "r1-format-excluded".into(),
                detail: None,
            });
            continue;
        };
        let mut builder = rbl_anlz::write::AnlzBuilder::new();
        builder.path(&t.audio_path).vbr_table_zero().beat_grid(&[]);
        let mut dat = if t.analysis_path.is_empty() {
            rbl_anlz::parse(&builder.finish()).map_err(conflict)?
        } else {
            rbl_anlz::parse(&read_limited(
                &resolved(stage, existing, &t.analysis_path)?,
                cbm::MAX_FILE_BYTES,
            )?)
            .map_err(conflict)?
        };
        if let Some(cues) = &t.cues {
            dat.sections.retain(|s| !s.is_cue_list());
            dat.sections.extend(rbl_anlz::cues::sections(cues, false));
        }
        if dat.sections.iter().filter(|s| &s.tag.0 == b"PPTH").count() != 1
            || dat.path().as_deref() != Some(t.audio_path.as_str())
        {
            return Err(conflict("base DAT PPTH differs from exported audio"));
        }
        validate_cues(&dat)?;
        let data = dat.to_bytes();
        let directory = crate::analysis_directory(&t.audio_path, name);
        let mut candidate = None;
        for i in 0..=10 {
            let path = format!("{directory}/ANLZ{i:04}.DAT");
            let source = resolved(stage, existing, &path)?;
            if source.is_file() {
                let old = read_limited(&source, cbm::MAX_FILE_BYTES)?;
                if dat_path(&old)? != t.audio_path {
                    continue;
                }
                let is_classic = path == t.analysis_path;
                let owned = previous
                    .and_then(|m| m.generation.as_ref())
                    .is_some_and(|g| g.r1_files.contains_key(path.trim_start_matches('/')));
                if !is_classic && !owned && old != data {
                    return Err(conflict(
                        "unowned matching R1 analysis has edits; preserve it before syncing",
                    ));
                }
                candidate = Some(path);
                break;
            }
            if candidate.is_none() {
                candidate = Some(path);
            }
            // Continue through occupied candidates: native lookup must select a PPTH match.
        }
        let path = candidate
            .ok_or_else(|| conflict("all eleven base DAT collision slots are occupied"))?;
        store(stage, &path, &data, &mut files)?;
        let artwork = if t.image_id == 0 {
            String::new()
        } else {
            crate::artwork_path(t.image_id, "a", false)
        };
        let large = format!("{}_l.jpg", artwork.trim_end_matches(".jpg"));
        if !artwork.is_empty() && !files.contains_key(large.trim_start_matches('/')) {
            if normal_root != name {
                let normal = artwork.replacen("/PIONEER/", &format!("/{normal_root}/"), 1);
                let small =
                    read_limited(&resolved(stage, existing, &normal)?, cbm::MAX_FILE_BYTES)?;
                let target = crate::checked_under(existing, &artwork)?;
                let owned = previous
                    .and_then(|m| m.generation.as_ref())
                    .is_some_and(|g| g.r1_files.contains_key(artwork.trim_start_matches('/')));
                if target.exists() && !owned && read_limited(&target, cbm::MAX_FILE_BYTES)? != small
                {
                    return Err(conflict("unowned R1 artwork would be replaced"));
                }
                store(stage, &artwork, &small, &mut files)?;
            }
            let output = format!("{}_l.jpg", artwork.trim_end_matches(".jpg"));
            let image_path = sources
                .iter()
                .zip(export_ids)
                .find(|(_, id)| **id == Some(t.export_id))
                .and_then(|(source, _)| source.artwork.as_ref())
                .filter(|p| p.is_file())
                .cloned()
                .map_or_else(
                    || {
                        resolved(
                            stage,
                            existing,
                            &crate::artwork_path(t.image_id, "a", true).replacen(
                                "/PIONEER/",
                                &format!("/{normal_root}/"),
                                1,
                            ),
                        )
                    },
                    Ok,
                )?;
            let jpeg = large_artwork(&image_path)?;
            let target = crate::checked_under(existing, &output)?;
            if target.exists()
                && !previous
                    .and_then(|m| m.generation.as_ref())
                    .is_some_and(|g| g.r1_files.contains_key(output.trim_start_matches('/')))
                && read_limited(&target, cbm::MAX_FILE_BYTES)? != jpeg
            {
                return Err(conflict("unowned large artwork would be replaced"));
            }
            store(stage, &output, &jpeg, &mut files)?;
        }
        cbm_tracks.push(cbm::Track {
            id: t.export_id,
            audio: t.audio_path.clone(),
            filename: t.file_name.clone(),
            title: t.title.clone(),
            artist: t.artist.clone(),
            artist_id: artists.ids.get(&t.artist).copied().unwrap_or(0),
            album: t.album.clone(),
            album_id: albums.ids.get(&t.album).copied().unwrap_or(0),
            genre: t.genre.clone(),
            key: t.key.clone(),
            comment: t.comment.clone(),
            artwork,
            track_number: t.metadata.track_number,
            bpm: t.bpm_x100,
            hot_cue_auto_load: t.metadata.hot_cue_auto_load,
            bytes: info.bytes,
            data_offset: info.offset,
            frames75: info.frames75,
            rate: info.rate,
            format: info.format,
            emphasis: info.emphasis,
        });
    }
    // Input order is sibling order. Flatten folders depth first; detect cycles.

    let mut leaves = Vec::new();
    visit(
        0,
        0,
        playlists,
        playlist_ids,
        export_ids,
        &mut BTreeSet::new(),
        &mut leaves,
    )?;
    let timestamp = u32::try_from(rbl_core::time::unix_millis() / 1000).map_err(conflict)?;
    for (file, data) in cbm::build(
        &cbm_tracks,
        &leaves,
        playlists.len(),
        device_block_bytes(existing)?,
        timestamp,
    )? {
        store(stage, &format!("{name}/DBH/{file}"), &data, &mut files)?;
    }
    Ok(files)
}

/// Bound decoder and square allocations before decoding untrusted artwork.
/// 4096-square RGB needs at most 48 MiB; this is a deliberate safe admission limit.
fn large_artwork(path: &Path) -> Result<Vec<u8>> {
    let mut reader = image::ImageReader::open(path)?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().map_err(conflict)?.to_rgb8();
    let side = image.width().max(image.height());
    if side == 0 || side > 4096 {
        return Err(conflict(
            "invalid or oversized R1 artwork (maximum 4096 pixels)",
        ));
    }
    let mut square = image::RgbImage::new(side, side);
    image::imageops::replace(
        &mut square,
        &image,
        i64::from((side - image.width()) / 2),
        i64::from((side - image.height()) / 2),
    );
    let resized =
        image::imageops::resize(&square, 320, 320, image::imageops::FilterType::CatmullRom);
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 85)
        .encode_image(&image::DynamicImage::ImageRgb8(resized))
        .map_err(conflict)?;
    Ok(jpeg)
}

pub(crate) fn analysis_collision(root: &Path, directory: &str, audio: &str) -> Result<bool> {
    let path = crate::checked_under(root, &format!("{directory}/ANLZ0000.DAT"))?;
    if !path.is_file() {
        return Ok(false);
    }
    Ok(dat_path(&read_limited(&path, cbm::MAX_FILE_BYTES)?)? != audio)
}

fn visit(
    parent: u64,
    depth: usize,
    playlists: &[SourcePlaylist],
    ids: &[u32],
    tracks: &[Option<u32>],
    seen: &mut BTreeSet<usize>,
    out: &mut Vec<cbm::Playlist>,
) -> Result<()> {
    if depth > 8 {
        return Err(conflict("playlist nesting exceeds mapped traversal bound"));
    }
    for (i, p) in playlists
        .iter()
        .enumerate()
        .filter(|(_, p)| p.parent_id == parent)
    {
        if !seen.insert(i) {
            return Err(conflict("playlist cycle"));
        }
        if p.folder {
            visit(p.id, depth + 1, playlists, ids, tracks, seen, out)?;
        } else if depth <= 6 {
            out.push(cbm::Playlist {
                id: ids[i],
                name: p.name.clone(),
                tracks: p
                    .track_indices
                    .iter()
                    .filter_map(|i| tracks.get(*i).copied().flatten())
                    .collect(),
            });
        }
    }
    Ok(())
}

// macOS propagates diskutil/UTF-8 errors; other targets share its Result API.
#[cfg_attr(not(target_os = "macos"), allow(clippy::unnecessary_wraps))]
fn device_block_bytes(root: &Path) -> Result<u32> {
    // [OBS] Stock queries Device Block Size, defaulting to 512. Read-only.
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("/usr/sbin/diskutil")
            .arg("info")
            .arg(root)
            .output()?;
        if output.status.success() {
            let text = String::from_utf8(output.stdout).map_err(conflict)?;
            if let Some(value) = text
                .lines()
                .find_map(|line| line.trim().strip_prefix("Device Block Size:"))
                .and_then(|s| s.split_whitespace().next())
                .and_then(|n| n.parse::<u32>().ok())
            {
                if value > 512 && value % 512 == 0 {
                    return Ok(value);
                }
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = root;
    Ok(512)
}

fn validate_cues(dat: &rbl_anlz::Anlz) -> Result<()> {
    let mut collections = BTreeSet::new();
    for section in dat.sections.iter().filter(|s| &s.tag.0 == b"PCOB") {
        if section.header.len() < 12 {
            return Err(conflict("short R1 cue collection"));
        }
        let kind = u32::from_be_bytes(section.header[0..4].try_into().map_err(conflict)?);
        if !collections.insert(kind) {
            return Err(conflict("duplicate R1 cue collection"));
        }
        let count = usize::from(u16::from_be_bytes([section.header[6], section.header[7]]));
        let limit = if kind == 0 {
            10
        } else if kind == 1 {
            3
        } else {
            return Err(conflict("unknown R1 cue collection"));
        };
        if count > limit || section.payload.len() != count * 56 {
            return Err(conflict(
                "R1 cue collection exceeds native slot bounds or has an inconsistent size",
            ));
        }
        let mut slots = BTreeSet::new();
        for entry in section.payload.chunks_exact(56) {
            let slot = u32::from_be_bytes(entry[12..16].try_into().map_err(conflict)?);
            if (kind == 0 && slot != 0)
                || (kind == 1 && (!(1..=3).contains(&slot) || !slots.insert(slot)))
            {
                return Err(conflict(
                    "R1 hot cues require distinct slots A-C; memory cues require slot zero",
                ));
            }
            if &entry[..4] != b"PCPT"
                || entry[4..8] != 28_u32.to_be_bytes()
                || entry[8..12] != 56_u32.to_be_bytes()
                || ![1, 2].contains(&entry[28])
                || entry[30..32] == [0, 0]
            {
                return Err(conflict("invalid R1 cue entry"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod cue_bounds {
    #[test]
    fn malformed_hot_slots_and_duplicate_collections_are_refused() {
        let mut dat =
            rbl_anlz::parse(&rbl_anlz::AnlzBuilder::new().path("/x.wav").finish()).unwrap();
        dat.sections.extend(rbl_anlz::cues::sections(
            &[rbl_anlz::cues::ExportCue {
                kind: 1,
                time_ms: 100,
                ..Default::default()
            }],
            false,
        ));
        super::validate_cues(&dat).unwrap();
        let collection = dat
            .sections
            .iter()
            .position(|s| &s.tag.0 == b"PCOB")
            .unwrap();
        dat.sections[collection].payload[12..16].copy_from_slice(&4_u32.to_be_bytes());
        assert!(super::validate_cues(&dat).is_err());
        dat.sections[collection].payload[12..16].copy_from_slice(&1_u32.to_be_bytes());
        dat.sections.push(dat.sections[collection].clone());
        assert!(super::validate_cues(&dat).is_err());
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod artwork_bounds {
    #[test]
    fn oversized_artwork_is_refused_before_square_allocation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wide.png");
        // A narrow generated fixture keeps the test allocation small.
        image::RgbImage::new(4097, 1).save(&path).unwrap();
        assert!(super::large_artwork(&path).is_err());
        image::RgbImage::new(4096, 1).save(&path).unwrap();
        let jpeg = super::large_artwork(&path).unwrap();
        let decoded = image::load_from_memory(&jpeg).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (320, 320));
    }
}
