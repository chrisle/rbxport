use crate::{snapshot::{Library, Snapshot}, Result};
use std::{collections::BTreeSet, path::Path};
#[derive(Debug, Clone, Default)]
pub struct VerifyReport {
    pub parsed: bool,
    pub tracks: usize,
    pub playlists: usize,
    pub playlist_entries: usize,
    pub audio_present: usize,
    pub analysis_present: usize,
    pub overview_waveforms: usize,
    pub detail_waveforms: usize,
    pub beat_grids: usize,
    pub hot_cues: usize,
    pub memory_cues: usize,
    pub missing_audio: Vec<String>,
    pub errors: Vec<String>,
}
impl VerifyReport {
    pub fn is_ok(&self) -> bool {
        self.parsed && self.missing_audio.is_empty() && self.errors.is_empty()
    }
}
pub fn verify(root: &Path) -> Result<VerifyReport> {
    let snapshot = Snapshot::read(root)?;
    let mut report = verify_staged(root, root, &snapshot, &BTreeSet::new())?;
    if let Some(manifest) = crate::Manifest::load(root) {
        for track in manifest.tracks {
            for extension in track.analysis_extensions {
                let path = format!("{}/ANLZ0000.{extension}", track.anlz_dir);
                if !crate::checked_under(root, &path)?.is_file() {
                    report.errors.push(format!("Missing exported analysis companion: {path}"));
                }
            }
        }
    }
    Ok(report)
}

/// Read the two published databases back independently after an already
/// verified staged generation has been committed. Asset semantics were
/// checked before publication; this second pass proves that publication left
/// both database formats readable and equivalent without rereading every
/// analysis file from slow removable media.
pub fn verify_databases(root: &Path) -> Result<VerifyReport> {
    let snapshot = Snapshot::read(root)?;
    let mut report = VerifyReport::default();
    let (Some(legacy), Some(one)) = (&snapshot.legacy, &snapshot.one) else {
        report.errors.push("Both Device Library and OneLibrary must be present".into());
        return Ok(report);
    };
    report.parsed = true;
    verify_track_records(root, &mut report.errors)?;
    if legacy != one { report.errors.push(disagreement(legacy, one)); }
    report.tracks = legacy.tracks.len();
    report.playlists = legacy.playlists.len();
    report.playlist_entries = legacy.playlists.iter().map(|playlist| playlist.tracks.len()).sum();
    Ok(report)
}
/// `absent` names, by [`crate::path_key`], audio the stick was already
/// missing before the export and that the export carried without copying:
/// another writer's track, kept as the stick had it (#317). Its absence is not
/// a fault of the staged generation, so it is not reported as missing.
pub(crate) fn verify_staged(
    root: &Path,
    existing: &Path,
    snapshot: &Snapshot,
    absent: &BTreeSet<String>,
) -> Result<VerifyReport> {
    let mut report = VerifyReport::default();
    let resolve = |relative: &str| -> Result<std::path::PathBuf> {
        let path = crate::checked_under(root, relative)?;
        if path.is_file() {
            Ok(path)
        } else {
            crate::checked_under(existing, relative)
        }
    };
    let (Some(legacy), Some(one)) = (&snapshot.legacy, &snapshot.one) else {
        report
            .errors
            .push("Both Device Library and OneLibrary must be present".into());
        return Ok(report);
    };
    report.parsed = true;
    verify_track_records(root, &mut report.errors)?;
    if legacy != one {
        report.errors.push(disagreement(legacy, one));
    }
    report.tracks = legacy.tracks.len();
    report.playlists = legacy.playlists.len();
    let ids: BTreeSet<_> = legacy.tracks.iter().map(|t| t.id).collect();
    if ids.len() != legacy.tracks.len() {
        report.errors.push("Duplicate track IDs".into());
    }
    verify_assets(root, legacy, &resolve, absent, &mut report)?;
    let playlists: BTreeSet<_> = legacy.playlists.iter().map(|p| p.id).collect();
    for p in &legacy.playlists {
        if p.parent != 0
            && !legacy
                .playlists
                .iter()
                .any(|n| n.id == p.parent && n.folder)
        {
            report
                .errors
                .push(format!("Missing parent folder for {}", p.name));
        }
        let mut seen = BTreeSet::new();
        let mut parent = p.parent;
        while parent != 0 {
            if !seen.insert(parent) {
                report.errors.push("Playlist folder cycle".into());
                break;
            }
            parent = legacy
                .playlists
                .iter()
                .find(|n| n.id == parent)
                .map_or(0, |n| n.parent);
        }
        if p.folder && !p.tracks.is_empty() {
            report
                .errors
                .push("Folder contains direct track entries".into());
        }
        report.playlist_entries += p.tracks.len();
        if p.tracks.iter().any(|id| !ids.contains(id)) {
            report
                .errors
                .push(format!("Dangling playlist entry in {}", p.name));
        }
    }
    if playlists.len() != legacy.playlists.len() {
        report.errors.push("Duplicate playlist IDs".into());
    }
    snapshot.check_retained_history(snapshot)?;
    Ok(report)
}

