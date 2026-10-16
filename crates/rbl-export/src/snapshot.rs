//! Semantic snapshots, shared by conflict detection and cross-format verification.
//! Settings and encryption/page counters deliberately do not participate.
use crate::{ExportError, Manifest, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Track {
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub genre: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub key: String,
    pub id: u32,
    pub title: String,
    pub path: String,
    pub analysis: String,
    pub bpm: u32,
    pub rating: u32,
    pub color: u32,
    pub comment: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Playlist {
    pub id: u32,
    pub parent: u32,
    pub name: String,
    pub folder: bool,
    pub sequence: u32,
    pub tracks: Vec<u32>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Library {
    pub tracks: Vec<Track>,
    pub playlists: Vec<Playlist>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct History {
    pub id: i64,
    pub parent: i64,
    pub name: String,
    pub folder: bool,
    pub sequence: i64,
    pub tracks: Vec<u32>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub legacy: Option<Library>,
    pub one: Option<Library>,
    pub identity: BTreeMap<u32, (u64, u64)>,
    pub history: Vec<History>,
    #[serde(default)]
    pub legacy_history: Vec<History>,
    pub cues: Vec<String>,
    #[serde(default)]
    pub my_tags: Vec<crate::SourceMyTag>,
    #[serde(default)]
    pub tag_memberships: BTreeMap<u32, Vec<u64>>,
    /// "Background Color : Device Library": byte 9 of `export.pdb`'s
    /// `property` row, 0 when the stick has no such row.
    #[serde(default)]
    pub legacy_background: u8,
}
fn sql(e: impl std::fmt::Display) -> ExportError {
    ExportError::OneLibrary(e.to_string())
}

/// Stars, 0 to 5, from a `OneLibrary` `content.rating`.
///
/// rekordbox writes the stars themselves [OBS: a rekordbox 7 stick, ratings
/// 1 to 5 in `exportLibrary.db` equal to the same tracks' `export.pdb`
/// ratings]. rbxport has written them as multiples of 51, the scale of the
/// collection XML. No rekordbox value is above 5 and no rbxport value is a
/// non-zero one at or below it, so both read back to the same stars. Read
/// on rbxport's scale alone, every rekordbox rating was 0, the stick's two
/// databases disagreed, and its first sync from here was refused (#284).
pub(crate) fn onelibrary_stars(raw: i64) -> u8 {
    let stars = if raw > 5 { raw / 51 } else { raw };
    u8::try_from(stars.clamp(0, 5)).unwrap_or(0)
}
impl Snapshot {
    pub fn read(root: &Path) -> Result<Self> {
        Self::read_at(root, crate::export_root_name(root)?)
    }
    pub fn read_at(root: &Path, name: &str) -> Result<Self> {
        let dir = root.join(name).join("rekordbox");
        let mut out = Self::default();
        let pdb = dir.join("export.pdb");
        if pdb.exists() {
            read_legacy_into(&mut out, &pdb)?;
        }
        let one = dir.join("exportLibrary.db");
        if one.exists() {
            read_one_into(&mut out, &one)?;
        }
        // Merge histories by full session contents. Equal numeric IDs alone
        // are insufficient when the stick was used by both generations of player.
        for legacy in &out.legacy_history {
            if out
                .history
                .iter()
                .any(|h| h.name == legacy.name && h.tracks == legacy.tracks)
            {
                continue;
            }
            let mut h = legacy.clone();
            if out.history.iter().any(|old| old.id == h.id) {
                h.id = out.history.iter().map(|h| h.id).max().unwrap_or(0) + 1;
            }
            out.history.push(h);
        }
        out.history.sort_by_key(|h| h.id);
        Ok(out)
    }
    pub fn merged_library(&self) -> Option<Library> {
        self.merged_library_above(0)
    }

    /// [`Self::merged_library`], where a playlist only `export.pdb` holds
    /// whose id `exportLibrary.db` gives another playlist is carried under
    /// an id above `floor` as well as above every id either library uses
    /// (see [`legacy_playlist_ids`]). `floor` keeps it clear of ids an
    /// earlier export recorded for playlists the stick no longer holds.
    pub fn merged_library_above(&self, floor: u32) -> Option<Library> {
        let mut library = self.one.as_ref().or(self.legacy.as_ref())?.clone();
        if let Some(legacy) = &self.legacy {
            let mut tracks: BTreeSet<_> = library.tracks.iter().map(|t| t.id).collect();
            let mut playlists: BTreeSet<_> = library.playlists.iter().map(|p| p.id).collect();
            for track in &legacy.tracks {
                if tracks.insert(track.id) {
                    library.tracks.push(track.clone());
                }
            }
            if let Some(one) = &self.one {
                let ids = legacy_playlist_ids(legacy, one, floor);
                let id = |legacy_id: u32| ids.get(&legacy_id).copied().unwrap_or(legacy_id);
                for playlist in &legacy.playlists {
                    let merged = id(playlist.id);
                    if playlists.insert(merged) {
                        library.playlists.push(Playlist {
                            id: merged,
                            parent: if playlist.parent == 0 { 0 } else { id(playlist.parent) },
                            ..playlist.clone()
                        });
                    }
                }
            }
        }
        library.normalize();
        Some(library)
    }

    pub fn check_baseline(&self, previous: Option<&Manifest>, db_id: u64) -> Result<()> {
        self.check_baseline_except(previous, db_id, &Rewritten::default())
    }

    /// [`Self::check_baseline`], leaving out the stick's rows this sync
    /// writes afresh from the library.
    ///
    /// On a stick never synced from here there is no record of which of its
    /// two databases is right, so a track or playlist they describe
    /// differently stops the sync: carrying one version would silently drop
    /// the other. A row the selection rewrites is the exception. It ends up
    /// in both databases as the library has it, whatever either said before,
    /// as rekordbox's export overwrites a device track it already holds
    /// (`DatabaseMediator::overwrite_export`, #331).
    pub fn check_baseline_except(&self, previous: Option<&Manifest>, db_id: u64, rewritten: &Rewritten) -> Result<()> {
        if previous.and_then(|m| m.baseline.as_ref()).is_none() {
            if let (Some(a), Some(b)) = (&self.legacy, &self.one) {
                match first_difference(a, b, rewritten) {
                    Some(Difference::Track(detail)) => return Err(ExportError::Conflict(format!("The USB's two libraries, export.pdb and exportLibrary.db, differ on {detail}, so this first sync stopped before changing anything. Include that track in this sync if it is in your library, or delete it from the USB in rekordbox, then sync again."))),
                    Some(Difference::Playlist(detail)) => return Err(ExportError::Conflict(format!("The USB's two libraries, export.pdb and exportLibrary.db, differ on {detail}, so this first sync stopped before changing anything. Delete that playlist from the USB in rekordbox, then sync again."))),
                    None => {}
                }
            }
        }
        if let Some(previous) = previous {
            if let Some(baseline) = &previous.baseline {
                if self
                    .identity
                    .iter()
                    .any(|(id, owner)| baseline.identity.get(id).is_some_and(|old| old != owner))
                {
                    return Err(ExportError::Conflict("rekordbox changed the device track identities. Reconcile this device before reusing its previous selection.".into()));
                }
            }
            if previous.db_id != db_id
                && (!previous.tracks.is_empty() || !previous.playlists.is_empty())
            {
                return Err(ExportError::Conflict("This USB belongs to a different or older unverified master library. Import its contents in rekordbox before changing its ownership.".into()));
            }
        }
        Ok(())
    }
    pub fn check_changes(&self, previous: Option<&Manifest>, next: &Self) -> Result<()> {
        self.check_changes_except(previous, next, &BTreeSet::new())
    }

    /// [`Self::check_changes`], leaving out the playlists, by device id,
    /// that the sync rewrites on purpose although the stick added entries
    /// to them (`reconcile::Prepared::device_additions`, #429).
    pub fn check_changes_except(&self, previous: Option<&Manifest>, next: &Self, device_additions: &BTreeSet<u32>) -> Result<()> {
        for (label, current, target, baseline) in [
            (
                "Device Library",
                &self.legacy,
                &next.legacy,
                previous
                    .and_then(|m| m.baseline.as_ref())
                    .and_then(|s| s.legacy.as_ref()),
            ),
            (
                "OneLibrary",
                &self.one,
                &next.one,
                previous
                    .and_then(|m| m.baseline.as_ref())
                    .and_then(|s| s.one.as_ref()),
            ),
        ] {
            if let Some(current) = current {
                let Some(target) = target else {
                    return Err(ExportError::Conflict(format!("Missing {label} output")));
                };
                check_database_changes(label, current, target, baseline, device_additions)?;
            }
        }
        if let Some(baseline) = previous.and_then(|m| m.baseline.as_ref()) {
            for tag in &self.my_tags {
                if baseline
                    .my_tags
                    .iter()
                    .find(|t| t.id == tag.id)
                    .is_some_and(|old| old != tag)
                    && !next.my_tags.contains(tag)
                {
                    return Err(ExportError::Conflict(format!(
                        "My Tag '{name}' changed on the USB",
                        name = tag.name
                    )));
                }
            }
            for (id, tags) in &self.tag_memberships {
                if baseline.tag_memberships.get(id) != Some(tags)
                    && next.tag_memberships.get(id) != Some(tags)
                {
                    return Err(ExportError::Conflict(format!(
                        "My Tags changed on USB track {id}"
                    )));
                }
            }
        }
        if !self.cues.is_empty()
            && previous
                .and_then(|m| m.baseline.as_ref())
                .is_some_and(|b| b.cues != self.cues)
            && self.cues != next.cues
        {
            return Err(ExportError::Conflict("OneLibrary contains cue records that this export would replace. Import the cues in rekordbox first.".into()));
        }
        Ok(())
    }
    pub fn check_retained_history(&self, next: &Self) -> Result<()> {
        let tracks = next.one.as_ref().map(|l| &l.tracks);
        if self
            .history
            .iter()
            .flat_map(|h| &h.tracks)
            .any(|id| !tracks.is_some_and(|t| t.iter().any(|t| t.id == *id)))
        {
            return Err(ExportError::Conflict("A track being removed is still referenced by USB history. Import and clear that history in rekordbox first.".into()));
        }
        Ok(())
    }
}
/// The stick's rows a sync writes afresh from the library: the device ids of
/// the tracks and playlists its selection names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rewritten {
    pub tracks: BTreeSet<u32>,
    pub playlists: BTreeSet<u32>,
}

/// The fields two readings of one track differ on, by name.
pub(crate) fn track_differences(a: &Track, b: &Track) -> Vec<&'static str> {
    [
        ("title", a.title != b.title),
        ("artist", a.artist != b.artist),
        ("album", a.album != b.album),
        ("genre", a.genre != b.genre),
        ("label", a.label != b.label),
        ("key", a.key != b.key),
        ("comment", a.comment != b.comment),
        ("file path", a.path != b.path),
        ("analysis path", a.analysis != b.analysis),
        ("BPM", a.bpm != b.bpm),
        ("rating", a.rating != b.rating),
        ("colour", a.color != b.color),
    ]
    .into_iter()
    .filter_map(|(name, differs)| differs.then_some(name))
    .collect()
}

/// What [`first_difference`] found, described for a message.
#[derive(Debug, PartialEq, Eq)]
enum Difference {
    /// "the artist of track 'X'"
    Track(String),
    /// "playlist 'Y'"
    Playlist(String),
}

/// The first track or playlist both databases hold but describe differently,
/// leaving out what `rewritten` names.
fn first_difference(legacy: &Library, one: &Library, rewritten: &Rewritten) -> Option<Difference> {
    let tracks: BTreeMap<u32, &Track> = one.tracks.iter().map(|t| (t.id, t)).collect();
    for track in legacy.tracks.iter().filter(|t| !rewritten.tracks.contains(&t.id)) {
        let Some(other) = tracks.get(&track.id) else { continue };
        let differ = track_differences(track, other);
        if !differ.is_empty() {
            return Some(Difference::Track(format!("the {} of track '{}'", differ.join(", "), track.title)));
        }
    }
    let ids = legacy_playlist_ids(legacy, one, 0);
    let id = |legacy_id: u32| ids.get(&legacy_id).copied().unwrap_or(legacy_id);
    let playlists: BTreeMap<u32, &Playlist> = one.playlists.iter().map(|p| (p.id, p)).collect();
    legacy
        .playlists
        .iter()
        .filter(|p| !rewritten.playlists.contains(&p.id) && !rewritten.playlists.contains(&id(p.id)))
        .find(|p| {
            let parent = if p.parent == 0 { 0 } else { id(p.parent) };
            playlists.get(&id(p.id)).is_some_and(|other| {
                other.name != p.name || other.parent != parent || other.folder != p.folder || other.tracks != p.tracks
            })
        })
        .map(|p| Difference::Playlist(format!("playlist '{}'", p.name)))
}

/// Which of `one`'s playlists each of `legacy`'s is, by id: the id it is
/// carried under once the stick's two libraries are merged.
///
/// rekordbox keeps a stick's two libraries apart. An edit made under one of
/// them changes that one alone [DOC: rekordbox FAQ "Device Library Plus"],
/// and a playlist made on a player is saved in the library that player
/// reads, and only there [DOC: the `AlphaTheta` USB export guide 251007,
/// p.4: "The two types of libraries are managed separately."]. Each numbers its
/// own playlists, as `device_library` does here (the highest id plus one). Where its export adds a playlist to both, it finds
/// the place in the other library by name, never by id [OBS static,
/// rekordbox 7.2.19: `DatabaseMediator::createListWithSameDevDbList`
/// @0x1010e2ce4 → `SearchPlaylistPositionForSameDevDB::search`
/// @0x1010dd828, which calls `getListWithName` and `existsListName`]. So
/// one id can name two different playlists, one in each library: a Tag List
/// a player saved to one of them, say, and another playlist made under the
/// other (#353).
///
/// A legacy playlist is the `OneLibrary` playlist with its id when both
/// give it the same name and kind. A legacy playlist whose id `OneLibrary`
/// does not use keeps it, whatever `OneLibrary` holds under other ids, as
/// before #353: two same-named playlists are both kept. Only one whose id
/// `OneLibrary` gives to another playlist is looked for by name: the one
/// with its name, kind and tracks in the same folder that no other legacy
/// playlist has claimed. A same-named one with other tracks is another
/// playlist, not a disagreement: pairing it would drop one of the two. One
/// not found that way takes a new id above `floor` and every id either
/// library uses.
pub(crate) fn legacy_playlist_ids(legacy: &Library, one: &Library, floor: u32) -> BTreeMap<u32, u32> {
    let same = |a: &Playlist, b: &Playlist| a.name == b.name && a.folder == b.folder;
    let by_id: BTreeMap<u32, &Playlist> = one.playlists.iter().map(|p| (p.id, p)).collect();
    let mut ids = BTreeMap::new();
    let mut claimed = BTreeSet::new();
    for p in &legacy.playlists {
        if by_id.get(&p.id).is_some_and(|other| same(p, other)) {
            ids.insert(p.id, p.id);
            claimed.insert(p.id);
        }
    }
    // By name, a folder's playlists once the folder itself is placed. Each
    // pass places at least one playlist or ends the search.
    loop {
        let mut placed = false;
        for p in &legacy.playlists {
            // Only where `OneLibrary` gives its id to another playlist. One
            // whose id is free keeps it, as before: each database is checked
            // by id before the sync writes it (`check_database_changes`), so
            // a playlist moved off its own id would read as lost.
            if ids.contains_key(&p.id) || !by_id.contains_key(&p.id) {
                continue;
            }
            let parent = if p.parent == 0 {
                0
            } else if let Some(parent) = ids.get(&p.parent) {
                *parent
            } else {
                continue;
            };
            // Only the same playlist: one each library fills differently
            // is two playlists that share a name, and each is kept.
            if let Some(other) = one
                .playlists
                .iter()
                .find(|o| !claimed.contains(&o.id) && o.parent == parent && same(p, o) && o.tracks == p.tracks)
            {
                ids.insert(p.id, other.id);
                claimed.insert(other.id);
                placed = true;
            }
        }
        if !placed {
            break;
        }
    }
    let mut next = legacy.playlists.iter().chain(&one.playlists).map(|p| p.id).fold(floor, u32::max);
    for p in &legacy.playlists {
        if ids.contains_key(&p.id) {
            continue;
        }
        let id = if by_id.contains_key(&p.id) {
            next = next.saturating_add(1);
            next
        } else {
            p.id
        };
        ids.insert(p.id, id);
    }
    ids
}

/// The legacy `export.pdb` side of [`Snapshot::read_at`].
fn read_legacy_into(out: &mut Snapshot, pdb: &Path) -> Result<()> {
    use rbl_pdb::PageType;
    let bytes = std::fs::read(pdb)?;
    let parsed = rbl_pdb::Pdb::parse(&bytes)
        .map_err(|e| ExportError::Conflict(format!("Cannot read Device Library: {e}")))?;
    out.legacy_background = parsed.property().map_or(0, |p| p.background_color);
    let tracks = parsed
        .table(PageType::Tracks)
        .map(|t| parsed.track_rows(t))
        .unwrap_or_default();
    let entries = parsed
        .table(PageType::PlaylistEntries)
        .map(|t| parsed.playlist_entries(t))
        .unwrap_or_default();
    let nodes = parsed
        .table(PageType::PlaylistTree)
        .map(|t| parsed.playlist_nodes(t))
        .unwrap_or_default();
    let names = |kind| -> BTreeMap<u32, String> {
        parsed
            .table(kind)
            .map(|t| {
                parsed
                    .named_rows(t)
                    .into_iter()
                    .map(|n| (n.id, n.name))
                    .collect()
            })
            .unwrap_or_default()
    };
    let artists = names(PageType::Artists);
    let albums = names(PageType::Albums);
    let genres = names(PageType::Genres);
    let labels = names(PageType::Labels);
    let keys = names(PageType::Keys);
    let mut library = Library {
        tracks: tracks
            .into_iter()
            .map(|t| Track {
                artist: artists.get(&t.artist_id).cloned().unwrap_or_default(),
                album: albums.get(&t.album_id).cloned().unwrap_or_default(),
                genre: genres.get(&t.genre_id).cloned().unwrap_or_default(),
                label: labels.get(&t.label_id).cloned().unwrap_or_default(),
                key: keys.get(&t.key_id).cloned().unwrap_or_default(),
                id: t.id,
                title: t.title,
                path: t.file_path,
                analysis: t.analyze_path,
                bpm: t.tempo_x100,
                rating: u32::from(t.rating),
                color: u32::from(t.color_id),
                comment: t.comment,
            })
            .collect(),
        playlists: nodes
            .into_iter()
            .map(|n| {
                let mut rows: Vec<_> =
                    entries.iter().filter(|e| e.playlist_id == n.id).collect();
                rows.sort_by_key(|e| e.entry_index);
                Playlist {
                    id: n.id,
                    parent: n.parent_id,
                    name: n.name,
                    folder: n.is_folder,
                    sequence: n.sort_order,
                    tracks: rows.iter().map(|e| e.track_id).collect(),
                }
            })
            .collect(),
    };
    let entries = parsed.played_history_entries();
    for h in parsed.played_histories() {
        let mut rows: Vec<_> = entries.iter().filter(|e| e.playlist_id == h.id).collect();
        rows.sort_by_key(|e| e.entry_index);
        out.legacy_history.push(History {
            id: i64::from(h.id),
            name: h.name,
            parent: 0,
            folder: false,
            sequence: i64::from(h.id),
            tracks: rows.iter().map(|e| e.track_id).collect(),
        });
    }
    library.normalize();
    out.legacy = Some(library);
    Ok(())
}
/// The `OneLibrary` (`exportLibrary.db`) side of [`Snapshot::read_at`].
fn read_one_into(out: &mut Snapshot, path: &Path) -> Result<()> {
    let db = rbl_onelibrary::ExportLibrary::open_read_only(path).map_err(sql)?;
    let conn = db.connection();
    conn.execute_batch("BEGIN").map_err(sql)?;
    let integrity: String = conn
        .query_row("PRAGMA quick_check", [], |r| r.get(0))
        .map_err(sql)?;
    if integrity != "ok" {
        return Err(ExportError::Conflict(format!(
            "OneLibrary integrity check failed: {integrity}",
        )));
    }
    let version: String = conn
        .query_row("SELECT dbVersion FROM property LIMIT 1", [], |r| r.get(0))
        .map_err(sql)?;
    if version != rbl_onelibrary::build::DB_VERSION {
        return Err(ExportError::Conflict(format!(
            "Unsupported OneLibrary schema {version}; the device was left unchanged."
        )));
    }
    read_one_library(&db, out)?;
    read_one_history_and_tags(&db, out)?;
    Ok(())
}

/// The tracks and playlists half of [`read_one_into`].
fn read_one_library(db: &rbl_onelibrary::ExportLibrary, out: &mut Snapshot) -> Result<()> {
    let conn = db.connection();
    let mut q = conn.prepare("SELECT content_id, COALESCE(title,''), COALESCE(path,''), COALESCE(analysisDataFilePath,''), COALESCE(bpmx100,0), COALESCE(rating,0), COALESCE(color_id,0), COALESCE(djComment,''), COALESCE(masterDbId,0), COALESCE(masterContentId,0), COALESCE((SELECT name FROM artist WHERE artist_id=content.artist_id_artist),''), COALESCE((SELECT name FROM album WHERE album_id=content.album_id),''), COALESCE((SELECT name FROM genre WHERE genre_id=content.genre_id),''), COALESCE((SELECT name FROM label WHERE label_id=content.label_id),''), COALESCE((SELECT name FROM key WHERE key_id=content.key_id),'') FROM content ORDER BY content_id").map_err(sql)?;
    let rows = q
        .query_map([], |r| {
            Ok((
                Track {
                    artist: r.get(10)?,
                    album: r.get(11)?,
                    genre: r.get(12)?,
                    label: r.get(13)?,
                    key: r.get(14)?,
                    id: r.get(0)?,
                    title: r.get(1)?,
                    path: r.get(2)?,
                    analysis: r.get(3)?,
                    bpm: r.get(4)?,
                    rating: u32::from(onelibrary_stars(r.get(5)?)),
                    color: r.get(6)?,
                    comment: r.get(7)?,
                },
                (
                    u64::try_from(r.get::<_, i64>(8)?).unwrap_or(0),
                    u64::try_from(r.get::<_, i64>(9)?).unwrap_or(0),
                ),
            ))
        })
        .map_err(sql)?;
    let mut library = Library::default();
    for row in rows {
        let (track, identity) = row.map_err(sql)?;
        out.identity.insert(track.id, identity);
        library.tracks.push(track);
    }
    let mut q = conn.prepare("SELECT playlist_id, COALESCE(playlist_id_parent,0), COALESCE(name,''), COALESCE(attribute,0), COALESCE(sequenceNo,0) FROM playlist ORDER BY sequenceNo, playlist_id").map_err(sql)?;
    let nodes = q
        .query_map([], |r| {
            Ok(Playlist {
                id: r.get(0)?,
                parent: r.get(1)?,
                name: r.get(2)?,
                folder: r.get::<_, i64>(3)? == 1,
                sequence: r.get(4)?,
                tracks: Vec::new(),
            })
        })
        .map_err(sql)?;
    for node in nodes {
        let mut node = node.map_err(sql)?;
        let mut q = conn.prepare("SELECT content_id FROM playlist_content WHERE playlist_id=?1 ORDER BY sequenceNo").map_err(sql)?;
        node.tracks = q
            .query_map([node.id], |r| r.get(0))
            .map_err(sql)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql)?;
        library.playlists.push(node);
    }
    library.normalize();
    out.one = Some(library);
    Ok(())
}

/// The play history, My Tags and cue fingerprints half of [`read_one_into`].
fn read_one_history_and_tags(db: &rbl_onelibrary::ExportLibrary, out: &mut Snapshot) -> Result<()> {
    use std::fmt::Write as _;
    let conn = db.connection();
    let mut q = conn.prepare("SELECT history_id, COALESCE(history_id_parent,0), COALESCE(name,''), COALESCE(attribute,0), COALESCE(sequenceNo,0) FROM history ORDER BY history_id").map_err(sql)?;
    let histories = q
        .query_map([], |r| {
            Ok(History {
                id: r.get(0)?,
                parent: r.get(1)?,
                name: r.get(2)?,
                folder: r.get::<_, i64>(3)? == 1,
                sequence: r.get(4)?,
                tracks: Vec::new(),
            })
        })
        .map_err(sql)?;
    for h in histories {
        let mut h = h.map_err(sql)?;
        let mut q = conn.prepare("SELECT content_id FROM history_content WHERE history_id=?1 ORDER BY sequenceNo").map_err(sql)?;
        h.tracks = q
            .query_map([h.id], |r| r.get(0))
            .map_err(sql)?
            .collect::<std::result::Result<_, _>>()
            .map_err(sql)?;
        out.history.push(h);
    }
    let mut tags=conn.prepare("SELECT myTag_id, COALESCE(sequenceNo,0), COALESCE(name,''), COALESCE(attribute,0), COALESCE(myTag_id_parent,0) FROM myTag ORDER BY myTag_id").map_err(sql)?;
    out.my_tags = tags
        .query_map([], |r| {
            Ok(crate::SourceMyTag {
                id: u64::try_from(r.get::<_, i64>(0)?).unwrap_or(0),
                seq: r.get(1)?,
                name: r.get(2)?,
                attribute: r.get(3)?,
                parent: u64::try_from(r.get::<_, i64>(4)?).unwrap_or(0),
            })
        })
        .map_err(sql)?
        .collect::<std::result::Result<_, _>>()
        .map_err(sql)?;
    let mut tags = conn
        .prepare(
            "SELECT content_id, myTag_id FROM myTag_content ORDER BY content_id, myTag_id",
        )
        .map_err(sql)?;
    for row in tags
        .query_map([], |r| {
            Ok((
                r.get::<_, u32>(0)?,
                u64::try_from(r.get::<_, i64>(1)?).unwrap_or(0),
            ))
        })
        .map_err(sql)?
    {
        let (id, tag) = row.map_err(sql)?;
        out.tag_memberships.entry(id).or_default().push(tag);
    }
    // Preserve a fingerprint of every cue column, including format-specific data.
    let mut q = conn
        .prepare("SELECT * FROM cue ORDER BY cue_id")
        .map_err(sql)?;
    let columns = q.column_count();
    out.cues = q
        .query_map([], |r| {
            let mut row = String::new();
            for i in 0..columns {
                let _ = write!(row, "{:?}|", r.get_ref(i)?);
            }
            Ok(row)
        })
        .map_err(sql)?
        .collect::<std::result::Result<_, _>>()
        .map_err(sql)?;
    Ok(())
}
/// One database's (legacy or `OneLibrary`) tracks and playlists, checked for
/// changes on the device that this sync would overwrite or lose — the part
/// of [`Snapshot::check_changes`] repeated once per database.
fn check_database_changes(
    label: &str,
    current: &Library,
    target: &Library,
    baseline: Option<&Library>,
    device_additions: &BTreeSet<u32>,
) -> Result<()> {
    let originals: BTreeMap<_, _> = baseline.into_iter().flat_map(|b| &b.tracks).map(|t| (t.id, t)).collect();
    let targets: BTreeMap<_, _> = target.tracks.iter().map(|t| (t.id, t)).collect();
    let current_ids: BTreeSet<_> = current.tracks.iter().map(|t| t.id).collect();
    let original_playlists: BTreeMap<_, _> = baseline.into_iter().flat_map(|b| &b.playlists).map(|p| (p.id, p)).collect();
    let target_playlists: BTreeMap<_, _> = target.playlists.iter().map(|p| (p.id, p)).collect();
    let current_playlist_ids: BTreeSet<_> = current.playlists.iter().map(|p| p.id).collect();
    for track in &current.tracks {
        let original = originals.get(&track.id).copied();
        let next = targets.get(&track.id).copied();
        let unchanged = original.is_some_and(|old| {
            if label == "OneLibrary" && track.rating > 5 && old.rating <= 5 && track.rating == old.rating * 51 {
                let mut corrected = track.clone();
                corrected.rating = old.rating;
                &corrected == old
            } else { old == track }
        });
        if original.is_some() && !unchanged && next != Some(track) {
            return Err(ExportError::Conflict(format!(
                "{label}: '{title}' changed on the USB. Import its changes before syncing.",
                title = track.title
            )));
        }
        if original.is_none() && next.is_none() {
            return Err(ExportError::Conflict(format!(
                "{label}: device-only track '{title}' would be lost",
                title = track.title
            )));
        }
    }
    for playlist in &current.playlists {
        let original = original_playlists.get(&playlist.id).copied();
        let next = target_playlists.get(&playlist.id).copied();
        // Position changes caused only by inserting/removing siblings are harmless.
        let same = |a: &Playlist, b: &Playlist| {
            a.id == b.id
                && a.parent == b.parent
                && a.name == b.name
                && a.folder == b.folder
                && a.tracks == b.tracks
        };
        if original.is_some_and(|b| !same(b, playlist)) && !next.is_some_and(|n| same(n, playlist)) && !device_additions.contains(&playlist.id) {
            return Err(ExportError::Conflict(format!("{label}: playlist '{name}' changed on the USB. Import or reconcile it before syncing.", name = playlist.name)));
        }
        if original.is_none() && next.is_none() {
            return Err(ExportError::Conflict(format!(
                "Device-only playlist '{name}' would be lost",
                name = playlist.name
            )));
        }
    }
    // A deletion on the device is also an edit; don't silently resurrect it.
    if let Some(baseline) = baseline {
        if baseline.tracks.iter().any(|t| {
            !current_ids.contains(&t.id) && targets.contains_key(&t.id)
        }) || baseline.playlists.iter().any(|p| {
            !current_playlist_ids.contains(&p.id) && target_playlists.contains_key(&p.id)
        }) {
            return Err(ExportError::Conflict(format!("{label} contains device-side deletions. Reconcile them before syncing.")));
        }
    }
    Ok(())
}
impl Library {
    fn normalize(&mut self) {
        self.tracks.sort_by_key(|t| t.id);
        self.playlists.sort_by_key(|p| (p.parent, p.sequence, p.id));
        let mut positions = BTreeMap::<u32, u32>::new();
        for p in &mut self.playlists {
            let seq = positions.entry(p.parent).or_default();
            *seq += 1;
            p.sequence = *seq;
        }
    }
}

pub fn check_analysis(
    root: &Path,
    old: &crate::ManifestTrack,
    desired: &[(String, Vec<u8>)],
) -> Result<()> {
    if old.anlz_dir.is_empty() {
        return Ok(());
    }
    let mut current = Vec::new();
    for extension in ["DAT", "EXT", "2EX"] {
        let path = root
            .join(old.anlz_dir.trim_start_matches('/'))
            .join(format!("ANLZ0000.{extension}"));
        match std::fs::read(path) {
            Ok(bytes) => current.push((extension.to_owned(), bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    // Missing files are repairable. Changed valid files may be player edits.
    if !current.is_empty() && analysis_hash(&current) != old.analysis && current != desired {
        // Only compare the musical edits; a path/header rewrite or corruption can be repaired.
        let musical = |bytes: &[u8]| {
            rbl_anlz::parse(bytes).ok().map(|a| a.sections.into_iter()
                .filter(|s| s.is_cue_list() || s.as_beat_grid().is_some()).collect::<Vec<_>>())
        };
        for (extension, bytes) in &current {
            if old.analysis_hashes.get(extension).is_some_and(|h| *h == crate::manifest::hash(bytes)) {
                continue;
            }
            // Compare only surviving companions. A missing EXT is repairable,
            // not evidence that the user deleted all of its cue points.
            let target = desired.iter().find(|(e, _)| e == extension).map(|(_, b)| b.as_slice()).unwrap_or_default();
            if musical(bytes).is_some() && musical(bytes) != musical(target) {
                return Err(ExportError::Conflict("USB cues or beat grids changed since the last sync. Import the USB cues/grids before exporting.".into()));
            }
        }
    }
    Ok(())
}
pub fn analysis_hash(files: &[(String, Vec<u8>)]) -> u64 {
    files.iter().fold(0_u64, |h, (e, b)| {
        h.rotate_left(7) ^ crate::manifest::hash(e.as_bytes()) ^ crate::manifest::hash(b)
    })
}

/// Capture mutable player-side analysis before staging, then check it again
/// before publication so a late cue edit cannot be overwritten unnoticed.
pub fn analysis_stamp(root: &Path, snapshot: &Snapshot) -> Result<BTreeMap<String, Option<crate::incremental::FileRevision>>> {
    let mut files = BTreeMap::new();
    for t in snapshot
        .merged_library()
        .as_ref()
        .into_iter()
        .flat_map(|l| &l.tracks)
    {
        if t.analysis.is_empty() {
            continue;
        }
        for extension in ["DAT", "EXT", "2EX"] {
            let relative = Path::new(&t.analysis)
                .with_extension(extension)
                .to_string_lossy()
                .into_owned();
            let path = crate::checked_under(root, &relative)?;
            let stamp = crate::incremental::revision(&path)?;
            files.insert(relative, stamp);
        }
    }
    Ok(files)
}
