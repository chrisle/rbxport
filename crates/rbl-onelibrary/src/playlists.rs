//! The playlist tables of an existing `exportLibrary.db`, read and changed
//! where they are.
//!
//! rekordbox edits a stick's playlists from its Devices tree one library at
//! a time [DOC: rekordbox FAQ "Device Library Plus", "changes are only
//! reflected on the playlists in the library for which the controls had
//! been performed"]. This is that edit for the `OneLibrary` file: the
//! `playlist` and `playlist_content` rows change and nothing else does.

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{params, Connection, OpenFlags};

use crate::{key, unlock, Result};

/// One row of `playlist`. `attribute` 1 is a folder; anything else is a
/// list, and is written back as it was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistRow {
    pub id: i64,
    pub parent: i64,
    pub name: String,
    pub attribute: i64,
    pub sequence: i64,
}

/// The `attribute` of a folder.
pub const FOLDER: i64 = 1;

/// Every playlist row by id, and every list's tracks in `sequenceNo` order.
pub type Playlists = (Vec<PlaylistRow>, BTreeMap<i64, Vec<i64>>);

/// Reads the playlist tree and its contents from an open database.
pub fn read(conn: &Connection) -> Result<Playlists> {
    let mut statement = conn.prepare(
        "SELECT playlist_id, COALESCE(playlist_id_parent, 0), COALESCE(name, ''),
                COALESCE(attribute, 0), COALESCE(sequenceNo, 0)
         FROM playlist ORDER BY playlist_id",
    )?;
    let rows = statement
        .query_map([], |r| {
            Ok(PlaylistRow { id: r.get(0)?, parent: r.get(1)?, name: r.get(2)?, attribute: r.get(3)?, sequence: r.get(4)? })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut statement =
        conn.prepare("SELECT playlist_id, content_id FROM playlist_content ORDER BY playlist_id, sequenceNo, rowid")?;
    let mut contents: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
    for row in statement.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
        let (playlist, content) = row?;
        contents.entry(playlist).or_default().push(content);
    }
    Ok((rows, contents))
}

/// What one edit does to the two tables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Change {
    /// New rows, inserted with no artwork.
    pub create: Vec<PlaylistRow>,
    /// Ids whose name changes.
    pub rename: Vec<(i64, String)>,
    /// Ids whose `sequenceNo` changes.
    pub sequence: Vec<(i64, i64)>,
    /// Ids whose row and contents go.
    pub delete: Vec<i64>,
    /// Lists whose contents are replaced, numbered from 1 in this order.
    pub contents: Vec<(i64, Vec<i64>)>,
}

/// Opens a database for writing, with the durable rollback journal every
/// other write to a stick's `exportLibrary.db` uses.
pub fn open_read_write(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|source| crate::Error::Open { path: path.display().to_string(), source })?;
    unlock(&conn, &key::passphrase()?)?;
    conn.query_row("SELECT COUNT(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0))?;
    crate::durable_writes(&conn)?;
    Ok(conn)
}

/// Applies `change` to the database at `path`, in one transaction.
pub fn apply(path: &Path, change: &Change) -> Result<()> {
    let conn = open_read_write(path)?;
    let tx = rusqlite::Transaction::new_unchecked(&conn, rusqlite::TransactionBehavior::Immediate)?;
    for id in &change.delete {
        tx.execute("DELETE FROM playlist_content WHERE playlist_id = ?1", params![id])?;
        tx.execute("DELETE FROM playlist WHERE playlist_id = ?1", params![id])?;
    }
    for row in &change.create {
        tx.execute(
            "INSERT INTO playlist (playlist_id, sequenceNo, name, image_id, attribute, playlist_id_parent)
             VALUES (?1, ?2, ?3, NULL, ?4, ?5)",
            params![row.id, row.sequence, row.name, row.attribute, row.parent],
        )?;
    }
    for (id, name) in &change.rename {
        tx.execute("UPDATE playlist SET name = ?1 WHERE playlist_id = ?2", params![name, id])?;
    }
    for (id, sequence) in &change.sequence {
        tx.execute("UPDATE playlist SET sequenceNo = ?1 WHERE playlist_id = ?2", params![sequence, id])?;
    }
    for (id, tracks) in &change.contents {
        tx.execute("DELETE FROM playlist_content WHERE playlist_id = ?1", params![id])?;
        for (position, content) in tracks.iter().enumerate() {
            let sequence = i64::try_from(position).unwrap_or(i64::MAX - 1) + 1;
            tx.execute(
                "INSERT INTO playlist_content (playlist_id, content_id, sequenceNo) VALUES (?1, ?2, ?3)",
                params![id, content, sequence],
            )?;
        }
    }
    tx.commit()?;
    Ok(())
}
