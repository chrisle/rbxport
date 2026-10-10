//! A stick's own playlists, browsed and edited where they are: rekordbox's
//! Devices tree.
//!
//! rekordbox lists each library on a stick on its own under the device —
//! `Device Library` (`export.pdb`) and `OneLibrary` (`exportLibrary.db`) —
//! and an edit made there changes that library alone [DOC: rekordbox FAQ
//! "Device Library Plus": "changes are only reflected on the playlists in
//! the library for which the controls had been performed"; static,
//! rekordbox 7.2.11 macOS, `BrowseProperty::isDeviceLibraryRoot` and
//! `isDeviceLibraryPlusRoot` both sit at tree depth 3 and are told apart by
//! `DatabaseIF::isDeviceLibraryPlus(handle)`]. So this reads and writes one
//! [`Format`] at a time, and never copies an edit across.
//!
//! The edits are the ones rekordbox's menus offer over a device's playlists
//! [static, rekordbox 7.2.11 macOS, `BrowsePopupMenuManager::
//! showTreeViewPopupMenu` @0x1000ee370 device case @0x1000eec84 and
//! `showListViewPopupMenu` @0x1000ea9ec device case @0x1000ebda8]: Create
//! New Playlist and Create New Folder over the Playlists heading or a
//! folder, Delete over a playlist or folder, a rename by editing the name
//! (`FolderListTreeViewItem::isEditableItem` @0x1016c7868 allows it for a
//! device's lists), and over a device's tracks "Add To Playlist" naming that
//! device's playlists and, inside one, "Remove from Playlist".
//!
//! A deleted playlist leaves its tracks on the stick: rekordbox's "Delete
//! playlist tracks from device if playlist is deleted" preference is off by
//! default [static: `_kDeviceDeletePlaylistTracksDefaultValue` @0x102dfcde6
//! is 0], and this has no such preference.
//!
//! Every write is staged and published through the same journal as an
//! export, so an interrupted edit is finished or undone on the next read
//! rather than leaving half a database.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::{ExportError, Result};

/// One of the two libraries a stick can hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Format {
    /// `export.pdb`, the library every player reads.
    DeviceLibrary,
    /// `exportLibrary.db`, Device Library Plus.
    OneLibrary,
}

impl Format {
    /// The database file, under the stick's `PIONEER/rekordbox/`.
    #[must_use]
    pub const fn file_name(self) -> &'static str {
        match self {
            Self::DeviceLibrary => "export.pdb",
            Self::OneLibrary => "exportLibrary.db",
        }
    }
}

/// The libraries `root` holds, Device Library first, as rekordbox lists them.
#[must_use]
pub fn formats(root: &Path) -> Vec<Format> {
    let Ok(name) = crate::export_root_name(root) else { return Vec::new() };
    let dir = root.join(name).join("rekordbox");
    [Format::DeviceLibrary, Format::OneLibrary].into_iter().filter(|f| dir.join(f.file_name()).is_file()).collect()
}

/// A playlist or folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub id: u32,
    /// 0 for the top level.
    pub parent: u32,
    pub name: String,
    pub folder: bool,
    /// The order among siblings, as the database stores it.
    pub sequence: u32,
    /// Track ids in playlist order; empty for a folder.
    pub tracks: Vec<u32>,
}

/// A track as the library on the stick describes it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Track {
    pub id: u32,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub genre: String,
    pub label: String,
    pub key: String,
    pub comment: String,
    pub date_added: String,
    pub bpm_x100: u32,
    pub duration_sec: u32,
    /// Stars, 0 to 5.
    pub rating: u8,
    pub color: u8,
    /// Volume-relative, with a leading slash.
    pub path: String,
}

/// One library on the stick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Library {
    pub format: Format,
    /// Playlists and folders, depth first, siblings in their order.
    pub nodes: Vec<Node>,
    /// Every track, by id.
    pub tracks: Vec<Track>,
}

impl Library {
    #[must_use]
    pub fn node(&self, id: u32) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    #[must_use]
    pub fn track(&self, id: u32) -> Option<&Track> {
        self.tracks.binary_search_by_key(&id, |t| t.id).ok().and_then(|at| self.tracks.get(at))
    }

    /// How deep a node sits: 0 for the top level.
    #[must_use]
    pub fn depth(&self, id: u32) -> usize {
        let mut depth = 0;
        let mut at = self.node(id).map_or(0, |n| n.parent);
        while at != 0 && depth < self.nodes.len() {
            depth += 1;
            at = self.node(at).map_or(0, |n| n.parent);
        }
        depth
    }
}

