//! Disposable shared probe/conversion cache. Recovery never depends on it.
use std::{collections::HashMap, path::{Path, PathBuf}, sync::{Arc, Mutex, Condvar}};
use serde::{Deserialize, Serialize};
use crate::{incremental, Result, ExportError};

/// The conversion temp is limited to 2 GiB. Completed entries are reduced to
/// 6 GiB before a new conversion so the internal conversion cache stays below
/// 8 GiB while encoding. A larger track is converted on the destination.
pub const CONVERSION_TEMP_LIMIT: u64 = 2 * 1024 * 1024 * 1024;
pub const COMPLETED_CACHE_LIMIT: u64 = 6 * 1024 * 1024 * 1024;
pub const TOTAL_CACHE_LIMIT: u64 = COMPLETED_CACHE_LIMIT + CONVERSION_TEMP_LIMIT;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Receipt {
    source: incremental::FileRevision,
    output: incremental::FileRevision,
    size: u64,
    digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProbeReceipt {
    source: incremental::FileRevision,
    properties: rbl_audio::compatibility::AudioProperties,
    checksum: String,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn cached_output_receipts_round_trip_exactly() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let source = incremental::revision(file.path()).unwrap().unwrap();
        let output = incremental::copy_revision(file.path()).unwrap().unwrap();
        let receipt = Receipt { source: source.clone(), output: output.clone(), size: 0, digest: "test".into() };
        let bytes = serde_json::to_vec(&receipt).unwrap();
        let restored: Receipt = serde_json::from_slice(&bytes).expect("cache receipt must be readable");
        assert_eq!(restored.source, source);
        assert_eq!(restored.output, output);
    }

    #[test]
    fn conversion_is_reused_without_reencoding() {
        let dir = tempfile::tempdir().unwrap();
        let cache = PreparationCache::new(dir.path().join("cache"));
        let source = dir.path().join("source.flac");
        std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("../rbl-audio/tests/fixtures/stereo-96k.flac"), &source).unwrap();
        let first = cache.converted(&source, rbl_audio::compatibility::Format::Mp3, &mut |_, _| {}, &|| false).unwrap();
        let second = cache.converted(&source, rbl_audio::compatibility::Format::Mp3, &mut |_, _| {}, &|| false).unwrap();
        assert!(!first.2);
        assert!(second.2, "second conversion should reuse its receipt; source {:?}, output {:?}", incremental::revision(&source), incremental::copy_revision(&first.0));
    }

    #[test]
    fn pruning_bounds_completed_files_and_removes_interrupted_conversion() {
        let dir = tempfile::tempdir().unwrap();
        let cache = PreparationCache::new(dir.path().join("cache"));
        cache.gates.lock().unwrap().insert("old-source".into(), Arc::new(Mutex::new(())));
        cache.hashes.lock().unwrap().insert("old-source".into(), 42);
        std::fs::create_dir_all(&cache.root).unwrap();
        let interrupted = cache.root.join("conversion-interrupted");
        std::fs::create_dir(&interrupted).unwrap();
        std::fs::write(interrupted.join("audio.wav"), vec![1; 64]).unwrap();
        for digit in ['a', 'b'] {
            let key: String = std::iter::repeat_n(digit, 64).collect();
            std::fs::write(cache.root.join(format!("{key}.wav")), vec![1; 64]).unwrap();
        }
        cache.prune(64).unwrap();
        assert!(cache.gates.lock().unwrap().is_empty());
        assert!(cache.hashes.lock().unwrap().is_empty());
        assert!(!interrupted.exists());
        let retained_bytes: u64 = std::fs::read_dir(&cache.root).unwrap()
            .map(|entry| entry.unwrap().metadata().unwrap().len()).sum();
        assert!(retained_bytes <= 64, "retained {retained_bytes} bytes");
    }

    #[test]
    fn size_limited_cache_retries_conversion_in_destination_stage() {
        let source_dir = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let cache_dir = tempfile::tempdir().unwrap();
        let source = source_dir.path().join("source.flac");
        std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("../rbl-audio/tests/fixtures/stereo-96k.flac"), &source).unwrap();
        let limit = 1024;
        let mut cache = PreparationCache::new(cache_dir.path().to_owned());
        cache.conversion_temp_limit = limit;
        let cache_error = cache.with_converted(
            &source, rbl_audio::compatibility::Format::Wav,
            &mut |_, _| {}, &|| false, |_, _, _| Ok(()),
        ).unwrap_err();
        assert!(matches!(cache_error, ExportError::Io(ref error) if error.kind() == std::io::ErrorKind::StorageFull));
        let track = crate::SourceTrack {
            id: 1,
            source_path: source.clone(),
            title: "Converted track".into(),
            artist: "Artist".into(),
            duration_sec: 1,
            sample_rate: 96_000,
            ..Default::default()
        };
        let playlist = crate::SourcePlaylist {
            id: 10,
            name: "Playlist".into(),
            track_indices: vec![0],
            ..Default::default()
        };
        let mut saw_verification = false;
        let report = crate::export_with_options(
            destination.path(), &[track], &[playlist], &[],
            &crate::ExportOptions {
                compatibility: Some(crate::CompatibilityFormat::Wav),
                cache: Some(&cache),
                ..Default::default()
            },
            &mut |progress| { if progress.stage == "verifying" { saw_verification = true; } },
        ).unwrap();

        assert_eq!(report.tracks, 1);
        assert_eq!(report.converted, 1);
        assert_eq!(report.metrics.conversions, 1, "cache conversion must be attempted before destination fallback");
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        assert!(saw_verification, "export must reach destination-stage verification");
        assert!(report.verified);
        let manifest = crate::Manifest::load(destination.path()).unwrap();
        let exported = destination.path().join(manifest.tracks[0].audio.trim_start_matches('/'));
        let exported_size = std::fs::metadata(&exported).unwrap().len();
        assert_eq!(exported_size, 44 + 11_025 * 4);
        assert!(exported_size > limit);
        assert!(!rbl_audio::compatibility::needs_conversion(&exported).unwrap());
        assert!(crate::verify(destination.path()).unwrap().is_ok());
        assert_eq!(cache.converted_size(&source, rbl_audio::compatibility::Format::Wav), None);
        let mut internal_bytes = 0;
        for entry in std::fs::read_dir(cache_dir.path()).unwrap() {
            let entry = entry.unwrap();
            assert!(!entry.file_type().unwrap().is_dir(), "temporary conversion directory remains: {:?}", entry.path());
            internal_bytes += entry.metadata().unwrap().len();
        }
        assert!(internal_bytes <= limit, "internal cache retained {internal_bytes} bytes with a {limit}-byte conversion limit");
    }
}

