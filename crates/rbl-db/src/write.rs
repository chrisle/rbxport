//! Guarded writes to the library.
//!
//! # What the shapes are based on
//!
//! Every column value here was read off the user's own library rather than
//! guessed, using `cargo run -p rbl-db --example row_shapes`. The population
//! statistics matter more than any single row:
//!
//! - **A locally-created row has `rb_data_status = 0`.** All 57 playlists and
//!   all 56,376 playlist memberships with `usn IS NULL` — i.e. made on this
//!   machine and never synced — carry 0. The values 256/257/258 seen elsewhere
//!   are written by the cloud sync, not by creation, and 257 is *not* the
//!   folder marker it looks like from two samples: it appears under both
//!   `Attribute` values.
//! - **`usn` stays NULL** until sync assigns one; `rb_local_usn` is ours.
//! - **A soft delete sets `rb_local_deleted = 1` and nothing else** —
//!   `rb_data_status` is unchanged on all 919 deleted rows.
//! - **`TrackNo` is contiguous from 1** in every one of the 683 playlists.
//! - **`Seq` is not**: parents start at 0 or 1, so a new entry appends at
//!   `MAX(Seq) + 1` rather than assuming a base.
//! - **Timestamps are UTC with an explicit `+00:00`**, 1,575 of 1,602.
//!
//! # Cues
//!
//! Cue writing was blocked on three unexplained fields. Counting the
//! reference library's 1,040,598 cues settled all three:
//!
//! - **`ColorTableIndex` is not a per-slot palette.** Index 21 dominates every
//!   hot-cue kind alike — 169,389 of kind 1, 171,149 of kind 2 — so it is the
//!   default colour, not a slot's own. Memory cues carry 0 or NULL.
//! - **`Color`** is 255 on memory cues and -1 on hot ones.
//! - **`BeatLoopSize`** is NULL or 0 on every one of the 1,040,176 cues that
//!   is not a loop; only the 422 loops set it.
//!
//! **`BeatLoopSize` encodes the loop's length in beats** as
//! `(beats << 16) | 1`. Every value in the library fits: 65537, 524289,
//! 1048577, 2097153 and 4194305 are 1, 8, 16, 32 and 64 beats. The one loop
//! whose track is still live carries 262145 — four beats — and its In/Out span
//! measures exactly four beats at the track's own BPM. Zero means the length
//! is implied by In/Out rather than stated.
//!
//! So a plain cue and a loop are both determined. The complete desktop
//! `ColorTableIndex` palette has since been extracted, and memory cues use
//! the eight-value `Color` field, so both kinds can be recoloured safely.
//!
//! # Analysis
//!
//! [`Writer::register_analysis`] writes what rekordbox writes on a track it
//! has analysed, read off the reference library's 38,681 rows rather than a
//! recording: `AnalysisDataPath` derived from the row's `UUID` (38,674 rows),
//! `Analysed = 105` (37,663 rows, and the only value on a row whose files are
//! present), `BPM`, and the `djmdKey` row rekordbox itself uses for the key
//! name. `AnalysisUpdated` (0–10, meaning **[UNKNOWN]**) and the meaning of
//! the individual bits of `Analysed` are still unexplained and are left
//! alone.
//!
//! Recorded on 2026-09-17, rekordbox 7.2.11 loading such a row on a deck for
//! the first time [OBS]: it kept the `.DAT` (our grid), `BPM`, `KeyID` and
//! `Analysed`; rewrote the `.EXT`'s waveform bytes in place (same sections,
//! same sizes, `PQT2` still empty, no `PSSI`); added a `.2EX` (`PWV6`,
//! `PWV7`, `PWVC`) and a `.3EX`; and set `AnalysisUpdated` and
//! `TrackInfoUpdated` from NULL to 1.
//!
//! # What this deliberately will not do
//!
//! `contentCue`/`contentFile` are **not implemented**. Their values are still
//! unexplained, and a wrong one in a 38,681-track collection is not
//! recoverable by undo. See [`Unsupported`].
//! [`Writer::set_analysis`] registers an analysis this app made: the BPM,
//! the key, where the files went, and the length. `Analysed` is a bitfield
//! whose individual bits are not all explained (`analysed_bits`), but its
//! complete compatible value is settled: 105 on 37,652 of the reference
//! library's 38,681 tracks and every sampled row whose files are present;
//! 1 and 17 appeared only on rows whose files were missing [OBS]. The
//! registered-copy test also showed rekordbox accepting 105 without `PSSI`,
//! keeping the RBX-authored grid and generating its remaining files [OBS].
//! A track rekordbox had already analysed keeps any other value it carried.
//!
//! # What this deliberately will not do
//!
//! `contentCue`/`contentFile` are **not implemented**. Their values are still
//! unexplained, and a wrong one in a 38,681-track collection is not
//! recoverable by undo. See [`Unsupported`].

use std::path::{Path, PathBuf};

use rbl_core::ids::{Rng, MAX_CONTENT_ID, MAX_CUE_ID, MAX_PLAYLIST_ID};
use rbl_core::time;
use rusqlite::types::Value;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::{is_rekordbox_running, DbError, Library, OpenMode, Result};

/// Tables whose `rb_local_usn` participates in the shared counter.
const USN_TABLES: &[&str] = &[
    "djmdContent",
    "djmdPlaylist",
    "djmdSongPlaylist",
    "djmdSongHotCueBanklist",
];

/// `djmdContent.Analysed` for a complete RBX-authored analysis.
///
/// Kept as a named alias for callers that distinguish the author, although
/// the compatible stored value is the same as [`ANALYSED_FULL`].
pub const ANALYSED_BY_THIS_APP: i64 = ANALYSED_FULL;

/// Local-file registration written by rekordbox when it first opens an
/// RBX-imported track (0x2c0600). A NULL `ContentLink` suppresses its browser
/// preview even when all analysis files exist. Preserve existing values:
/// the other bits also describe states unrelated to our analysis.
const CONTENT_LINK_LOCAL: i64 = 0x002c_0600;

/// What [`Writer::set_analysis`] registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalysisWrite<'a> {
    /// BPM x100, as the column holds it.
    pub bpm_x100: u32,
    /// The key's `ScaleName`; `None` or an unknown name leaves the key alone.
    pub key: Option<&'a str>,
    /// Share-relative, `/PIONEER/USBANLZ/…/ANLZ0000.DAT`.
    pub analysis_path: &'a str,
    /// Whole seconds; `None` keeps what the row had.
    pub length_sec: Option<u32>,
}

/// `djmdPlaylist.Attribute`: an ordinary playlist.
pub const ATTRIBUTE_PLAYLIST: i64 = 0;
/// `djmdPlaylist.Attribute`: a folder that holds other playlists.
pub const ATTRIBUTE_FOLDER: i64 = 1;
/// `djmdPlaylist.Attribute`: an intelligent playlist, whose tracks are what
/// its `SmartList` rule admits rather than rows of `djmdSongPlaylist`.
pub const ATTRIBUTE_SMART: i64 = 4;

/// `ParentID` of a top-level playlist or folder. A string, not an id.
pub const ROOT: &str = "root";

/// How many backups of the library to keep.
const BACKUPS_KEPT: usize = 5;

/// `djmdContent.Analysed` on a track rekordbox has analysed: 105 on 37,663 of
/// the reference library's 38,681 live rows, and the only value on a row whose
/// analysis files are present [OBS]. A bitfield whose bits are **[UNKNOWN]**;
/// the value is mirrored whole.
pub const ANALYSED_FULL: i64 = 105;

/// Attempts before giving up on finding an unused id.
const ID_ATTEMPTS: usize = 64;

/// Columns [`Writer::touch`] will set. A column name is interpolated into SQL,
/// so the set of legal names is spelled out rather than trusted.
const WRITABLE_COLUMNS: &[&str] = &[
    "Name", "Rating", "Commnt", "ColorID", "FolderPath", "FileNameL", "SmartList",
    // The tempo, which a grid edit changes with the `.DAT`'s grid.
    "BPM",
    // The information panel's Info tab.
    "Title", "Lyricist", "ReleaseYear", "TrackNo", "DiscNo", "DJPlayCount", "KeyID", "BPM", "ImagePath",
    "ArtistID", "OrgArtistID", "ComposerID", "RemixerID", "AlbumID", "GenreID", "LabelID",
];

/// Lookup tables [`intern`] may add a row to. Same reason as above.
const LOOKUP_TABLES: &[&str] = &["djmdArtist", "djmdAlbum", "djmdGenre", "djmdLabel"];

/// A field of a track the information panel can edit.
///
/// Only what is settled: plain columns whose meaning is certain, and the
/// references whose lookup row [`Writer::import_file`] already makes for a
/// new track. What is *not* here, and why, is recorded in `details.rs`:
/// the album artist lives on the shared album row, BPM also lives in the
/// analysis grid, and the mix name, message and the two flags are read from
/// columns whose spelling on write has not been seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TrackField {
    Title,
    Artist,
    Album,
    Year,
    TrackNumber,
    DiscNumber,
    OriginalArtist,
    Composer,
    Remixer,
    Lyricist,
    PlayCount,
    Genre,
    Label,
    Key,
    /// A BPM typed over the analysed one. The beat grid in the analysis
    /// file is retimed to it from its first beat, so the CDJ's grid and the
    /// column agree; see [`Writer::set_bpm`].
    Bpm,
}

impl TrackField {
    /// The wire name, as the frontend spells it.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "title" => Self::Title,
            "artist" => Self::Artist,
            "album" => Self::Album,
            "year" => Self::Year,
            "trackNumber" => Self::TrackNumber,
            "discNumber" => Self::DiscNumber,
            "originalArtist" => Self::OriginalArtist,
            "composer" => Self::Composer,
            "remixer" => Self::Remixer,
            "lyricist" => Self::Lyricist,
            "playCount" => Self::PlayCount,
            "genre" => Self::Genre,
            "label" => Self::Label,
            "key" => Self::Key,
            "bpm" => Self::Bpm,
            _ => return None,
        })
    }
}

/// Things the writer refuses to do, and why.
///
/// Returned as an error rather than silently skipped, so a caller cannot
/// believe an unsupported edit succeeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Unsupported {
    /// Nothing is known about what rekordbox does with these.
    ContentCueOrFile,
}

impl Unsupported {
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::ContentCueOrFile =>
                "contentCue and contentFile are not understood and must not be touched",
        }
    }
}

/// What [`Writer::register_analysis`] records on a track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalysisRegistration<'a> {
    /// BPM x100, as `djmdContent.BPM` stores it.
    pub bpm_x100: u32,
    /// The key by rekordbox's name (`Dbm`, `F#`, …), or `None` to leave the
    /// column as it is.
    pub key: Option<&'a str>,
    /// The `.DAT`'s path relative to the share root, from
    /// [`Writer::analysis_data_path_for`]. The files must already be there.
    pub analysis_data_path: &'a str,
}

/// [`Writer::analysis_data_path_for`] over any connection to the library,
/// so a caller that only needs the path reads it without opening the
/// library for writing: each such open derives the key again (about 155 ms
/// on an M2), which a stick of 20,000 tracks paid once per track (#284).
pub fn analysis_data_path(conn: &Connection, content: &str) -> Result<String> {
    let (existing, uuid): (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT AnalysisDataPath, UUID FROM djmdContent
             WHERE ID = ?1 AND rb_local_deleted = 0",
            params![content],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| DbError::WriteRefused(format!("{content} is not a live track")))?;
    if let Some(path) = existing.filter(|p| !p.is_empty()) {
        return Ok(path);
    }
    let uuid = uuid.unwrap_or_default();
    match (uuid.get(..3), uuid.get(3..)) {
        (Some(head), Some(tail)) if !tail.is_empty() => {
            Ok(format!("/PIONEER/USBANLZ/{head}/{tail}/ANLZ0000.DAT"))
        }
        _ => Err(DbError::WriteRefused(format!(
            "{content} has no UUID to derive an analysis path from"
        ))),
    }
}

/// A guarded write session.
///
/// Holds the library open read-write. Every action is one immediate
/// transaction, and the process gate is re-checked before each: rekordbox can
/// be launched at any moment, and a check made when the session opened would
/// be stale by the time an edit happens.
#[derive(Debug)]
pub struct Writer {
    library: Library,
    rng: Rng,
    backup_dir: PathBuf,
    backup_taken: bool,
    automatic_backups: bool,
}

/// What one action changed. Returned so a caller can report it and a test can
/// assert on it without re-querying.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Changed {
    pub rows: usize,
    /// The USN assigned to the last row written.
    pub usn: i64,
}

/// The exact tombstones made by one playlist deletion.
///
/// Keeping row ids, rather than only the deleted root, matters for undo: a
/// playlist can already contain old membership tombstones, and restoring all
/// rows that point at it would bring previously removed tracks back too.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlaylistDeletion {
    pub playlist_ids: Vec<String>,
    pub membership_ids: Vec<String>,
}

/// What [`Writer::import_folder_as_playlist`] did with one folder.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FolderPlaylist {
    /// The playlist made; `None` when nothing was written, either because the
    /// folder held no audio or because of [`Self::conflict`].
    pub playlist: Option<String>,
    /// A sibling with the folder's name, to be replaced only once the user
    /// agrees. Nothing is written while this is set.
    pub conflict: Option<String>,
    /// Tracks added to the library, with the file each came from.
    pub imported: Vec<(String, PathBuf)>,
    /// Tracks the library already held, now in the playlist too.
    pub existing: Vec<String>,
    /// Files that could not be imported, each with the reason.
    pub skipped: Vec<String>,
    /// The place under the target the playlist was put, or would have been:
    /// the insert index rekordbox keeps for every folder of one drop. Pass it
    /// back as `at` for the drop's next folder. `None` when the folder held
    /// no audio, so nothing was looked at.
    pub at: Option<usize>,
}

/// One playlist or folder move, with both positions counted among the
/// destination parent's live children.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistMove {
    id: String,
    before_parent: String,
    before_index: usize,
    after_parent: String,
    after_index: usize,
}

/// One playlist or folder rename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistRename {
    id: String,
    before: String,
    after: String,
}

/// The exact membership rows removed by one playlist edit, plus the complete
/// order needed to put them back in their original places.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlaylistTrackRemoval {
    playlist: String,
    removed: Vec<String>,
    order: Vec<String>,
}

impl PlaylistTrackRemoval {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.removed.is_empty()
    }
}

/// The exact database value changed by one editable track field.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackEdit {
    content: String,
    column: &'static str,
    before: Value,
    after: Value,
}

/// The My Tag set before and after one information-panel edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackTagEdit {
    content: String,
    before: Vec<String>,
    after: Vec<String>,
}

impl TrackTagEdit {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.before == self.after
    }
}

impl TrackEdit {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.before == self.after
    }
}

impl PlaylistDeletion {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.playlist_ids.is_empty()
    }
}

impl Writer {
    /// Opens the library for writing and prepares the backup directory.
    ///
    /// Fails if rekordbox is running, or if this is the real library under
    /// `RBXPORT_TEST` — both enforced by [`Library::open`].
    pub fn open(location: crate::LibraryLocation, backup_dir: impl Into<PathBuf>) -> Result<Self> {
        let library = Library::open(location, OpenMode::ReadWrite)?;
        if let crate::SchemaSupport::Degraded { missing } = &library.schema().support {
            return Err(DbError::WriteRefused(format!(
                "the schema is missing {}; writes are disabled rather than guessed",
                missing.join(", ")
            )));
        }
        Ok(Self {
            library,
            rng: Rng::from_entropy(),
            backup_dir: backup_dir.into(),
            backup_taken: false,
            automatic_backups: true,
        })
    }

    /// Disables the automatic first-write copy. Explicit backups remain available.
    pub fn disable_automatic_backups(&mut self) {
        self.automatic_backups = false;
    }

    /// Tells the writer the session already holds a backup, so this one
    /// takes none. A writer is opened per edit and dropped after it (rekordbox
    /// must be able to take the file back between edits), so "once per
    /// session" is the caller's to keep: without this every rating click
    /// copied the whole library again.
    pub fn mark_backed_up(&mut self) {
        self.backup_taken = true;
    }

    /// Whether a backup has been taken, by this writer or as told to it.
    pub fn backed_up(&self) -> bool {
        self.backup_taken
    }

    pub fn library(&self) -> &Library {
        &self.library
    }

    /// Every unsupported edit, refused with its reason.
    pub fn refuse(action: Unsupported) -> DbError {
        DbError::WriteRefused(action.reason().to_owned())
    }

    // ------------------------------------------------------------- playlists

    /// Creates a playlist and returns its id.
    pub fn create_playlist(&mut self, name: &str, parent: &str) -> Result<String> {
        self.create_node(name, parent, ATTRIBUTE_PLAYLIST)
    }

    /// Creates a folder and returns its id.
    pub fn create_folder(&mut self, name: &str, parent: &str) -> Result<String> {
        self.create_node(name, parent, ATTRIBUTE_FOLDER)
    }

    /// Creates an intelligent playlist with `smart_list`, its rule as
    /// `djmdPlaylist.SmartList` holds one, and returns its id. The rule
    /// names the playlist's own id inside it (`NODE Id`), which is only
    /// known here, so `smart_list` is a function of that id.
    pub fn create_smart_playlist(
        &mut self,
        name: &str,
        parent: &str,
        smart_list: impl FnOnce(&str) -> String,
    ) -> Result<String> {
        self.create_node_with(name, parent, ATTRIBUTE_SMART, Some(smart_list))
    }

    /// Replaces an intelligent playlist's rule.
    pub fn set_smart_list(&mut self, id: &str, smart_list: &str) -> Result<Changed> {
        self.prepare()?;
        let is_smart: bool = self
            .library
            .connection()
            .query_row(
                "SELECT Attribute = ?2 FROM djmdPlaylist WHERE ID = ?1 AND rb_local_deleted = 0",
                params![id, ATTRIBUTE_SMART],
                |r| r.get(0),
            )
            .unwrap_or(false);
        if !is_smart {
            return Err(DbError::WriteRefused(format!("{id} is not an intelligent playlist")));
        }
        self.touch_playlist(id, "SmartList", &Value::Text(smart_list.to_owned()))
    }

    fn create_node(&mut self, name: &str, parent: &str, attribute: i64) -> Result<String> {
        self.create_node_with(name, parent, attribute, None::<fn(&str) -> String>)
    }