/// Puts nodes in tree order: depth first from the top level, siblings by
/// their sequence, then id. Nodes under a parent that is missing or not a
/// folder are listed at the top level rather than dropped.
fn tree_order(mut nodes: Vec<Node>) -> Vec<Node> {
    let folders: BTreeSet<u32> = nodes.iter().filter(|n| n.folder).map(|n| n.id).collect();
    for node in &mut nodes {
        if node.parent != 0 && (!folders.contains(&node.parent) || node.parent == node.id) {
            node.parent = 0;
        }
    }
    let mut children: BTreeMap<u32, Vec<Node>> = BTreeMap::new();
    for node in nodes {
        children.entry(node.parent).or_default().push(node);
    }
    for siblings in children.values_mut() {
        siblings.sort_by_key(|n| (n.sequence, n.id));
    }
    let mut out = Vec::new();
    let mut stack: Vec<Node> = children.remove(&0).unwrap_or_default().into_iter().rev().collect();
    while let Some(node) = stack.pop() {
        if let Some(under) = children.remove(&node.id) {
            stack.extend(under.into_iter().rev());
        }
        out.push(node);
    }
    // Whatever is left sits in a cycle of folders; list it rather than hide it.
    out.extend(children.into_values().flatten());
    out
}

fn db_dir(root: &Path) -> Result<PathBuf> {
    Ok(root.join(crate::export_root_name(root)?).join("rekordbox"))
}

fn missing(format: Format) -> ExportError {
    ExportError::Conflict(format!("This device has no {file}.", file = format.file_name()))
}

/// Reads one library from the stick at `root`.
pub fn read(root: &Path, format: Format) -> Result<Library> {
    crate::recover(root)?;
    read_unlocked(root, format)
}

fn read_unlocked(root: &Path, format: Format) -> Result<Library> {
    let path = db_dir(root)?.join(format.file_name());
    if !path.is_file() {
        return Err(missing(format));
    }
    let (nodes, mut tracks) = match format {
        Format::DeviceLibrary => read_pdb(&std::fs::read(&path)?)?,
        Format::OneLibrary => read_one(&path)?,
    };
    tracks.sort_by_key(|t| t.id);
    Ok(Library { format, nodes: tree_order(nodes), tracks })
}

fn unreadable(e: impl std::fmt::Display) -> ExportError {
    ExportError::Conflict(format!("Cannot read the device library: {e}"))
}

fn read_pdb(bytes: &[u8]) -> Result<(Vec<Node>, Vec<Track>)> {
    use rbl_pdb::PageType;
    let pdb = rbl_pdb::Pdb::parse(bytes).map_err(unreadable)?;
    let entries = pdb.table(PageType::PlaylistEntries).map(|t| pdb.playlist_entries(t)).unwrap_or_default();
    let mut members: BTreeMap<u32, Vec<(u32, u32)>> = BTreeMap::new();
    for entry in entries {
        members.entry(entry.playlist_id).or_default().push((entry.entry_index, entry.track_id));
    }
    let nodes = pdb
        .table(PageType::PlaylistTree)
        .map(|t| pdb.playlist_nodes(t))
        .unwrap_or_default()
        .into_iter()
        .map(|n| {
            let mut tracks = members.remove(&n.id).unwrap_or_default();
            tracks.sort_by_key(|&(index, _)| index);
            Node {
                id: n.id,
                parent: n.parent_id,
                name: n.name,
                folder: n.is_folder,
                sequence: n.sort_order,
                tracks: if n.is_folder { Vec::new() } else { tracks.into_iter().map(|(_, track)| track).collect() },
            }
        })
        .collect();
    let names = |kind| -> BTreeMap<u32, String> {
        pdb.table(kind).map(|t| pdb.named_rows(t).into_iter().map(|n| (n.id, n.name)).collect()).unwrap_or_default()
    };
    let (artists, albums, genres, labels, keys) =
        (names(PageType::Artists), names(PageType::Albums), names(PageType::Genres), names(PageType::Labels), names(PageType::Keys));
    let name = |map: &BTreeMap<u32, String>, id: u32| map.get(&id).cloned().unwrap_or_default();
    let tracks = pdb
        .table(PageType::Tracks)
        .map(|t| pdb.track_rows(t))
        .unwrap_or_default()
        .into_iter()
        .map(|t| Track {
            id: t.id,
            artist: name(&artists, t.artist_id),
            album: name(&albums, t.album_id),
            genre: name(&genres, t.genre_id),
            label: name(&labels, t.label_id),
            key: name(&keys, t.key_id),
            title: t.title,
            comment: t.comment,
            date_added: t.date_added,
            bpm_x100: t.tempo_x100,
            duration_sec: u32::from(t.duration_sec),
            rating: t.rating.min(5),
            color: t.color_id,
            path: t.file_path,
        })
        .collect();
    Ok((nodes, tracks))
}

