//! Schema probe.
//!
//! rekordbox's schema changes between versions. Rather than assume, we read the
//! declared version and check the columns we depend on exist. An unknown or
//! incomplete schema degrades the library to read-only instead of risking a
//! wrong write.

use rusqlite::Connection;

use crate::{DbError, Result};

/// Columns this application reads. Anything missing means the schema moved.
const REQUIRED: &[(&str, &[&str])] = &[
    ("djmdContent", &[
        "ID", "Title", "ArtistID", "AlbumID", "GenreID", "LabelID", "KeyID",
        "BPM", "Length", "Rating", "ColorID", "FolderPath", "FileNameL",
        "AnalysisDataPath", "DJPlayCount", "StockDate", "ReleaseDate",
        "Commnt", "rb_local_deleted",
    ]),
    ("djmdPlaylist", &["ID", "Name", "ParentID", "Seq", "rb_local_deleted"]),
    ("djmdSongPlaylist", &["ID", "PlaylistID", "ContentID", "TrackNo", "rb_local_deleted"]),
    ("djmdArtist", &["ID", "Name"]),
    ("djmdAlbum", &["ID", "Name"]),
    ("djmdGenre", &["ID", "Name"]),
    ("djmdKey", &["ID", "ScaleName"]),
    ("djmdLabel", &["ID", "Name"]),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaSupport {
    /// Everything we read is present.
    Full,
    /// Readable, but something is missing; writes must be disabled.
    Degraded { missing: Vec<String> },
}

#[derive(Debug, Clone)]
pub struct SchemaProbe {
    /// `djmdProperty.DBVersion`, e.g. 6000. `None` when the row is absent.
    pub db_version: Option<i64>,
    pub support: SchemaSupport,
    pub table_count: usize,
}

impl SchemaProbe {
    pub fn probe(conn: &Connection) -> Result<Self> {
        // This is also the first real read, so a wrong passphrase surfaces here.
        let table_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table'", [], |r| r.get(0))
            .map_err(|e| match &e {
                rusqlite::Error::SqliteFailure(code, _)
                    if matches!(code.code, rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => DbError::Sqlite(e),
                _ => DbError::Open(format!(
                    "could not read the schema (usually a wrong passphrase): {e}"
                )),
            })?;

        // Stored as TEXT ("6000"), not an integer. Reading it as i64 fails, and
        // an earlier version swallowed that with `.ok()` and reported no version
        // at all — so parse explicitly and say so when it does not parse.
        let db_version: Option<i64> = match conn.query_row(
            "SELECT DBVersion FROM djmdProperty LIMIT 1",
            [],
            |r| r.get::<_, String>(0),
        ) {
            Ok(text) => match text.trim().parse::<i64>() {
                Ok(v) => Some(v),
                Err(e) => {
                    tracing::warn!(value = %text, error = %e, "DBVersion is not a number");
                    None
                }
            },
            Err(e) => {
                tracing::warn!(error = %e, "could not read djmdProperty.DBVersion");
                None
            }
        };

        let mut missing = Vec::new();
        for (table, columns) in REQUIRED {
            let present = column_names(conn, table)?;
            if present.is_empty() {
                missing.push((*table).to_owned());
                continue;
            }
            for column in *columns {
                if !present.iter().any(|c| c.eq_ignore_ascii_case(column)) {
                    missing.push(format!("{table}.{column}"));
                }
            }
        }

        let support = if missing.is_empty() {
            SchemaSupport::Full
        } else {
            tracing::warn!(?missing, "schema is missing expected columns; library is read-only");
            SchemaSupport::Degraded { missing }
        };

        Ok(Self {
            db_version,
            support,
            table_count: usize::try_from(table_count).unwrap_or(0),
        })
    }

    /// Whether the *schema* is complete enough to write. This is not permission
    /// to write: `Library::open` still refuses while rekordbox is running.
    pub fn schema_supports_writes(&self) -> bool {
        matches!(self.support, SchemaSupport::Full)
    }
}

fn column_names(conn: &Connection, table: &str) -> Result<Vec<String>> {
    // table_info takes an identifier, so it cannot be bound as a parameter;
    // the names come from the constant above, never from user input.
    let sql = format!("PRAGMA table_info({table})");
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return Ok(Vec::new()); // table absent; the caller reports it as missing
    };
    let rows = stmt.query_map([], |r| r.get::<_, String>(1))?;
    Ok(rows.filter_map(std::result::Result::ok).collect())
}
