//! Revision receipts for selective sync. Missing caches never grant ownership.
use std::{collections::{BTreeMap, BTreeSet}, io::Read, path::{Path, PathBuf}};
use serde::{Deserialize, Serialize};
use crate::{Manifest, ManifestTrack, SourceTrack, SourcePlaylist, SourceMyTag, ExportOptions, Result};

/// Analysis bytes must be regenerated when an export transform changes.
/// Revision 6 completes default hot-cue RGB after revision 5 FLAC indexes.
pub const TRANSFORM_VERSION: u32 = 6;

/// What `export.pdb`'s rows hold. Part of the request, so a stick written
/// with an older layout is rebuilt on its next sync even when nothing else
/// changed, while its analysis files are still reused. 2: the date added in
/// track string slot 15 (#637). 3: zero-based `OneLibrary` My Tag positions
/// within each sibling group (#893). 4: current `OneLibrary` property version
/// and exported analysis flags; audio and analysis bytes are unchanged.
const PDB_LAYOUT: u32 = 4;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Metrics {
    pub audio_bytes_read: u64,
    pub analysis_bytes_read: u64,
    pub database_bytes_read: u64,
    pub metadata_lookups: u64,
    pub database_builds: u64,
    pub conversions: u64,
    pub probes: u64,
}

thread_local! {
    static METRICS: std::cell::RefCell<Metrics> = std::cell::RefCell::new(Metrics::default());
    static REVISIONS: std::cell::RefCell<Option<std::collections::HashMap<PathBuf, Option<FileRevision>>>> = const { std::cell::RefCell::new(None) };
}

pub(crate) struct Inspection(Option<std::collections::HashMap<PathBuf, Option<FileRevision>>>);
impl Inspection {
    pub fn new() -> Self {
        Self(REVISIONS.with(|r| r.replace(Some(std::collections::HashMap::new()))))
    }
}
impl Drop for Inspection {
    fn drop(&mut self) { REVISIONS.with(|r| { r.replace(self.0.take()); }); }
}
pub(crate) fn recheck() { REVISIONS.with(|r| { if let Some(map) = &mut *r.borrow_mut() { map.clear(); } }); }