/// What differs between the two databases, first difference only: which
/// track or playlist, and which of its fields, so a failed sync names
/// something a user can look at rather than only that they disagree.
fn disagreement(legacy: &Library, one: &Library) -> String {
    const PREFIX: &str = "Device Library and OneLibrary disagree";
    for track in &legacy.tracks {
        let Some(other) = one.tracks.iter().find(|t| t.id == track.id) else {
            return format!("{PREFIX}: track '{}' is only in export.pdb", track.title);
        };
        let fields = [
            ("title", track.title != other.title),
            ("artist", track.artist != other.artist),
            ("album", track.album != other.album),
            ("genre", track.genre != other.genre),
            ("label", track.label != other.label),
            ("key", track.key != other.key),
            ("comment", track.comment != other.comment),
            ("file path", track.path != other.path),
            ("analysis path", track.analysis != other.analysis),
            ("BPM", track.bpm != other.bpm),
            ("rating", track.rating != other.rating),
            ("colour", track.color != other.color),
        ];
        let differ: Vec<&str> = fields.iter().filter(|(_, differs)| *differs).map(|(name, _)| *name).collect();
        if !differ.is_empty() {
            return format!("{PREFIX} on the {} of track '{}'", differ.join(", "), track.title);
        }
    }
    if let Some(track) = one.tracks.iter().find(|t| !legacy.tracks.iter().any(|l| l.id == t.id)) {
        return format!("{PREFIX}: track '{}' is only in exportLibrary.db", track.title);
    }
    for playlist in &legacy.playlists {
        match one.playlists.iter().find(|p| p.id == playlist.id) {
            None => return format!("{PREFIX}: playlist '{}' is only in export.pdb", playlist.name),
            Some(other) if other != playlist => {
                return format!("{PREFIX} on playlist '{}'", playlist.name);
            }
            Some(_) => {}
        }
    }
    if let Some(playlist) = one.playlists.iter().find(|p| !legacy.playlists.iter().any(|l| l.id == p.id)) {
        return format!("{PREFIX}: playlist '{}' is only in exportLibrary.db", playlist.name);
    }
    PREFIX.to_owned()
}

fn verify_track_records(root: &Path, errors: &mut Vec<String>) -> Result<()> {
    // Semantic read-back alone cannot validate a DeviceSQL record: our
    // reader knows the offsets and used to accept records that the CDJ
    // ignored. Check the on-disk discriminator and playback format too.
    let pdb_path = root.join(crate::export_root_name(root)?).join("rekordbox/export.pdb");
    let bytes = std::fs::read(pdb_path)?;
    let pdb = rbl_pdb::Pdb::parse(&bytes)
        .map_err(|e| crate::ExportError::Conflict(format!("Invalid Device Library: {e}")))?;
    let one = rbl_onelibrary::ExportLibrary::open_read_only(&root.join(crate::export_root_name(root)?).join("rekordbox/exportLibrary.db"))
        .map_err(|e| crate::ExportError::OneLibrary(e.to_string()))?;
    let mut query = one.connection().prepare("SELECT content_id, COALESCE(isHotCueAutoLoadOn,0) FROM content")
        .map_err(|e| crate::ExportError::OneLibrary(e.to_string()))?;
    let flags = query.query_map([], |r| Ok((r.get::<_, u32>(0)?, r.get::<_, bool>(1)?)))
        .map_err(|e| crate::ExportError::OneLibrary(e.to_string()))?
        .collect::<std::result::Result<std::collections::BTreeMap<_,_>,_>>()
        .map_err(|e| crate::ExportError::OneLibrary(e.to_string()))?;
    if let Some(table) = pdb.table(rbl_pdb::PageType::Tracks) {
        for row in pdb.rows(table) {
            let id = pdb.u4_at(row, 0x48);
            let auto_load = pdb.string_ref(row, 0x5e + rbl_pdb::rows::slot::HOT_CUE_AUTO_LOAD * 2) == "ON";
            if flags.get(&id).is_some_and(|expected| *expected != auto_load) {
                errors.push(format!("Track {id}: hot-cue auto-load disagrees between databases"));
            }
            if pdb.u2_at(row, 0) != 0x24 {
                errors.push(format!("Track {id}: invalid DeviceSQL record subtype"));
            }
            if pdb.u2_at(row, 0x56) == 0 || pdb.u2_at(row, 0x5c) == 0 {
                errors.push(format!("Track {id}: missing DeviceSQL record trailer"));
            }
            let filename = pdb.string_ref(row, 0x5e + 19 * 2);
            let expected = rbl_pdb::rows::audio_file_type(&filename);
            if expected != 0 && pdb.u2_at(row, 0x5a) != expected {
                errors.push(format!("Track {id}: DeviceSQL audio format does not match {filename}"));
            }
        }
    }
    Ok(())
}

