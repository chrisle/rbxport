//! The `content` rows of an existing `exportLibrary.db`, changed where they
//! are.
//!
//! A beat grid edited on a Devices tree track changes that track's tempo in
//! the library it was browsed from (#660); nothing else in the row moves.

use std::path::Path;

use rusqlite::params;

use crate::Result;

/// Sets one track's `bpmx100`. Answers how many rows changed: 1, or 0 when
/// the library has no such track.
pub fn set_tempo(path: &Path, content_id: u32, bpm_x100: u32) -> Result<usize> {
    let conn = crate::playlists::open_read_write(path)?;
    Ok(conn.execute("UPDATE content SET bpmx100 = ?1 WHERE content_id = ?2", params![bpm_x100, content_id])?)
}