fn one_error(e: impl std::fmt::Display) -> ExportError {
    ExportError::OneLibrary(e.to_string())
}

fn read_one(path: &Path) -> Result<(Vec<Node>, Vec<Track>)> {
    let db = rbl_onelibrary::ExportLibrary::open_read_only(path).map_err(one_error)?;
    let (rows, mut contents) = rbl_onelibrary::playlists::read(db.connection()).map_err(one_error)?;
    let id = |v: i64| u32::try_from(v).unwrap_or(0);
    let nodes = rows
        .into_iter()
        .map(|row| {
            let folder = row.attribute == rbl_onelibrary::playlists::FOLDER;
            let tracks = contents.remove(&row.id).unwrap_or_default();
            Node {
                id: id(row.id),
                parent: id(row.parent),
                name: row.name,
                folder,
                sequence: id(row.sequence),
                tracks: if folder { Vec::new() } else { tracks.into_iter().map(id).collect() },
            }
        })
        .collect();
    let mut statement = db
        .connection()
        .prepare(
            "SELECT c.content_id, COALESCE(c.title,''), COALESCE(a.name,''), COALESCE(al.name,''),
                    COALESCE(g.name,''), COALESCE(l.name,''), COALESCE(k.name,''), COALESCE(c.djComment,''),
                    COALESCE(c.dateAdded,''), COALESCE(c.bpmx100,0), COALESCE(c.length,0), COALESCE(c.rating,0),
                    COALESCE(c.color_id,0), COALESCE(c.path,'')
             FROM content c
             LEFT JOIN artist a ON a.artist_id = c.artist_id_artist
             LEFT JOIN album al ON al.album_id = c.album_id
             LEFT JOIN genre g ON g.genre_id = c.genre_id
             LEFT JOIN label l ON l.label_id = c.label_id
             LEFT JOIN key k ON k.key_id = c.key_id",
        )
        .map_err(one_error)?;
    let tracks = statement
        .query_map([], |r| {
            Ok(Track {
                id: r.get(0)?,
                title: r.get(1)?,
                artist: r.get(2)?,
                album: r.get(3)?,
                genre: r.get(4)?,
                label: r.get(5)?,
                key: r.get(6)?,
                comment: r.get(7)?,
                date_added: r.get(8)?,
                bpm_x100: r.get(9)?,
                duration_sec: r.get(10)?,
                rating: crate::snapshot::onelibrary_stars(r.get(11)?),
                color: r.get(12)?,
                path: r.get(13)?,
            })
        })
        .map_err(one_error)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(one_error)?;
    Ok((nodes, tracks))
}

/// One change to a library's playlists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// A playlist, or a folder, at the top of `parent` (0 for the top level).
    Create { parent: u32, name: String, folder: bool },
    Rename { id: u32, name: String },
    /// A playlist, or a folder and everything under it. The tracks stay on
    /// the stick.
    Delete { id: u32 },
    /// Tracks of this library appended to a playlist; one already in it is
    /// not added twice.
    Add { playlist: u32, tracks: Vec<u32> },
    /// Every entry of these tracks taken out of a playlist. The interface
    /// names a stick's rows by their file, so two entries of one track are
    /// one row id there and are selected, and taken out, together.
    Remove { playlist: u32, tracks: Vec<u32> },
}

/// What an edit did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Applied {
    /// The playlist or folder the edit was about; the new one for a create.
    pub id: u32,
    /// Playlists, folders or entries added, renamed or removed. Zero means
    /// nothing was written.
    pub changed: usize,
}

fn refused(message: &str) -> ExportError {
    ExportError::Conflict(message.to_owned())
}

/// A name a playlist can take: trimmed, and not empty.
fn checked_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(refused("A playlist needs a name."));
    }
    Ok(name.to_owned())
}

