//! Revision receipts for selective sync. Missing caches never grant ownership.
use std::{collections::{BTreeMap, BTreeSet}, io::Read, path::{Path, PathBuf}};
use serde::{Deserialize, Serialize};
use crate::{Manifest, ManifestTrack, SourceTrack, SourcePlaylist, SourceMyTag, ExportOptions, Result};

pub const TRANSFORM_VERSION: u32 = 1;

/// What `export.pdb`'s rows hold. Part of the request, so a stick written
/// with an older layout is rebuilt on its next sync even when nothing else
/// changed, while its analysis files are still reused. 2: the date added in
/// track string slot 15 (#637).
const PDB_LAYOUT: u32 = 2;

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
    digest(&(manifest.db_id, &manifest.tracks, &manifest.playlists, &manifest.loose, &manifest.baseline,
        manifest.generation.as_ref().map(|g| &g.device_sources)))
}

pub fn request(tracks: &[SourceTrack], playlists: &[SourcePlaylist], tags: &[SourceMyTag], options: &ExportOptions<'_>) -> Result<String> {
    // SyncSource and StickSettings have complete, deterministic Debug forms;
    // the transform/schema version controls changes to this representation.
    digest(&(tracks, playlists, tags, options.compatibility, options.keep_unlisted,
        format!("{:?}", options.sync), format!("{:?}", options.defaults), TRANSFORM_VERSION, PDB_LAYOUT))
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

fn input_paths(track: &SourceTrack) -> Vec<PathBuf> {
    let mut paths = vec![track.source_path.clone()];
    if let Some(base) = &track.analysis_source {
        paths.extend(["DAT", "EXT", "2EX"].map(|e| base.with_extension(e)));
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
        ["DAT", "EXT", "2EX"].map(|ext| Ok((ext, revision(&base.with_extension(ext))?))).into_iter()
            .collect::<std::io::Result<Vec<_>>>()
    }).transpose()?;
    digest(&(audio, &track.cues, &track.analysis, &track.analysis_source, revisions, audio_revision, TRANSFORM_VERSION))
}

pub fn load_analysis(track: &SourceTrack) -> Result<Vec<(String, Vec<u8>)>> {
    let Some(base) = &track.analysis_source else { return Ok(track.analysis.clone()); };
    let mut files = Vec::new();
    for ext in ["DAT", "EXT", "2EX"] {
        let path = base.with_extension(ext);
        match std::fs::read(&path) {
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
