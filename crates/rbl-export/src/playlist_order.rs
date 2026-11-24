//! Canonical ranks preserve signed SQLite sibling order without narrowing positions.
use rbl_onelibrary::playlists::PlaylistRow;

use crate::{ExportError, Result};

pub(crate) fn ranked(mut rows: Vec<PlaylistRow>) -> Result<Vec<(PlaylistRow, u32)>> {
    rows.sort_by_key(|row| (row.parent, row.sequence, row.id));
    let mut parent = None;
    let mut rank = 0_u64;
    rows.into_iter()
        .map(|row| {
            if parent != Some(row.parent) {
                parent = Some(row.parent);
                rank = 0;
            } else {
                rank += 1;
            }
            Ok((
                row,
                u32::try_from(rank).map_err(|e| ExportError::OneLibrary(e.to_string()))?,
            ))
        })
        .collect()
}