/// The library's playlists after `edit`, and what it did. Nothing is read or
/// written here.
///
/// The numbers are rekordbox's own [OBS rekordbox 7.2.14 for Windows, Winrig
/// 2026-10-08, both libraries of a fixture stick read back after each edit;
/// evidence in `parity/issue-186/rig-stick-*`]:
/// - a new playlist or folder takes the next id after the largest and
///   sequence 0, at the top of its parent, and every sibling's sequence goes
///   up by one, gaps and all;
/// - a delete takes the node and everything under it, and numbers what is
///   left in its parent from 0 with no gaps;
/// - a rename changes the name alone;
/// - added tracks go on the end; entries are numbered from 1 with no gaps
///   after a removal.
///
/// rekordbox asks whether to add a track a playlist already holds; this
/// takes its "Skip", as the collection's own Add To Playlist here does.
fn plan(nodes: &[Node], tracks: &BTreeSet<u32>, edit: &Edit) -> Result<(Vec<Node>, Applied)> {
    let find = |id: u32| nodes.iter().position(|n| n.id == id);
    let mut after = nodes.to_vec();
    let applied = match edit {
        Edit::Create { parent, name, folder } => {
            if *parent != 0 && !find(*parent).is_some_and(|at| nodes[at].folder) {
                return Err(refused("That folder is no longer on the device."));
            }
            let name = checked_name(name)?;
            let id = nodes.iter().map(|n| n.id).max().unwrap_or(0).checked_add(1).ok_or_else(|| ExportError::Conflict("The device has no room for another playlist.".to_owned()))?;
            for sibling in after.iter_mut().filter(|n| n.parent == *parent) {
                sibling.sequence = sibling.sequence.saturating_add(1);
            }
            after.push(Node { id, parent: *parent, name, folder: *folder, sequence: 0, tracks: Vec::new() });
            Applied { id, changed: 1 }
        }
        Edit::Rename { id, name } => {
            let at = find(*id).ok_or_else(|| refused("That playlist is no longer on the device."))?;
            let name = checked_name(name)?;
            if after[at].name == name {
                return Ok((after, Applied { id: *id, changed: 0 }));
            }
            after[at].name = name;
            Applied { id: *id, changed: 1 }
        }
        Edit::Delete { id } => {
            let at = find(*id).ok_or_else(|| refused("That playlist is no longer on the device."))?;
            let parent = nodes[at].parent;
            let mut gone: BTreeSet<u32> = BTreeSet::from([*id]);
            loop {
                let more: Vec<u32> = nodes.iter().filter(|n| gone.contains(&n.parent) && !gone.contains(&n.id)).map(|n| n.id).collect();
                if more.is_empty() {
                    break;
                }
                gone.extend(more);
            }
            after.retain(|n| !gone.contains(&n.id));
            let mut left: Vec<&mut Node> = after.iter_mut().filter(|n| n.parent == parent).collect();
            left.sort_by_key(|n| (n.sequence, n.id));
            for (position, node) in left.into_iter().enumerate() {
                node.sequence = u32::try_from(position).unwrap_or(u32::MAX);
            }
            Applied { id: *id, changed: gone.len() }
        }
        Edit::Add { playlist, tracks: added } | Edit::Remove { playlist, tracks: added } => {
            let at = find(*playlist).ok_or_else(|| refused("That playlist is no longer on the device."))?;
            if after[at].folder {
                return Err(refused("A folder holds playlists, not tracks."));
            }
            if let Some(stranger) = added.iter().find(|t| !tracks.contains(t)) {
                return Err(ExportError::Conflict(format!("Track {stranger} is not in this library on the device.")));
            }
            let list = &mut after[at].tracks;
            let before = list.len();
            if matches!(edit, Edit::Add { .. }) {
                for track in added {
                    if !list.contains(track) {
                        list.push(*track);
                    }
                }
                Applied { id: *playlist, changed: list.len() - before }
            } else {
                let removing: BTreeSet<u32> = added.iter().copied().collect();
                list.retain(|t| !removing.contains(t));
                Applied { id: *playlist, changed: before - list.len() }
            }
        }
    };
    Ok((after, applied))
}

/// The same publication journal an export uses, so either one finishes the
/// other's interrupted write before going on.
const PUBLICATION: &str = ".rbxport-publication";