#[derive(Debug)]
pub struct PreparationCache {
    root: PathBuf,
    conversion_temp_limit: u64,
    consumers: Mutex<()>,
    gates: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    probes: Mutex<HashMap<String, rbl_audio::compatibility::AudioProperties>>,
    hashes: Mutex<HashMap<String, u64>>,
    preparing: Mutex<usize>,
    ready: Condvar,
    dirty: std::sync::atomic::AtomicBool,
}

struct Permit<'a>(&'a PreparationCache);
impl Drop for Permit<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.0.preparing.lock() {
            *active = active.saturating_sub(1);
            self.0.ready.notify_all();
        }
    }
}

impl PreparationCache {
    fn permit(&self, cancelled: &dyn Fn() -> bool) -> Result<Permit<'_>> {
        let mut active = self.preparing.lock().map_err(|e| std::io::Error::other(e.to_string()))?;
        while *active >= 2 {
            if cancelled() { return Err(ExportError::Cancelled); }
            active = self.ready.wait_timeout(active, std::time::Duration::from_millis(100))
                .map_err(|e| std::io::Error::other(e.to_string()))?.0;
        }
        *active += 1;
        Ok(Permit(self))
    }

    fn lock_gate<'a>(gate: &'a Mutex<()>, cancelled: &dyn Fn() -> bool) -> Result<std::sync::MutexGuard<'a, ()>> {
        loop {
            if cancelled() { return Err(ExportError::Cancelled); }
            match gate.try_lock() {
                Ok(guard) => return Ok(guard),
                Err(std::sync::TryLockError::WouldBlock) => std::thread::sleep(std::time::Duration::from_millis(10)),
                Err(error) => return Err(std::io::Error::other(error.to_string()).into()),
            }
        }
    }

    pub fn original(&self, source: &Path, progress: &mut dyn FnMut(u64, u64), cancelled: &dyn Fn() -> bool) -> Result<PathBuf> {
        let (key, source_revision) = Self::key(source, "original-v1")?;
        let gate = self.gates.lock().map_err(|e| std::io::Error::other(e.to_string()))?
            .entry(key.clone()).or_insert_with(|| Arc::new(Mutex::new(()))).clone();
        let _guard = Self::lock_gate(&gate, cancelled)?;
        if cancelled() { return Err(ExportError::Cancelled); }
        let path = self.root.join(format!("{key}.source"));
        let receipt_path = self.root.join(format!("{key}.json"));
        let saved = std::fs::read(&receipt_path).ok().and_then(|b| serde_json::from_slice::<Receipt>(&b).ok());
        if saved.is_some_and(|r| r.source == source_revision && incremental::revision_fresh(&path).is_ok_and(|now| now.as_ref() == Some(&r.output))) {
            return Ok(path);
        }
        std::fs::create_dir_all(&self.root)?;
        self.dirty.store(true, std::sync::atomic::Ordering::Relaxed);
        let _permit = self.permit(cancelled)?;
        let temp = tempfile::NamedTempFile::new_in(&self.root)?;
        let (size, hash) = crate::copy_staged_observed(source, temp.path(), &mut |done, total| { progress(done, total); !cancelled() })?;
        if incremental::revision_fresh(source)?.as_ref() != Some(&source_revision) {
            return Err(ExportError::Conflict("Source changed while it was being prepared".into()));
        }
        temp.persist(&path).map_err(|e| e.error)?;
        let output = incremental::revision_fresh(&path)?.ok_or_else(|| std::io::Error::other("Prepared source disappeared"))?;
        let receipt = Receipt { source: source_revision, output, size, digest: format!("fnv1a:{hash:016x}") };
        rbl_core::durable::write(&receipt_path, &serde_json::to_vec(&receipt).map_err(std::io::Error::other)?)?;
        Ok(path)
    }
    pub fn new(root: PathBuf) -> Self {
        Self { root, conversion_temp_limit: CONVERSION_TEMP_LIMIT, consumers: Mutex::new(()), gates: Mutex::new(HashMap::new()), probes: Mutex::new(HashMap::new()), hashes: Mutex::new(HashMap::new()), preparing: Mutex::new(0), ready: Condvar::new(), dirty: std::sync::atomic::AtomicBool::new(true) }
    }

    fn key(source: &Path, profile: &str) -> Result<(String, incremental::FileRevision)> {
        let revision = incremental::revision(source)?.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "Source audio is unavailable"))?;
        Ok((incremental::digest(&(source, &revision, profile))?, revision))
    }

    pub fn needs_conversion(&self, source: &Path) -> Result<bool> {
        self.properties(source).map(|p| !p.compatible)
    }

    /// Legacy comparison hash, shared once per source revision across sticks.
    pub fn source_hash(&self, source: &Path, cancelled: &dyn Fn() -> bool) -> Result<u64> {
        let (key, revision) = Self::key(source, "source-hash-v1")?;
        let gate = self.gates.lock().map_err(|e| std::io::Error::other(e.to_string()))?
            .entry(key.clone()).or_insert_with(|| Arc::new(Mutex::new(()))).clone();
        let _guard = Self::lock_gate(&gate, cancelled)?;
        if let Some(hash) = self.hashes.lock().map_err(|e| std::io::Error::other(e.to_string()))?.get(&key) { return Ok(*hash); }
        let hash = crate::file_hash_cancellable(source, cancelled)?;
        if incremental::revision_fresh(source)?.as_ref() != Some(&revision) { return Err(ExportError::Conflict("Source changed while it was being prepared".into())); }
        self.hashes.lock().map_err(|e| std::io::Error::other(e.to_string()))?.insert(key, hash);
        Ok(hash)
    }

    /// Discard receipts after an explicit content check detects a revision
    /// that file information cannot distinguish. Keep files open consumers use.
    pub fn invalidate(&self, source: &Path) -> Result<()> {
        for profile in ["original-v1", "probe-v2", "source-hash-v1", "pcm16-44100-stereo-v1", "pcm16be-44100-stereo-v1", "mp3-320-44100-stereo-v1"] {
            let (key, _) = Self::key(source, profile)?;
            let gate = self.gates.lock().map_err(|e| std::io::Error::other(e.to_string()))?
                .entry(key.clone()).or_insert_with(|| Arc::new(Mutex::new(()))).clone();
            let _guard = gate.lock().map_err(|e| std::io::Error::other(e.to_string()))?;
            self.probes.lock().map_err(|e| std::io::Error::other(e.to_string()))?.remove(&key);
            self.hashes.lock().map_err(|e| std::io::Error::other(e.to_string()))?.remove(&key);
            for extension in ["json", "probe"] {
                match std::fs::remove_file(self.root.join(format!("{key}.{extension}"))) {
                    Ok(()) => {}, Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}, Err(e) => return Err(e.into()),
                }
            }
        }
        self.dirty.store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    /// Exact completed output size for selection previews; never encodes.
    pub fn converted_size(&self, source: &Path, target: rbl_audio::compatibility::Format) -> Option<u64> {
        let (key, source_revision) = Self::key(source, target.profile()).ok()?;
        let receipt: Receipt = serde_json::from_slice(&std::fs::read(self.root.join(format!("{key}.json"))).ok()?).ok()?;
        let path = self.root.join(format!("{key}.{}", target.extension()));
        (receipt.source == source_revision && incremental::revision_fresh(&path).ok()?.as_ref() == Some(&receipt.output)).then_some(receipt.size)
    }

    pub fn properties(&self, source: &Path) -> Result<rbl_audio::compatibility::AudioProperties> {
        let (key, revision) = Self::key(source, "probe-v2")?;
        let mut probes = self.probes.lock().map_err(|e| std::io::Error::other(e.to_string()))?;
        if let Some(value) = probes.get(&key) { return Ok(value.clone()); }
        let path = self.root.join(format!("{key}.probe"));
        let saved = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<ProbeReceipt>(&b).ok());
        let value = if let Some(probe) = saved.filter(|p| p.source == revision && incremental::digest(&(&p.source, &p.properties)).is_ok_and(|hash| hash == p.checksum)) {
            probe.properties
        } else {
                incremental::count(false, false, true);
                let properties = rbl_audio::compatibility::inspect(source).map_err(|e| std::io::Error::other(e.to_string()))?;
                if incremental::revision_fresh(source)?.as_ref() != Some(&revision) { return Err(ExportError::Conflict("Source changed during inspection".into())); }
                properties
        };
        probes.insert(key, value.clone());
        // Failure to cache a probe does not fail an otherwise valid export.
        if std::fs::create_dir_all(&self.root).is_ok() {
            self.dirty.store(true, std::sync::atomic::Ordering::Relaxed);
            let receipt = ProbeReceipt { checksum: incremental::digest(&(&revision, &value))?, source: revision, properties: value.clone() };
            if let Ok(bytes) = serde_json::to_vec(&receipt) { let _ = rbl_core::durable::write(&path, &bytes); }
        }
        Ok(value)
    }

    /// Hold the cache consumer lock through use of the converted file. This
    /// prevents another stick from evicting it during its destination copy.
    pub fn with_converted<T>(
        &self, source: &Path, target: rbl_audio::compatibility::Format,
        progress: &mut dyn FnMut(u64, Option<u64>), cancelled: &dyn Fn() -> bool,
        consume: impl FnOnce(&Path, u64, bool) -> Result<T>,
    ) -> Result<T> {
        let _consumer = Self::lock_gate(&self.consumers, cancelled)?;
        let (path, size, reused) = self.converted(source, target, progress, cancelled)?;
        let result = consume(&path, size, reused);
        // Evict only after the caller has closed its source file. Keep the
        // result of the actual export even if disposable cache cleanup fails.
        let _ = self.prune_inner(TOTAL_CACHE_LIMIT);
        result
    }

    /// Legacy path-returning API for callers that manage file lifetime.
    fn converted(
        &self, source: &Path, target: rbl_audio::compatibility::Format,
        progress: &mut dyn FnMut(u64, Option<u64>), cancelled: &dyn Fn() -> bool,
    ) -> Result<(PathBuf, u64, bool)> {
        let (key, source_revision) = Self::key(source, target.profile())?;
        let gate = self.gates.lock().map_err(|e| std::io::Error::other(e.to_string()))?
            .entry(key.clone()).or_insert_with(|| Arc::new(Mutex::new(()))).clone();
        let _guard = Self::lock_gate(&gate, cancelled)?;
        if cancelled() { return Err(ExportError::Cancelled); }
        let path = self.root.join(format!("{key}.{}", target.extension()));
        let receipt_path = self.root.join(format!("{key}.json"));
        let receipt = std::fs::read(&receipt_path).ok().and_then(|b| serde_json::from_slice::<Receipt>(&b).ok());
        if let Some(saved) = receipt.filter(|r| r.source == source_revision) {
            if incremental::revision_fresh(&path)?.as_ref() == Some(&saved.output) {
                return Ok((path, saved.size, true));
            }
        }
        std::fs::create_dir_all(&self.root)?;
        self.dirty.store(true, std::sync::atomic::Ordering::Relaxed);
        self.prune_inner(COMPLETED_CACHE_LIMIT)?;
        let _permit = self.permit(cancelled)?;
        let temp = tempfile::Builder::new().prefix("conversion-").tempdir_in(&self.root)?;
        let prepared = temp.path().join(format!("audio.{}", target.extension()));
        incremental::count(false, true, false);
        let size = rbl_audio::compatibility::convert_observed_limited(source, &prepared, target, Some(self.conversion_temp_limit), &mut |done, total| {
            if cancelled() { return false; }
            progress(done, total);
            true
        }).map_err(|e| if cancelled() { ExportError::Cancelled } else {
            match e { rbl_audio::AudioError::Io(error) => ExportError::Io(error), error => std::io::Error::other(error.to_string()).into() }
        })?;
        if incremental::revision_fresh(source)?.as_ref() != Some(&source_revision) {
            return Err(ExportError::Conflict("Source changed during conversion; retry the sync".into()));
        }
        if rbl_audio::compatibility::needs_conversion(&prepared).map_err(|e| std::io::Error::other(e.to_string()))? {
            return Err(ExportError::Conflict("Converted audio failed its compatibility check".into()));
        }
        let digest = incremental::content_digest(&prepared)?.ok_or_else(|| std::io::Error::other("Converted audio disappeared"))?;
        std::fs::rename(&prepared, &path)?;
        let output = incremental::revision_fresh(&path)?.ok_or_else(|| std::io::Error::other("Converted audio disappeared"))?;
        let receipt = Receipt { source: source_revision, output, size, digest };
        rbl_core::durable::write(&receipt_path, &serde_json::to_vec(&receipt).map_err(std::io::Error::other)?)?;
        Ok((path, size, false))
    }

    /// Evict only known completed entries before attaching active consumers.
    pub fn prune(&self, budget: u64) -> std::io::Result<()> {
        let _consumer = self.consumers.lock().map_err(|e| std::io::Error::other(e.to_string()))?;
        // Export-boundary pruning also discards per-source bookkeeping. The
        // process-wide cache must not retain keys from every prior export.
        self.gates.lock().map_err(|e| std::io::Error::other(e.to_string()))?.clear();
        self.probes.lock().map_err(|e| std::io::Error::other(e.to_string()))?.clear();
        self.hashes.lock().map_err(|e| std::io::Error::other(e.to_string()))?.clear();
        self.prune_inner(budget)
    }

    fn prune_inner(&self, budget: u64) -> std::io::Result<()> {
        self.dirty.store(false, std::sync::atomic::Ordering::Relaxed);
        let Ok(entries) = std::fs::read_dir(&self.root) else { return Ok(()); };
        let mut files = Vec::new();
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            // A process killed during conversion cannot drop its TempDir.
            // Every normal conversion holds `consumers`, so these directories
            // are leftovers rather than active work when pruning runs.
            if entry.file_type()?.is_dir() && entry.file_name().to_string_lossy().starts_with("conversion-") {
                std::fs::remove_dir_all(path)?;
                continue;
            }
            let key = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()) || !entry.file_type()?.is_file() { continue; }
            let meta = entry.metadata()?;
            files.push((meta.modified()?, meta.len(), path));
        }
        files.sort_by_key(|(time, _, _)| *time);
        let mut total: u64 = files.iter().map(|(_, size, _)| size).sum();
        for (_, size, path) in files {
            if total <= budget { break; }
            std::fs::remove_file(path)?;
            total = total.saturating_sub(size);
        }
        Ok(())
    }
}