fn verify_assets(
    root: &Path,
    legacy: &crate::snapshot::Library,
    resolve: &impl Fn(&str) -> Result<std::path::PathBuf>,
    absent: &BTreeSet<String>,
    report: &mut VerifyReport,
) -> Result<()> {
    let mut paths = BTreeSet::new();
    for track in &legacy.tracks {
        if !paths.insert(crate::path_key(&track.path)) {
            report
                .errors
                .push(format!("Audio path collision: {}", track.path));
        }
        let path = resolve(&track.path)?;
        if path.is_file() {
            report.audio_present += 1;
        } else if !absent.contains(&crate::path_key(&track.path)) {
            report.missing_audio.push(track.path.clone());
        }
        if !track.analysis.is_empty() {
            let expected = format!("{}/ANLZ0000.DAT", crate::analysis_directory(&track.path, crate::export_root_name(root)?));
            if track.analysis != expected {
                report.errors.push(format!("Analysis path is not discoverable by the CDJ: {} (expected {expected})", track.analysis));
            }
            let path = resolve(&track.analysis)?;
            if path.is_file() && rbl_anlz::Anlz::read(&path).is_ok() {
                report.analysis_present += 1;
                let mut overview = false;
                let mut detail = false;
                let mut grid = false;
                for extension in ["DAT", "EXT", "2EX"] {
                    let relative = Path::new(&track.analysis).with_extension(extension).to_string_lossy().into_owned();
                    let companion = resolve(&relative)?;
                    if !companion.is_file() { continue; }
                    match rbl_anlz::Anlz::read(&companion) {
                        Ok(file) => {
                            if file.path().is_some_and(|p| p != track.path) {
                                report.errors.push(format!("Analysis audio path mismatch: {relative}"));
                            }
                            for section in &file.sections {
                                match &section.tag.0 {
                                    b"PWAV" | b"PWV4" | b"PWV6" => overview |= !section.payload.is_empty(),
                                    b"PWV3" | b"PWV5" | b"PWV7" => detail |= !section.payload.is_empty(),
                                    b"PQTZ" => grid |= section.as_beat_grid().is_some_and(|b| !b.is_empty()),
                                    b"PCO2" => {
                                        let entries = section.as_cue_entries().unwrap_or_default();
                                        let declared = section.header.get(4..6).map_or(0, |b| usize::from(u16::from_be_bytes([b[0], b[1]])));
                                        if entries.len() != declared { report.errors.push(format!("Truncated cue list: {relative}")); }
                                        for cue in entries {
                                            if cue.hot_cue == 0 { report.memory_cues += 1; } else { report.hot_cues += 1; }
                                        }
                                    }
                                    _ => {},
                                }
                            }
                        }
                        Err(e) => report.errors.push(format!("Invalid analysis {relative}: {e}")),
                    }
                }
                report.overview_waveforms += usize::from(overview);
                report.detail_waveforms += usize::from(detail);
                report.beat_grids += usize::from(grid);
            } else {
                report
                    .errors
                    .push(format!("Missing or invalid analysis: {}", track.analysis));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::snapshot::{Playlist, Track};

    /// A failed sync said only that the two databases disagree
    /// (#161); it has to say where.
    #[test]
    fn a_disagreement_names_the_track_and_the_field() {
        let track = Track { id: 1, title: "Kesä".into(), comment: "é".into(), ..Track::default() };
        let legacy = Library { tracks: vec![track.clone()], playlists: vec![] };
        let one = Library { tracks: vec![Track { comment: "é\0x".into(), ..track }], playlists: vec![] };
        assert_eq!(disagreement(&legacy, &one), "Device Library and OneLibrary disagree on the comment of track 'Kesä'");

        let playlist = Playlist { id: 4, name: "Warm up".into(), ..Playlist::default() };
        let legacy = Library { tracks: vec![], playlists: vec![playlist.clone()] };
        let one = Library { tracks: vec![], playlists: vec![Playlist { tracks: vec![1], ..playlist }] };
        assert_eq!(disagreement(&legacy, &one), "Device Library and OneLibrary disagree on playlist 'Warm up'");
        assert_eq!(disagreement(&legacy, &Library::default()), "Device Library and OneLibrary disagree: playlist 'Warm up' is only in export.pdb");
    }
}