pub fn reset_metrics() { METRICS.with(|m| *m.borrow_mut() = Metrics::default()); }
pub fn metrics() -> Metrics { METRICS.with(|m| m.borrow().clone()) }
pub(crate) fn count(build: bool, convert: bool, probe: bool) {
    METRICS.with(|m| { let mut m = m.borrow_mut(); m.database_builds += u64::from(build); m.conversions += u64::from(convert); m.probes += u64::from(probe); });
}
pub(crate) fn read_bytes(path: &Path, bytes: u64) {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    METRICS.with(|m| {
        let mut m = m.borrow_mut();
        match ext.as_str() {
            "dat" | "ext" | "2ex" if path.file_stem().is_some_and(|s| s == "ANLZ0000") => m.analysis_bytes_read += bytes,
            "mp3" | "wav" | "aif" | "aiff" | "flac" | "m4a" | "ogg" | "opus" | "source" => m.audio_bytes_read += bytes,
            "db" | "pdb" | "db-wal" | "db-shm" => m.database_bytes_read += bytes,
            _ => {},
        }
    });
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRevision {
    pub size: u64,
    pub modified: u128,
    /// Source change time catches replacements that restore the old mtime.
    pub changed: Option<i128>,
    pub identity: Option<(u64, u64)>,
}

pub fn revision(path: &Path) -> std::io::Result<Option<FileRevision>> {
    if let Some(revision) = REVISIONS.with(|r| r.borrow().as_ref().and_then(|map| map.get(path).cloned())) {
        return Ok(revision);
    }
    let revision = revision_fresh(path)?;
    REVISIONS.with(|r| { if let Some(map) = &mut *r.borrow_mut() { map.insert(path.to_owned(), revision.clone()); } });
    Ok(revision)
}

pub fn revision_fresh(path: &Path) -> std::io::Result<Option<FileRevision>> {
    METRICS.with(|m| m.borrow_mut().metadata_lookups += 1);
    let meta = match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => meta,
        Ok(_) => return Ok(None),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let modified = meta.modified()?.duration_since(std::time::UNIX_EPOCH)
        .map_err(std::io::Error::other)?.as_nanos();
    #[cfg(unix)]
    let (changed, identity) = {
        use std::os::unix::fs::MetadataExt;
        (Some(i128::from(meta.ctime()) * 1_000_000_000 + i128::from(meta.ctime_nsec())), Some((meta.dev(), meta.ino())))
    };
    #[cfg(not(unix))]
    let (changed, identity) = (None, None);
    Ok(Some(FileRevision { size: meta.len(), modified, changed, identity }))
}

/// Destination identity/change time is not stable across renames or remounts.
pub fn copy_revision(path: &Path) -> std::io::Result<Option<FileRevision>> {
    Ok(revision(path)?.map(|mut r| { r.changed = None; r.identity = None; r }))
}

pub fn copy_revision_fresh(path: &Path) -> std::io::Result<Option<FileRevision>> {
    Ok(revision_fresh(path)?.map(|mut r| { r.changed = None; r.identity = None; r }))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Generation {
    /// Exact ownership/profile for Cbm databases and their dependent assets.
    #[serde(default)]
    pub r1_files: BTreeMap<String, String>,
    #[serde(default)]
    pub structure: String,
    #[serde(default)]
    pub device_sources: BTreeMap<u32, SourceTrack>,
    #[serde(default)]
    pub kept_existing: usize,
    #[serde(default)]
    pub warnings: Vec<crate::ExportWarning>,
    pub request: String,
    pub inputs: BTreeMap<String, Option<FileRevision>>,
    pub outputs: BTreeMap<String, Option<FileRevision>>,
    pub databases: BTreeMap<String, Option<String>>,
    pub analysis_recipes: BTreeMap<String, String>,
    pub artwork_recipes: BTreeMap<String, String>,
    pub transform_version: u32,
}

pub fn digest(value: &impl Serialize) -> Result<String> {
    use std::io::Write;
    let mut hash = blake3::Hasher::new();
    {
        let mut writer = std::io::BufWriter::with_capacity(256 * 1024, &mut hash);
        serde_json::to_writer(&mut writer, value).map_err(std::io::Error::other)?;
        writer.flush()?;
    }
    Ok(hash.finalize().to_hex().to_string())
}

pub fn manifest_structure(manifest: &Manifest) -> Result<String> {
    let normal = digest(&(manifest.db_id, &manifest.tracks, &manifest.playlists, &manifest.loose, &manifest.baseline,
        manifest.generation.as_ref().map(|g| &g.device_sources)))?;
    match manifest.generation.as_ref().map(|g| &g.r1_files).filter(|files| !files.is_empty()) {
        Some(files) => digest(&("xdj-r1-v1", normal, files)),
        None => Ok(normal),
    }
}

pub fn request(tracks: &[SourceTrack], playlists: &[SourcePlaylist], tags: &[SourceMyTag], options: &ExportOptions<'_>) -> Result<String> {
    // Preserve current dev's OFF digest, including DB IDs and PDB layout.
    let normal = (tracks, playlists, tags, options.compatibility, options.keep_unlisted,
        format!("{:?}", options.sync), options.db_id_override, options.my_tag_master_db_id_override,
        format!("{:?}", options.defaults), TRANSFORM_VERSION, PDB_LAYOUT);
    if options.xdj_r1_support { digest(&("xdj-r1-v3", normal)) } else { digest(&normal) }
}

pub fn inputs(tracks: &[SourceTrack]) -> std::io::Result<BTreeMap<String, Option<FileRevision>>> {
    let mut out = BTreeMap::new();
    for track in tracks {
        for path in input_paths(track) {
            if let std::collections::btree_map::Entry::Vacant(entry) = out.entry(path.to_string_lossy().into_owned()) {
                entry.insert(revision(&path)?);
            }
        }
    }
    Ok(out)
}

/// The caller supplies the exact primary DAT. Only companion extensions
/// are derived; loading and every source receipt must describe the same files.
pub(crate) fn analysis_paths(base: &Path) -> [(&'static str, PathBuf); 3] {
    [("DAT", base.to_owned()), ("EXT", base.with_extension("EXT")), ("2EX", base.with_extension("2EX"))]
}

fn input_paths(track: &SourceTrack) -> Vec<PathBuf> {
    let mut paths = vec![track.source_path.clone()];
    if let Some(base) = &track.analysis_source {
        paths.extend(analysis_paths(base).map(|(_, path)| path));
    }
    if let Some(image) = &track.artwork {
        paths.push(image.clone());
        paths.push(image.with_file_name("artwork_s.jpg"));
        paths.push(image.with_file_name("artwork_m.jpg"));
    }
    paths
}

pub fn databases(root: &Path) -> Result<BTreeMap<String, Option<String>>> {
    let name = crate::export_root_name(root)?;
    let paths = ["export.pdb", "exportExt.pdb", "exportLibrary.db", "exportLibrary.db-wal", "exportLibrary.db-shm"];
    let mut out = BTreeMap::new();
    for path in paths {
        let relative = format!("{name}/rekordbox/{path}");
        out.insert(relative.clone(), content_digest(&root.join(relative))?);
    }
    for family_root in ["PIONEER", ".PIONEER"] {
        for file in crate::cbm::FILES {
            let relative = format!("{family_root}/DBH/{file}");
            if let Some(hash) = content_digest(&root.join(&relative))? {
                out.insert(relative, Some(hash));
            }
        }
    }
    for path in crate::sync_record::FILES {
        let relative = path.replacen("PIONEER/", &format!("{name}/"), 1);
        out.insert(relative.clone(), content_digest(&root.join(relative))?);
    }
    for path in crate::MY_SETTINGS_FILES.iter().copied().chain(["DEVSETTING.DAT"]) {
        let relative = format!("{name}/{path}");
        out.insert(relative.clone(), content_digest(&root.join(relative))?);
    }
    Ok(out)
}

pub fn content_digest(path: &Path) -> std::io::Result<Option<String>> {
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let before = revision_fresh(path)?;
    let mut hash = blake3::Hasher::new();
    let mut buffer = vec![0; 256 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 { break; }
        hash.update(&buffer[..count]);
        read_bytes(path, count as u64);
    }
    if revision_fresh(path)? != before { return Err(std::io::Error::other("File changed while it was being checked")); }
    Ok(Some(hash.finalize().to_hex().to_string()))
}

pub fn output_paths(tracks: &[ManifestTrack]) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    for track in tracks {
        paths.insert(track.audio.trim_start_matches('/').to_owned());
        if !track.anlz_dir.is_empty() {
            for ext in ["DAT", "EXT", "2EX"] {
                paths.insert(format!("{}/ANLZ0000.{ext}", track.anlz_dir.trim_start_matches('/')));
            }
        }
        if !track.artwork.is_empty() {
            for (path, _) in crate::artwork_names(crate::artwork_id_from_path(&track.artwork)) {
                let root = track.artwork.split('/').find(|s| !s.is_empty()).unwrap_or("PIONEER");
                paths.insert(path.trim_start_matches('/').replacen("PIONEER/", &format!("{root}/"), 1));
            }
        }
    }
    paths
}

pub fn output_revisions(root: &Path, tracks: &[ManifestTrack]) -> std::io::Result<BTreeMap<String, Option<FileRevision>>> {
    output_paths(tracks).into_iter().map(|path| Ok((path.clone(), copy_revision(&root.join(path))?))).collect()
}

pub fn receipts_match(root: &Path, generation: &Generation) -> std::io::Result<bool> {
    for (path, stamp) in &generation.outputs {
        if copy_revision(&crate::checked_under(root, path).map_err(std::io::Error::other)?)? != *stamp { return Ok(false); }
    }
    Ok(true)
}

pub fn trusted_paths(root: &Path, previous: Option<&Manifest>) -> std::io::Result<BTreeSet<String>> {
    let mut paths = BTreeSet::new();
    if let Some(g) = previous.and_then(|m| m.generation.as_ref()).filter(|g| g.transform_version == TRANSFORM_VERSION) {
        for (path, stamp) in &g.outputs {
            if stamp.is_some() && copy_revision(&crate::checked_under(root, path).map_err(std::io::Error::other)?)? == *stamp {
                paths.insert(path.clone());
            }
        }
    }
    Ok(paths)
}

pub fn analysis_recipe(track: &SourceTrack, audio: &str) -> Result<String> {
    analysis_recipe_with_revision(track, audio, &revision(&track.source_path)?)
}

pub fn analysis_recipe_with_revision(track: &SourceTrack, audio: &str, audio_revision: &Option<FileRevision>) -> Result<String> {
    let revisions = track.analysis_source.as_ref().map(|base| {
        analysis_paths(base).map(|(ext, path)| Ok((ext, revision(&path)?))).into_iter()
            .collect::<std::io::Result<Vec<_>>>()
    }).transpose()?;
    digest(&(audio, &track.cues, &track.analysis, &track.analysis_source, revisions, audio_revision, TRANSFORM_VERSION))
}

pub fn load_analysis(track: &SourceTrack) -> Result<Vec<(String, Vec<u8>)>> {
    load_analysis_with(track, |path| std::fs::read(path))
}

fn load_analysis_with(track: &SourceTrack, mut read: impl FnMut(&Path) -> std::io::Result<Vec<u8>>) -> Result<Vec<(String, Vec<u8>)>> {
    let Some(base) = &track.analysis_source else { return Ok(track.analysis.clone()); };
    let mut files = Vec::new();
    for (ext, path) in analysis_paths(base) {
        match read(&path) {
            Ok(bytes) => {
                read_bytes(&path, bytes.len() as u64);
                rbl_anlz::parse(&bytes).map_err(|e| std::io::Error::other(format!("Invalid analysis {}: {e}", path.display())))?;
                files.push((ext.to_owned(), bytes));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && ext != "DAT" => {},
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(files)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod primary_analysis_tests {
    use super::*;

    fn image() -> Vec<u8> {
        rbl_anlz::AnlzBuilder::new().path("/generated.wav").waveform_preview(b"PWAV", &[9; 400]).finish()
    }

    #[test]
    fn primary_reader_preserves_the_stored_dat_spelling() {
        for ext in ["dat", "dAt"] {
            let base = PathBuf::from(format!("/fixture/ANLZ0000.{ext}"));
            let track = SourceTrack { analysis_source: Some(base.clone()), ..Default::default() };
            let bytes = image();
            let mut requested = Vec::new();
            let loaded = load_analysis_with(&track, |path| {
                requested.push(path.to_owned());
                if path == base { Ok(bytes.clone()) }
                else { Err(std::io::ErrorKind::NotFound.into()) }
            }).unwrap();
            assert_eq!(requested.first(), Some(&base), "the primary file is the exact supplied pathname");
            assert_eq!(loaded, [("DAT".to_owned(), bytes)]);
        }
    }

    #[test]
    fn uppercase_primary_and_optional_companions_keep_their_existing_reads() {
        let base = PathBuf::from("/fixture/ANLZ0000.DAT");
        let track = SourceTrack { analysis_source: Some(base.clone()), ..Default::default() };
        let bytes = image();
        let mut requested = Vec::new();
        let loaded = load_analysis_with(&track, |path| {
            requested.push(path.to_owned()); Ok(bytes.clone())
        }).unwrap();
        assert_eq!(requested, [base.clone(), base.with_extension("EXT"), base.with_extension("2EX")]);
        assert_eq!(loaded.len(), 3);
    }

    #[test]
    fn a_missing_primary_does_not_substitute_a_differently_spelled_file() {
        let base = PathBuf::from("/fixture/ANLZ0000.dat");
        let other = base.with_extension("DAT");
        let track = SourceTrack { analysis_source: Some(base), ..Default::default() };
        let loaded = load_analysis_with(&track, |path| {
            if path == other { Ok(image()) }
            else { Err(std::io::ErrorKind::NotFound.into()) }
        }).unwrap();
        assert_eq!(loaded, []);
    }

    #[test]
    fn source_receipts_name_the_supplied_primary_file() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("ANLZ0000.dAt");
        std::fs::write(&base, image()).unwrap();
        let track = SourceTrack { analysis_source: Some(base.clone()), ..Default::default() };
        let recorded = inputs(&[track]).unwrap();
        assert!(recorded.get(&base.to_string_lossy().into_owned()).is_some_and(Option::is_some),
            "source changes must be tracked under the supplied primary pathname");
    }
}