/// Applies `edit` to one library on the stick at `root` and publishes the
/// changed database file. The other library, the audio and every other file
/// are left alone.
pub fn apply(root: &Path, format: Format, edit: &Edit) -> Result<Applied> {
    crate::recover(root)?;
    let dir = db_dir(root)?;
    let path = dir.join(format.file_name());
    if !path.is_file() {
        return Err(missing(format));
    }
    // Held to the end: no export or other edit of ours starts meanwhile.
    let publication = rbl_core::durable::Publication::new(root, PUBLICATION)?;
    let stamp = (file_stamp(&path)?, file_stamp(&dir.join(format!("{}-wal", format.file_name())))?);
    let current = read_unlocked(root, format)?;
    let tracks: BTreeSet<u32> = current.tracks.iter().map(|t| t.id).collect();
    let (after, applied) = plan(&current.nodes, &tracks, edit)?;
    if applied.changed == 0 {
        return Ok(applied);
    }
    let relative = dir.strip_prefix(root).map_err(std::io::Error::other)?.to_path_buf();
    let staged_dir = publication.stage().join(&relative);
    rbl_core::durable::create_dir_all(&staged_dir)?;
    let file = relative.join(format.file_name());
    let files = match format {
        Format::DeviceLibrary => {
            let bytes = std::fs::read(&path)?;
            let next = rewrite_pdb(&bytes, &current.nodes, &after)?;
            rbl_core::durable::write(&staged_dir.join(format.file_name()), &next)?;
            vec![file]
        }
        Format::OneLibrary => {
            let staged = staged_dir.join(format.file_name());
            std::fs::copy(&path, &staged)?;
            // The write-ahead log holds committed rows rekordbox has not
            // checkpointed yet; SQLite rebuilds its index from it, so the
            // `-shm` beside it is not copied.
            let wal = dir.join(format!("{}-wal", format.file_name()));
            if wal.is_file() {
                std::fs::copy(&wal, staged_dir.join(format!("{}-wal", format.file_name())))?;
            }
            rbl_onelibrary::playlists::apply(&staged, &one_change(&current.nodes, &after)).map_err(one_error)?;
            // The rollback journal leaves nothing beside the file; a stale
            // `-wal` or `-shm` on the stick goes with the publication.
            for suffix in ["-wal", "-shm"] {
                let _ = std::fs::remove_file(staged_dir.join(format!("{}{suffix}", format.file_name())));
            }
            let (written, _) = read_one(&staged)?;
            check_written(&written, &after)?;
            vec![
                relative.join(format!("{}-wal", format.file_name())),
                relative.join(format!("{}-shm", format.file_name())),
                file,
            ]
        }
    };
    // rekordbox, or a player, may have written the file while this staged.
    if (file_stamp(&path)?, file_stamp(&dir.join(format!("{}-wal", format.file_name())))?) != stamp {
        return Err(refused("The device library changed while it was being edited. Nothing was changed; try again."));
    }
    publication.commit(&files)?;
    Ok(applied)
}