    fn create_node_with(
        &mut self,
        name: &str,
        parent: &str,
        attribute: i64,
        smart_list: Option<impl FnOnce(&str) -> String>,
    ) -> Result<String> {
        self.prepare()?;
        let id = self.unused_id("djmdPlaylist")?;
        let uuid = self.rng.uuid4();
        let stamp = time::now();
        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        if parent != ROOT && !node_exists(&tx, parent)? {
            return Err(DbError::WriteRefused(format!("no playlist or folder {parent}")));
        }
        // Seq appends: parents in the reference library start at 0 or 1, so
        // there is no base to assume.
        let seq: i64 = tx.query_row(
            "SELECT COALESCE(MAX(Seq), -1) + 1 FROM djmdPlaylist
             WHERE ParentID = ?1 AND rb_local_deleted = 0",
            params![parent],
            |r| r.get(0),
        )?;
        let usn = next_usn(&tx)?;
        let smart_list: Option<String> = smart_list.map(|rule| rule(&id));
        tx.execute(
            "INSERT INTO djmdPlaylist
                (ID, Seq, Name, ImagePath, Attribute, ParentID, SmartList, UUID,
                 rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced,
                 usn, rb_local_usn, created_at, updated_at)
             VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?9, ?6, 0, 0, 0, 0, NULL, ?7, ?8, ?8)",
            params![id, seq, name, attribute, parent, uuid, usn, stamp, smart_list],
        )?;
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok(id)
    }

    /// Renames a playlist or folder.
    pub fn rename(&mut self, id: &str, name: &str) -> Result<Changed> {
        self.touch_playlist(id, "Name", &Value::Text(name.to_owned()))
    }

    /// Renames a playlist or folder and retains the old value for undo.
    pub fn rename_with_undo(&mut self, id: &str, name: &str) -> Result<(Changed, PlaylistRename)> {
        let before = self.library.connection().query_row(
            "SELECT Name FROM djmdPlaylist WHERE ID = ?1 AND rb_local_deleted = 0",
            params![id],
            |row| row.get::<_, String>(0),
        )?;
        let changed = self.rename(id, name)?;
        Ok((changed, PlaylistRename { id: id.to_owned(), before, after: name.to_owned() }))
    }

    pub fn undo_rename(&mut self, edit: &PlaylistRename) -> Result<Changed> {
        self.rename(&edit.id, &edit.before)
    }

    pub fn redo_rename(&mut self, edit: &PlaylistRename) -> Result<Changed> {
        self.rename(&edit.id, &edit.after)
    }

    /// Moves a playlist or folder under a new parent.
    ///
    /// `index` is the place to take among the parent's children, counted once
    /// the node has been lifted out of wherever it was; `None` appends, which
    /// is where a node with no say in the matter goes.
    ///
    /// The whole sibling run has its `Seq` rewritten rather than the moved
    /// node alone: `Seq` is the order rekordbox reads the tree in, and
    /// inserting between two neighbours has no number to use unless the rest
    /// are renumbered around it.
    pub fn move_to(&mut self, id: &str, parent: &str, index: Option<usize>) -> Result<Changed> {
        self.prepare()?;
        let stamp = time::now();
        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if parent != ROOT && !node_exists(&tx, parent)? {
            return Err(DbError::WriteRefused(format!("no playlist or folder {parent}")));
        }
        if parent == id || is_descendant(&tx, parent, id)? {
            // Reparenting a folder under itself detaches the whole subtree from
            // the tree and it is never seen again.
            return Err(DbError::WriteRefused(
                "that would put a folder inside itself".to_owned(),
            ));
        }
        // The parent's children as they stand, without the one being moved —
        // which may already be one of them, when this is a reorder rather
        // than a reparenting.
        let mut stmt = tx.prepare(
            "SELECT ID FROM djmdPlaylist
             WHERE ParentID = ?1 AND rb_local_deleted = 0 AND ID <> ?2
             ORDER BY Seq, ID",
        )?;
        let mut order: Vec<String> = stmt
            .query_map(params![parent, id], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<_>>()?;
        drop(stmt);
        let at = index.unwrap_or(order.len()).min(order.len());
        order.insert(at, id.to_owned());

        let mut rows = 0;
        let mut usn = 0;
        for (seq, node) in order.iter().enumerate() {
            usn = next_usn(&tx)?;
            let seq = i64::try_from(seq).unwrap_or(i64::MAX);
            rows += tx.execute(
                "UPDATE djmdPlaylist SET ParentID = ?1, Seq = ?2, rb_local_usn = ?3, updated_at = ?4
                 WHERE ID = ?5 AND rb_local_deleted = 0",
                params![parent, seq, usn, stamp, node],
            )?;
        }
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok(Changed { rows, usn })
    }

    /// Moves a playlist or folder and retains both exact tree positions.
    pub fn move_with_undo(
        &mut self,
        id: &str,
        parent: &str,
        index: Option<usize>,
    ) -> Result<(Changed, PlaylistMove)> {
        let (before_parent, before_index) = self.playlist_position(id)?;
        let changed = self.move_to(id, parent, index)?;
        let (after_parent, after_index) = self.playlist_position(id)?;
        Ok((changed, PlaylistMove {
            id: id.to_owned(), before_parent, before_index, after_parent, after_index,
        }))
    }

    pub fn undo_move(&mut self, edit: &PlaylistMove) -> Result<Changed> {
        self.move_to(&edit.id, &edit.before_parent, Some(edit.before_index))
    }

    pub fn redo_move(&mut self, edit: &PlaylistMove) -> Result<Changed> {
        self.move_to(&edit.id, &edit.after_parent, Some(edit.after_index))
    }

    /// The live children of `parent`, in tree order.
    fn child_ids(&self, parent: &str) -> Result<Vec<String>> {
        let mut statement = self.library.connection().prepare(
            "SELECT ID FROM djmdPlaylist WHERE ParentID = ?1 AND rb_local_deleted = 0 ORDER BY Seq, ID",
        )?;
        let ids = statement
            .query_map(params![parent], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(ids)
    }

    fn playlist_position(&self, id: &str) -> Result<(String, usize)> {
        let conn = self.library.connection();
        let parent = conn.query_row(
            "SELECT ParentID FROM djmdPlaylist WHERE ID = ?1 AND rb_local_deleted = 0",
            params![id],
            |row| row.get::<_, String>(0),
        )?;
        let mut statement = conn.prepare(
            "SELECT ID FROM djmdPlaylist WHERE ParentID = ?1 AND rb_local_deleted = 0 ORDER BY Seq, ID",
        )?;
        let siblings: Vec<String> = statement
            .query_map(params![parent], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let index = siblings.iter().position(|sibling| sibling == id)
            .ok_or_else(|| DbError::WriteRefused(format!("no playlist or folder {id}")))?;
        Ok((parent, index))
    }

    /// Soft-deletes a playlist or folder, everything inside it, and every
    /// membership that pointed at it.
    ///
    /// Never `DELETE`: rekordbox's own sync relies on the tombstone.
    pub fn delete_playlist(&mut self, id: &str) -> Result<Changed> {
        self.delete_playlist_with_undo(id).map(|(changed, _)| changed)
    }

    /// Deletes a playlist and returns the exact rows needed to undo it.
    pub fn delete_playlist_with_undo(&mut self, id: &str) -> Result<(Changed, PlaylistDeletion)> {
        self.prepare()?;
        let stamp = time::now();
        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        let exists = tx.query_row(
            "SELECT 1 FROM djmdPlaylist WHERE ID = ?1 AND rb_local_deleted = 0",
            params![id],
            |_| Ok(()),
        ).optional()?.is_some();
        if !exists {
            return Err(DbError::WriteRefused(format!("no playlist or folder {id}")));
        }

        // Collect the subtree first: deleting as we walk would hide children
        // from the walk.
        let mut doomed = vec![id.to_owned()];
        let mut frontier = vec![id.to_owned()];
        while let Some(parent) = frontier.pop() {
            let mut stmt = tx.prepare(
                "SELECT ID FROM djmdPlaylist WHERE ParentID = ?1 AND rb_local_deleted = 0",
            )?;
            let children: Vec<String> = stmt
                .query_map(params![parent], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<_>>()?;
            for child in children {
                doomed.push(child.clone());
                frontier.push(child);
            }
        }

        let mut membership_ids = Vec::new();
        for node in &doomed {
            let mut stmt = tx.prepare(
                "SELECT ID FROM djmdSongPlaylist
                 WHERE PlaylistID = ?1 AND rb_local_deleted = 0",
            )?;
            membership_ids.extend(
                stmt.query_map(params![node], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?,
            );
        }

        let mut rows = 0;
        let mut usn = 0;
        for node in &doomed {
            usn = next_usn(&tx)?;
            rows += tx.execute(
                "UPDATE djmdSongPlaylist SET rb_local_deleted = 1, rb_local_usn = ?1,
                    updated_at = ?2 WHERE PlaylistID = ?3 AND rb_local_deleted = 0",
                params![usn, stamp, node],
            )?;
            usn = next_usn(&tx)?;
            rows += tx.execute(
                "UPDATE djmdPlaylist SET rb_local_deleted = 1, rb_local_usn = ?1,
                    updated_at = ?2 WHERE ID = ?3 AND rb_local_deleted = 0",
                params![usn, stamp, node],
            )?;
        }
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok((Changed { rows, usn }, PlaylistDeletion {
            playlist_ids: doomed,
            membership_ids,
        }))
    }

    /// Restores exactly the rows made into tombstones by a deletion.
    pub fn restore_playlist(&mut self, deletion: &PlaylistDeletion) -> Result<Changed> {
        self.set_playlist_deletion(deletion, false)
    }

    /// Reapplies a deletion after it has been undone.
    pub fn redo_playlist_deletion(&mut self, deletion: &PlaylistDeletion) -> Result<Changed> {
        self.set_playlist_deletion(deletion, true)
    }

    fn set_playlist_deletion(&mut self, deletion: &PlaylistDeletion, deleted: bool) -> Result<Changed> {
        if deletion.is_empty() {
            return Err(DbError::WriteRefused("empty playlist deletion history".to_owned()));
        }
        self.prepare()?;
        let stamp = time::now();
        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let from = i64::from(!deleted);
        let to = i64::from(deleted);
        let mut rows = 0;
        let mut usn = 0;

        // Foreign-key order even though the reference schema does not enforce
        // it: children disappear first and return only after their playlist.
        if deleted {
            for id in &deletion.membership_ids {
                usn = next_usn(&tx)?;
                let changed = tx.execute(
                    "UPDATE djmdSongPlaylist SET rb_local_deleted = ?1, rb_local_usn = ?2,
                        updated_at = ?3 WHERE ID = ?4 AND rb_local_deleted = ?5",
                    params![to, usn, stamp, id, from],
                )?;
                if changed != 1 {
                    return Err(DbError::WriteRefused("that playlist deletion can no longer be redone".to_owned()));
                }
                rows += changed;
            }
            for id in &deletion.playlist_ids {
                usn = next_usn(&tx)?;
                let changed = tx.execute(
                    "UPDATE djmdPlaylist SET rb_local_deleted = ?1, rb_local_usn = ?2,
                        updated_at = ?3 WHERE ID = ?4 AND rb_local_deleted = ?5",
                    params![to, usn, stamp, id, from],
                )?;
                if changed != 1 {
                    return Err(DbError::WriteRefused("that playlist deletion can no longer be redone".to_owned()));
                }
                rows += changed;
            }
        } else {
            for id in &deletion.playlist_ids {
                usn = next_usn(&tx)?;
                let changed = tx.execute(
                    "UPDATE djmdPlaylist SET rb_local_deleted = ?1, rb_local_usn = ?2,
                        updated_at = ?3 WHERE ID = ?4 AND rb_local_deleted = ?5",
                    params![to, usn, stamp, id, from],
                )?;
                if changed != 1 {
                    return Err(DbError::WriteRefused("that playlist deletion can no longer be undone".to_owned()));
                }
                rows += changed;
            }
            for id in &deletion.membership_ids {
                usn = next_usn(&tx)?;
                let changed = tx.execute(
                    "UPDATE djmdSongPlaylist SET rb_local_deleted = ?1, rb_local_usn = ?2,
                        updated_at = ?3 WHERE ID = ?4 AND rb_local_deleted = ?5",
                    params![to, usn, stamp, id, from],
                )?;
                if changed != 1 {
                    return Err(DbError::WriteRefused("that playlist deletion can no longer be undone".to_owned()));
                }
                rows += changed;
            }
        }
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok(Changed { rows, usn })
    }

    // ------------------------------------------------------------ membership

    /// Appends tracks to a playlist, skipping any already in it.
    ///
    /// `TrackNo` stays contiguous from 1, which is true of every playlist in
    /// the reference library.
    pub fn add_tracks(&mut self, playlist: &str, contents: &[String]) -> Result<Changed> {
        self.add_tracks_allowing(playlist, contents, false)
    }

    /// [`Writer::add_tracks`], with the choice of what a track already in the
    /// playlist does: left out when `allow_duplicates` is false, added again
    /// on the end when it is true.
    pub fn add_tracks_allowing(&mut self, playlist: &str, contents: &[String], allow_duplicates: bool) -> Result<Changed> {
        self.prepare()?;
        let stamp = time::now();
        let mut ids: Vec<(String, String)> = Vec::with_capacity(contents.len());
        for _ in contents {
            ids.push((self.rng.uuid4(), self.rng.uuid4()));
        }

        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !node_exists(&tx, playlist)? {
            return Err(DbError::WriteRefused(format!("no playlist {playlist}")));
        }
        refuse_if_smart(&tx, playlist)?;
        let mut track_no: i64 = tx.query_row(
            "SELECT COALESCE(MAX(TrackNo), 0) FROM djmdSongPlaylist
             WHERE PlaylistID = ?1 AND rb_local_deleted = 0",
            params![playlist],
            |r| r.get(0),
        )?;

        // The playlist's members, read once: a lookup per dropped track scans
        // the whole membership table, which takes minutes for a few hundred.
        let mut members: std::collections::HashSet<String> = {
            let mut stmt = tx.prepare(
                "SELECT ContentID FROM djmdSongPlaylist
                 WHERE PlaylistID = ?1 AND rb_local_deleted = 0",
            )?;
            let found = stmt
                .query_map(params![playlist], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<_>>()?;
            found
        };

        let mut rows = 0;
        let mut usn = 0;
        for (content, (row_id, uuid)) in contents.iter().zip(ids) {
            if !members.insert(content.clone()) && !allow_duplicates {
                continue;
            }
            track_no += 1;
            // `next_usn` scans every sync table, so it is read once and
            // counted up from there: this loop is the only writer.
            usn = if rows == 0 { next_usn(&tx)? } else { usn + 1 };
            rows += tx.execute(
                "INSERT INTO djmdSongPlaylist
                    (ID, PlaylistID, ContentID, TrackNo, UUID,
                     rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced,
                     usn, rb_local_usn, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, 0, 0, NULL, ?6, ?7, ?7)",
                params![row_id, playlist, content, track_no, uuid, usn, stamp],
            )?;
        }
        if rows > 0 {
            set_counter(&tx, usn)?;
        }
        tx.commit()?;
        Ok(Changed { rows, usn })
    }

    /// The live playlist, folder or intelligent playlist under `parent` named
    /// exactly `name`, if there is one.
    pub fn child_named(&self, parent: &str, name: &str) -> Result<Option<String>> {
        Ok(self.library.connection().query_row(
            "SELECT ID FROM djmdPlaylist
             WHERE ParentID = ?1 AND Name = ?2 AND rb_local_deleted = 0
             ORDER BY Seq, ID LIMIT 1",
            params![parent, name],
            |r| r.get::<_, String>(0),
        ).optional()?)
    }

    /// A folder from disk dropped onto the playlist tree: one playlist named
    /// after it, holding every audio file found under it.
    ///
    /// What rekordbox 7.2.19 does for a folder dropped onto the Playlists root
    /// or a playlist folder (`TreeViewer::treeMessageImportExternalFoldersToList`)
    /// [OBS, static]: nothing at all when the folder holds no file it plays;
    /// otherwise a playlist under the drop target named after the folder, its
    /// subfolders flattened into it rather than made into playlists of their
    /// own, with new files imported and files already in the library reused.
    ///
    /// `files` is the folder's contents in the order they belong in the
    /// playlist (see [`crate::import::audio_files_in`]).
    ///
    /// A sibling with the same name is the one question rekordbox asks
    /// ("One or several lists with the same name already exist. Do you want
    /// to replace them with the one you're importing?",
    /// `TreeViewer::showReplaceListAlert`). Nothing is written until it is
    /// answered: the clash comes back in [`FolderPlaylist::conflict`], and the
    /// caller calls again with `replace` set to that id to replace it.
    ///
    /// `at` is where the playlist goes among `parent`'s children; `None`
    /// means the end, as for a drop onto the middle of a folder row. Every
    /// folder of one drop goes to the same index, so pass each call the
    /// [`FolderPlaylist::at`] the previous one returned. rekordbox does the
    /// same [OBS rekordbox 7.2.19 static]: `treeMessageImportExternalFoldersToList`
    /// (0x1015677ec) reads the drop's insert index once and passes it to
    /// `createTargetList` for every folder; `rekordboxDBController::createNewList`
    /// (0x1017e6808) appends with `insertPlaylist` (seq one past
    /// `getPlaylistFolderSeqMax`) and then `movePlaylist`s the new list to that
    /// index whenever its seq is not below it. So a later folder lands before an earlier one: two
    /// folders `A`, `B` end up `B`, `A`. Replacing a clash that sat before the
    /// index takes one off it (`checkSameNameList` 0x10155d5a8, @0x10155d748..0x10155d75c),
    /// and that lower index holds for the rest of the drop.
    pub fn import_folder_as_playlist(
        &mut self,
        name: &str,
        parent: &str,
        files: &[PathBuf],
        replace: Option<&str>,
        at: Option<usize>,
    ) -> Result<FolderPlaylist> {
        let mut outcome = FolderPlaylist::default();
        if files.is_empty() {
            outcome.at = at;
            return Ok(outcome);
        }
        let mut at = match at {
            Some(at) => at,
            None => self.child_ids(parent)?.len(),
        };
        if let Some(clash) = self.child_named(parent, name)? {
            if replace != Some(clash.as_str()) {
                outcome.conflict = Some(clash);
                outcome.at = Some(at);
                return Ok(outcome);
            }
            let (_, place) = self.playlist_position(&clash)?;
            self.delete_playlist(&clash)?;
            if place < at {
                at -= 1;
            }
        }
        outcome.at = Some(at);
        let playlist = self.create_playlist(name, parent)?;
        if self.child_ids(parent)?.iter().position(|id| *id == playlist) != Some(at) {
            self.move_to(&playlist, parent, Some(at))?;
        }
        let mut members = Vec::with_capacity(files.len());
        for (file, imported) in files.iter().zip(self.import_files(files)?) {
            match imported {
                ImportOutcome::Existing(id) => {
                    outcome.existing.push(id.clone());
                    members.push(id);
                }
                ImportOutcome::Added(id) => {
                    outcome.imported.push((id.clone(), file.clone()));
                    members.push(id);
                }
                ImportOutcome::Refused(reason) => {
                    outcome.skipped.push(format!("{}: {reason}", file.display()));
                }
            }
        }
        if !members.is_empty() {
            self.add_tracks(&playlist, &members)?;
        }
        outcome.playlist = Some(playlist);
        Ok(outcome)
    }

    /// Makes a playlist hold exactly `contents`, in that order, the first of a
    /// repeated track counting: the current members are soft-deleted and the
    /// new ones written with `TrackNo` 1..N, all in one transaction, so a
    /// refusal or error partway leaves the old members and order as they were.
    /// Writes nothing when the playlist already holds exactly these. Refuses an
    /// intelligent playlist, a folder, and a track not in the library.
    /// `rows` is the membership rows written for the new members.
    pub fn set_tracks(&mut self, playlist: &str, contents: &[String]) -> Result<Changed> {
        let mut seen = std::collections::HashSet::new();
        let wanted: Vec<&String> = contents.iter().filter(|c| seen.insert(c.as_str())).collect();
        self.prepare()?;
        let stamp = time::now();
        let mut ids: Vec<(String, String)> = Vec::with_capacity(wanted.len());
        for _ in &wanted {
            ids.push((self.rng.uuid4(), self.rng.uuid4()));
        }

        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let attribute: Option<i64> = tx
            .query_row(
                "SELECT Attribute FROM djmdPlaylist WHERE ID = ?1 AND rb_local_deleted = 0",
                params![playlist],
                |r| r.get(0),
            )
            .optional()?;
        match attribute {
            None => return Err(DbError::WriteRefused(format!("no playlist {playlist}"))),
            Some(ATTRIBUTE_FOLDER) => {
                return Err(DbError::WriteRefused(format!("{playlist} is a folder, not a playlist")))
            }
            Some(_) => refuse_if_smart(&tx, playlist)?,
        }
        let current: Vec<(String, String)> = {
            let mut stmt = tx.prepare(
                "SELECT ID, ContentID FROM djmdSongPlaylist
                 WHERE PlaylistID = ?1 AND rb_local_deleted = 0 ORDER BY TrackNo, ID",
            )?;
            let rows = stmt.query_map(params![playlist], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        if current.len() == wanted.len()
            && current.iter().zip(&wanted).all(|((_, have), want)| have == *want)
        {
            return Ok(Changed { rows: 0, usn: 0 });
        }

        let mut usn = 0;
        for (row_id, _) in &current {
            usn = next_usn(&tx)?;
            tx.execute(
                "UPDATE djmdSongPlaylist SET rb_local_deleted = 1, rb_local_usn = ?1,
                    updated_at = ?2 WHERE ID = ?3",
                params![usn, stamp, row_id],
            )?;
        }
        let mut rows = 0;
        for ((track_no, content), (row_id, uuid)) in (1_i64..).zip(&wanted).zip(ids) {
            // Checked here, after the old rows are gone, so a refusal is the
            // transaction rolling back, never a half-replaced playlist.
            if !content_exists(&tx, content)? {
                return Err(DbError::WriteRefused(format!("no track {content}")));
            }
            usn = next_usn(&tx)?;
            rows += tx.execute(
                "INSERT INTO djmdSongPlaylist
                    (ID, PlaylistID, ContentID, TrackNo, UUID,
                     rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced,
                     usn, rb_local_usn, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, 0, 0, NULL, ?6, ?7, ?7)",
                params![row_id, playlist, content, track_no, uuid, usn, stamp],
            )?;
        }
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok(Changed { rows, usn })
    }

    /// Removes tracks from a playlist and closes the gaps in `TrackNo`.
    pub fn remove_tracks(&mut self, playlist: &str, contents: &[String]) -> Result<Changed> {
        self.prepare()?;
        let stamp = time::now();
        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        refuse_if_smart(&tx, playlist)?;

        let mut rows = 0;
        let mut usn = 0;
        for content in contents {
            usn = next_usn(&tx)?;
            rows += tx.execute(
                "UPDATE djmdSongPlaylist SET rb_local_deleted = 1, rb_local_usn = ?1,
                    updated_at = ?2
                 WHERE PlaylistID = ?3 AND ContentID = ?4 AND rb_local_deleted = 0",
                params![usn, stamp, playlist, content],
            )?;
        }
        if rows > 0 {
            usn = renumber(&tx, playlist, &stamp)?;
            set_counter(&tx, usn)?;
        }
        tx.commit()?;
        Ok(Changed { rows, usn })
    }

    /// Removes tracks while retaining the exact membership rows and ordering
    /// needed to undo the operation without manufacturing replacement rows.
    pub fn remove_tracks_with_undo(
        &mut self,
        playlist: &str,
        contents: &[String],
    ) -> Result<(Changed, PlaylistTrackRemoval)> {
        let conn = self.library.connection();
        let mut statement = conn.prepare(
            "SELECT ID, ContentID FROM djmdSongPlaylist
             WHERE PlaylistID = ?1 AND rb_local_deleted = 0 ORDER BY TrackNo, ID",
        )?;
        let memberships: Vec<(String, String)> = statement
            .query_map(params![playlist], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        drop(statement);
        let order = memberships.iter().map(|(id, _)| id.clone()).collect();
        let removed = memberships.into_iter()
            .filter_map(|(id, content)| contents.contains(&content).then_some(id))
            .collect();
        let edit = PlaylistTrackRemoval { playlist: playlist.to_owned(), removed, order };
        let changed = self.remove_tracks(playlist, contents)?;
        Ok((changed, edit))
    }

    pub fn undo_track_removal(&mut self, edit: &PlaylistTrackRemoval) -> Result<Changed> {
        self.set_track_removal(edit, false)
    }

    pub fn redo_track_removal(&mut self, edit: &PlaylistTrackRemoval) -> Result<Changed> {
        self.set_track_removal(edit, true)
    }

    fn set_track_removal(&mut self, edit: &PlaylistTrackRemoval, deleted: bool) -> Result<Changed> {
        if edit.is_empty() {
            return Err(DbError::WriteRefused("empty playlist track-removal history".to_owned()));
        }
        self.prepare()?;
        let stamp = time::now();
        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut rows = 0;
        let mut usn = 0;
        for membership in &edit.removed {
            usn = next_usn(&tx)?;
            rows += tx.execute(
                "UPDATE djmdSongPlaylist SET rb_local_deleted = ?1, rb_local_usn = ?2,
                    updated_at = ?3 WHERE ID = ?4 AND rb_local_deleted = ?5",
                params![i64::from(deleted), usn, stamp, membership, i64::from(!deleted)],
            )?;
        }
        if deleted {
            usn = renumber(&tx, &edit.playlist, &stamp)?;
        } else {
            for (index, membership) in edit.order.iter().enumerate() {
                usn = next_usn(&tx)?;
                rows += tx.execute(
                    "UPDATE djmdSongPlaylist SET TrackNo = ?1, rb_local_usn = ?2, updated_at = ?3
                     WHERE ID = ?4 AND rb_local_deleted = 0",
                    params![i64::try_from(index + 1).unwrap_or(i64::MAX), usn, stamp, membership],
                )?;
            }
        }
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok(Changed { rows, usn })
    }

    // ---------------------------------------------------------------- tag list

    /// Adds tracks to the Tag List, rekordbox's one temporary list, on the
    /// end in the order given; a track already on it stays where it is.
    ///
    /// Rows in the shape rekordbox 7.2.11 wrote its own [OBS: 45 rows of
    /// `djmdSongTagList` in the reference library, 2026-09-13]: an id and a
    /// UUID of their own, `TrackNo` running from 1, and both `usn` and
    /// `rb_local_usn` NULL — the update counter is not moved for these,
    /// since rekordbox did not.
    pub fn tag_list_add(&mut self, contents: &[String]) -> Result<Changed> {
        self.prepare()?;
        let stamp = time::now();
        let mut ids: Vec<(String, String)> = Vec::with_capacity(contents.len());
        for _ in contents {
            ids.push((self.rng.uuid4(), self.rng.uuid4()));
        }
        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut track_no: i64 = tx.query_row(
            "SELECT COALESCE(MAX(TrackNo), 0) FROM djmdSongTagList WHERE rb_local_deleted = 0",
            [],
            |r| r.get(0),
        )?;
        let mut rows = 0;
        for (content, (row_id, uuid)) in contents.iter().zip(ids) {
            if !content_exists(&tx, content)? {
                return Err(DbError::WriteRefused(format!("no track {content}")));
            }
            let already: i64 = tx.query_row(
                "SELECT COUNT(*) FROM djmdSongTagList WHERE ContentID = ?1 AND rb_local_deleted = 0",
                params![content],
                |r| r.get(0),
            )?;
            if already > 0 {
                continue;
            }
            track_no += 1;
            rows += tx.execute(
                "INSERT INTO djmdSongTagList
                    (ID, ContentID, TrackNo, UUID,
                     rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced,
                     usn, rb_local_usn, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 0, 0, 0, 0, NULL, NULL, ?5, ?5)",
                params![row_id, content, track_no, uuid, stamp],
            )?;
        }
        tx.commit()?;
        Ok(Changed { rows, usn: 0 })
    }

    /// Takes tracks off the Tag List and closes the gaps in `TrackNo`.
    /// Soft-deleted, as every other membership row is [ASSUME: no capture
    /// of rekordbox taking one off].
    pub fn tag_list_remove(&mut self, contents: &[String]) -> Result<Changed> {
        self.prepare()?;
        let stamp = time::now();
        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut rows = 0;
        for content in contents {
            rows += tx.execute(
                "UPDATE djmdSongTagList SET rb_local_deleted = 1, updated_at = ?1
                 WHERE ContentID = ?2 AND rb_local_deleted = 0",
                params![stamp, content],
            )?;
        }
        if rows > 0 {
            let mut stmt = tx.prepare(
                "SELECT ID FROM djmdSongTagList WHERE rb_local_deleted = 0 ORDER BY TrackNo, created_at",
            )?;
            let remaining: Vec<String> =
                stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<_>>()?;
            drop(stmt);
            for (position, id) in remaining.iter().enumerate() {
                let track_no = i64::try_from(position).unwrap_or(0) + 1;
                tx.execute(
                    "UPDATE djmdSongTagList SET TrackNo = ?1, updated_at = ?2 WHERE ID = ?3 AND TrackNo != ?1",
                    params![track_no, stamp, id],
                )?;
            }
        }
        tx.commit()?;
        Ok(Changed { rows, usn: 0 })
    }

    /// Empties the Tag List.
    pub fn tag_list_clear(&mut self) -> Result<Changed> {
        self.prepare()?;
        let stamp = time::now();
        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let rows = tx.execute(
            "UPDATE djmdSongTagList SET rb_local_deleted = 1, updated_at = ?1 WHERE rb_local_deleted = 0",
            params![stamp],
        )?;
        tx.commit()?;
        Ok(Changed { rows, usn: 0 })
    }

    /// Reorders a playlist to exactly this sequence of tracks.
    ///
    /// Anything in the playlist and not in `order` keeps its place after them,
    /// so a partial order cannot silently drop tracks.
    pub fn reorder(&mut self, playlist: &str, order: &[String]) -> Result<Changed> {
        self.prepare()?;
        let stamp = time::now();
        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        refuse_if_smart(&tx, playlist)?;

        let mut stmt = tx.prepare(
            "SELECT ContentID FROM djmdSongPlaylist
             WHERE PlaylistID = ?1 AND rb_local_deleted = 0 ORDER BY TrackNo",
        )?;
        let existing: Vec<String> = stmt
            .query_map(params![playlist], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<_>>()?;
        drop(stmt);

        let mut sequence: Vec<String> =
            order.iter().filter(|c| existing.contains(c)).cloned().collect();
        for content in &existing {
            if !sequence.contains(content) {
                sequence.push(content.clone());
            }
        }

        let mut rows = 0;
        let mut usn = 0;
        for (index, content) in sequence.iter().enumerate() {
            usn = next_usn(&tx)?;
            rows += tx.execute(
                "UPDATE djmdSongPlaylist SET TrackNo = ?1, rb_local_usn = ?2, updated_at = ?3
                 WHERE PlaylistID = ?4 AND ContentID = ?5 AND rb_local_deleted = 0",
                params![i64::try_from(index + 1).unwrap_or(i64::MAX), usn, stamp, playlist, content],
            )?;
        }
        if rows > 0 {
            set_counter(&tx, usn)?;
        }
        tx.commit()?;
        Ok(Changed { rows, usn })
    }

    // ---------------------------------------------------------------- import

    /// The id of the live track already imported from `path`, if any.
    ///
    /// Read-only. It lets a caller that was handed a file that is already in
    /// the library (a drop onto a playlist) use the existing row rather than
    /// treat the file as unimportable. The path is cleaned the same way
    /// [`Self::import_file`] cleans it, so the two agree on what "already
    /// there" means.
    pub fn track_id_at(&self, path: &Path) -> Result<Option<String>> {
        let path = normalized(path);
        let folder = path.to_string_lossy().into_owned();
        let id = self
            .library
            .connection()
            .query_row(
                "SELECT ID FROM djmdContent WHERE FolderPath = ?1 AND rb_local_deleted = 0 ORDER BY ID LIMIT 1",
                params![folder],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        Ok(id)
    }

    /// Adds a file to the library, returning the new track's id.
    ///
    /// The row shape is the one the reference library shows for a track made
    /// on this machine: `rb_data_status` 0 on all 634 of them, `usn` NULL
    /// until the sync assigns one.
    ///
    /// **`Analysed` is left NULL**, which is the column's own default. Every
    /// track in the reference library has been analysed, so it cannot show
    /// what the field holds *before* analysis — and NULL asserts nothing
    /// rather than asserting something unverified. rekordbox sets it when it
    /// analyses the track.
    pub fn import_file(&mut self, path: &Path) -> Result<String> {
        // Lexically clean: rekordbox marks a row whose path holds `..` as
        // missing, and a caller building a path from a manifest directory
        // hands one in.
        let path = normalized(path);
        let path = path.as_path();
        let tags = crate::import::read_tags(path)
            .map_err(|e| DbError::WriteRefused(e.to_string()))?;
        self.prepare()?;

        let id = self.unused_id_below("djmdContent", MAX_CONTENT_ID)?;
        let uuid = self.rng.uuid4();
        let stamp = time::now();
        let today = stamp.get(..10).unwrap_or_default().to_owned();
        let folder = path.to_string_lossy().into_owned();
        // The library's own device: `djmdProperty` names it, and every one of
        // the 645 rows rekordbox 7 imported on this machine carries the pair.
        let (master_db, device): (Option<String>, Option<String>) = self
            .library
            .connection()
            .query_row("SELECT DBID, DeviceID FROM djmdProperty LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap_or((None, None));
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        // Already in the library: importing again would give one file two
        // rows, and every playlist pointing at the wrong one.
        let existing: i64 = tx.query_row(
            "SELECT COUNT(*) FROM djmdContent WHERE FolderPath = ?1 AND rb_local_deleted = 0",
            params![folder],
            |r| r.get(0),
        )?;
        if existing > 0 {
            return Err(DbError::WriteRefused(format!(
                "{} is already in the library",
                path.display()
            )));
        }

        let artist = intern(&tx, "djmdArtist", "Name", &tags.artist, &mut self.rng, &stamp)?;
        let album = intern(&tx, "djmdAlbum", "Name", &tags.album, &mut self.rng, &stamp)?;
        let genre = intern(&tx, "djmdGenre", "Name", &tags.genre, &mut self.rng, &stamp)?;
        let label = intern(&tx, "djmdLabel", "Name", &tags.label, &mut self.rng, &stamp)?;
        let key = tag_key_id(&tx, &tags.key, &mut self.rng, &stamp)?;

        let usn = next_usn(&tx)?;
        insert_content(
            &tx,
            &NewContent {
                id: &id, uuid: &uuid, folder: &folder, file_name: &file_name, path, tags: &tags,
                artist: &artist, album: &album, genre: &genre, label: &label, key: &key,
                today: &today, stamp: &stamp, master_db: &master_db, device: &device, usn,
            },
        )?;
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok(id)
    }

    /// Imports many files in one transaction, one outcome per path, in order.
    ///
    /// Each file ends up as [`Writer::import_file`] would leave it, with the
    /// same checks (a path the library holds is `Existing`, an unreadable or
    /// non-audio file is `Refused`), but the work around the rows is shared:
    /// tags are read on several threads before the write starts, the paths
    /// the library holds are read once, each artist, album, genre, label and
    /// key name is looked up once however many files carry it, and the USN
    /// is counted up from one read instead of found again for every row.
    /// One commit covers the batch. A database error, unlike a refused file,
    /// stops the batch and nothing in it is kept.
    pub fn import_files(&mut self, paths: &[PathBuf]) -> Result<Vec<ImportOutcome>> {
        use std::collections::HashMap;

        let paths: Vec<PathBuf> = paths.iter().map(|p| normalized(p)).collect();
        let tags = read_tags_parallel(&paths);
        self.prepare()?;
        let stamp = time::now();
        let today = stamp.get(..10).unwrap_or_default().to_owned();

        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (master_db, device): (Option<String>, Option<String>) = tx
            .query_row("SELECT DBID, DeviceID FROM djmdProperty LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap_or((None, None));

        // Where the library already holds each path, the lowest id first as
        // `track_id_at` answers it. One pass over the table, kept for the
        // paths of this batch only.
        let wanted: std::collections::HashSet<String> =
            paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        let mut held: HashMap<String, String> = HashMap::new();
        {
            let mut stmt = tx.prepare(
                "SELECT FolderPath, ID FROM djmdContent WHERE rb_local_deleted = 0 ORDER BY ID",
            )?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let folder: Option<String> = row.get(0)?;
                if let Some(folder) = folder.filter(|f| wanted.contains(f)) {
                    held.entry(folder).or_insert(row.get::<_, String>(1)?);
                }
            }
        }

        let mut names: HashMap<(&'static str, String), Option<String>> = HashMap::new();
        let mut next_usn_value: Option<i64> = None;
        let mut last_usn: Option<i64> = None;
        let mut outcomes = Vec::with_capacity(paths.len());

        for (path, tags) in paths.iter().zip(tags) {
            let folder = path.to_string_lossy().into_owned();
            if let Some(id) = held.get(&folder) {
                outcomes.push(ImportOutcome::Existing(id.clone()));
                continue;
            }
            let tags = match tags {
                Ok(tags) => tags,
                Err(e) => {
                    outcomes.push(ImportOutcome::Refused(e.to_string()));
                    continue;
                }
            };

            let mut name_id = |table: &'static str, column: &str, name: &str, rng: &mut Rng| -> Result<Option<String>> {
                if let Some(id) = names.get(&(table, name.to_owned())) {
                    return Ok(id.clone());
                }
                let id = if table == "djmdKey" {
                    tag_key_id(&tx, name, rng, &stamp)?
                } else {
                    intern(&tx, table, column, name, rng, &stamp)?
                };
                names.insert((table, name.to_owned()), id.clone());
                Ok(id)
            };
            let artist = name_id("djmdArtist", "Name", &tags.artist, &mut self.rng)?;
            let album = name_id("djmdAlbum", "Name", &tags.album, &mut self.rng)?;
            let genre = name_id("djmdGenre", "Name", &tags.genre, &mut self.rng)?;
            let label = name_id("djmdLabel", "Name", &tags.label, &mut self.rng)?;
            let key = name_id("djmdKey", "ScaleName", &tags.key, &mut self.rng)?;

            let id = unused_id_in(&tx, &mut self.rng, "djmdContent", MAX_CONTENT_ID)?;
            let uuid = self.rng.uuid4();
            // Making a lookup row above moves the registry's counter, so the
            // next USN is whichever is larger: what this batch has counted to,
            // or what the registry now says.
            let usn = match next_usn_value {
                None => next_usn(&tx)?,
                Some(counted) => counted.max(registry_counter(&tx)?.saturating_add(1)),
            };
            let file_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            insert_content(
                &tx,
                &NewContent {
                    id: &id, uuid: &uuid, folder: &folder, file_name: &file_name, path, tags: &tags,
                    artist: &artist, album: &album, genre: &genre, label: &label, key: &key,
                    today: &today, stamp: &stamp, master_db: &master_db, device: &device, usn,
                },
            )?;
            next_usn_value = Some(usn.checked_add(1).ok_or_else(|| {
                DbError::WriteRefused("local update counter overflow".into())
            })?);
            last_usn = Some(usn);
            held.insert(folder, id.clone());
            outcomes.push(ImportOutcome::Added(id));
        }
        if let Some(usn) = last_usn {
            set_counter(&tx, usn)?;
        }
        tx.commit()?;
        Ok(outcomes)
    }

    // -------------------------------------------------------------- analysis

    /// Where a track's analysis files belong.
    ///
    /// rekordbox derives the path from the row's `UUID`:
    /// `/PIONEER/USBANLZ/<first three>/<rest>/ANLZ0000.DAT`, on 38,674 of
    /// the reference library's 38,681 rows [OBS]; the counter rises on each
    /// re-analysis. A row that already has a path keeps it, so files are
    /// replaced in place rather than left behind.
    pub fn analysis_data_path_for(&self, content: &str) -> Result<String> {
        analysis_data_path(self.library.connection(), content)
    }

    /// Registers an analysis on a track: BPM, key, the analysis path, and the
    /// `Analysed` value rekordbox sets on a track it has analysed itself.
    ///
    /// The files must already be at the path — a row is never pointed at
    /// nothing. Every value is one rekordbox writes, read off the reference
    /// library (see the module docs): `Analysed` is [`ANALYSED_FULL`], and
    /// the `KeyID` is the `djmdKey` row rekordbox uses for that name — two
    /// rows share some names, and the one on thousands of tracks is taken
    /// over the one on a dozen. A key name no `djmdKey` row carries is
    /// refused rather than invented. `AnalysisUpdated` is left alone.
    pub fn register_analysis(
        &mut self,
        content: &str,
        analysis: &AnalysisRegistration<'_>,
    ) -> Result<Changed> {
        self.prepare()?;
        let stamp = time::now();
        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        let key_id = match analysis.key {
            Some(name) if !name.is_empty() => Some(key_id_for(&tx, name)?),
            _ => None,
        };
        let usn = next_usn(&tx)?;
        let rows = tx.execute(
            "UPDATE djmdContent
             SET BPM = ?1, KeyID = COALESCE(?2, KeyID), AnalysisDataPath = ?3, Analysed = ?4,
                 rb_local_usn = ?5, updated_at = ?6
             WHERE ID = ?7 AND rb_local_deleted = 0",
            params![
                i64::from(analysis.bpm_x100),
                key_id,
                analysis.analysis_data_path,
                ANALYSED_FULL,
                usn,
                stamp,
                content
            ],
        )?;
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok(Changed { rows, usn })
    }

    /// Finds an unused id below a ceiling, for tables whose ids are smaller.
    fn unused_id_below(&mut self, table: &str, limit: u64) -> Result<String> {
        unused_id_in(self.library.connection(), &mut self.rng, table, limit)
    }

    // ------------------------------------------------------------------ cues

    /// Replaces an existing Hot Cue Bank slot with the value received from a
    /// player.  A bank has fixed membership in rekordbox, so this refuses to
    /// invent a row where the selected slot does not already exist.
    pub fn set_hot_cue_bank_cue(&mut self, bank: &str, cue: &crate::details::HotCueBankCue) -> Result<Changed> {
        if !(1..=3).contains(&cue.slot) || cue.content == 0 || cue.out_ms.is_some_and(|out| out < cue.in_ms) {
            return Err(DbError::WriteRefused("invalid Hot Cue Bank cue".into()));
        }
        self.prepare()?;
        let stamp = time::now();
        let tx = self.library.connection_mut().transaction_with_behavior(TransactionBehavior::Immediate)?;
        let usn = next_usn(&tx)?;
        let rows = tx.execute(
            "UPDATE djmdSongHotCueBanklist
             SET ContentID=?1, InMsec=?2, OutMsec=?3, Color=?4, ColorTableIndex=?5,
                 ActiveLoop=?6, BeatLoopSize=?7, CueMicrosec=?8,
                 rb_local_usn=?9, updated_at=?10
             WHERE HotCueBanklistID=?11 AND TrackNo=?12 AND rb_local_deleted=0",
            params![
                cue.content.to_string(), i64::from(cue.in_ms), cue.out_ms.map(i64::from),
                i64::from(cue.color), i64::from(cue.color_table_index), i64::from(u8::from(cue.active_loop)),
                i64::from(cue.beat_loop_size), i64::from(cue.cue_microsec), usn, stamp, bank, i64::from(cue.slot),
            ],
        )?;
        if rows != 1 {
            return Err(DbError::WriteRefused("Hot Cue Bank slot does not exist".into()));
        }
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok(Changed { rows, usn })
    }

    /// Adds a cue to a track.
    ///
    /// `kind` is rekordbox's own: 0 for a memory cue, 1-3 and 5 for hot cues
    /// A to D, 6-9 for E to H, and 10-17 for I to P. Kind 4 is unused.
    ///
    /// Every column is set to what the reference library shows for a plain,
    /// default-coloured cue — see the module docs. Loops are refused.
    pub fn add_cue(&mut self, content: &str, kind: u8, position_ms: u32) -> Result<String> {
        if kind == 4 || kind > 17 {
            return Err(DbError::WriteRefused(format!(
                "{kind} is not a cue kind rekordbox uses"
            )));
        }
        self.prepare()?;
        // A decimal id like rekordbox's own, not a UUID: every one of the
        // library's 1,041,056 cue ids is a number under 2^32, and the index
        // holds them as such.
        let id = self.unused_id_below("djmdCue", MAX_CUE_ID)?;
        let uuid = self.rng.uuid4();
        let stamp = time::now();
        let memory = kind == 0;

        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        // A cue points at a track by id *and* by UUID; both have to match or
        // rekordbox's sync sees a cue with no owner.
        let content_uuid: Option<String> = tx
            .query_row(
                "SELECT UUID FROM djmdContent WHERE ID = ?1 AND rb_local_deleted = 0",
                params![content],
                |r| r.get(0),
            )
            .ok();
        if content_uuid.is_none() {
            return Err(DbError::WriteRefused(format!("no track {content}")));
        }

        let usn = next_usn(&tx)?;
        tx.execute(
            "INSERT INTO djmdCue
                (ID, ContentID, InMsec, InFrame, InMpegFrame, InMpegAbs,
                 OutMsec, OutFrame, OutMpegFrame, OutMpegAbs,
                 Kind, Color, ColorTableIndex, ActiveLoop, Comment, BeatLoopSize,
                 CueMicrosec, ContentUUID, UUID,
                 rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced,
                 usn, rb_local_usn, created_at, updated_at)
             VALUES (?1, ?2, ?3, 0, 0, 0, NULL, NULL, NULL, NULL,
                     ?4, ?5, ?6, 0, '', NULL,
                     NULL, ?7, ?8, 0, 0, 0, 0, NULL, ?9, ?10, ?10)",
            params![
                id,
                content,
                i64::from(position_ms),
                i64::from(kind),
                // 255 on a memory cue, -1 on a hot one.
                if memory { 255 } else { -1 },
                // 0 is "no colour"; 21 is the default rekordbox writes when
                // the user has not chosen one.
                if memory { 0 } else { 21 },
                content_uuid,
                uuid,
                usn,
                stamp
            ],
        )?;
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok(id)
    }

    /// Changes a cue's colour. Memory cues use rekordbox's eight-value
    /// `Color` field (255 means no colour); hot cues use `ColorTableIndex`.
    pub fn set_cue_colour(&mut self, cue: &str, colour: Option<u8>) -> Result<Changed> {
        self.prepare()?;
        let tx = self.library.connection_mut().transaction_with_behavior(TransactionBehavior::Immediate)?;
        let kind: i64 = tx.query_row(
            "SELECT Kind FROM djmdCue WHERE ID=?1 AND rb_local_deleted=0", [cue], |row| row.get(0),
        )?;
        let maximum = if kind == 0 { 7 } else { 64 };
        if colour.is_some_and(|value| value > maximum) {
            return Err(DbError::WriteRefused("cue colour is outside rekordbox's palette".into()));
        }
        let usn = next_usn(&tx)?;
        let stamp = time::now();
        let rows = if kind == 0 {
            let value = colour.map_or(255, i64::from);
            tx.execute("UPDATE djmdCue SET Color=?2, rb_local_usn=?3, updated_at=?4 WHERE ID=?1 AND rb_local_deleted=0", params![cue, value, usn, stamp])?
        } else {
            let value = colour.map_or(21, i64::from);
            tx.execute("UPDATE djmdCue SET ColorTableIndex=?2, rb_local_usn=?3, updated_at=?4 WHERE ID=?1 AND rb_local_deleted=0", params![cue, value, usn, stamp])?
        };
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok(Changed { rows, usn })
    }

    /// Sets a cue's name; an empty name leaves it unnamed.
    pub fn set_cue_comment(&mut self, cue: &str, comment: &str) -> Result<Changed> {
        self.prepare()?;
        let stamp = time::now();
        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let usn = next_usn(&tx)?;
        let rows = tx.execute(
            "UPDATE djmdCue SET Comment = ?1, rb_local_usn = ?2, updated_at = ?3
             WHERE ID = ?4 AND rb_local_deleted = 0",
            params![comment, usn, stamp, cue],
        )?;
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok(Changed { rows, usn })
    }

    /// Adds a loop: a cue with an end as well as a start.
    ///
    /// `beats` is the loop's length in beats, which `BeatLoopSize` carries as
    /// `(beats << 16) | 1`. Passing 0 leaves the length implied by the In and
    /// Out points, which is what most of the library's loops do.
    pub fn add_loop(
        &mut self,
        content: &str,
        kind: u8,
        start_ms: u32,
        end_ms: u32,
        beats: u16,
    ) -> Result<String> {
        if end_ms <= start_ms {
            return Err(DbError::WriteRefused(
                "a loop has to end after it starts".to_owned(),
            ));
        }
        let id = self.add_cue(content, kind, start_ms)?;
        let stamp = time::now();
        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let usn = next_usn(&tx)?;
        // The low half is always 1 across every value in the reference
        // library; it reads as the denominator of a beats-per-loop fraction.
        let size = if beats == 0 { 0_i64 } else { (i64::from(beats) << 16) | 1 };
        tx.execute(
            "UPDATE djmdCue SET OutMsec = ?1, BeatLoopSize = ?2, rb_local_usn = ?3,
                updated_at = ?4 WHERE ID = ?5",
            params![i64::from(end_ms), size, usn, stamp, id],
        )?;
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok(id)
    }

    /// Moves a cue to a new position.
    pub fn move_cue(&mut self, cue: &str, position_ms: u32) -> Result<Changed> {
        self.prepare()?;
        let stamp = time::now();
        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let usn = next_usn(&tx)?;
        let rows = tx.execute(
            "UPDATE djmdCue SET InMsec = ?1, rb_local_usn = ?2, updated_at = ?3
             WHERE ID = ?4 AND rb_local_deleted = 0",
            params![i64::from(position_ms), usn, stamp, cue],
        )?;
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok(Changed { rows, usn })
    }

    /// Moves a hot cue to another pad slot, shifting the cues between the two
    /// slots by one to make room, as dragging a row in a list does.
    ///
    /// `kind` is the slot's `djmdCue.Kind`, as for [`Writer::add_cue`]. A
    /// slot is only the cue's `Kind`, so a move rewrites that column: moving
    /// the cue from slot C up to A makes A's cue B's slot and B's cue C's,
    /// all in one transaction. Memory cues have no slot and are refused.
    /// rekordbox's own window offering this has not been observed
    /// [UNKNOWN]; the rows written are ones rekordbox itself writes.
    pub fn move_hot_cue(&mut self, cue: &str, kind: u8) -> Result<Changed> {
        // The slots in pad order: A to C are Kind 1 to 3, D onward 5 to 17.
        const SLOTS: [i64; 16] = [1, 2, 3, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17];
        let slot_of = |kind: i64| SLOTS.iter().position(|&k| k == kind);
        let Some(to) = slot_of(i64::from(kind)) else {
            return Err(DbError::WriteRefused(format!("{kind} is not a hot cue slot rekordbox has")));
        };
        self.prepare()?;
        let stamp = time::now();
        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (content, from_kind): (Option<String>, i64) = tx
            .query_row(
                "SELECT ContentID, Kind FROM djmdCue WHERE ID = ?1 AND rb_local_deleted = 0",
                params![cue],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| DbError::WriteRefused(format!("no cue {cue}")))?;
        if from_kind == 0 {
            return Err(DbError::WriteRefused("a memory cue has no hot cue slot".to_owned()));
        }
        let Some(from) = slot_of(from_kind) else {
            return Err(DbError::WriteRefused(format!("cue {cue} is not in a hot cue slot")));
        };
        if from == to {
            return Ok(Changed { rows: 1, usn: 0 });
        }
        // Every live hot cue of the track, with the slot it takes after the
        // move: the moved cue lands on `to`, those between slide one toward
        // the slot it left.
        let held: Vec<(String, i64)> = {
            let mut stmt = tx.prepare(
                "SELECT ID, Kind FROM djmdCue WHERE ContentID IS ?1 AND Kind > 0 AND rb_local_deleted = 0",
            )?;
            let rows = stmt.query_map(params![content], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let usn = next_usn(&tx)?;
        let mut rows = 0;
        for (id, current) in held {
            let Some(slot) = slot_of(current) else { continue };
            let target = if id == cue {
                to
            } else if from < to && slot > from && slot <= to {
                slot - 1
            } else if from > to && slot >= to && slot < from {
                slot + 1
            } else {
                continue;
            };
            rows += tx.execute(
                "UPDATE djmdCue SET Kind = ?1, rb_local_usn = ?2, updated_at = ?3 WHERE ID = ?4",
                params![SLOTS[target], usn, stamp, id],
            )?;
        }
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok(Changed { rows, usn })
    }

    /// The track a live cue belongs to, or `None` for a cue that is not there.
    ///
    /// A read, so nothing is prepared or gated: a caller that is about to move
    /// or delete a cue needs to know whose cues to re-read afterwards, and the
    /// cue's own id is all the interface holds.
    pub fn cue_owner(&self, cue: &str) -> Result<Option<String>> {
        let owner = self
            .library
            .connection()
            .query_row(
                "SELECT ContentID FROM djmdCue WHERE ID = ?1 AND rb_local_deleted = 0",
                params![cue],
                |r| r.get::<_, Option<String>>(0),
            );
        match owner {
            Ok(content) => Ok(content),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Soft-deletes a cue.
    pub fn delete_cue(&mut self, cue: &str) -> Result<Changed> {
        self.prepare()?;
        let stamp = time::now();
        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let usn = next_usn(&tx)?;
        let rows = tx.execute(
            "UPDATE djmdCue SET rb_local_deleted = 1, rb_local_usn = ?1, updated_at = ?2
             WHERE ID = ?3 AND rb_local_deleted = 0",
            params![usn, stamp, cue],
        )?;
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok(Changed { rows, usn })
    }

    // -------------------------------------------------------------- metadata

    /// Sets a track's rating, 0 to 5 stars.
    pub fn set_rating(&mut self, content: &str, stars: u8) -> Result<Changed> {
        if stars > 5 {
            return Err(DbError::WriteRefused(format!("{stars} is not a rating between 0 and 5")));
        }
        // Stars as they are. The reference library holds 0 to 5 and nothing
        // else across 38,681 rows [OBS]; the multiples of 51 are the XML
        // export's scale, not the database's.
        self.touch_content(content, "Rating", &Value::Integer(i64::from(stars)))
    }

    pub fn set_rating_with_undo(&mut self, content: &str, stars: u8) -> Result<(Changed, TrackEdit)> {
        self.track_edit(content, "Rating", |writer| writer.set_rating(content, stars))
    }

    /// Sets a track's tempo, BPM x100, as a grid edit that changed the tempo
    /// records it. [`Writer::set_bpm`] is the other way round: a BPM typed
    /// over, which retimes the grid to match.
    ///
    /// The same column [`Writer::register_analysis`] writes, and the same
    /// value: `djmdContent.BPM` is the grid's tempo x100 on every one of
    /// the reference library's analysed rows [OBS]. The grid itself lives
    /// in the `.DAT`, which the caller rewrites first; this keeps the row in
    /// step with it. Zero is refused — a track with a grid has a tempo.
    pub fn set_bpm_x100(&mut self, content: &str, bpm_x100: u32) -> Result<Changed> {
        if bpm_x100 == 0 {
            return Err(DbError::WriteRefused("a grid's tempo cannot be zero".to_owned()));
        }
        self.touch_content(content, "BPM", &Value::Integer(i64::from(bpm_x100)))
    }

    /// Sets a track's comment.
    pub fn set_comment(&mut self, content: &str, comment: &str) -> Result<Changed> {
        self.touch_content(content, "Commnt", &Value::Text(comment.to_owned()))
    }

    pub fn set_comment_with_undo(&mut self, content: &str, comment: &str) -> Result<(Changed, TrackEdit)> {
        self.track_edit(content, "Commnt", |writer| writer.set_comment(content, comment))
    }

    /// Sets a track's colour, or clears it with `None`.
    pub fn set_color(&mut self, content: &str, color: Option<&str>) -> Result<Changed> {
        self.touch_content(
            content,
            "ColorID",
            &color.map_or(Value::Null, |c| Value::Text(c.to_owned())),
        )
    }

    pub fn set_color_with_undo(&mut self, content: &str, color: Option<&str>) -> Result<(Changed, TrackEdit)> {
        self.track_edit(content, "ColorID", |writer| writer.set_color(content, color))
    }

    /// Sets one of the information panel's editable fields.
    ///
    /// Plain columns are written as they are; a reference field finds or
    /// makes its lookup row with [`intern`] — the same shape [`Self::import_file`]
    /// gives a new track's artist, album, genre and label — and points the
    /// track at it, in one transaction. An empty value clears the reference to
    /// NULL, which is how the reference library spells an absent artist on
    /// 3,942 of its 38,681 tracks (75 carry `""`).
    ///
    /// The key is found, never made: `djmdKey` rows carry a `Seq` whose rule
    /// is not known, so a name that is not already there is refused.
    ///
    /// A number that does not parse is refused rather than written as zero:
    /// a typo in the year box must not erase the year.
    pub fn set_field(&mut self, content: &str, field: TrackField, value: &str) -> Result<Changed> {
        match field {
            TrackField::Title => self.touch_content(content, "Title", &Value::Text(value.to_owned())),
            TrackField::Lyricist => {
                self.touch_content(content, "Lyricist", &Value::Text(value.to_owned()))
            }
            TrackField::Year => self.touch_number(content, "ReleaseYear", value, 9999),
            TrackField::TrackNumber => self.touch_number(content, "TrackNo", value, 9999),
            TrackField::DiscNumber => self.touch_number(content, "DiscNo", value, 999),
            TrackField::PlayCount => self.touch_number(content, "DJPlayCount", value, 999_999),
            TrackField::Artist => self.touch_reference(content, "ArtistID", "djmdArtist", value),
            TrackField::OriginalArtist => {
                self.touch_reference(content, "OrgArtistID", "djmdArtist", value)
            }
            TrackField::Composer => self.touch_reference(content, "ComposerID", "djmdArtist", value),
            TrackField::Remixer => self.touch_reference(content, "RemixerID", "djmdArtist", value),
            TrackField::Album => self.touch_reference(content, "AlbumID", "djmdAlbum", value),
            TrackField::Genre => self.touch_reference(content, "GenreID", "djmdGenre", value),
            TrackField::Label => self.touch_reference(content, "LabelID", "djmdLabel", value),
            TrackField::Key => self.touch_key(content, value),
            TrackField::Bpm => self.set_bpm(content, value),
        }
    }

    /// Finds a detected musical key, creating its lookup row when this
    /// library has not encountered that key before.
    ///
    /// Unlike [`Self::set_field`], this is deliberately only for a result
    /// produced by the analyser. A typed key is still required to name an
    /// existing rekordbox key, but a new or sparsely imported library starts
    /// with no `djmdKey` rows at all and must be able to retain analysis.
    pub fn ensure_detected_key(&mut self, name: &str) -> Result<()> {
        self.prepare()?;
        let name = name.trim();
        if name.is_empty() {
            return Ok(());
        }
        let stamp = time::now();
        let _ = intern(
            self.library.connection(),
            "djmdKey",
            "ScaleName",
            name,
            &mut self.rng,
            &stamp,
        )?;
        Ok(())
    }

    /// Writes one information-panel field and retains its exact stored value.
    /// BPM is deliberately excluded: it owns analysis-file history in the
    /// grid editor rather than this database-only history.
    pub fn set_field_with_undo(
        &mut self,
        content: &str,
        field: TrackField,
        value: &str,
    ) -> Result<(Changed, TrackEdit)> {
        let column = match field {
            TrackField::Title => "Title",
            TrackField::Artist => "ArtistID",
            TrackField::Album => "AlbumID",
            TrackField::Year => "ReleaseYear",
            TrackField::TrackNumber => "TrackNo",
            TrackField::DiscNumber => "DiscNo",
            TrackField::OriginalArtist => "OrgArtistID",
            TrackField::Composer => "ComposerID",
            TrackField::Remixer => "RemixerID",
            TrackField::Lyricist => "Lyricist",
            TrackField::PlayCount => "DJPlayCount",
            TrackField::Genre => "GenreID",
            TrackField::Label => "LabelID",
            TrackField::Key => "KeyID",
            TrackField::Bpm => return Err(DbError::WriteRefused(
                "BPM history belongs to the beat grid editor".to_owned(),
            )),
        };
        self.track_edit(content, column, |writer| writer.set_field(content, field, value))
    }

    /// Whether `content` is a track in the library: a row that is there and
    /// not soft-deleted.
    pub fn has_track(&self, content: &str) -> Result<bool> {
        content_exists(self.library.connection(), content)
    }

    pub fn undo_track_edit(&mut self, edit: &TrackEdit) -> Result<Changed> {
        self.touch_content(&edit.content, edit.column, &edit.before)
    }

    pub fn redo_track_edit(&mut self, edit: &TrackEdit) -> Result<Changed> {
        self.touch_content(&edit.content, edit.column, &edit.after)
    }

    fn track_edit(
        &mut self,
        content: &str,
        column: &'static str,
        write: impl FnOnce(&mut Self) -> Result<Changed>,
    ) -> Result<(Changed, TrackEdit)> {
        let before = self.track_value(content, column)?;
        let changed = write(self)?;
        let after = self.track_value(content, column)?;
        Ok((changed, TrackEdit { content: content.to_owned(), column, before, after }))
    }

    fn track_value(&self, content: &str, column: &'static str) -> Result<Value> {
        if !WRITABLE_COLUMNS.contains(&column) {
            return Err(DbError::WriteRefused(format!("{column} is not writable")));
        }
        self.library.connection().query_row(
            &format!("SELECT `{column}` FROM djmdContent WHERE ID = ?1 AND rb_local_deleted = 0"),
            params![content],
            |row| row.get(0),
        ).map_err(Into::into)
    }

    // ---------------------------------------------------------------- my tag

    /// Sets the My Tags on a track to exactly `tags`: memberships not in
    /// the set are soft-deleted, new ones added on the end of each tag's
    /// list. Rows in the shape pyrekordbox documents for `djmdSongMyTag`
    /// [DOC; the reference library held none to transcribe]. A tag id the
    /// library does not hold is refused before anything is written.
    pub fn set_my_tags(&mut self, content: &str, tags: &[String]) -> Result<Changed> {
        self.prepare()?;
        let stamp = time::now();
        let mut ids: Vec<(String, String)> = Vec::with_capacity(tags.len());
        for _ in tags {
            ids.push((self.rng.uuid4(), self.rng.uuid4()));
        }
        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !content_exists(&tx, content)? {
            return Err(DbError::WriteRefused(format!("no track {content}")));
        }
        for tag in tags {
            let known: i64 = tx.query_row(
                "SELECT COUNT(*) FROM djmdMyTag WHERE ID = ?1 AND Attribute = 0 AND rb_local_deleted = 0",
                params![tag],
                |r| r.get(0),
            )?;
            if known == 0 {
                return Err(DbError::WriteRefused(format!("no My Tag {tag}")));
            }
        }
        let mut stmt = tx.prepare(
            "SELECT MyTagID FROM djmdSongMyTag WHERE ContentID = ?1 AND rb_local_deleted = 0",
        )?;
        let current: Vec<String> = stmt
            .query_map(params![content], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<_>>()?;
        drop(stmt);
        let mut rows = 0;
        let mut usn = next_usn(&tx)?;
        for gone in current.iter().filter(|t| !tags.contains(t)) {
            usn = next_usn(&tx)?;
            rows += tx.execute(
                "UPDATE djmdSongMyTag SET rb_local_deleted = 1, rb_local_usn = ?1, updated_at = ?2
                 WHERE ContentID = ?3 AND MyTagID = ?4 AND rb_local_deleted = 0",
                params![usn, stamp, content, gone],
            )?;
        }
        for (tag, (id, uuid)) in tags.iter().zip(&ids) {
            if current.contains(tag) {
                continue;
            }
            let track_no: i64 = tx.query_row(
                "SELECT COALESCE(MAX(TrackNo), 0) + 1 FROM djmdSongMyTag WHERE MyTagID = ?1 AND rb_local_deleted = 0",
                params![tag],
                |r| r.get(0),
            )?;
            usn = next_usn(&tx)?;
            rows += tx.execute(
                "INSERT INTO djmdSongMyTag
                    (ID, MyTagID, ContentID, TrackNo, UUID,
                     rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced,
                     usn, rb_local_usn, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, 0, 0, NULL, ?6, ?7, ?7)",
                params![id, tag, content, track_no, uuid, usn, stamp],
            )?;
        }
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok(Changed { rows, usn })
    }

    pub fn set_my_tags_with_undo(
        &mut self,
        content: &str,
        tags: &[String],
    ) -> Result<(Changed, TrackTagEdit)> {
        let before = self.track_tags(content)?;
        let changed = self.set_my_tags(content, tags)?;
        let after = self.track_tags(content)?;
        Ok((changed, TrackTagEdit { content: content.to_owned(), before, after }))
    }

    pub fn undo_tag_edit(&mut self, edit: &TrackTagEdit) -> Result<Changed> {
        self.set_my_tags(&edit.content, &edit.before)
    }

    pub fn redo_tag_edit(&mut self, edit: &TrackTagEdit) -> Result<Changed> {
        self.set_my_tags(&edit.content, &edit.after)
    }

    fn track_tags(&self, content: &str) -> Result<Vec<String>> {
        let mut statement = self.library.connection().prepare(
            "SELECT MyTagID FROM djmdSongMyTag
             WHERE ContentID = ?1 AND rb_local_deleted = 0 ORDER BY MyTagID",
        )?;
        let tags: Vec<String> = statement.query_map(params![content], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(tags)
    }

    /// Persist an edited grid's tempo and invalidate cached analysis consumers.
    pub fn save_grid_revision(&mut self, content: &str, bpm: u32) -> Result<()> {
        self.prepare()?;
        let tx = self.library.connection_mut().transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous: Option<String> = tx.query_row("SELECT AnalysisUpdated FROM djmdContent WHERE ID=?1 AND rb_local_deleted=0", [content], |r| r.get(0))?;
        // modifyAnalysisUpdated increments the first UTF-16 code unit, not
        // a parsed decimal integer. Preserve that reference wire behavior.
        let unit = previous.as_deref().unwrap_or("").encode_utf16().next().unwrap_or(0).wrapping_add(1);
        let revision = if unit == 0 { String::new() } else { String::from_utf16_lossy(&[unit]) };
        let usn = next_usn(&tx)?;
        let rows = tx.execute("UPDATE djmdContent SET BPM=?2, AnalysisUpdated=?5, rb_local_usn=?3, updated_at=?4 WHERE ID=?1 AND rb_local_deleted=0", params![content, bpm, usn, time::now(), revision])?;
        if rows != 1 { return Err(DbError::WriteRefused("Track no longer exists".into())); }
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok(())
    }

    /// `BeatGridAdjustment` uses bit 7; preserve the other analysis flags.
    pub fn set_analysis_lock(&mut self, content: &str, on: bool) -> Result<()> {
        self.prepare()?;
        let tx = self.library.connection_mut().transaction_with_behavior(TransactionBehavior::Immediate)?;
        let usn = next_usn(&tx)?;
        let rows = tx.execute("UPDATE djmdContent SET Analysed=(COALESCE(Analysed,0)&127)|?2, rb_local_usn=?3, updated_at=?4 WHERE ID=?1 AND rb_local_deleted=0", params![content, if on {128} else {0}, usn, time::now()])?;
        if rows != 1 { return Err(DbError::WriteRefused("Track no longer exists".into())); }
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok(())
    }

    /// Replaces a track's cues from a USB export and updates its grid metadata atomically.
    pub fn import_usb_cues(&mut self, content: &str, cues: &[rbl_anlz::CueEntry], bpm: u32) -> Result<()> {
        self.prepare()?;
        if cues.iter().any(|c| c.hot_cue > 16 || !matches!(c.kind, 1 | 2) || (c.kind == 2 && c.loop_time_ms <= c.time_ms)) {
            return Err(DbError::WriteRefused("Invalid USB cue data".into()));
        }
        let ids: Vec<_> = cues.iter().map(|_| self.unused_id_below("djmdCue", MAX_CUE_ID)).collect::<Result<_>>()?;
        let uuids: Vec<_> = cues.iter().map(|_| self.rng.uuid4()).collect();
        let tx = self.library.connection_mut().transaction_with_behavior(TransactionBehavior::Immediate)?;
        let owner: String = tx.query_row("SELECT UUID FROM djmdContent WHERE ID=?1 AND rb_local_deleted=0", [content], |r| r.get(0))?;
        let stamp = time::now();
        let usn = next_usn(&tx)?;
        tx.execute("UPDATE djmdCue SET rb_local_deleted=1, rb_local_usn=?2, updated_at=?3 WHERE ContentID=?1 AND rb_local_deleted=0", params![content, usn, stamp])?;
        for ((cue, id), uuid) in cues.iter().zip(ids).zip(uuids) {
            let kind = if cue.hot_cue >= 4 { cue.hot_cue + 1 } else { cue.hot_cue };
            let end = (cue.kind == 2).then_some(i64::from(cue.loop_time_ms));
            tx.execute("INSERT INTO djmdCue (ID, ContentID, InMsec, InFrame, InMpegFrame, InMpegAbs, OutMsec, Kind, Color, ColorTableIndex, ActiveLoop, Comment, ContentUUID, UUID, rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced, rb_local_usn, created_at, updated_at) VALUES (?1,?2,?3,0,0,0,?4,?5,?6,?7,0,?8,?9,?10,0,0,0,0,?11,?12,?12)", params![id, content, cue.time_ms, end, kind, if kind == 0 {255} else {-1}, cue.color_code.unwrap_or(cue.color_id), cue.comment.as_deref().unwrap_or(""), owner, uuid, usn, stamp])?;
        }
        tx.execute("UPDATE djmdContent SET BPM=CASE WHEN ?2>0 THEN ?2 ELSE BPM END, AnalysisUpdated=CAST(COALESCE(AnalysisUpdated, '0') AS INTEGER)+1, rb_local_usn=?3, updated_at=?4 WHERE ID=?1", params![content, bpm, usn, stamp])?;
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok(())
    }

    /// Imports a USB session once, preserving order and repeated plays.
    pub fn import_usb_history(&mut self, name: &str, source_uuid: &str, tracks: &[String]) -> Result<usize> {
        self.prepare()?;
        let candidate = self.unused_id("djmdHistory")?;
        let stamp = time::now();
        let created = time::local_stamp();
        let ids: Vec<_> = tracks.iter().map(|_| (self.rng.uuid4(), self.rng.uuid4())).collect();
        let tx = self.library.connection_mut().transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<String> = tx.query_row("SELECT ID FROM djmdHistory WHERE UUID=?1 AND rb_local_deleted=0", [source_uuid], |r| r.get(0)).optional()?;
        let session = match existing {
            Some(id) => id,
            None => history_node(&tx, name, ROOT, 0, &candidate, source_uuid, &created, &stamp)?,
        };
        let previous: i64 = tx.query_row("SELECT COUNT(*) FROM djmdSongHistory WHERE HistoryID=?1 AND rb_local_deleted=0", [&session], |r| r.get(0))?;
        let previous_tracks: Vec<String> = tx.prepare("SELECT ContentID FROM djmdSongHistory WHERE HistoryID=?1 AND rb_local_deleted=0 ORDER BY TrackNo")?
            .query_map([&session], |r| r.get(0))?.collect::<std::result::Result<_,_>>()?;
        if !tracks.starts_with(&previous_tracks) {
            return Err(DbError::WriteRefused("This USB history changed since its last import; it was left unchanged.".into()));
        }
        if previous_tracks.len() == tracks.len() { return Ok(0); }
        let mut count = 0;
        let usn = next_usn(&tx)?;
        for (index, (content, (id, uuid))) in tracks.iter().zip(ids).enumerate().skip(usize::try_from(previous.max(0)).map_err(|_| DbError::WriteRefused("history length exceeds this platform".into()))?) {
            if !content_exists(&tx, content)? { return Err(DbError::WriteRefused("USB history contains an unknown track".into())); }
            tx.execute("INSERT INTO djmdSongHistory (ID, HistoryID, ContentID, TrackNo, UUID, rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced, rb_local_usn, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,0,0,0,0,?6,?7,?7)", params![id, session, content, i64::try_from(index + 1).map_err(|_| DbError::WriteRefused("history length exceeds database capacity".into()))?, uuid, usn, stamp])?;
            count += 1;
        }
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok(count)
    }

    // --------------------------------------------------------------- history

    /// Records a play: the track goes on the end of today's history session
    /// and its `DJPlayCount` goes up by one.
    ///
    /// The session is `HISTORY yyyy-mm-dd` in local time, filed under a
    /// month folder named by the month's number inside a year folder named
    /// by the year — the tree rekordbox keeps, as the fixture transcribes it
    /// from the reference library. Folders and the session are made when
    /// missing, appended by `Seq` like a playlist. `DateCreated` is the
    /// local time without an offset, as the transcribed rows have it
    /// [ASSUME: the seconds are rekordbox's own; the transcription carries
    /// them]. A track played twice is listed twice, which is what a history
    /// is.
    pub fn record_play(&mut self, content: &str) -> Result<Changed> {
        self.prepare()?;
        // Ids for whatever has to be made, found before the transaction.
        let candidates = [
            self.unused_id("djmdHistory")?,
            self.unused_id("djmdHistory")?,
            self.unused_id("djmdHistory")?,
        ];
        let node_uuids = [self.rng.uuid4(), self.rng.uuid4(), self.rng.uuid4()];
        let play = [self.rng.uuid4(), self.rng.uuid4()];
        let stamp = time::now();
        let created = time::local_stamp();
        let session_name = format!("HISTORY {}", created.get(..10).unwrap_or_default());

        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !content_exists(&tx, content)? {
            return Err(DbError::WriteRefused(format!("no track {content}")));
        }
        let month_id = month_folder(&tx, [&candidates[0], &candidates[1]], [&node_uuids[0], &node_uuids[1]], &created, &stamp)?;
        let session_id = history_node(&tx, &session_name, &month_id, 0, &candidates[2], &node_uuids[2], &created, &stamp)?;
        let changed = append_play(&tx, &session_id, content, &play, &stamp)?;
        tx.commit()?;
        Ok(changed)
    }

    /// Starts the history of a LINK session, as rekordbox does when a player
    /// first adds a track to history over the link: always a new session,
    /// `LINK HISTORY yyyy-mm-dd` in local time, in today's month folder like
    /// any other history. A name already taken gets ` (n)` after it, n one
    /// past the highest in use and 1 after the bare name — rekordbox
    /// 7.2.11's `PSvAppSyncDBIF::makeNewHistory`, which counts every history
    /// name that is not deleted, folders included. Returns the session's id.
    pub fn new_link_history(&mut self) -> Result<String> {
        self.prepare()?;
        let candidates = [
            self.unused_id("djmdHistory")?,
            self.unused_id("djmdHistory")?,
            self.unused_id("djmdHistory")?,
        ];
        let node_uuids = [self.rng.uuid4(), self.rng.uuid4(), self.rng.uuid4()];
        let stamp = time::now();
        let created = time::local_stamp();
        let base = format!("LINK HISTORY {}", created.get(..10).unwrap_or_default());

        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let names: Vec<String> = tx
            .prepare("SELECT Name FROM djmdHistory WHERE rb_local_deleted = 0")?
            .query_map([], |r| r.get::<_, Option<String>>(0))?
            .filter_map(std::result::Result::transpose)
            .collect::<rusqlite::Result<_>>()?;
        let name = match name_suffix(&base, &names) {
            0 => base,
            n => format!("{base} ({n})"),
        };
        let month_id = month_folder(&tx, [&candidates[0], &candidates[1]], [&node_uuids[0], &node_uuids[1]], &created, &stamp)?;
        let session_id = history_node(&tx, &name, &month_id, 0, &candidates[2], &node_uuids[2], &created, &stamp)?;
        // The highest update number the rows above took.
        set_counter(&tx, next_usn(&tx)? - 1)?;
        tx.commit()?;
        Ok(session_id)
    }

    /// A play added to a history session that already exists: on its end,
    /// with the track's `DJPlayCount` up by one, which is what rekordbox does
    /// with a play a player adds to its link history
    /// (`DatabaseMediator::notifyDBUpdatedHistory` → `updateDjPlayCount`).
    pub fn add_to_history(&mut self, history: &str, content: &str) -> Result<Changed> {
        self.prepare()?;
        let play = [self.rng.uuid4(), self.rng.uuid4()];
        let stamp = time::now();
        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !content_exists(&tx, content)? {
            return Err(DbError::WriteRefused(format!("no track {content}")));
        }
        let session: i64 = tx.query_row(
            "SELECT COUNT(*) FROM djmdHistory WHERE ID = ?1 AND Attribute = 0 AND rb_local_deleted = 0",
            params![history],
            |r| r.get(0),
        )?;
        if session == 0 {
            return Err(DbError::WriteRefused(format!("no history {history}")));
        }
        let changed = append_play(&tx, history, content, &play, &stamp)?;
        tx.commit()?;
        Ok(changed)
    }

    /// Remove from History: the tracks' plays leave the session, which closes
    /// its gaps as a playlist does.
    pub fn remove_from_history(&mut self, history: &str, contents: &[String]) -> Result<Changed> {
        self.prepare()?;
        let stamp = time::now();
        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut rows = 0;
        let mut usn = 0;
        for content in contents {
            usn = next_usn(&tx)?;
            rows += tx.execute(
                "UPDATE djmdSongHistory SET rb_local_deleted = 1, rb_local_usn = ?1, updated_at = ?2
                 WHERE HistoryID = ?3 AND ContentID = ?4 AND rb_local_deleted = 0",
                params![usn, stamp, history, content],
            )?;
        }
        if rows > 0 {
            usn = renumber_history(&tx, history, &stamp)?;
        }
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok(Changed { rows, usn })
    }

    /// Add Artwork and Delete Artwork on the information panel.
    ///
    /// An image is copied into the share tree where rekordbox files an
    /// imported sleeve — `/PIONEER/Artwork/<3 hex>/<uuid>/artwork.<ext>`
    /// [OBS for the shape; ASSUME that the three hex digits are the
    /// uuid's own first three, which is what a bucketing by name looks
    /// like] — and `ImagePath` points at it. `None` clears `ImagePath` to
    /// empty, which is what the library holds for a track without artwork
    /// [OBS on the index: empty for the half of the library that has none];
    /// the file is left where it is.
    pub fn set_artwork(&mut self, content: &str, image: Option<&Path>) -> Result<Changed> {
        let Some(image) = image else {
            return self.touch_content(content, "ImagePath", &Value::Text(String::new()));
        };
        let extension = image
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .filter(|e| matches!(e.as_str(), "jpg" | "jpeg" | "png"))
            .ok_or_else(|| DbError::WriteRefused(format!("{} is not a JPEG or PNG", image.display())))?;
        let bytes = std::fs::read(image)?;
        let uuid = self.rng.uuid4();
        let bucket = uuid.get(..3).unwrap_or("000").to_owned();
        let relative = format!("/PIONEER/Artwork/{bucket}/{uuid}/artwork.{extension}");
        let target = rbl_anlz::resolve(&self.library.location().share_root, &relative);
        write_artwork_sizes(&target, &bytes)?;
        self.touch_content(content, "ImagePath", &Value::Text(relative))
    }

    pub fn set_artwork_with_undo(
        &mut self,
        content: &str,
        image: Option<&Path>,
    ) -> Result<(Changed, TrackEdit)> {
        self.track_edit(content, "ImagePath", |writer| writer.set_artwork(content, image))
    }

    /// Import a file's embedded cover only when the track has no artwork.
    /// Read the current database row so a stale index cannot replace custom art.
    pub fn import_artwork(&mut self, content: &str) -> Result<bool> {
        self.prepare()?;
        let (path, image): (String, Option<String>) = self.library.connection().query_row(
            "SELECT FolderPath, ImagePath FROM djmdContent WHERE ID = ?1 AND rb_local_deleted = 0",
            [content], |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if image.is_some_and(|p| !p.is_empty()) {
            return Ok(false);
        }
        let path = self.real_path(&path);
        let Some(bytes) = crate::import::read_artwork(Path::new(&path))
            .map_err(|e| DbError::WriteRefused(e.to_string()))? else {
            return Ok(false);
        };
        let format = image::guess_format(&bytes)
            .map_err(|e| DbError::WriteRefused(format!("invalid embedded artwork: {e}")))?;
        let extension = match format {
            image::ImageFormat::Jpeg => "jpg",
            image::ImageFormat::Png => "png",
            _ => return Ok(false),
        };
        // Validate before publishing an ImagePath that rekordbox cannot draw.
        image::load_from_memory(&bytes)
            .map_err(|e| DbError::WriteRefused(format!("invalid embedded artwork: {e}")))?;
        let uuid = self.rng.uuid4();
        let bucket = uuid.get(..3).unwrap_or("000");
        let relative = format!("/PIONEER/Artwork/{bucket}/{uuid}/artwork.{extension}");
        let target = rbl_anlz::resolve(&self.library.location().share_root, &relative);
        write_artwork_sizes(&target, &bytes)?;
        self.touch_content(content, "ImagePath", &Value::Text(relative))?;
        Ok(true)
    }

    /// Gives a playlist artwork, or takes it away with `None`. The image
    /// goes where a track's does [ASSUME: the reference library has no
    /// playlist with artwork to copy the path from].
    pub fn set_playlist_artwork(&mut self, playlist: &str, image: Option<&Path>) -> Result<Changed> {
        let Some(image) = image else {
            return self.touch_playlist(playlist, "ImagePath", &Value::Text(String::new()));
        };
        let extension = image
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .filter(|e| matches!(e.as_str(), "jpg" | "jpeg" | "png"))
            .ok_or_else(|| DbError::WriteRefused(format!("{} is not a JPEG or PNG", image.display())))?;
        let bytes = std::fs::read(image)?;
        let uuid = self.rng.uuid4();
        let bucket = uuid.get(..3).unwrap_or("000").to_owned();
        let relative = format!("/PIONEER/Artwork/{bucket}/{uuid}/artwork.{extension}");
        let target = rbl_anlz::resolve(&self.library.location().share_root, &relative);
        write_artwork_sizes(&target, &bytes)?;
        self.touch_playlist(playlist, "ImagePath", &Value::Text(relative))
    }

    /// Reload Tag: reads the file's tags again and writes what they say
    /// over the row — title, artist, album, genre, label, comment, key, year
    /// and track number, each from the tag rekordbox reads it from (see
    /// [`crate::import::read_tags`]). rekordbox's Reload Tag
    /// (`DatabaseMediator::readTag` @0x100c459ec, 7.2.19 macOS arm64) and its
    /// import share `convertTagData` @0x100c463e0, which takes the key only
    /// when the tag has one and never reads the BPM [OBS static]. Fields the
    /// file leaves empty are left as they are [ASSUME: `convertTagData`
    /// copies the text fields even when empty; what rekordbox then shows for
    /// one has not been observed]. Returns how many changed.
    pub fn reload_tags(&mut self, content: &str) -> Result<usize> {
        self.prepare()?;
        let Some(stored) = self.library.stored_path(content)? else {
            return Err(DbError::WriteRefused(format!("no track {content}")));
        };
        // The file rekordbox opens: a cloud-synced track's local copy, as
        // playback resolves it, not the raw stored path.
        let path = self.library.track_paths().resolve(&stored);
        let tags = crate::import::read_tags(Path::new(&path))
            .map_err(|e| DbError::WriteRefused(e.to_string()))?;
        let stamp = time::now();
        let tx = self.library.connection_mut().transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut fields = Vec::new();
        for (column, value) in [("Title", &tags.title), ("Commnt", &tags.comment)] {
            if !value.is_empty() { fields.push((column, Value::Text(value.clone()))); }
        }
        for (column, table, value) in [("ArtistID", "djmdArtist", &tags.artist), ("AlbumID", "djmdAlbum", &tags.album),
            ("GenreID", "djmdGenre", &tags.genre), ("LabelID", "djmdLabel", &tags.label)] {
            if !value.is_empty() {
                let id = intern(&tx, table, "Name", value.trim(), &mut self.rng, &stamp)?;
                fields.push((column, id.map_or(Value::Null, Value::Text)));
            }
        }
        if let Some(key) = tag_key_id(&tx, &tags.key, &mut self.rng, &stamp)? {
            fields.push(("KeyID", Value::Text(key)));
        }
        if tags.year != 0 { fields.push(("ReleaseYear", Value::Integer(i64::from(tags.year)))); }
        if tags.track_no != 0 { fields.push(("TrackNo", Value::Integer(i64::from(tags.track_no)))); }
        let mut changed = 0;
        for (column, value) in fields {
            let usn = next_usn(&tx)?;
            changed += tx.execute(&format!("UPDATE djmdContent SET {column}=?1, rb_local_usn=?2, updated_at=?3 WHERE ID=?4 AND rb_local_deleted=0"),
                params![value, usn, stamp, content])?;
            set_counter(&tx, usn)?;
        }
        tx.commit()?;
        Ok(changed)
    }

    /// A BPM typed over the analysed one: `128`, `128.5`.
    ///
    /// The grid in the track's `.DAT` is retimed to the new tempo from its
    /// first beat — the beats keep their count and their downbeats, only
    /// the spacing changes — and the `PQT2` copy of it is dropped, since a
    /// stale one beside a new grid is worse than none. New immutable files
    /// are flushed before BPM and `AnalysisDataPath` commit together. A crash
    /// can leave unreferenced files, but cannot change the old grid in place.
    pub fn set_bpm(&mut self, content: &str, value: &str) -> Result<Changed> {
        let bpm: f64 = value.trim().parse().map_err(|_| DbError::WriteRefused(format!("{value:?} is not a BPM")))?;
        if !bpm.is_finite() || !(20.0..=400.0).contains(&bpm) {
            return Err(DbError::WriteRefused(format!("{bpm} is outside 20 to 400 BPM")));
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "20 to 400, checked above")]
        let bpm_x100 = (bpm * 100.0).round() as u32;

        self.prepare()?;
        let location = self.library.location().clone();
        let uuid = self.rng.uuid4();
        let tx = self.library.connection_mut().transaction_with_behavior(TransactionBehavior::Immediate)?;
        let relative: Option<String> = tx.query_row(
            "SELECT AnalysisDataPath FROM djmdContent WHERE ID=?1 AND rb_local_deleted=0", [content], |r| r.get(0))?;
        let mut next_path = None;
        if let Some(relative) = relative.filter(|p| !p.is_empty()) {
            let dat = rbl_anlz::resolve(&location.share_root, &relative);
            // An unreadable existing grid must not silently become a mismatched BPM.
            let anlz = rbl_anlz::Anlz::read(&dat).map_err(|e| DbError::WriteRefused(e.to_string()))?;
            if let Some(beats) = anlz.beat_grid().filter(|b| !b.is_empty()) {
                let retimed = retime(&beats, bpm_x100);
                let path = format!("/PIONEER/USBANLZ/{}/{uuid}/ANLZ0000.DAT", &uuid[..3]);
                let target = rbl_anlz::resolve(&location.share_root, &path);
                let dir = target.parent().ok_or_else(|| DbError::WriteRefused("invalid analysis path".into()))?;
                rbl_core::durable::create_dir_all(dir)?;
                let primary = rbl_anlz::Anlz { header_extra: anlz.header_extra, sections: anlz.sections.into_iter().filter(|s| s.tag != rbl_core::FourCc::new(b"PQT2")).collect() };
                rbl_core::durable::write(&target, &primary.with_beat_grid(&retimed))?;
                // Keep every companion (including stems), with a consistent grid
                // in each ANLZ file. The old row and files stay usable until commit.
                for entry in std::fs::read_dir(dat.parent().ok_or_else(|| DbError::WriteRefused("invalid analysis path".into()))?)? {
                    let entry = entry?;
                    let source = entry.path();
                    if source == dat || !std::fs::metadata(&source)?.is_file() { continue; }
                    let is_anlz = source.file_stem() == dat.file_stem() && source.extension().is_some_and(|e| ["DAT", "EXT", "2EX"].iter().any(|ext| e.eq_ignore_ascii_case(ext)));
                    if is_anlz {
                        let file = rbl_anlz::Anlz::read(&source).map_err(|e| DbError::WriteRefused(e.to_string()))?;
                        let kept = rbl_anlz::Anlz { header_extra: file.header_extra, sections: file.sections.into_iter().filter(|s| s.tag != rbl_core::FourCc::new(b"PQT2")).collect() };
                        let destination = target.with_extension(source.extension().unwrap_or_default().to_string_lossy().to_ascii_uppercase());
                        rbl_core::durable::write(&destination, &kept.with_beat_grid(&retimed))?;
                    } else {
                        rbl_core::durable::copy(&source, &dir.join(entry.file_name()))?;
                    }
                }
                next_path = Some(path);
            }
        }
        let usn = next_usn(&tx)?;
        let rows = tx.execute("UPDATE djmdContent SET BPM=?1, AnalysisDataPath=COALESCE(?2, AnalysisDataPath), AnalysisUpdated=CAST(COALESCE(AnalysisUpdated, '0') AS INTEGER)+CASE WHEN ?2 IS NULL THEN 0 ELSE 1 END, rb_local_usn=?3, updated_at=?4 WHERE ID=?5 AND rb_local_deleted=0",
            params![bpm_x100, next_path, usn, time::now(), content])?;
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok(Changed { rows, usn })
    }

    /// A non-negative integer column, refused when the text is not one.
    fn touch_number(&mut self, content: &str, column: &str, value: &str, max: i64) -> Result<Changed> {
        let n: i64 = value.trim().parse().map_err(|_| {
            DbError::WriteRefused(format!("{value:?} is not a whole number"))
        })?;
        if !(0..=max).contains(&n) {
            return Err(DbError::WriteRefused(format!("{n} is outside 0 to {max}")));
        }
        self.touch_content(content, column, &Value::Integer(n))
    }

    /// A reference column: intern the name, then point the track at it.
    fn touch_reference(
        &mut self,
        content: &str,
        column: &str,
        table: &str,
        name: &str,
    ) -> Result<Changed> {
        if !WRITABLE_COLUMNS.contains(&column) || !LOOKUP_TABLES.contains(&table) {
            return Err(DbError::WriteRefused(format!("{table}.{column} is not a writable reference")));
        }
        self.prepare()?;
        let stamp = time::now();
        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = intern(&tx, table, "Name", name.trim(), &mut self.rng, &stamp)?;
        let usn = next_usn(&tx)?;
        let sql = format!(
            "UPDATE djmdContent SET {column} = ?1, rb_local_usn = ?2, updated_at = ?3
             WHERE ID = ?4 AND rb_local_deleted = 0"
        );
        let rows = tx.execute(&sql, params![id, usn, stamp, content])?;
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok(Changed { rows, usn })
    }

    /// The key, looked up by its `ScaleName`; empty clears it.
    fn touch_key(&mut self, content: &str, name: &str) -> Result<Changed> {
        let name = name.trim();
        if name.is_empty() {
            return self.touch_content(content, "KeyID", &Value::Null);
        }
        let id: Option<String> = self
            .library
            .connection()
            .query_row(
                "SELECT ID FROM djmdKey WHERE ScaleName = ?1 AND rb_local_deleted = 0",
                params![name],
                |r| r.get(0),
            )
            .ok();
        let Some(id) = id else {
            return Err(DbError::WriteRefused(format!(
                "{name:?} is not a key the library knows; a new djmdKey row needs its Seq explained by a diff recording"
            )));
        };
        self.touch_content(content, "KeyID", &Value::Text(id))
    }

    /// A stored `FolderPath` as rekordbox reads it. See
    /// [`crate::Library::real_folder_path`].
    fn real_path(&self, folder_path: &str) -> String {
        self.library.real_folder_path(folder_path)
    }

    /// Points a track at a different file.
    ///
    /// For a track whose audio has moved. Only the location changes — the
    /// analysis, cues and playlist memberships all key off the track's id and
    /// stay where they are.
    pub fn relocate(&mut self, content: &str, path: &Path) -> Result<Changed> {
        if !path.is_file() {
            return Err(DbError::WriteRefused(format!(
                "{} is not a file; a track must point at one",
                path.display()
            )));
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let full = path.to_string_lossy().into_owned();
        self.prepare()?;
        let tx = self.library.connection_mut().transaction_with_behavior(TransactionBehavior::Immediate)?;
        let usn = next_usn(&tx)?;
        let rows = tx.execute("UPDATE djmdContent SET FolderPath=?1, FileNameL=?2, rb_local_usn=?3, updated_at=?4 WHERE ID=?5 AND rb_local_deleted=0",
            params![full, name, usn, time::now(), content])?;
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok(Changed { rows, usn })
    }

    /// Soft-deletes a track and every playlist membership pointing at it.
    pub fn delete_track(&mut self, content: &str) -> Result<Changed> {
        self.prepare()?;
        let stamp = time::now();
        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        let mut playlists = Vec::new();
        {
            let mut stmt = tx.prepare(
                "SELECT DISTINCT PlaylistID FROM djmdSongPlaylist
                 WHERE ContentID = ?1 AND rb_local_deleted = 0",
            )?;
            playlists.extend(
                stmt.query_map(params![content], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?,
            );
        }

        let mut usn = next_usn(&tx)?;
        let mut rows = tx.execute(
            "UPDATE djmdSongPlaylist SET rb_local_deleted = 1, rb_local_usn = ?1, updated_at = ?2
             WHERE ContentID = ?3 AND rb_local_deleted = 0",
            params![usn, stamp, content],
        )?;
        // Each affected playlist has to close its gaps, or TrackNo stops being
        // contiguous and rekordbox renders the playlist with holes. The USN it
        // hands back is discarded: `next_usn` below takes the maximum across
        // the tables, so it already accounts for whatever renumbering wrote.
        for playlist in &playlists {
            renumber(&tx, playlist, &stamp)?;
        }
        usn = next_usn(&tx)?;
        rows += tx.execute(
            "UPDATE djmdContent SET rb_local_deleted = 1, rb_local_usn = ?1, updated_at = ?2
             WHERE ID = ?3 AND rb_local_deleted = 0",
            params![usn, stamp, content],
        )?;
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok(Changed { rows, usn })
    }

    /// Soft-deletes many tracks and every membership pointing at them, in one
    /// transaction: the same rows [`Writer::delete_track`] leaves for each,
    /// without its per-track scans. The memberships are found together, each
    /// affected playlist closes its gaps once, and the USN is counted up from
    /// one read. A track that is not live is skipped, as it is there.
    pub fn delete_tracks(&mut self, contents: &[String]) -> Result<Changed> {
        use std::collections::BTreeSet;

        self.prepare()?;
        let stamp = time::now();
        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        let mut memberships: Vec<String> = Vec::new();
        let mut playlists: BTreeSet<String> = BTreeSet::new();
        for chunk in contents.chunks(500) {
            let marks = vec!["?"; chunk.len()].join(",");
            let mut stmt = tx.prepare(&format!(
                "SELECT ID, PlaylistID FROM djmdSongPlaylist
                 WHERE rb_local_deleted = 0 AND ContentID IN ({marks})"
            ))?;
            let found = stmt
                .query_map(rusqlite::params_from_iter(chunk), |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (id, playlist) in found {
                memberships.push(id);
                playlists.insert(playlist);
            }
        }

        let start = next_usn(&tx)?;
        let mut next = start;
        let mut rows = 0;
        let take = |next: &mut i64| -> Result<i64> {
            let used = *next;
            *next = used.checked_add(1).ok_or_else(|| DbError::WriteRefused("local update counter overflow".into()))?;
            Ok(used)
        };
        for id in &memberships {
            let usn = take(&mut next)?;
            rows += tx.execute(
                "UPDATE djmdSongPlaylist SET rb_local_deleted = 1, rb_local_usn = ?1, updated_at = ?2
                 WHERE ID = ?3 AND rb_local_deleted = 0",
                params![usn, stamp, id],
            )?;
        }
        // Each affected playlist closes its gaps, or TrackNo stops being
        // contiguous and rekordbox renders the playlist with holes.
        for playlist in &playlists {
            renumber_from(&tx, playlist, &stamp, &mut next)?;
        }
        for content in contents {
            let usn = take(&mut next)?;
            rows += tx.execute(
                "UPDATE djmdContent SET rb_local_deleted = 1, rb_local_usn = ?1, updated_at = ?2
                 WHERE ID = ?3 AND rb_local_deleted = 0",
                params![usn, stamp, content],
            )?;
        }
        // The last USN handed out, or the first free one when nothing was.
        let usn = if next > start { next - 1 } else { start };
        if next > start {
            set_counter(&tx, usn)?;
        }
        tx.commit()?;
        Ok(Changed { rows, usn })
    }

    // --------------------------------------------------------------- plumbing

    /// One-column update on a playlist, with the bookkeeping attached.
    fn touch_playlist(&mut self, id: &str, column: &str, value: &Value) -> Result<Changed> {
        self.touch("djmdPlaylist", id, column, value)
    }

    /// One-column update on a track, with the bookkeeping attached.
    fn touch_content(&mut self, id: &str, column: &str, value: &Value) -> Result<Changed> {
        self.touch("djmdContent", id, column, value)
    }

    /// Sets one column, bumps the USN, stamps `updated_at`, moves the counter.
    ///
    /// The column name is not user input — it comes from the call sites in this
    /// file — but it is still checked against a list, because a name reaching
    /// this through a future caller would be a SQL injection straight into the
    /// user's library.
    fn touch(&mut self, table: &str, id: &str, column: &str, value: &Value) -> Result<Changed> {
        if !WRITABLE_COLUMNS.contains(&column) {
            return Err(DbError::WriteRefused(format!("{column} is not a writable column")));
        }
        self.prepare()?;
        let stamp = time::now();
        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let usn = next_usn(&tx)?;
        let sql = format!(
            "UPDATE {table} SET {column} = ?1, rb_local_usn = ?2, updated_at = ?3
             WHERE ID = ?4 AND rb_local_deleted = 0"
        );
        let rows = tx.execute(&sql, params![value, usn, stamp, id])?;
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok(Changed { rows, usn })
    }

    /// Takes the analysis off a track: the row goes back to how an
    /// unanalysed one reads.
    ///
    /// `BPM` to 0, `KeyID` and `AnalysisUpdated` to null, `AnalysisDataPath`
    /// to empty and `Analysed` to 0 — the value [`set_analysis`] looks for
    /// before it writes its own, so a track cleared here is re-registered
    /// rather than left with whatever analysed it first. `Length` stays: it
    /// is the file's, not the analysis's. The files themselves are the
    /// caller's to delete, in that order — a row naming files that are gone
    /// is worse than files nothing names.
    ///
    /// [`set_analysis`]: Self::set_analysis
    pub fn clear_analysis(&mut self, content: &str) -> Result<Changed> {
        self.prepare()?;
        let stamp = time::now();
        let tx = self
            .library
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let usn = next_usn(&tx)?;
        let rows = tx.execute(
            "UPDATE djmdContent SET
                BPM = 0,
                KeyID = NULL,
                AnalysisDataPath = '',
                Analysed = 0,
                AnalysisUpdated = NULL,
                rb_local_usn = ?1,
                updated_at = ?2
             WHERE ID = ?3 AND rb_local_deleted = 0",
            params![usn, stamp, content],
        )?;
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok(Changed { rows, usn })
    }

    /// Registers an analysis this app made for a track.
    ///
    /// One transaction: `BPM`, `KeyID` when the key is one the library
    /// names (an unknown name leaves the key as it was rather than creating
    /// a `djmdKey` row, whose `Seq` is unexplained), `AnalysisDataPath`,
    /// `Length`, `Analysed` and the integer `AnalysisUpdated` revision, with
    /// the usual bookkeeping. Initializes missing local-file `ContentLink`
    /// registration so rekordbox can display the preview. Existing flags
    /// are preserved. The files themselves are the caller's to have written
    /// first: a row that names files that are not there is worse than files
    /// nothing names.
    pub fn set_analysis(&mut self, content: &str, analysis: &AnalysisWrite<'_>) -> Result<Changed> {
        self.prepare()?;
        let stamp = time::now();
        let tx = self.library.connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let key_id: Option<String> = match analysis.key.map(str::trim).filter(|k| !k.is_empty()) {
            Some(name) => tx
                .query_row(
                    "SELECT ID FROM djmdKey WHERE ScaleName = ?1 AND rb_local_deleted = 0",
                    params![name],
                    |r| r.get(0),
                )
                .optional()?,
            None => None,
        };
        let usn = next_usn(&tx)?;
        let rows = tx.execute(
            "UPDATE djmdContent SET
                BPM = ?1,
                KeyID = COALESCE(?2, KeyID),
                AnalysisDataPath = ?3,
                Length = COALESCE(?4, Length),
                Analysed = CASE
                    WHEN (COALESCE(Analysed, 0) & 127) IN (0, 1)
                    THEN (COALESCE(Analysed, 0) & 128) | ?5
                    ELSE Analysed
                END,
                ContentLink = COALESCE(ContentLink, ?9),
                AnalysisUpdated = CAST(COALESCE(AnalysisUpdated, '0') AS INTEGER) + 1,
                rb_local_usn = ?7,
                updated_at = ?6
             WHERE ID = ?8 AND rb_local_deleted = 0",
            params![
                i64::from(analysis.bpm_x100),
                key_id,
                analysis.analysis_path,
                analysis.length_sec.map(i64::from),
                ANALYSED_BY_THIS_APP,
                stamp,
                usn,
                content,
                CONTENT_LINK_LOCAL
            ],
        )?;
        set_counter(&tx, usn)?;
        tx.commit()?;
        Ok(Changed { rows, usn })
    }

    /// Runs before every transaction.
    ///
    /// rekordbox can be launched between one edit and the next, so the check
    /// made when the session opened is not enough. The backup is taken once,
    /// before the first write of the session.
    fn prepare(&mut self) -> Result<()> {
        // Only the installed library needs this: rekordbox holds that file's
        // WAL, and nothing else's.
        if self.library.location().is_real_install && is_rekordbox_running() {
            return Err(DbError::WriteRefused(
                "rekordbox is running. Quit it before making changes.".to_owned(),
            ));
        }
        if self.automatic_backups && !self.backup_taken {
            self.back_up()?;
            self.backup_taken = true;
        }
        Ok(())
    }

    /// Publishes a consistent, standalone `SQLCipher` snapshot. VACUUM INTO
    /// reads a SQLite snapshot, including committed WAL pages. A partial
    /// copy is never listed as a usable backup.
    fn back_up(&mut self) -> Result<PathBuf> {
        rbl_core::durable::create_dir_all(&self.backup_dir)
            .map_err(|e| DbError::Open(format!("{}: {e}", self.backup_dir.display())))?;
        let stamp = time::now().replace([' ', ':', '+', '.'], "-");
        let target = self.backup_dir.join(format!("master-{stamp}.db"));
        if target.exists() {
            return Err(DbError::WriteRefused(format!(
                "{} already exists; refusing to write over a backup",
                target.display()
            )));
        }

        let staging = tempfile::tempdir_in(&self.backup_dir)?;
        let staged = staging.path().join("master.db");
        self.library.connection().execute("VACUUM main INTO ?1", [staged.to_string_lossy().as_ref()])?;
        rbl_core::durable::replace(&staged, &target)?;

        prune_backups(&self.backup_dir, BACKUPS_KEPT);
        tracing::info!(path = %target.display(), "backed up the library before writing");
        Ok(target)
    }

    /// Copies the library aside on request — Preferences › Advanced ›
    /// Database management — and says where the copy went.
    pub fn back_up_now(&mut self) -> Result<PathBuf> {
        let copy = self.back_up()?;
        // The session's backup is this one: a write that follows in the
        // same millisecond must not take another under the same name.
        self.backup_taken = true;
        Ok(copy)
    }

    /// Finds an id no row in `table` is using.
    fn unused_id(&mut self, table: &str) -> Result<String> {
        let sql = format!("SELECT COUNT(*) FROM {table} WHERE ID = ?1");
        for _ in 0..ID_ATTEMPTS {
            let candidate = self.rng.numeric_id(MAX_PLAYLIST_ID);
            let taken: i64 =
                self.library.connection().query_row(&sql, params![candidate], |r| r.get(0))?;
            if taken == 0 {
                return Ok(candidate);
            }
        }
        Err(DbError::WriteRefused(format!(
            "could not find an unused id for {table} in {ID_ATTEMPTS} attempts"
        )))
    }
}

/// Reads the tags of every path, on as many threads as the machine has cores
/// (up to eight). Reading a file's tags is disk and decode work that touches
/// no database, so it is the one part of an import that can overlap.
fn read_tags_parallel(
    paths: &[PathBuf],
) -> Vec<std::result::Result<crate::import::TrackTags, crate::import::ImportError>> {
    let threads = std::thread::available_parallelism().map_or(1, usize::from).min(8);
    if threads <= 1 || paths.len() < 2 {
        return paths.iter().map(|p| crate::import::read_tags(p)).collect();
    }
    let size = paths.len().div_ceil(threads);
    std::thread::scope(|scope| {
        let workers: Vec<_> = paths
            .chunks(size)
            .map(|chunk| {
                let worker = scope.spawn(move || chunk.iter().map(|p| crate::import::read_tags(p)).collect::<Vec<_>>());
                (chunk, worker)
            })
            .collect();
        // One result per path, in order, even if a reader thread panicked:
        // its files are refused rather than dropping out of step.
        workers
            .into_iter()
            .flat_map(|(chunk, worker)| {
                worker.join().unwrap_or_else(|_| {
                    chunk
                        .iter()
                        .map(|p| Err(crate::import::ImportError::Unreadable {
                            path: p.display().to_string(),
                            reason: "reading its tags failed".to_owned(),
                        }))
                        .collect()
                })
            })
            .collect()
    })
}

/// A random id below `limit` that no row of `table` has.
fn unused_id_in(conn: &Connection, rng: &mut Rng, table: &str, limit: u64) -> Result<String> {
    let sql = format!("SELECT COUNT(*) FROM {table} WHERE ID = ?1");
    for _ in 0..ID_ATTEMPTS {
        let candidate = rng.numeric_id(limit);
        let taken: i64 = conn.query_row(&sql, params![candidate], |r| r.get(0))?;
        if taken == 0 {
            return Ok(candidate);
        }
    }
    Err(DbError::WriteRefused(format!(
        "could not find an unused id for {table} in {ID_ATTEMPTS} attempts"
    )))
}

/// The registry's own count of local updates, without the scan of every
/// table that [`next_usn`] makes.
fn registry_counter(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(int_1, 0) FROM agentRegistry WHERE registry_id = 'localUpdateCount'",
        [],
        |r| r.get(0),
    )?)
}

/// What importing one path of a batch came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportOutcome {
    /// A new row, with its id.
    Added(String),
    /// The library already held the file, under this id.
    Existing(String),
    /// Not imported, and why.
    Refused(String),
}

/// What one imported file puts in a `djmdContent` row.
struct NewContent<'a> {
    id: &'a str,
    uuid: &'a str,
    folder: &'a str,
    file_name: &'a str,
    path: &'a Path,
    tags: &'a crate::import::TrackTags,
    artist: &'a Option<String>,
    album: &'a Option<String>,
    genre: &'a Option<String>,
    label: &'a Option<String>,
    key: &'a Option<String>,
    today: &'a str,
    stamp: &'a str,
    master_db: &'a Option<String>,
    device: &'a Option<String>,
    usn: i64,
}

/// The one `INSERT` an imported file goes through, whether it comes alone
/// ([`Writer::import_file`]) or in a batch ([`Writer::import_files`]).
fn insert_content(conn: &Connection, row: &NewContent<'_>) -> Result<()> {
    let NewContent {
        id, uuid, folder, file_name, path, tags, artist, album, genre, label, key,
        today, stamp, master_db, device, usn,
    } = row;
// Every column rekordbox 7 fills on a file it imports itself, as on
// the 645 rows it made on this machine [OBS] — the empty strings are
// empty strings there, not NULLs. Left unset because their values are
// [UNKNOWN]: `rb_file_id` (a counter of unknown ownership),
// `ContentLink` (one constant on 643 of 645 rows), and the three
// `*Updated` counters, which rekordbox sets as it goes.
    conn.execute(
        "INSERT INTO djmdContent
                (ID, FolderPath, FileNameL, FileNameS, Title, Subtitle, ArtistID, AlbumID, GenreID, LabelID, KeyID,
                 Length, BitRate, BitDepth, SampleRate, FileSize, FileType, ReleaseYear, TrackNo, DiscNo,
                 Commnt, Rating, ColorID, DJPlayCount, Analysed, UUID,
                 StockDate, DateCreated, MasterDBID, MasterSongID, DeviceID, HotCueAutoLoad,
                 OrgFolderPath, ModifiedByRBM, DeliveryControl, DeliveryComment, Lyricist, Reserved1, ExtInfo,
                 SamplerTrackInfo, SamplerPlayOffset, SamplerGain, VideoAssociate, LyricStatus, ServiceID,
                 rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced,
                 usn, rb_local_usn, created_at, updated_at)
             VALUES (?1, ?2, ?3, '', ?4, '', ?5, ?6, ?7, ?8, ?24,
                     ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, 0,
                     ?17, 0, 0, 0, NULL, ?18,
                     ?19, ?19, ?20, ?1, ?21, 'on',
                     '', '', '', '', '', '', 'null',
                     0, 0, 0, 0, 0, 0,
                     0, 0, 0, 0,
                     NULL, ?22, ?23, ?23)",
        params![
            id,
            folder,
            file_name,
            tags.title,
            artist,
            album,
            genre,
            label,
            i64::from(tags.duration_sec),
            i64::from(tags.bitrate),
            i64::from(tags.bit_depth),
            i64::from(tags.sample_rate),
            i64::try_from(tags.file_size).unwrap_or(0),
            crate::import::file_type(path),
            i64::from(tags.year),
            i64::from(tags.track_no),
            tags.comment,
            uuid,
            today,
            master_db,
            device,
            usn,
            stamp,
            key
        ],
    )?;
    Ok(())
}

/// Finds a lookup row by name, or makes one, returning its id.
///
/// An empty name is `NO_ID` — the empty string — because rekordbox leaves the
/// reference off rather than pointing at a blank row.
fn intern(
    conn: &Connection,
    table: &str,
    column: &str,
    name: &str,
    rng: &mut Rng,
    stamp: &str,
) -> Result<Option<String>> {
    if name.is_empty() {
        return Ok(None);
    }
    let found: Option<String> = conn
        .query_row(
            &format!("SELECT ID FROM {table} WHERE {column} = ?1 AND rb_local_deleted = 0"),
            params![name],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = found {
        return Ok(Some(id));
    }
    let id = rng.numeric_id(MAX_PLAYLIST_ID);
    let usn = next_usn(conn)?;
    conn.execute(
        &format!(
            "INSERT INTO {table} (ID, {column}, UUID,
                rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced,
                usn, rb_local_usn, created_at, updated_at)
             VALUES (?1, ?2, ?3, 0, 0, 0, 0, NULL, ?4, ?5, ?5)"
        ),
        params![id, name, rng.uuid4(), usn, stamp],
    )?;
    set_counter(conn, usn)?;
    Ok(Some(id))
}

/// Whether a playlist or folder exists and is not deleted.
fn node_exists(conn: &Connection, id: &str) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM djmdPlaylist WHERE ID = ?1 AND rb_local_deleted = 0",
        params![id],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// An intelligent playlist has no membership rows to add to, remove from or
/// reorder: its tracks are its rule. Writing `djmdSongPlaylist` rows under
/// one would leave rows rekordbox never reads.
fn refuse_if_smart(conn: &Connection, playlist: &str) -> Result<()> {
    let attribute: Option<i64> = conn
        .query_row(
            "SELECT Attribute FROM djmdPlaylist WHERE ID = ?1 AND rb_local_deleted = 0",
            params![playlist],
            |r| r.get(0),
        )
        .optional()?;
    if attribute == Some(ATTRIBUTE_SMART) {
        return Err(DbError::WriteRefused(
            "an intelligent playlist's tracks are its rule; they cannot be edited by hand".into(),
        ));
    }
    Ok(())
}

/// Whether `candidate` sits somewhere under `ancestor`.
fn is_descendant(conn: &Connection, candidate: &str, ancestor: &str) -> Result<bool> {
    let mut at = candidate.to_owned();
    // The tree is shallow, but a corrupt parent chain could loop; the bound
    // makes that terminate instead of hanging.
    for _ in 0..256 {
        if at == ROOT {
            return Ok(false);
        }
        let parent: Option<String> = conn
            .query_row("SELECT ParentID FROM djmdPlaylist WHERE ID = ?1", params![at], |r| r.get(0))
            .optional()?;
        match parent {
            Some(p) if p == ancestor => return Ok(true),
            Some(p) => at = p,
            None => return Ok(false),
        }
    }
    Err(DbError::WriteRefused("cyclic or excessively deep playlist tree".into()))
}

/// The next local USN.
///
/// `max(the registry counter, the largest USN in use) + 1`. Taking the larger
/// of the two matters: the counter has been observed lagging the table maximum,
/// and reusing a USN makes rekordbox's sync skip the row.
/// The path with `.` and `..` components resolved lexically — no symlink is
/// followed, so what the person chose is what is stored.
fn normalized(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// The `djmdKey` row for a key a file's tag names, made when the library has
/// none of that name; `None` for no key.
///
/// rekordbox does make one for a tag's key: on the reference library a `2A`
/// row was created 4 ms before the imported track that points at it [OBS].
/// Its `Seq` rule is unknown, so the row is made the way
/// [`Writer::ensure_detected_key`] makes one for an analysed key, without a
/// `Seq`. An existing name resolves as an analysis does, by [`key_id_for`].
/// The name is matched as the tag wrote it [UNKNOWN: whether rekordbox
/// normalises it, say trims spaces, when it looks the row up].
fn tag_key_id(conn: &Connection, name: &str, rng: &mut Rng, stamp: &str) -> Result<Option<String>> {
    if name.is_empty() {
        return Ok(None);
    }
    let existing: Option<String> = conn
        .query_row(
            "SELECT k.ID FROM djmdKey k
             LEFT JOIN djmdContent c ON c.KeyID = k.ID AND c.rb_local_deleted = 0
             WHERE k.ScaleName = ?1 AND k.rb_local_deleted = 0
             GROUP BY k.ID ORDER BY COUNT(c.ID) DESC, k.ID LIMIT 1",
            params![name],
            |r| r.get(0),
        )
        .optional()?;
    match existing {
        Some(id) => Ok(Some(id)),
        None => intern(conn, "djmdKey", "ScaleName", name, rng, stamp),
    }
}

/// The `djmdKey` row for a key name: where two rows share a name, the one
/// rekordbox's own analyses point at, which is the one on the most tracks.
fn key_id_for(conn: &Connection, name: &str) -> Result<String> {
    conn.query_row(
        "SELECT k.ID FROM djmdKey k
         LEFT JOIN djmdContent c ON c.KeyID = k.ID AND c.rb_local_deleted = 0
         WHERE k.ScaleName = ?1 AND k.rb_local_deleted = 0
         GROUP BY k.ID ORDER BY COUNT(c.ID) DESC, k.ID LIMIT 1",
        params![name],
        |r| r.get(0),
    )
    .map_err(|_| DbError::WriteRefused(format!("no djmdKey row is named {name:?}")))
}

/// Writes an image where the library keeps one: the file as given, and
/// beside it the two sizes rekordbox keeps and a stick takes, `artwork_m.jpg`
/// at 240×240 and `artwork_s.jpg` at 80×80 [OBS: every folder under
/// `share/PIONEER/Artwork` holds the three]. A picture that is not square
/// is cut to its middle square first [ASSUME: which of a crop and a stretch
/// rekordbox does has not been measured]. An image that does not decode
/// still lands as given, with no small sizes: the track has its picture,
/// the stick gets the big one to scale.
fn write_artwork_sizes(target: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = target.parent() {
        rbl_core::durable::create_dir_all(dir)?;
    }
    rbl_core::durable::write(target, bytes)?;
    let Ok(decoded) = image::load_from_memory(bytes) else {
        return Ok(());
    };
    let side = decoded.width().min(decoded.height());
    let square = decoded.crop_imm((decoded.width() - side) / 2, (decoded.height() - side) / 2, side, side);
    for (name, size) in [("artwork_m.jpg", 240_u32), ("artwork_s.jpg", 80_u32)] {
        let small = square.resize_exact(size, size, image::imageops::FilterType::Lanczos3).to_rgb8();
        let path = target.with_file_name(name);
        let mut out = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90);
        if encoder.encode_image(&small).is_ok() {
            rbl_core::durable::write(&path, &out)?;
        }
    }
    Ok(())
}

pub(crate) fn next_usn(conn: &Connection) -> Result<i64> {
    let counter: i64 = conn
        .query_row(
            "SELECT COALESCE(int_1, 0) FROM agentRegistry WHERE registry_id = 'localUpdateCount'",
            [],
            |r| r.get(0),
        )
        ?;
    let mut highest = counter;
    for table in USN_TABLES {
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |r| r.get(0),
        )?;
        if exists == 0 {
            continue;
        }
        let max: i64 = conn
            .query_row(&format!("SELECT COALESCE(MAX(rb_local_usn), 0) FROM {table}"), [], |r| {
                r.get(0)
            })
            ?;
        highest = highest.max(max);
    }
    highest.checked_add(1).ok_or_else(|| DbError::WriteRefused("local update counter overflow".into()))
}

/// Writes the registry counter in the same transaction as its rows.
/// Both commit or both roll back, including after a crash.
pub(crate) fn set_counter(conn: &Connection, usn: i64) -> Result<()> {
    let rows = conn.execute(
        "UPDATE agentRegistry SET int_1 = ?1, updated_at = ?2 WHERE registry_id = 'localUpdateCount'",
        params![usn, time::now()],
    )?;
    if rows != 1 { return Err(DbError::WriteRefused("missing or duplicate local update counter".into())); }
    Ok(())
}

/// Renumbers a playlist's `TrackNo` to 1..N in its current order.
fn renumber(conn: &Connection, playlist: &str, stamp: &str) -> Result<i64> {
    let start = next_usn(conn)?;
    let mut next = start;
    renumber_from(conn, playlist, stamp, &mut next)?;
    Ok(if next > start { next - 1 } else { start })
}

/// [`renumber`] with the USNs handed out from `next`, which is left one past
/// the last used. A caller renumbering several playlists reads the counter
/// once and passes it through, instead of finding it again for every row.
fn renumber_from(conn: &Connection, playlist: &str, stamp: &str, next: &mut i64) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT ID, TrackNo FROM djmdSongPlaylist WHERE PlaylistID = ?1 AND rb_local_deleted = 0
         ORDER BY TrackNo",
    )?;
    let rows: Vec<(String, i64)> = stmt
        .query_map(params![playlist], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    drop(stmt);

    for (index, (id, current)) in rows.iter().enumerate() {
        let wanted = i64::try_from(index + 1).unwrap_or(i64::MAX);
        // Only touch rows whose number actually moves: an untouched row should
        // not get a new USN and look changed to the sync.
        if *current == wanted {
            continue;
        }
        conn.execute(
            "UPDATE djmdSongPlaylist SET TrackNo = ?1, rb_local_usn = ?2, updated_at = ?3
             WHERE ID = ?4",
            params![wanted, *next, stamp, id],
        )?;
        *next = next.checked_add(1).ok_or_else(|| DbError::WriteRefused("local update counter overflow".into()))?;
    }
    Ok(())
}

/// `master.db` plus `-wal` gives `master.db-wal`, which is how SQLite names
/// them — an extension, not a suffix on the stem.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// Whether a track is in the library.
fn content_exists(conn: &Connection, id: &str) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM djmdContent WHERE ID = ?1 AND rb_local_deleted = 0",
        params![id],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// A history folder or session by name under a parent, made when missing
/// with `id`, appended by `Seq`.
#[allow(clippy::too_many_arguments, reason = "one row's columns, given rather than guessed")]
fn history_node(
    conn: &Connection,
    name: &str,
    parent: &str,
    attribute: i64,
    id: &str,
    uuid: &str,
    created: &str,
    stamp: &str,
) -> Result<String> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT ID FROM djmdHistory
             WHERE Name = ?1 AND ParentID = ?2 AND Attribute = ?3 AND rb_local_deleted = 0
             ORDER BY Seq LIMIT 1",
            params![name, parent, attribute],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(found) = existing {
        return Ok(found);
    }
    let seq: i64 = conn.query_row(
        "SELECT COALESCE(MAX(Seq), 0) + 1 FROM djmdHistory WHERE ParentID = ?1 AND rb_local_deleted = 0",
        params![parent],
        |r| r.get(0),
    )?;
    let usn = next_usn(conn)?;
    conn.execute(
        "INSERT INTO djmdHistory
            (ID, Seq, Name, Attribute, ParentID, DateCreated, UUID,
             rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced,
             usn, rb_local_usn, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 0, 0, 0, NULL, ?8, ?9, ?9)",
        params![id, seq, name, attribute, parent, created, uuid, usn, stamp],
    )?;
    Ok(id.to_owned())
}

/// Today's month folder in the history tree — named by the month's number,
/// inside a year folder named by the year — made when missing. `created` is
/// the local `YYYY-MM-DD HH:MM:SS` the new rows are dated with, and the
/// day the folders are for.
fn month_folder(conn: &Connection, ids: [&str; 2], uuids: [&str; 2], created: &str, stamp: &str) -> Result<String> {
    let year = created.get(..4).unwrap_or("1970");
    let month = created.get(5..7).and_then(|m| m.parse::<u32>().ok()).unwrap_or(1).to_string();
    let year_id = history_node(conn, year, ROOT, 1, ids[0], uuids[0], created, stamp)?;
    history_node(conn, &month, &year_id, 1, ids[1], uuids[1], created, stamp)
}

/// A play on the end of a session, and one more DJ play for the track.
/// `ids` are the new `djmdSongHistory` row's `ID` and `UUID`.
fn append_play(conn: &Connection, session: &str, content: &str, ids: &[String; 2], stamp: &str) -> Result<Changed> {
    let track_no: i64 = conn.query_row(
        "SELECT COALESCE(MAX(TrackNo), 0) + 1 FROM djmdSongHistory
         WHERE HistoryID = ?1 AND rb_local_deleted = 0",
        params![session],
        |r| r.get(0),
    )?;
    let usn = next_usn(conn)?;
    conn.execute(
        "INSERT INTO djmdSongHistory
            (ID, HistoryID, ContentID, TrackNo, UUID,
             rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced,
             usn, rb_local_usn, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, 0, 0, NULL, ?6, ?7, ?7)",
        params![ids[0], session, content, track_no, ids[1], usn, stamp],
    )?;
    let usn = next_usn(conn)?;
    let rows = conn.execute(
        "UPDATE djmdContent SET DJPlayCount = COALESCE(DJPlayCount, 0) + 1,
            rb_local_usn = ?1, updated_at = ?2
         WHERE ID = ?3 AND rb_local_deleted = 0",
        params![usn, stamp, content],
    )?;
    set_counter(conn, usn)?;
    Ok(Changed { rows, usn })
}

/// The number rekordbox puts after a history name already in use
/// (`getSubNumber`): 0 when no name starts with `base`, 1 when only the bare
/// name does, and one past the highest `(n)` otherwise.
fn name_suffix(base: &str, names: &[String]) -> u32 {
    let mut next = 0;
    for rest in names.iter().filter_map(|name| name.strip_prefix(base)) {
        // What follows the `(`, when anything does; the number is its
        // leading digits, 0 when there are none, as `String::getIntValue`
        // reads it.
        match rest.find('(').and_then(|open| rest.get(open + 1..)).filter(|after| !after.is_empty()) {
            Some(after) => {
                let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
                let n = digits.parse::<u32>().unwrap_or(0);
                if next <= n {
                    next = n.saturating_add(1);
                }
            }
            None if next == 0 => next = 1,
            None => {}
        }
    }
    next
}

/// `renumber`, for a history session.
fn renumber_history(conn: &Connection, history: &str, stamp: &str) -> Result<i64> {
    let mut stmt = conn.prepare(
        "SELECT ID, TrackNo FROM djmdSongHistory WHERE HistoryID = ?1 AND rb_local_deleted = 0
         ORDER BY TrackNo",
    )?;
    let rows: Vec<(String, i64)> = stmt
        .query_map(params![history], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    drop(stmt);
    let mut usn = next_usn(conn)?;
    for (index, (id, current)) in rows.iter().enumerate() {
        let wanted = i64::try_from(index + 1).unwrap_or(i64::MAX);
        if *current == wanted {
            continue;
        }
        usn = next_usn(conn)?;
        conn.execute(
            "UPDATE djmdSongHistory SET TrackNo = ?1, rb_local_usn = ?2, updated_at = ?3 WHERE ID = ?4",
            params![wanted, usn, stamp, id],
        )?;
    }
    Ok(usn)
}

/// The grid at a new tempo: the same beats, numbered as they were, spaced
/// from the first at the new interval.
fn retime(beats: &[rbl_anlz::Beat], bpm_x100: u32) -> Vec<rbl_anlz::Beat> {
    let first = beats.first().map_or(0.0, |b| f64::from(b.time_ms));
    let interval_ms = 6_000_000.0 / f64::from(bpm_x100.max(1));
    let tempo = u16::try_from(bpm_x100).unwrap_or(u16::MAX);
    beats
        .iter()
        .enumerate()
        .map(|(i, beat)| {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss, reason = "milliseconds of a track, far inside u32")]
            let time_ms = (first + i as f64 * interval_ms).round().max(0.0) as u32;
            rbl_anlz::Beat { beat_number: beat.beat_number, tempo_x100: tempo, time_ms }
        })
        .collect()
}

/// The backups in a directory, oldest first: the name carries the
/// timestamp, so sorting by name sorts by age.
#[must_use]
pub fn backups_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut backups: Vec<PathBuf> = entries
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("master-"))
                && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("db"))
        })
        .collect();
    backups.sort();
    backups
}

/// Puts a backup back as the library.
///
/// The backup is validated and normalized to a standalone database, then
/// atomically replaces the checkpointed live file. Refused
/// while rekordbox holds the installed library, and for a file that is not
/// one of this app's backups. The caller reopens every handle it holds:
/// one on the old inode would answer with the old rows for ever.
pub fn restore_backup(location: &crate::LibraryLocation, backup: &Path) -> Result<()> {
    if let Some(reason) = crate::write_refusal_reason(location.is_real_install,
        crate::test_mode(), is_rekordbox_running()) {
        return Err(DbError::WriteRefused(reason.into()));
    }
    let is_ours = backup
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("master-") && Path::new(n).extension().is_some_and(|e| e.eq_ignore_ascii_case("db")));
    if !is_ours || !backup.is_file() {
        return Err(DbError::WriteRefused(format!("{} is not a backup of the library", backup.display())));
    }
    let live = &location.master_db;
    let parent = live.parent().ok_or_else(|| DbError::WriteRefused("invalid library path".into()))?;
    let staging = tempfile::tempdir_in(parent)?;
    let staged = staging.path().join("master.db");
    std::fs::copy(backup, &staged)?;
    let wal = with_suffix(backup, "-wal");
    if wal.exists() { std::fs::copy(wal, with_suffix(&staged, "-wal"))?; }
    let mut staged_location = location.clone();
    staged_location.master_db.clone_from(&staged);
    staged_location.is_real_install = false;
    {
        let db = Library::open(staged_location, OpenMode::ReadWrite)?;
        let check: String = db.connection().query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        if check != "ok" { return Err(DbError::WriteRefused(format!("invalid backup: {check}"))); }
        db.connection().pragma_update(None, "journal_mode", "DELETE")?;
    }
    // Checkpoint the OLD library first. If interrupted here, SQLite recovers
    // the old library. Once closed, one atomic rename publishes the new one.
    {
        let db = Library::open(location.clone(), OpenMode::ReadWrite)?;
        db.connection().pragma_update(None, "journal_mode", "DELETE")?;
    }
    rbl_core::durable::replace(&staged, live)?;
    tracing::info!(path = %backup.display(), "restored the library from a backup");
    Ok(())
}

/// Keeps the newest `keep` backups and removes the rest.
fn prune_backups(dir: &Path, keep: usize) {
    let backups = backups_in(dir);
    let excess = backups.len().saturating_sub(keep);
    for path in backups.into_iter().take(excess) {
        // The sidecars go with it, or the directory fills with orphans.
        for suffix in ["-wal", "-shm"] {
            drop(std::fs::remove_file(with_suffix(&path, suffix)));
        }
        drop(std::fs::remove_file(path));
    }
}
