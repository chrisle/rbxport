//! Authenticated restoration of one naturally retained pre-overlap-fix export.
//!
//! This is deliberately not a general journal repair. The old finalizer copied
//! the old source back after relocating its replacement, then retained an
//! `.incomplete` journal. A durable, format-owned intent keeps every generic
//! publication entry point refusing until this restoration has verified.
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

use rbl_core::durable::{self, PendingRecovery};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use crate::{
    Manifest,
    incremental::{self, FileRevision},
};

const INTENT: &str = ".legacy-single-move-recovery.json";
const REPLACEMENT: &str = ".legacy-single-move-replacement";
const JOURNAL: &str = ".rbxport-publication";

fn refused() -> io::Error {
    io::Error::other("Interrupted export could not be safely restored; recovery data retained")
}
fn invalid<T>() -> io::Result<T> {
    Err(refused())
}
fn digest(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

pub(crate) fn pending(root: &Path) -> bool {
    [INTENT, ".incomplete"].iter().any(|name| {
        match fs::symlink_metadata(root.join(JOURNAL).join(name)) {
            Ok(_) => true,
            Err(error) => error.kind() != io::ErrorKind::NotFound,
        }
    })
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: PathBuf,
    present: bool,
    had_target: Option<bool>,
    exact: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stamp {
    size: u64,
    modified: u128,
}
impl From<&FileRevision> for Stamp {
    fn from(value: &FileRevision) -> Self {
        Self {
            size: value.size,
            modified: value.modified,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    version: u8,
    old_manifest: Box<RawValue>,
    sealed_digest: String,
    publication_digest: String,
    moves_digest: String,
    from: PathBuf,
    to: PathBuf,
    stamps: BTreeMap<String, Vec<Stamp>>,
}

#[derive(Clone)]
struct Image {
    hash: Hash,
    size: Option<u64>,
}
#[derive(Clone)]
enum Hash {
    Blake(String),
    Fnv(u64),
}
impl Image {
    fn key(&self) -> String {
        match &self.hash {
            Hash::Blake(hash) => format!("b3:{hash}"),
            Hash::Fnv(hash) => format!("fnv:{hash:016x}"),
        }
    }
    fn matches(&self, measured: &Measured) -> bool {
        self.size.is_none_or(|size| size == measured.stamp.size)
            && match &self.hash {
                Hash::Blake(hash) => hash == &measured.blake,
                Hash::Fnv(hash) => *hash == measured.fnv,
            }
    }
}
struct Measured {
    blake: String,
    fnv: u64,
    stamp: Stamp,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest<'a> {
    version: u32,
    #[serde(borrow)]
    db_id: &'a RawValue,
    #[serde(borrow)]
    tracks: &'a RawValue,
    #[serde(borrow)]
    playlists: &'a RawValue,
    #[serde(borrow)]
    loose: &'a RawValue,
    #[serde(borrow)]
    baseline: &'a RawValue,
    #[serde(borrow)]
    generation: RawGeneration<'a>,
    #[serde(rename = "written", borrow)]
    _written: &'a RawValue,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGeneration<'a> {
    structure: String,
    #[serde(borrow)]
    device_sources: &'a RawValue,
    #[serde(rename = "kept_existing", default, borrow)]
    _kept: Option<&'a RawValue>,
    #[serde(rename = "warnings", default, borrow)]
    _warnings: Option<&'a RawValue>,
    #[serde(rename = "request", borrow)]
    _request: &'a RawValue,
    #[serde(rename = "inputs", borrow)]
    _inputs: &'a RawValue,
    #[serde(rename = "outputs", borrow)]
    _outputs: &'a RawValue,
    #[serde(rename = "databases", borrow)]
    _databases: &'a RawValue,
    #[serde(rename = "analysis_recipes", borrow)]
    _analysis: &'a RawValue,
    #[serde(rename = "artwork_recipes", borrow)]
    _artwork: &'a RawValue,
    #[serde(rename = "transform_version", borrow)]
    _transform: &'a RawValue,
}

/// Preserve the original struct/map field order and string bytes. Inserting
/// current defaults (notably `ExportMetadata::content_link`) changes old hashes.
fn compact(raw: &str) -> Vec<u8> {
    let mut quoted = false;
    let mut escaped = false;
    raw.bytes()
        .filter(|byte| {
            if quoted {
                if escaped {
                    escaped = false;
                } else if *byte == b'\\' {
                    escaped = true;
                } else if *byte == b'"' {
                    quoted = false;
                }
                true
            } else if *byte == b'"' {
                quoted = true;
                true
            } else {
                !byte.is_ascii_whitespace()
            }
        })
        .collect()
}

fn schema_known(raw: &serde_json::Value, typed: &serde_json::Value) -> bool {
    match (raw, typed) {
        (serde_json::Value::Object(raw), serde_json::Value::Object(typed)) => {
            raw.iter().all(|(key, value)| {
                typed
                    .get(key)
                    .is_some_and(|known| schema_known(value, known))
            })
        }
        (serde_json::Value::Array(raw), serde_json::Value::Array(typed)) => {
            raw.len() == typed.len()
                && raw
                    .iter()
                    .zip(typed)
                    .all(|(raw, typed)| schema_known(raw, typed))
        }
        _ => true,
    }
}

// Serde maps ordinarily keep the last duplicate key. Receipts must be
// unambiguous, including keys outside the six-component structure checksum.
struct UniqueJson;
impl<'de> Deserialize<'de> for UniqueJson {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = UniqueJson;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("unambiguous JSON")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut keys = BTreeSet::new();
                while let Some(key) = map.next_key::<String>()? {
                    if !keys.insert(key) {
                        return Err(serde::de::Error::custom("duplicate recovery field"));
                    }
                    map.next_value::<UniqueJson>()?;
                }
                Ok(UniqueJson)
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut values: A,
            ) -> Result<Self::Value, A::Error> {
                while values.next_element::<UniqueJson>()?.is_some() {}
                Ok(UniqueJson)
            }
            fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<Self::Value, E> {
                Ok(UniqueJson)
            }
            fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Self::Value, E> {
                Ok(UniqueJson)
            }
            fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<Self::Value, E> {
                Ok(UniqueJson)
            }
            fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<Self::Value, E> {
                Ok(UniqueJson)
            }
            fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Self::Value, E> {
                Ok(UniqueJson)
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueJson)
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

fn parse<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> io::Result<T> {
    let _: UniqueJson = serde_json::from_slice(bytes).map_err(|_| refused())?;
    serde_json::from_slice(bytes).map_err(|_| refused())
}

fn manifest(bytes: &[u8]) -> io::Result<Manifest> {
    let _: UniqueJson = serde_json::from_slice(bytes).map_err(|_| refused())?;
    let raw: RawManifest<'_> = serde_json::from_slice(bytes).map_err(|_| refused())?;
    if raw.version != crate::manifest::MANIFEST_VERSION {
        return invalid();
    }
    let mut tuple = vec![b'['];
    for (index, value) in [
        raw.db_id,
        raw.tracks,
        raw.playlists,
        raw.loose,
        raw.baseline,
        raw.generation.device_sources,
    ]
    .iter()
    .enumerate()
    {
        if index != 0 {
            tuple.push(b',');
        }
        tuple.extend(compact(value.get()));
    }
    tuple.push(b']');
    if digest(&tuple) != raw.generation.structure {
        return invalid();
    }
    let typed: Manifest = serde_json::from_slice(bytes).map_err(|_| refused())?;
    let value = serde_json::from_slice(bytes).map_err(|_| refused())?;
    if !schema_known(&value, &serde_json::to_value(&typed)?) {
        return invalid();
    }
    Ok(typed)
}

fn relative(path: &Path) -> io::Result<()> {
    if path.as_os_str().is_empty()
        || !path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return invalid();
    }
    Ok(())
}

/// Permit a mounted/aliased root itself, but no descendant symlink or junction.
/// Missing suffixes are permitted only after every existing ancestor is plain.
fn safe(root: &Path, relative_path: &Path) -> io::Result<PathBuf> {
    relative(relative_path)?;
    let canonical_root = root.canonicalize()?;
    let mut path = root.to_owned();
    let count = relative_path.components().count();
    for (index, part) in relative_path.components().enumerate() {
        path.push(part.as_os_str());
        match fs::symlink_metadata(&path) {
            Ok(meta) => {
                if meta.file_type().is_symlink()
                    || (index + 1 != count && !meta.is_dir())
                    || !path.canonicalize()?.starts_with(&canonical_root)
                {
                    return invalid();
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(path)
}
fn plain(root: &Path, path: &Path) -> io::Result<Option<PathBuf>> {
    let path = safe(root, path)?;
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() => Ok(Some(path)),
        Ok(_) => invalid(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}
fn read(root: &Path, path: &Path) -> io::Result<Vec<u8>> {
    fs::read(plain(root, path)?.ok_or_else(refused)?)
}
fn join(container: &str, path: &Path) -> PathBuf {
    Path::new(container).join(path)
}

struct Plan {
    intent: Intent,
    entries: Vec<Entry>,
    old: Manifest,
    new: Manifest,
    manifest_path: PathBuf,
    old_images: BTreeMap<PathBuf, Option<Image>>,
    new_images: BTreeMap<PathBuf, Option<Image>>,
    replacement: Image,
    checked_analysis: BTreeMap<PathBuf, FileRevision>,
}

fn images(
    manifest: &Manifest,
    manifest_path: &Path,
    bytes: &[u8],
) -> io::Result<BTreeMap<PathBuf, Option<Image>>> {
    let generation = manifest.generation.as_ref().ok_or_else(refused)?;
    let mut images = BTreeMap::new();
    for (path, hash) in &generation.databases {
        let path = PathBuf::from(path);
        relative(&path)?;
        images.insert(
            path,
            hash.as_ref().map(|hash| Image {
                hash: Hash::Blake(hash.clone()),
                size: None,
            }),
        );
    }
    for track in &manifest.tracks {
        let audio = PathBuf::from(track.audio.trim_start_matches('/'));
        relative(&audio)?;
        if !track.in_place && track.preserved != Some(true) && track.audio_hash != 0 {
            let (size, _) = track.copy_stamp.ok_or_else(refused)?;
            images.insert(
                audio,
                Some(Image {
                    hash: Hash::Fnv(track.audio_hash),
                    size: Some(size),
                }),
            );
        }
        for extension in &track.analysis_extensions {
            if !matches!(extension.as_str(), "DAT" | "EXT" | "2EX") {
                return invalid();
            }
            let hash = track
                .analysis_hashes
                .get(extension)
                .filter(|hash| **hash != 0)
                .ok_or_else(refused)?;
            let path = PathBuf::from(format!(
                "{}/ANLZ0000.{extension}",
                track.anlz_dir.trim_start_matches('/')
            ));
            relative(&path)?;
            let size = generation
                .outputs
                .get(path.to_str().ok_or_else(refused)?)
                .and_then(Option::as_ref)
                .ok_or_else(refused)?
                .size;
            images.insert(
                path,
                Some(Image {
                    hash: Hash::Fnv(*hash),
                    size: Some(size),
                }),
            );
        }
    }
    for (path, stamp) in &generation.outputs {
        if stamp.is_none() {
            images.entry(PathBuf::from(path)).or_insert(None);
        }
    }
    images.insert(
        manifest_path.to_owned(),
        Some(Image {
            hash: Hash::Blake(digest(bytes)),
            size: Some(bytes.len() as u64),
        }),
    );
    Ok(images)
}

fn measure(pending: &PendingRecovery<'_>, path: &Path) -> io::Result<Measured> {
    pending.check_root()?;
    let before = incremental::revision_fresh(path)?.ok_or_else(refused)?;
    let mut file = fs::File::open(path)?;
    pending.check_file(&file, path)?;
    let mut blake = blake3::Hasher::new();
    let mut fnv = 0xcbf2_9ce4_8422_2325_u64;
    let mut buffer = vec![0; 256 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        blake.update(&buffer[..count]);
        for byte in &buffer[..count] {
            fnv = (fnv ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
        }
        incremental::read_bytes(path, count as u64);
    }
    pending.check_file(&file, path)?;
    if incremental::revision_fresh(path)? != Some(before.clone()) {
        return invalid();
    }
    #[cfg(test)]
    HASH_PASSES.with(|passes| {
        *passes.borrow_mut().entry(path.to_owned()).or_default() += 1;
    });
    Ok(Measured {
        blake: blake.finalize().to_hex().to_string(),
        fnv,
        stamp: Stamp::from(&before),
    })
}

impl Plan {
    // Keep the complete authority preflight visible before the first mutation.
    #[allow(clippy::too_many_lines)]
    fn load(pending: &PendingRecovery<'_>, saved: Option<Intent>) -> io::Result<Self> {
        pending.check_root()?;
        let journal = pending.journal();
        let publication = read(journal, Path::new("publication.json"))?;
        let moves_bytes = read(journal, Path::new(".moves.json"))?;
        let entries: Vec<Entry> = parse(&publication)?;
        let moves: BTreeMap<PathBuf, PathBuf> = parse(&moves_bytes)?;
        if entries.is_empty()
            || moves.len() != 1
            || entries
                .iter()
                .any(|entry| !entry.exact || entry.had_target.is_none())
        {
            return invalid();
        }
        let mut names = BTreeSet::new();
        for entry in &entries {
            relative(&entry.path)?;
            if !names.insert(entry.path.clone()) {
                return invalid();
            }
        }
        let manifest_paths: Vec<_> = entries
            .iter()
            .filter(|entry| entry.path.ends_with("rbxport/manifest.json"))
            .map(|entry| entry.path.clone())
            .collect();
        let [manifest_path] = manifest_paths.as_slice() else {
            return invalid();
        };
        let prefix = manifest_path
            .components()
            .next()
            .ok_or_else(refused)?
            .as_os_str()
            .to_str()
            .ok_or_else(refused)?;
        if !matches!(prefix, "PIONEER" | ".PIONEER") {
            return invalid();
        }
        let (to, from) = moves.iter().next().ok_or_else(refused)?;
        relative(to)?;
        relative(from)?;
        if from == to
            || !to.starts_with("Contents")
            || !from.starts_with("Contents")
            || !entries
                .iter()
                .any(|e| e.path == *from && e.present && e.had_target == Some(true))
            || !entries
                .iter()
                .any(|e| e.path == *to && e.present && e.had_target == Some(false))
        {
            return invalid();
        }
        if read(journal, Path::new(".incomplete"))? != from.to_string_lossy().as_bytes()
            || plain(journal, Path::new(".rollback"))?.is_some()
            || plain(journal, Path::new(".verification-required"))?.is_some()
        {
            return invalid();
        }
        let sealed = read(journal, &join(".sealed", manifest_path))?;
        let old_bytes = match &saved {
            Some(intent) => intent.old_manifest.get().as_bytes().to_vec(),
            None => read(journal, &join(".previous", manifest_path))?,
        };
        // The intent preserves the original representation; it is not a new
        // authority for edited receipts. Throughout rollback the exact old
        // manifest must still occupy its previous slot or its restored slot.
        let previous_manifest = plain(journal, &join(".previous", manifest_path))?
            .map(fs::read)
            .transpose()?;
        let current_manifest = plain(pending.root(), manifest_path)?
            .map(fs::read)
            .transpose()?;
        if previous_manifest.as_deref() != Some(old_bytes.as_slice())
            && current_manifest.as_deref() != Some(old_bytes.as_slice())
        {
            return invalid();
        }
        let old = manifest(&old_bytes)?;
        let new = manifest(&sealed)?;
        if old.db_id != new.db_id || old.db_id == 0 {
            return invalid();
        }
        let old_generation = old.generation.as_ref().ok_or_else(refused)?;
        let new_generation = new.generation.as_ref().ok_or_else(refused)?;
        let mut expected_databases = BTreeSet::new();
        for name in [
            "export.pdb",
            "exportExt.pdb",
            "exportLibrary.db",
            "exportLibrary.db-wal",
            "exportLibrary.db-shm",
        ] {
            expected_databases.insert(format!("{prefix}/rekordbox/{name}"));
        }
        expected_databases.extend(
            crate::sync_record::FILES
                .iter()
                .map(|path| path.replacen("PIONEER/", &format!("{prefix}/"), 1)),
        );
        expected_databases.extend(
            crate::MY_SETTINGS_FILES
                .iter()
                .copied()
                .chain(["DEVSETTING.DAT"])
                .map(|name| format!("{prefix}/{name}")),
        );
        if old_generation
            .databases
            .keys()
            .ne(expected_databases.iter())
            || new_generation
                .databases
                .keys()
                .ne(expected_databases.iter())
            || old_generation
                .databases
                .keys()
                .filter(|key| key.ends_with("-wal") || key.ends_with("-shm"))
                .any(|key| {
                    old_generation.databases[key].is_some()
                        || new_generation.databases[key].is_some()
                })
        {
            return invalid();
        }
        for manifest in [&old, &new] {
            let generation = manifest.generation.as_ref().ok_or_else(refused)?;
            if generation.outputs.keys().cloned().collect::<BTreeSet<_>>()
                != incremental::output_paths(&manifest.tracks)
            {
                return invalid();
            }
        }
        let old_track = old
            .tracks
            .iter()
            .find(|track| Path::new(track.audio.trim_start_matches('/')) == from)
            .ok_or_else(refused)?;
        let relocated_track = new
            .tracks
            .iter()
            .find(|track| Path::new(track.audio.trim_start_matches('/')) == to)
            .ok_or_else(refused)?;
        let replacement_track = new
            .tracks
            .iter()
            .find(|track| Path::new(track.audio.trim_start_matches('/')) == from)
            .ok_or_else(refused)?;
        if old_track.key() != relocated_track.key()
            || old_track.export_id != relocated_track.export_id
            || old_track.conversion != relocated_track.conversion
            || old_track.audio_hash == 0
            || relocated_track.audio_hash != old_track.audio_hash
            || old_track.copy_stamp.map(|s| s.0) != relocated_track.copy_stamp.map(|s| s.0)
            || replacement_track.key() == old_track.key()
            || replacement_track.audio_hash == 0
            || replacement_track.audio_hash == old_track.audio_hash
            || old
                .tracks
                .iter()
                .any(|track| Path::new(track.audio.trim_start_matches('/')) == to)
            || new_generation
                .outputs
                .get(from.to_str().ok_or_else(refused)?)
                != Some(&None)
        {
            return invalid();
        }
        let replacement = Image {
            hash: Hash::Fnv(replacement_track.audio_hash),
            size: Some(replacement_track.copy_stamp.ok_or_else(refused)?.0),
        };
        let old_images = images(&old, manifest_path, &old_bytes)?;
        let new_images = images(&new, manifest_path, &sealed)?;
        for entry in &entries {
            if !matches!(
                entry
                    .path
                    .components()
                    .next()
                    .map(|c| c.as_os_str().to_str()),
                Some(Some("Contents"))
            ) && !entry.path.starts_with(prefix)
            {
                return invalid();
            }
            if entry.had_target == Some(true)
                && !old_images.get(&entry.path).is_some_and(Option::is_some)
            {
                return invalid();
            }
            if entry.present && !new_images.get(&entry.path).is_some_and(Option::is_some) {
                return invalid();
            }
            if entry.had_target == Some(false)
                && old_images.get(&entry.path).is_some_and(Option::is_some)
            {
                return invalid();
            }
            if !entry.present && new_images.get(&entry.path).is_some_and(Option::is_some) {
                return invalid();
            }
        }
        let intent = match saved {
            Some(intent) => {
                if intent.version != 1
                    || intent.from != *from
                    || intent.to != *to
                    || intent.sealed_digest != digest(&sealed)
                    || intent.publication_digest != digest(&publication)
                    || intent.moves_digest != digest(&moves_bytes)
                {
                    return invalid();
                }
                intent
            }
            None => Intent {
                version: 1,
                old_manifest: RawValue::from_string(
                    String::from_utf8(old_bytes).map_err(|_| refused())?,
                )?,
                sealed_digest: digest(&sealed),
                publication_digest: digest(&publication),
                moves_digest: digest(&moves_bytes),
                from: from.clone(),
                to: to.clone(),
                stamps: BTreeMap::new(),
            },
        };
        Ok(Self {
            intent,
            entries,
            old,
            new,
            manifest_path: manifest_path.clone(),
            old_images,
            new_images,
            replacement,
            checked_analysis: BTreeMap::new(),
        })
    }

    fn check_image(
        &mut self,
        pending: &PendingRecovery<'_>,
        container: &Path,
        path: &Path,
        allowed: &[Image],
        capture: bool,
    ) -> io::Result<bool> {
        let Some(file) = plain(container, path)? else {
            return Ok(false);
        };
        let measured = measure(pending, &file)?;
        let image = allowed
            .iter()
            .find(|image| image.matches(&measured))
            .ok_or_else(refused)?;
        let stamps = self.intent.stamps.entry(image.key()).or_default();
        if capture {
            if !stamps.contains(&measured.stamp) {
                stamps.push(measured.stamp);
            }
        } else if !stamps.contains(&measured.stamp) {
            return invalid();
        }
        Ok(true)
    }

    // Audit initial and resumable states together for each publication entry.
    #[allow(clippy::too_many_lines)]
    fn check_path(
        &mut self,
        pending: &PendingRecovery<'_>,
        path: &Path,
        capture: bool,
    ) -> io::Result<()> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.path == path)
            .ok_or_else(refused)?
            .clone();
        let old = self.old_images.get(path).and_then(Clone::clone);
        let new = self.new_images.get(path).and_then(Clone::clone);
        let previous = if let Some(old) = &old {
            self.check_image(
                pending,
                pending.journal(),
                &join(".previous", path),
                std::slice::from_ref(old),
                capture,
            )?
        } else {
            if plain(pending.journal(), &join(".previous", path))?.is_some() {
                return invalid();
            }
            false
        };
        if capture {
            if let Some(Some(stamp)) = self
                .old
                .generation
                .as_ref()
                .ok_or_else(refused)?
                .outputs
                .get(path.to_str().ok_or_else(refused)?)
            {
                let retained = if previous {
                    safe(pending.journal(), &join(".previous", path))?
                } else {
                    safe(pending.root(), path)?
                };
                if incremental::copy_revision_fresh(&retained)? != Some(stamp.clone()) {
                    return invalid();
                }
            }
            if entry.had_target == Some(true) && !previous {
                return invalid();
            }
            if plain(pending.journal(), path)?.is_some()
                || plain(pending.journal(), &join(".rejected", path))?.is_some()
            {
                return invalid();
            }
        }
        if path == self.intent.to {
            let quarantined = plain(pending.journal(), Path::new(REPLACEMENT))?.is_some();
            let allowed = [self.replacement.clone()];
            let present = self.check_image(pending, pending.root(), path, &allowed, capture)?;
            if present == quarantined {
                return invalid();
            }
        } else if path == self.intent.from {
            let old = old.as_ref().ok_or_else(refused)?;
            let present = self.check_image(
                pending,
                pending.root(),
                path,
                std::slice::from_ref(old),
                capture,
            )?;
            if !present && !previous {
                return invalid();
            }
        } else {
            let allowed: Vec<_> = if capture {
                new.iter().cloned().collect()
            } else {
                old.iter().chain(new.iter()).cloned().collect()
            };
            let present = self.check_image(pending, pending.root(), path, &allowed, capture)?;
            if capture && present != entry.present {
                return invalid();
            }
            if capture {
                if let Some(expected) = self
                    .new
                    .generation
                    .as_ref()
                    .ok_or_else(refused)?
                    .outputs
                    .get(path.to_str().ok_or_else(refused)?)
                {
                    if incremental::copy_revision_fresh(&safe(pending.root(), path)?)? != *expected
                    {
                        return invalid();
                    }
                }
            }
            if entry.had_target == Some(true) && !previous {
                let old = old.as_ref().ok_or_else(refused)?;
                if !present
                    || !self.check_image(
                        pending,
                        pending.root(),
                        path,
                        std::slice::from_ref(old),
                        capture,
                    )?
                {
                    return invalid();
                }
            }
        }
        let rejected: Vec<_> = if path == self.intent.from {
            old.iter().cloned().collect()
        } else {
            new.iter().cloned().collect()
        };
        let rejected = self.check_image(
            pending,
            pending.journal(),
            &join(".rejected", path),
            &rejected,
            capture,
        )?;
        let target = plain(pending.root(), path)?.is_some();
        if path == self.intent.to {
            if rejected {
                return invalid();
            }
        } else if entry.had_target == Some(false) {
            if previous
                || (entry.present && target == rejected)
                || (!entry.present && (target || rejected))
            {
                return invalid();
            }
        } else {
            if previous && target && rejected {
                return invalid();
            }
            if entry.present && !previous && !rejected {
                return invalid();
            }
            if entry.present && previous && !target && !rejected {
                return invalid();
            }
            if !entry.present && rejected {
                return invalid();
            }
        }
        // Neither this repair nor its rollback ever recreates a staged image.
        // Such an image, even with known bytes, is outside the proven states.
        if plain(pending.journal(), path)?.is_some() {
            return invalid();
        }
        Ok(())
    }

    fn check_all(&mut self, pending: &PendingRecovery<'_>, capture: bool) -> io::Result<()> {
        fn walk(root: &Path, at: &Path, allowed: &BTreeSet<PathBuf>) -> io::Result<()> {
            for entry in fs::read_dir(at)? {
                let entry = entry?;
                let path = entry.path();
                let relative_path = path.strip_prefix(root).map_err(|_| refused())?;
                let kind = entry.file_type()?;
                if kind.is_symlink() {
                    return invalid();
                }
                if kind.is_dir() {
                    safe(root, relative_path)?;
                    walk(root, &path, allowed)?;
                } else if !kind.is_file() || !allowed.contains(relative_path) {
                    return invalid();
                }
            }
            Ok(())
        }
        pending.check_root()?;
        let mut allowed: BTreeSet<_> = [
            "publication.json",
            ".moves.json",
            ".incomplete",
            INTENT,
            REPLACEMENT,
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect();
        allowed.insert(join(".sealed", &self.manifest_path));
        for entry in &self.entries {
            allowed.insert(entry.path.clone());
            allowed.insert(join(".previous", &entry.path));
            allowed.insert(join(".rejected", &entry.path));
        }
        walk(pending.journal(), pending.journal(), &allowed)?;
        for entry in self.entries.clone() {
            self.check_path(pending, &entry.path, capture)?;
        }
        let affected: BTreeSet<_> = self
            .entries
            .iter()
            .map(|entry| entry.path.to_str().unwrap_or("").to_owned())
            .collect();
        let generation = self.old.generation.as_ref().ok_or_else(refused)?;
        let untouched_databases: Vec<_> = generation
            .databases
            .iter()
            .filter(|(path, _)| !affected.contains(path.as_str()))
            .map(|(path, value)| (path.clone(), value.clone()))
            .collect();
        for (path, expected) in untouched_databases {
            if self
                .new
                .generation
                .as_ref()
                .ok_or_else(refused)?
                .databases
                .get(&path)
                != Some(&expected)
            {
                return invalid();
            }
            let image = self.old_images.get(Path::new(&path)).and_then(Clone::clone);
            match image {
                Some(image) => {
                    if !self.check_image(
                        pending,
                        pending.root(),
                        Path::new(&path),
                        &[image],
                        capture,
                    )? {
                        return invalid();
                    }
                }
                None => {
                    if plain(pending.root(), Path::new(&path))?.is_some() {
                        return invalid();
                    }
                }
            }
        }
        let outputs = self
            .old
            .generation
            .as_ref()
            .ok_or_else(refused)?
            .outputs
            .clone();
        for (path, expected) in outputs {
            if affected.contains(path.as_str()) {
                continue;
            }
            let target = safe(pending.root(), Path::new(&path))?;
            if incremental::copy_revision_fresh(&target)? != expected {
                return invalid();
            }
            // Unchanged audio/artwork retains ordinary generation stamp trust.
            // Explicitly hashed required ANLZ gets one content pass per recovery
            // attempt, reused only while its fresh revision remains unchanged.
            let image = self.old_images.get(Path::new(&path)).and_then(Clone::clone);
            if Path::new(&path)
                .file_stem()
                .is_some_and(|name| name == "ANLZ0000")
            {
                if let Some(image) = image {
                    let revision = incremental::revision_fresh(&target)?.ok_or_else(refused)?;
                    if self.checked_analysis.get(&target) != Some(&revision) {
                        if !self.check_image(
                            pending,
                            pending.root(),
                            Path::new(&path),
                            &[image],
                            capture,
                        )? {
                            return invalid();
                        }
                        self.checked_analysis.insert(target, revision);
                    }
                }
            }
        }
        Ok(())
    }
}

pub(crate) fn recover(pending: &PendingRecovery<'_>) -> io::Result<bool> {
    let journal = safe(pending.root(), Path::new(JOURNAL))?;
    let meta = fs::symlink_metadata(&journal)?;
    if !meta.is_dir()
        || meta.file_type().is_symlink()
        || journal.canonicalize()? != pending.root().canonicalize()?.join(JOURNAL)
    {
        return invalid();
    }
    let saved = plain(pending.journal(), Path::new(INTENT))?;
    let resuming = saved.is_some();
    let saved = saved
        .map(fs::read)
        .transpose()?
        .map(|bytes| parse::<Intent>(&bytes))
        .transpose()?;
    let mut plan = match Plan::load(pending, saved) {
        Ok(plan) => plan,
        Err(_) if !resuming => return Ok(false),
        Err(error) => return Err(error),
    };
    if let Err(error) = plan.check_all(pending, !resuming) {
        if !resuming {
            return Ok(false);
        }
        return Err(error);
    }
    let original = plain(pending.root(), &plan.intent.to)?;
    let rejected = plain(pending.journal(), Path::new(REPLACEMENT))?;
    if !resuming {
        if original.is_none()
            || rejected.is_some()
            || plain(pending.journal(), &join(".rejected", &plan.intent.from))?.is_some()
            || read(pending.root(), &plan.manifest_path)?
                != read(pending.journal(), &join(".sealed", &plan.manifest_path))?
        {
            return Ok(false);
        }
        let stamp = incremental::copy_revision_fresh(original.as_ref().ok_or_else(refused)?)?;
        if plan
            .new
            .generation
            .as_ref()
            .ok_or_else(refused)?
            .outputs
            .get(plan.intent.to.to_str().ok_or_else(refused)?)
            != Some(&stamp)
        {
            return Ok(false);
        }
        pending.check_root()?;
        durable::write(
            &pending.journal().join(INTENT),
            &serde_json::to_vec(&plan.intent)?,
        )?;
        #[cfg(test)]
        checkpoint()?;
    }
    if original.is_some() == rejected.is_some() {
        return invalid();
    }
    if let Some(original) = original {
        pending.check_root()?;
        plan.check_path(pending, &plan.intent.to.clone(), false)?;
        if plain(pending.journal(), Path::new(REPLACEMENT))?.is_some() {
            return invalid();
        }
        durable::rename(&original, &pending.journal().join(REPLACEMENT))?;
        durable::sync_dir(original.parent().ok_or_else(refused)?)?;
        durable::sync_dir(pending.journal())?;
        #[cfg(test)]
        checkpoint()?;
    }
    let replacement = [plan.replacement.clone()];
    if !plan.check_image(
        pending,
        pending.journal(),
        Path::new(REPLACEMENT),
        &replacement,
        false,
    )? {
        return invalid();
    }
    pending.restore_previous(&mut |path, point| {
        plan.check_path(pending, path, false)?;
        #[cfg(test)]
        if point == durable::RecoveryPoint::After {
            checkpoint()?;
        }
        #[cfg(not(test))]
        let _ = point;
        Ok(())
    })?;
    plan.check_all(pending, false)?;
    for entry in &plan.entries {
        let expected = plan.old_images.get(&entry.path).and_then(Option::as_ref);
        if expected.is_none() && plain(pending.root(), &entry.path)?.is_some() {
            return invalid();
        }
        if expected.is_some() && plain(pending.root(), &entry.path)?.is_none() {
            return invalid();
        }
    }
    let old = plan.old.generation.as_ref().ok_or_else(refused)?;
    if incremental::databases(pending.root()).map_err(io::Error::other)? != old.databases
        || !incremental::receipts_match(pending.root(), old)?
    {
        return invalid();
    }
    let check = crate::verification::verify_databases(pending.root()).map_err(io::Error::other)?;
    if !check.is_ok() {
        return invalid();
    }
    if incremental::databases(pending.root()).map_err(io::Error::other)? != old.databases
        || !incremental::receipts_match(pending.root(), old)?
    {
        return invalid();
    }
    plan.check_all(pending, false)?;
    pending.check_root()?;
    #[cfg(test)]
    checkpoint()?;
    pending.retire()?;
    Ok(true)
}

#[cfg(test)]
std::thread_local! {
    static STOP_AT: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
    static HASH_PASSES: std::cell::RefCell<BTreeMap<PathBuf, usize>> = const { std::cell::RefCell::new(BTreeMap::new()) };
}
#[cfg(test)]
fn checkpoint() -> io::Result<()> {
    STOP_AT.with(|stop| match stop.get() {
        Some(0) => {
            stop.set(None);
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "generated recovery interruption",
            ))
        }
        Some(left) => {
            stop.set(Some(left - 1));
            Ok(())
        }
        None => Ok(()),
    })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines
)]
mod tests {
    use super::*;
    #[allow(dead_code, clippy::unwrap_used, clippy::panic, clippy::pedantic)]
    mod fixtures {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/common/legacy_overlap.rs"
        ));
    }

    fn raw_before(mode: &str) -> Vec<u8> {
        fixtures::before_tree(mode, false)[&PathBuf::from(format!("{mode}/rbxport/manifest.json"))]
            .clone()
    }

    #[test]
    fn original_legacy_structure_is_checked_without_inserting_current_defaults() {
        for mode in ["PIONEER", ".PIONEER"] {
            let raw = raw_before(mode);
            let old = manifest(&raw).unwrap();
            assert_ne!(
                incremental::manifest_structure(&old).unwrap(),
                old.generation.as_ref().unwrap().structure
            );
            assert!(manifest(&compact(std::str::from_utf8(&raw).unwrap())).is_ok());
        }
        let directory = tempfile::tempdir().unwrap();
        fixtures::put(
            &directory.path().join("PIONEER/rbxport/manifest.json"),
            &raw_before("PIONEER"),
        );
        assert!(
            Manifest::load(directory.path())
                .unwrap()
                .generation
                .is_none(),
            "ordinary loading must retain its original compatibility behavior"
        );
    }

    fn resign(raw: &str) -> String {
        let parsed: RawManifest<'_> = serde_json::from_str(raw).unwrap();
        let previous = parsed.generation.structure.clone();
        let mut tuple = vec![b'['];
        for (index, value) in [
            parsed.db_id,
            parsed.tracks,
            parsed.playlists,
            parsed.loose,
            parsed.baseline,
            parsed.generation.device_sources,
        ]
        .iter()
        .enumerate()
        {
            if index != 0 {
                tuple.push(b',');
            }
            tuple.extend(compact(value.get()));
        }
        tuple.push(b']');
        assert_eq!(raw.matches(&previous).count(), 1);
        raw.replacen(&previous, &digest(&tuple), 1)
    }

    #[test]
    fn duplicate_and_unknown_receipt_fields_refuse_even_with_a_matching_raw_checksum() {
        let raw = String::from_utf8(raw_before("PIONEER")).unwrap();
        let parsed: RawManifest<'_> = serde_json::from_str(&raw).unwrap();
        let sources: serde_json::Value =
            serde_json::from_str(parsed.generation.device_sources.get()).unwrap();
        let first = serde_json::to_string(&sources["1"]).unwrap();
        let duplicate = raw.replacen(
            "\"device_sources\": {",
            &format!("\"device_sources\": {{\"1\":{first},"),
            1,
        );
        assert!(manifest(resign(&duplicate).as_bytes()).is_err());
        let unknown = raw.replacen("\"metadata\": {", "\"metadata\": {\"unrecognized\":1,", 1);
        assert!(manifest(resign(&unknown).as_bytes()).is_err());
        let unknown_root = raw.replacen('{', "{\"unrecognized\":1,", 1);
        assert!(manifest(unknown_root.as_bytes()).is_err());
        let parsed: RawManifest<'_> = serde_json::from_str(&raw).unwrap();
        assert!(manifest(raw.replacen(&parsed.generation.structure, "", 1).as_bytes()).is_err());
    }

    #[cfg(target_os = "linux")]
    fn interrupted(root: &Path, boundary: usize) -> io::Result<()> {
        let previous = STOP_AT.with(|stop| stop.replace(Some(boundary)));
        let result = crate::recover(root);
        STOP_AT.with(|stop| stop.set(previous));
        result
    }

    #[cfg(target_os = "linux")]
    fn assert_original(root: &Path, mode: &str) {
        assert_eq!(fixtures::tree(root), fixtures::before_tree(mode, false));
        assert_eq!(fixtures::stamps(root), fixtures::before_stamps(mode, false));
        assert!(!crate::recovery_pending(root));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn every_durable_repair_boundary_resumes_with_generic_paths_still_refusing() {
        for mode in ["PIONEER", ".PIONEER"] {
            let mut stopped = 0;
            for boundary in 0..64 {
                let root = fixtures::image(mode, false);
                let result = interrupted(&root, boundary);
                if result.is_ok() {
                    assert_original(&root, mode);
                    break;
                }
                assert_eq!(
                    result.unwrap_err().kind(),
                    io::ErrorKind::Interrupted,
                    "boundary {boundary}"
                );
                stopped += 1;
                assert!(root.join(JOURNAL).join(INTENT).is_file());
                assert!(root.join(JOURNAL).join(".incomplete").is_file());
                assert!(!root.join(JOURNAL).join(".rollback").exists());
                let before = fixtures::tree(&root);
                assert!(durable::Publication::recover(&root, JOURNAL).is_err());
                assert!(durable::Publication::new(&root, JOURNAL).is_err());
                assert!(durable::Publication::finalize_pending(&root, JOURNAL).is_err());
                assert_eq!(
                    fixtures::tree(&root),
                    before,
                    "generic replay must never bypass format authority"
                );
                crate::recover(&root).unwrap();
                assert_original(&root, mode);
                crate::recover(&root).unwrap();
                assert_original(&root, mode);
                assert!(boundary != 63, "all durable boundaries must be enumerated");
            }
            assert!(
                stopped >= 5,
                "intent, quarantine, each restore and verified state are exercised"
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn resume_refuses_changed_old_image_quarantine_or_immutable_records_without_writes() {
        for mode in ["PIONEER", ".PIONEER"] {
            for damage in [
                "old-audio",
                "quarantine",
                "missing-quarantine",
                "publication",
                "sealed",
                "staged-known",
                "unknown-journal",
            ] {
                let root = fixtures::image(mode, false);
                assert_eq!(
                    interrupted(&root, 1).unwrap_err().kind(),
                    io::ErrorKind::Interrupted
                );
                let journal = root.join(JOURNAL);
                let manifest_path = PathBuf::from(format!("{mode}/rbxport/manifest.json"));
                let old =
                    manifest(&read(&journal, &join(".previous", &manifest_path)).unwrap()).unwrap();
                let from = PathBuf::from(old.tracks[0].audio.trim_start_matches('/'));
                match damage {
                    "old-audio" | "quarantine" => {
                        let path = if damage == "old-audio" {
                            journal.join(".previous").join(&from)
                        } else {
                            journal.join(REPLACEMENT)
                        };
                        let modified = fs::metadata(&path).unwrap().modified().unwrap();
                        let mut bytes = fs::read(&path).unwrap();
                        bytes[0] ^= 1;
                        fixtures::put(&path, &bytes);
                        fs::File::options()
                            .write(true)
                            .open(path)
                            .unwrap()
                            .set_times(fs::FileTimes::new().set_modified(modified))
                            .unwrap();
                    }
                    "missing-quarantine" => fs::remove_file(journal.join(REPLACEMENT)).unwrap(),
                    "publication" => fixtures::put(&journal.join("publication.json"), b"[]"),
                    "sealed" => fixtures::put(&journal.join(".sealed").join(&manifest_path), b"{}"),
                    "staged-known" => {
                        let new =
                            manifest(&read(&journal, &join(".sealed", &manifest_path)).unwrap())
                                .unwrap();
                        let replacement = new
                            .tracks
                            .iter()
                            .find(|track| track.library_id != old.tracks[0].library_id)
                            .unwrap();
                        let path = PathBuf::from(format!(
                            "{}/ANLZ0000.DAT",
                            replacement.anlz_dir.trim_start_matches('/')
                        ));
                        let original = root.join(&path);
                        let modified = fs::metadata(&original).unwrap().modified().unwrap();
                        fixtures::put(&journal.join(&path), &fs::read(original).unwrap());
                        fs::File::options()
                            .write(true)
                            .open(journal.join(path))
                            .unwrap()
                            .set_times(fs::FileTimes::new().set_modified(modified))
                            .unwrap();
                    }
                    "unknown-journal" => fixtures::put(
                        &journal.join("user-data.bin"),
                        b"unknown generated user payload",
                    ),
                    _ => unreachable!(),
                }
                let before = fixtures::tree(&root);
                let stamps = fixtures::stamps(&root);
                assert!(crate::recover(&root).is_err(), "{damage}");
                assert_eq!(fixtures::tree(&root), before, "{damage}");
                assert_eq!(fixtures::stamps(&root), stamps, "{damage}");
            }
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn known_content_reappearing_beside_previous_and_rejected_is_not_a_resume_state() {
        for mode in ["PIONEER", ".PIONEER"] {
            let mut exercised = false;
            for boundary in 0..64 {
                let root = fixtures::image(mode, false);
                if interrupted(&root, boundary).is_ok() {
                    break;
                }
                let journal = root.join(JOURNAL);
                let old: Manifest = serde_json::from_slice(&raw_before(mode)).unwrap();
                let from = PathBuf::from(old.tracks[0].audio.trim_start_matches('/'));
                let previous = journal.join(".previous").join(&from);
                let rejected = journal.join(".rejected").join(&from);
                if previous.is_file() && rejected.is_file() && !root.join(&from).exists() {
                    let modified = fs::metadata(&rejected).unwrap().modified().unwrap();
                    fixtures::put(&root.join(&from), &fs::read(&rejected).unwrap());
                    fs::File::options()
                        .write(true)
                        .open(root.join(&from))
                        .unwrap()
                        .set_times(fs::FileTimes::new().set_modified(modified))
                        .unwrap();
                    let before = fixtures::tree(&root);
                    assert!(crate::recover(&root).is_err());
                    assert_eq!(fixtures::tree(&root), before);
                    exercised = true;
                    break;
                }
            }
            assert!(exercised);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn untouched_required_analysis_is_hashed_once_and_same_stamp_tampering_refuses() {
        for mode in ["PIONEER", ".PIONEER"] {
            let good = fixtures::image_kind(mode, true, true);
            let old: Manifest = serde_json::from_slice(
                &fixtures::before_kind(mode, true, true)
                    [&PathBuf::from(format!("{mode}/rbxport/manifest.json"))],
            )
            .unwrap();
            let stable = old
                .tracks
                .iter()
                .find(|track| track.library_id == 9)
                .unwrap();
            let dat = good
                .join(stable.anlz_dir.trim_start_matches('/'))
                .join("ANLZ0000.DAT");
            let audio = good.join(stable.audio.trim_start_matches('/'));
            HASH_PASSES.with(|passes| passes.borrow_mut().clear());
            crate::recover(&good).unwrap();
            HASH_PASSES.with(|passes| {
                let passes = passes.borrow();
                assert_eq!(passes.get(&dat), Some(&1));
                assert!(
                    !passes.contains_key(&audio),
                    "unchanged audio keeps ordinary stamp trust"
                );
            });
            assert_eq!(
                fixtures::tree(&good),
                fixtures::before_kind(mode, true, true)
            );

            let root = fixtures::image_kind(mode, true, true);
            let old: Manifest = serde_json::from_slice(
                &fixtures::before_kind(mode, true, true)
                    [&PathBuf::from(format!("{mode}/rbxport/manifest.json"))],
            )
            .unwrap();
            let stable = old
                .tracks
                .iter()
                .find(|track| track.library_id == 9)
                .unwrap();
            let path = PathBuf::from(format!(
                "{}/ANLZ0000.DAT",
                stable.anlz_dir.trim_start_matches('/')
            ));
            let modified = fs::metadata(root.join(&path)).unwrap().modified().unwrap();
            let mut bytes = fs::read(root.join(&path)).unwrap();
            bytes[0] ^= 1;
            fixtures::put(&root.join(&path), &bytes);
            fs::File::options()
                .write(true)
                .open(root.join(&path))
                .unwrap()
                .set_times(fs::FileTimes::new().set_modified(modified))
                .unwrap();
            let before = fixtures::tree(&root);
            assert!(crate::recover(&root).is_err());
            assert_eq!(fixtures::tree(&root), before);
            assert!(!root.join(JOURNAL).join(INTENT).exists());
        }
    }

    #[cfg(unix)]
    #[test]
    fn containment_validation_rejects_descendant_aliases_without_publication() {
        let owned = tempfile::tempdir().unwrap();
        let root = owned.path().join("volume");
        let other = owned.path().join("other");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&other).unwrap();
        fixtures::put(&other.join("marker"), b"untouched");
        std::os::unix::fs::symlink(&other, root.join("alias")).unwrap();
        assert!(safe(&root, Path::new("alias/marker")).is_err());
        assert!(safe(&root, Path::new("../other/marker")).is_err());
        assert_eq!(fs::read(other.join("marker")).unwrap(), b"untouched");
    }
}