/// Size and modification time, or nothing for a file that is not there.
fn file_stamp(path: &Path) -> Result<Option<(u64, Option<std::time::SystemTime>)>> {
    match std::fs::metadata(path) {
        Ok(meta) => Ok(Some((meta.len(), meta.modified().ok()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// The written library must hold exactly the planned playlists.
fn check_written(written: &[Node], planned: &[Node]) -> Result<()> {
    let key = |nodes: &[Node]| -> BTreeMap<u32, (u32, String, bool, u32, Vec<u32>)> {
        nodes.iter().map(|n| (n.id, (n.parent, n.name.clone(), n.folder, n.sequence, n.tracks.clone()))).collect()
    };
    if key(written) != key(planned) {
        return Err(refused("The edited device library did not read back as planned. Nothing was changed."));
    }
    Ok(())
}

/// The rows of `exportLibrary.db` an edit changes.
fn one_change(before: &[Node], after: &[Node]) -> rbl_onelibrary::playlists::Change {
    use rbl_onelibrary::playlists::{Change, PlaylistRow, FOLDER};
    let old: BTreeMap<u32, &Node> = before.iter().map(|n| (n.id, n)).collect();
    let new: BTreeMap<u32, &Node> = after.iter().map(|n| (n.id, n)).collect();
    let mut change = Change::default();
    for (id, node) in &new {
        match old.get(id) {
            None => {
                change.create.push(PlaylistRow {
                    id: i64::from(*id),
                    parent: i64::from(node.parent),
                    name: node.name.clone(),
                    attribute: if node.folder { FOLDER } else { 0 },
                    sequence: i64::from(node.sequence),
                });
                if !node.tracks.is_empty() {
                    change.contents.push((i64::from(*id), node.tracks.iter().map(|&t| i64::from(t)).collect()));
                }
            }
            Some(was) => {
                if was.name != node.name {
                    change.rename.push((i64::from(*id), node.name.clone()));
                }
                if was.sequence != node.sequence {
                    change.sequence.push((i64::from(*id), i64::from(node.sequence)));
                }
                if was.tracks != node.tracks {
                    change.contents.push((i64::from(*id), node.tracks.iter().map(|&t| i64::from(t)).collect()));
                }
            }
        }
    }
    change.delete = old.keys().filter(|id| !new.contains_key(id)).map(|&id| i64::from(id)).collect();
    change
}

/// `export.pdb` with its two playlist tables rewritten to `after`.
///
/// Rows that did not change are written back as the file held them, in the
/// order it held them; a renamed or renumbered node keeps its first five
/// words but the sort order; new nodes and a changed playlist's entries come
/// last. Every other table's pages
/// stay byte for byte (see [`rbl_pdb::build::replace_table`]).
fn rewrite_pdb(bytes: &[u8], before: &[Node], after: &[Node]) -> Result<Vec<u8>> {
    use rbl_pdb::{rows, PageType};
    let pdb = rbl_pdb::Pdb::parse(bytes).map_err(unreadable)?;
    let old: BTreeMap<u32, &Node> = before.iter().map(|n| (n.id, n)).collect();
    let new: BTreeMap<u32, &Node> = after.iter().map(|n| (n.id, n)).collect();
    let cannot = || ExportError::Conflict("This device's library could not be changed in place. Nothing was changed.".to_owned());

    let mut next = bytes.to_vec();
    let same_row = |was: &Node, now: &Node| was.name == now.name && was.sequence == now.sequence;
    if old.iter().any(|(id, n)| new.get(id).is_none_or(|m| !same_row(n, m))) || new.keys().any(|id| !old.contains_key(id)) {
        let table = pdb.table(PageType::PlaylistTree).ok_or_else(cannot)?;
        let mut tree_rows = Vec::with_capacity(after.len());
        let mut written = BTreeSet::new();
        for row in pdb.rows(table) {
            let id = pdb.u4_at(row, 12);
            let Some(node) = new.get(&id) else { continue };
            if !written.insert(id) {
                continue;
            }
            let raw = pdb.playlist_row_bytes(row).ok_or_else(cannot)?;
            if old.get(&id).is_some_and(|was| same_row(was, node)) {
                tree_rows.push(raw);
            } else {
                // The first five words as they were, with the sort order
                // at their third; then the name.
                let mut changed = raw.get(..rows::PLAYLIST_NAME_AT).ok_or_else(cannot)?.to_vec();
                changed.get_mut(8..12).ok_or_else(cannot)?.copy_from_slice(&node.sequence.to_le_bytes());
                changed.extend_from_slice(&rbl_pdb::build::device_sql_string(&node.name));
                tree_rows.push(changed);
            }
        }
        for node in after.iter().filter(|n| !written.contains(&n.id)) {
            tree_rows.push(rows::playlist_row(node.id, node.parent, node.sequence, node.folder, &node.name));
        }
        next = rbl_pdb::build::replace_table(&next, PLAYLIST_TREE, &tree_rows).ok_or_else(cannot)?;
    }

    let changed: BTreeSet<u32> = old
        .iter()
        .filter(|(id, n)| new.get(id).is_none_or(|m| m.tracks != n.tracks))
        .map(|(id, _)| *id)
        .chain(new.iter().filter(|(id, n)| !old.contains_key(id) && !n.tracks.is_empty()).map(|(id, _)| *id))
        .collect();
    if !changed.is_empty() {
        let table = pdb.table(PageType::PlaylistEntries).ok_or_else(cannot)?;
        let entries_of = |node: &Node| -> Vec<Vec<u8>> {
            node.tracks
                .iter()
                .enumerate()
                .map(|(i, &track)| rows::playlist_entry_row(u32::try_from(i + 1).unwrap_or(u32::MAX), track, node.id))
                .collect()
        };
        let mut entry_rows = Vec::new();
        let mut emitted = BTreeSet::new();
        for row in pdb.rows(table) {
            let playlist = pdb.u4_at(row, 8);
            if !changed.contains(&playlist) {
                entry_rows.push(next_row(bytes, row.offset, 12).ok_or_else(cannot)?);
            } else if emitted.insert(playlist) {
                if let Some(node) = new.get(&playlist) {
                    entry_rows.extend(entries_of(node));
                }
            }
        }
        for id in &changed {
            if !emitted.contains(id) {
                if let Some(node) = new.get(id) {
                    entry_rows.extend(entries_of(node));
                }
            }
        }
        // `next` may have moved the tree's pages but not the entries'.
        next = rbl_pdb::build::replace_table(&next, PLAYLIST_ENTRIES, &entry_rows).ok_or_else(cannot)?;
    }

    let (written, _) = read_pdb(&next)?;
    check_written(&written, after)?;
    // Nothing but the two playlist tables may have moved.
    let census = |b: &[u8]| -> Result<BTreeMap<String, usize>> {
        let mut c = rbl_pdb::Pdb::parse(b).map_err(unreadable)?.census();
        c.remove(&PageType::PlaylistTree.name());
        c.remove(&PageType::PlaylistEntries.name());
        Ok(c)
    };
    if census(&next)? != census(bytes)? {
        return Err(cannot());
    }
    Ok(next)
}

/// `export.pdb`'s table types for the playlist tree and its entries.
const PLAYLIST_TREE: u32 = 7;
const PLAYLIST_ENTRIES: u32 = 8;

fn next_row(bytes: &[u8], offset: usize, len: usize) -> Option<Vec<u8>> {
    bytes.get(offset..offset + len).map(<[u8]>::to_vec)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn node(id: u32, parent: u32, name: &str, folder: bool, sequence: u32, tracks: &[u32]) -> Node {
        Node { id, parent, name: name.into(), folder, sequence, tracks: tracks.to_vec() }
    }

    fn library() -> Vec<Node> {
        vec![
            node(1, 0, "Now Playing Test", false, 2, &[1, 2, 3, 4]),
            node(2, 0, "Melodic Vox", false, 1, &[5, 6]),
            node(3, 0, "NP3-TEST-MP3", false, 0, &[]),
            node(4, 0, "Sets", true, 3, &[]),
            node(5, 4, "Friday", false, 1, &[1]),
        ]
    }

    fn tracks() -> BTreeSet<u32> {
        (1..=6).collect()
    }

    #[test]
    fn a_new_playlist_takes_the_next_id_and_the_top_of_its_parent() {
        let (after, applied) = plan(&library(), &tracks(), &Edit::Create { parent: 0, name: " New playlist ".into(), folder: false }).unwrap();
        assert_eq!(applied, Applied { id: 6, changed: 1 });
        assert_eq!(after.last().unwrap(), &node(6, 0, "New playlist", false, 0, &[]));
        // Every sibling moves down one; the folder's child does not.
        let sequences: Vec<(u32, u32)> = after.iter().map(|n| (n.id, n.sequence)).collect();
        assert_eq!(sequences, vec![(1, 3), (2, 2), (3, 1), (4, 4), (5, 1), (6, 0)]);
        let (after, _) = plan(&library(), &tracks(), &Edit::Create { parent: 4, name: "Saturday".into(), folder: true }).unwrap();
        assert_eq!(after.last().unwrap(), &node(6, 4, "Saturday", true, 0, &[]));
        assert_eq!(after.iter().find(|n| n.id == 5).unwrap().sequence, 2);
    }

    /// What rekordbox 7.2.14 wrote to a fixture stick for each edit made in
    /// its Devices tree [OBS Winrig 2026-10-08, `parity/issue-186/
    /// rig-stick-0-connected` to `rig-stick-9`], replayed here from the same
    /// starting rows.
    #[test]
    fn rekordboxs_own_device_edits_come_out_the_same() {
        let shape = |nodes: &[Node]| -> Vec<(u32, u32, u32, String, Vec<u32>)> {
            let mut out: Vec<_> = nodes.iter().map(|n| (n.id, n.parent, n.sequence, n.name.clone(), n.tracks.clone())).collect();
            out.sort();
            out
        };
        let row = |id: u32, parent: u32, sequence: u32, name: &str, tracks: &[u32]| (id, parent, sequence, name.to_owned(), tracks.to_vec());
        let tracks: BTreeSet<u32> = (1..=4).collect();
        let step = |nodes: &[Node], edit: Edit| plan(nodes, &tracks, &edit).unwrap().0;

        // Device Library, as `export.pdb` held it once rekordbox had read it.
        let start = vec![
            node(1, 0, "Folder A", true, 1, &[]),
            node(2, 1, "Inside A", false, 2, &[1, 2]),
            node(3, 0, "Top List", false, 3, &[3, 4, 1]),
        ];
        // Create New Playlist, then typed over: rig-stick-1-created.
        let created = step(&start, Edit::Create { parent: 0, name: "Untitled Playlist".into(), folder: false });
        let created = step(&created, Edit::Rename { id: 4, name: "RB New".into() });
        assert_eq!(shape(&created), vec![
            row(1, 0, 2, "Folder A", &[]), row(2, 1, 2, "Inside A", &[1, 2]), row(3, 0, 4, "Top List", &[3, 4, 1]), row(4, 0, 0, "RB New", &[]),
        ]);
        // Create New Folder, its name kept: rig-stick-2.
        let folder = step(&created, Edit::Create { parent: 0, name: "Untitled Folder".into(), folder: true });
        assert_eq!(shape(&folder), vec![
            row(1, 0, 3, "Folder A", &[]), row(2, 1, 2, "Inside A", &[1, 2]), row(3, 0, 5, "Top List", &[3, 4, 1]),
            row(4, 0, 1, "RB New", &[]), row(5, 0, 0, "Untitled Folder", &[]),
        ]);
        // Add To Playlist > RB New, then Remove from Playlist in Top List,
        // then the rename: rig-stick-4, -6 and -7.
        let added = step(&folder, Edit::Add { playlist: 4, tracks: vec![3] });
        let removed = step(&added, Edit::Remove { playlist: 3, tracks: vec![4] });
        let renamed = step(&removed, Edit::Rename { id: 3, name: "Top Renamed".into() });
        assert_eq!(shape(&renamed), vec![
            row(1, 0, 3, "Folder A", &[]), row(2, 1, 2, "Inside A", &[1, 2]), row(3, 0, 5, "Top Renamed", &[3, 1]),
            row(4, 0, 1, "RB New", &[3]), row(5, 0, 0, "Untitled Folder", &[]),
        ]);
        // Delete Playlist on RB New: rig-stick-8.
        let deleted = step(&renamed, Edit::Delete { id: 4 });
        assert_eq!(shape(&deleted), vec![
            row(1, 0, 1, "Folder A", &[]), row(2, 1, 2, "Inside A", &[1, 2]), row(3, 0, 2, "Top Renamed", &[3, 1]), row(5, 0, 0, "Untitled Folder", &[]),
        ]);

        // OneLibrary, from its own starting rows: rig-stick-3 and -9.
        let start = vec![
            node(1, 0, "Folder A", true, 0, &[]),
            node(2, 1, "Inside A", false, 1, &[1, 2]),
            node(3, 0, "Top List", false, 2, &[3, 4, 1]),
        ];
        let created = step(&start, Edit::Create { parent: 0, name: "Untitled Playlist".into(), folder: false });
        assert_eq!(shape(&created), vec![
            row(1, 0, 1, "Folder A", &[]), row(2, 1, 1, "Inside A", &[1, 2]), row(3, 0, 3, "Top List", &[3, 4, 1]), row(4, 0, 0, "Untitled Playlist", &[]),
        ]);
        let deleted = step(&created, Edit::Delete { id: 4 });
        assert_eq!(shape(&deleted), vec![
            row(1, 0, 0, "Folder A", &[]), row(2, 1, 1, "Inside A", &[1, 2]), row(3, 0, 1, "Top List", &[3, 4, 1]),
        ]);
    }

    #[test]
    fn creating_under_a_playlist_or_with_no_name_is_refused() {
        assert!(plan(&library(), &tracks(), &Edit::Create { parent: 1, name: "X".into(), folder: false }).is_err());
        assert!(plan(&library(), &tracks(), &Edit::Create { parent: 0, name: "  ".into(), folder: false }).is_err());
        assert!(plan(&library(), &tracks(), &Edit::Create { parent: 77, name: "X".into(), folder: false }).is_err());
    }

    #[test]
    fn deleting_a_folder_takes_everything_under_it() {
        let (after, applied) = plan(&library(), &tracks(), &Edit::Delete { id: 4 }).unwrap();
        assert_eq!(applied.changed, 2);
        assert!(after.iter().all(|n| n.id != 4 && n.id != 5));
        // What is left at the top level is numbered from 0 again.
        let sequences: Vec<(u32, u32)> = after.iter().map(|n| (n.id, n.sequence)).collect();
        assert_eq!(sequences, vec![(1, 2), (2, 1), (3, 0)]);
    }

    #[test]
    fn adding_appends_what_is_not_there_yet_and_removing_takes_every_entry() {
        let (after, applied) = plan(&library(), &tracks(), &Edit::Add { playlist: 2, tracks: vec![6, 1, 1, 3] }).unwrap();
        assert_eq!(applied.changed, 2);
        assert_eq!(after[1].tracks, vec![5, 6, 1, 3]);
        let mut nodes = library();
        nodes[0].tracks = vec![1, 2, 1, 3];
        let (after, applied) = plan(&nodes, &tracks(), &Edit::Remove { playlist: 1, tracks: vec![1] }).unwrap();
        assert_eq!(applied.changed, 2);
        assert_eq!(after[0].tracks, vec![2, 3]);
    }

    #[test]
    fn tracks_go_into_playlists_of_their_own_library_only() {
        assert!(plan(&library(), &tracks(), &Edit::Add { playlist: 4, tracks: vec![1] }).is_err(), "a folder");
        assert!(plan(&library(), &tracks(), &Edit::Add { playlist: 2, tracks: vec![99] }).is_err(), "a stranger");
    }

    #[test]
    fn an_unchanged_name_writes_nothing() {
        let (_, applied) = plan(&library(), &tracks(), &Edit::Rename { id: 2, name: "Melodic Vox".into() }).unwrap();
        assert_eq!(applied.changed, 0);
    }

    #[test]
    fn tree_order_is_depth_first_by_sequence() {
        let ordered: Vec<u32> = tree_order(library()).iter().map(|n| n.id).collect();
        assert_eq!(ordered, vec![3, 2, 1, 4, 5]);
    }
}
