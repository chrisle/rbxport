//! When each track's file was created on this machine's file system.
//!
//! An RBXport-only column (issue #308, approved by the maintainer on
//! 2026-10-12 as a narrow exception to rekordbox parity): rekordbox has no
//! browser column for the file system's own creation time. Its "Date
//! Created" is `djmdContent.DateCreated`, a date stored in the library, which
//! is a different thing and stays as it is.
//!
//! The library never stores this time, so it is read from the file system,
//! and only when the browser asks for it: a page of rows reads the files on
//! that page, and only sorting by the column reads every file in the library,
//! once per library snapshot, on worker threads.
//!
//! Where the file system cannot say when a file was created the value is
//! `None`, which the browser shows as a blank cell and which sorts with the
//! other blanks, first. The modified time is deliberately not used in its
//! place: a column titled "created" that silently showed when a file was last
//! written (a tag edit, say) would be wrong rather than merely missing.
//! `std::fs::Metadata::created` reads the birth time on macOS and Windows,
//! and on Linux through `statx`, which leaves it out on file systems and
//! kernels that do not record one (Linux before 4.11; some network and FUSE
//! mounts).

use std::path::Path;
use std::time::UNIX_EPOCH;

use crate::{Library, Row};

/// Seconds since the Unix epoch at which the file at `path` was created, or
/// `None` when there is no such file or its file system does not record a
/// creation time.
#[must_use]
pub fn created_secs(path: impl AsRef<Path>) -> Option<i64> {
    let path = path.as_ref();
    if path.as_os_str().is_empty() {
        return None;
    }
    let created = std::fs::metadata(path).ok()?.created().ok()?;
    Some(match created.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
        Err(before) => i64::try_from(before.duration().as_secs()).map_or(i64::MIN, |secs| -secs),
    })
}

/// Below this many tracks one thread reads them all; thread startup would
/// cost more than it saves.
const PARALLEL_FROM: usize = 2048;
/// Reading metadata waits on the disk, not the CPU, so this is more workers
/// than the CPU-bound index build uses: a network share answers each stat
/// slowly, and several in flight hide that.
const MAX_WORKERS: usize = 8;

impl Library {
    /// When a track's file was created, in seconds since the Unix epoch.
    ///
    /// From the whole-library read when a sort has already made one, so the
    /// cell agrees with the order; otherwise read now, from the one file.
    #[must_use]
    pub fn file_created(&self, row: Row) -> Option<i64> {
        match self.file_created.get() {
            Some(all) => all.get(row as usize).copied().flatten(),
            None => created_secs(self.folder_path.get(row as usize)),
        }
    }

    /// Every track's file creation time, read on first use and kept for the
    /// life of this snapshot.
    pub(crate) fn file_created_all(&self) -> &[Option<i64>] {
        self.file_created.get_or_init(|| {
            let read = |rows: std::ops::Range<usize>| -> Vec<Option<i64>> {
                rows.map(|row| created_secs(self.folder_path.get(row))).collect()
            };
            let n = self.count;
            let workers = std::thread::available_parallelism()
                .map_or(1, std::num::NonZero::get)
                .clamp(1, MAX_WORKERS);
            if n < PARALLEL_FROM || workers < 2 {
                return read(0..n);
            }
            let chunk = n.div_ceil(workers);
            std::thread::scope(|scope| {
                let jobs: Vec<_> = (0..n)
                    .step_by(chunk)
                    .map(|start| {
                        let rows = start..start.saturating_add(chunk).min(n);
                        let job = std::thread::Builder::new()
                            .name("file-created".into())
                            .spawn_scoped(scope, {
                                let rows = rows.clone();
                                move || read(rows)
                            });
                        (rows, job)
                    })
                    .collect();
                let mut all = Vec::with_capacity(n);
                for (rows, job) in jobs {
                    // A thread that could not start, or died, has its share
                    // read here instead.
                    let part = job.ok().and_then(|job| job.join().ok()).unwrap_or_else(|| read(rows));
                    all.extend(part);
                }
                all
            })
        })
    }

    /// The sort order under the file creation column, built on the first
    /// sort by it.
    pub(crate) fn file_created_rank(&self) -> &[u32] {
        self.file_created_rank
            .get_or_init(|| self.column_rank(crate::SortColumn::FileCreated))
    }

    /// Forgets the creation times read so far, for when the paths they were
    /// read from change.
    pub(crate) fn forget_file_created(&mut self) {
        self.file_created = std::sync::OnceLock::new();
        self.file_created_rank = std::sync::OnceLock::new();
    }
}
