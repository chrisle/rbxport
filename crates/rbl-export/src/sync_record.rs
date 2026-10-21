//! `PIONEER/rekordbox/playlists3.sync` and `playlists3Plus.sync`: the record
//! rekordbox leaves on a stick of which library it was synced from and which
//! playlists were ticked, so its Sync Manager can open on the same
//! selection and its automatic sync knows what to refresh.
//!
//! Both files are the same bytes [OBS, 2026-09-17 parity test]. The shape,
//! from a one-playlist export:
//!
//! ```text
//! <?xml version="1.0" encoding="UTF-8"?>
//!
//! <Sync DBID="1912725212" AutomaticSync="1" AllPlaylists="0" IncludeCue="1" ForcedSync="0" Timestamp="0">
//!   <Playlists>
//!     <NODE Id="0" ParentId="0" Attribute="1" Lib_Type="0" Dev_ID="0" Timestamp="0" CheckType="2"/>
//!     <NODE Id="FFB7D23B" ParentId="0" Attribute="0" Lib_Type="0" Dev_ID="1" Timestamp="1789685972345" CheckType="1"/>
//!   </Playlists>
//! </Sync>
//! ```
//!
//! CRLF line endings, a blank line after the declaration. `DBID` is the
//! library's `djmdProperty.DBID`; a node's `Id` is `djmdPlaylist.ID` in
//! upper-case hex, `Attribute` its `djmdPlaylist.Attribute` (1 a folder),
//! `Timestamp` milliseconds since the epoch when the playlist was first
//! ticked for this device — rekordbox wrote the same value on 2026-09-17
//! and again on 2026-09-18 after the stick had been erased, so it keeps
//! it in its own library, not on the stick — and `CheckType` 1 for a
//! ticked playlist, 2 for a folder with something ticked under it. The root is a folder with id 0. What a folder with
//! every child ticked gets, and what `Dev_ID`, `Lib_Type` and the
//! `AllPlaylists`/`IncludeCue`/`ForcedSync` attributes mean, is
//! [UNKNOWN]: only the one-playlist record has been captured, so the folder
//! rows here take the root's values.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

/// What the record is written from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncSource {
    /// `djmdProperty.DBID` of the library the tracks came from.
    pub db_id: u64,
    /// The library's playlist tree, or as much of it as holds the exported
    /// playlists and their ancestors. Anything else is left out of the
    /// record.
    pub tree: Vec<SyncNode>,
    /// Whether the stick is to be synced again on its own when it is
    /// plugged in: `AutomaticSync`.
    pub automatic: bool,
    /// The library's Cloud Library Sync id (`rbl_db::export_info::cloud_db_id`),
    /// 0 when it has none: the stick's My Tag master id when set.
    pub cloud_db_id: u32,
}

/// One playlist or folder of the library's tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncNode {
    /// `djmdPlaylist.ID`.
    pub id: u64,
    /// `djmdPlaylist.ParentID`, 0 at the root.
    pub parent: u64,
    /// `djmdPlaylist.Attribute`: 0 a playlist, 1 a folder, 4 an intelligent
    /// playlist.
    pub attribute: u8,
}

/// The files the record is written under, relative to the stick's root.
pub const FILES: [&str; 2] = ["PIONEER/rekordbox/playlists3.sync", "PIONEER/rekordbox/playlists3Plus.sync"];

/// The record for `ticked`, the ids of the playlists on the stick, as the
/// bytes of either file. `synced_at_ms` is written on every ticked
/// playlist that `kept` (a playlist id → timestamp read from the stick's
/// previous record) does not already have a time for, so a playlist keeps
/// its first-ticked time across syncs, as rekordbox's does.
#[must_use]
pub fn render(source: &SyncSource, ticked: &[u64], synced_at_ms: u64, kept: &BTreeMap<u64, u64>) -> Vec<u8> {
    let device_ids = ticked.iter().enumerate().map(|(i,id)| (*id, u32::try_from(i+1).unwrap_or(0))).collect();
    render_with_ids(source, ticked, synced_at_ms, kept, &device_ids)
}

pub fn render_with_ids(source: &SyncSource, ticked: &[u64], synced_at_ms: u64, kept: &BTreeMap<u64,u64>, device_ids: &BTreeMap<u64,u32>) -> Vec<u8> {
    let by_id: BTreeMap<u64, &SyncNode> = source.tree.iter().map(|node| (node.id, node)).collect();
    // The ticked playlists that the tree knows, and every folder above them.
    let mut folders: BTreeSet<u64> = BTreeSet::new();
    let mut playlists: Vec<&SyncNode> = Vec::new();
    for id in ticked {
        let Some(node) = by_id.get(id) else { continue };
        playlists.push(node);
        let mut parent = node.parent;
        // Bounded by the tree's size: a cycle in `ParentID` stops at a
        // folder already seen.
        while parent != 0 && folders.insert(parent) {
            parent = by_id.get(&parent).map_or(0, |folder| folder.parent);
        }
    }
    // Folders before the playlists under them, each parent before its
    // children, as rekordbox lists the root first.
    let mut ordered: Vec<&SyncNode> = Vec::with_capacity(folders.len() + playlists.len());
    let mut placed: BTreeSet<u64> = BTreeSet::new();
    let mut pending: Vec<&SyncNode> = folders.iter().filter_map(|id| by_id.get(id).copied()).collect();
    while !pending.is_empty() {
        let before = pending.len();
        pending.retain(|folder| {
            if folder.parent == 0 || placed.contains(&folder.parent) {
                placed.insert(folder.id);
                ordered.push(folder);
                false
            } else {
                true
            }
        });
        if pending.len() == before {
            // A folder whose parent is not in the tree: list it at the root
            // rather than never.
            ordered.append(&mut pending);
        }
    }
    ordered.extend(playlists);

    let mut out = String::with_capacity(256 + ordered.len() * 120);
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n\r\n");
    let _ = write!(
        out,
        "<Sync DBID=\"{}\" AutomaticSync=\"{}\" AllPlaylists=\"0\" IncludeCue=\"1\" ForcedSync=\"0\" Timestamp=\"0\">\r\n",
        source.db_id,
        u8::from(source.automatic)
    );
    out.push_str("  <Playlists>\r\n");
    out.push_str("    <NODE Id=\"0\" ParentId=\"0\" Attribute=\"1\" Lib_Type=\"0\" Dev_ID=\"0\" Timestamp=\"0\" CheckType=\"2\"/>\r\n");
    for node in ordered {
        let folder = node.attribute == 1;
        let _ = write!(
            out,
            "    <NODE Id=\"{:X}\" ParentId=\"{:X}\" Attribute=\"{}\" Lib_Type=\"0\" Dev_ID=\"{}\" Timestamp=\"{}\" CheckType=\"{}\"/>\r\n",
            node.id,
            node.parent,
            node.attribute,
            device_ids.get(&node.id).copied().unwrap_or(0),
            if folder { 0 } else { kept.get(&node.id).copied().unwrap_or(synced_at_ms) },
            if folder { 2 } else { 1 },
        );
    }
    out.push_str("  </Playlists>\r\n</Sync>\r\n");
    out.into_bytes()
}

/// What a record on a stick says, read back.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncRecord {
    /// Master IDs explicitly marked as folders in this record.
    pub folders: BTreeSet<u64>,
    pub device_ids: BTreeMap<u64, u32>,
    /// `DBID` of the library that synced the stick.
    pub db_id: u64,
    /// `AutomaticSync`.
    pub automatic: bool,
    /// The ticked playlists' `djmdPlaylist.ID`s, in the file's order;
    /// folders left out.
    pub ticked: Vec<u64>,
    /// Each ticked playlist's `Timestamp`.
    pub timestamps: BTreeMap<u64, u64>,
}

/// Reads the record from `playlists3.sync` on a stick, ours or
/// rekordbox's. `None` when there is none or it is not one.
#[must_use]
pub fn read(mount: &Path) -> Option<SyncRecord> {
    let root = crate::export_root(mount).join("rekordbox");
    ["playlists3.sync", "playlists3Plus.sync"].into_iter().find_map(|name| std::fs::read(root.join(name)).ok().and_then(|b| parse(&b)))
}

/// Parses a record's bytes. The file is small and regular enough that
/// reading its attributes by name is the whole of the job; an XML parser
/// would be a dependency for one tag.
#[must_use]
pub fn parse(bytes: &[u8]) -> Option<SyncRecord> {
    let text = std::str::from_utf8(bytes).ok()?;
    let sync = text.find("<Sync ")?;
    let header = &text[sync..text[sync..].find('>').map_or(text.len(), |end| sync + end)];
    let db_id = attribute(header, "DBID")?.parse().ok()?;
    let automatic = attribute(header, "AutomaticSync") == Some("1");
    let mut ticked = Vec::new();
    let mut device_ids = BTreeMap::new();
    let mut folders = BTreeSet::new();
    let mut timestamps = BTreeMap::new();
    for node in text.split("<NODE ").skip(1) {
        let node = &node[..node.find('>').unwrap_or(node.len())];
        if let (Some(id), Some(device)) = (attribute(node, "Id").and_then(|s| u64::from_str_radix(s,16).ok()), attribute(node, "Dev_ID").and_then(|s| s.parse::<u32>().ok())) {
            if id != 0 && device != 0 {
                device_ids.insert(id, device);
                if attribute(node, "Attribute") == Some("1") { folders.insert(id); }
            }
        }
        if attribute(node, "Attribute") == Some("1") || attribute(node, "CheckType") != Some("1") {
            continue;
        }
        if let Some(id) = attribute(node, "Id").and_then(|hex| u64::from_str_radix(hex, 16).ok()) {
            ticked.push(id);
            if let Some(stamp) = attribute(node, "Timestamp").and_then(|t| t.parse::<u64>().ok()) {
                timestamps.insert(id, stamp);
            }
        }
    }
    Some(SyncRecord { folders, device_ids, db_id, automatic, ticked, timestamps })
}

/// The value of `name="…"` in one tag, if there.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = tag;
    while let Some(at) = rest.find(name) {
        let after = &rest[at + name.len()..];
        // The name must start a word and be followed by `="`, or it is part
        // of another attribute's name (`Id` inside `Dev_ID`, say).
        let starts_word = at == 0 || !rest.as_bytes()[at - 1].is_ascii_alphanumeric() && rest.as_bytes()[at - 1] != b'_';
        if starts_word {
            if let Some(value) = after.strip_prefix("=\"") {
                return value.find('"').map(|end| &value[..end]);
            }
        }
        rest = after;
    }
    None
}

/// Writes both files under `destination`, keeping the times the previous
/// record there gave the playlists that are still ticked.
pub fn write(destination: &Path, source: &SyncSource, ticked: &[u64], synced_at_ms: u64) -> std::io::Result<()> {
    crate::recover(destination)?;
    let publication = rbl_core::durable::Publication::new(destination, ".rbxport-publication")?;
    let kept = read(destination).map(|r| r.timestamps).unwrap_or_default();
    let bytes = render(source, ticked, synced_at_ms, &kept);
    for file in FILES {
        let path = publication.stage().join(file);
        if let Some(parent) = path.parent() { rbl_core::durable::create_dir_all(parent)?; }
        rbl_core::durable::write(&path, &bytes)?;
    }
    publication.commit(&FILES.map(std::path::PathBuf::from))?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    const REFERENCE: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n\r\n<Sync DBID=\"1912725212\" AutomaticSync=\"1\" AllPlaylists=\"0\" IncludeCue=\"1\" ForcedSync=\"0\" Timestamp=\"0\">\r\n  <Playlists>\r\n    <NODE Id=\"0\" ParentId=\"0\" Attribute=\"1\" Lib_Type=\"0\" Dev_ID=\"0\" Timestamp=\"0\" CheckType=\"2\"/>\r\n    <NODE Id=\"FFB7D23B\" ParentId=\"0\" Attribute=\"0\" Lib_Type=\"0\" Dev_ID=\"1\" Timestamp=\"1789685972345\" CheckType=\"1\"/>\r\n  </Playlists>\r\n</Sync>\r\n";

    #[test]
    fn one_playlist_at_the_root_is_the_captured_record_byte_for_byte() {
        let source = SyncSource {
            db_id: 1_912_725_212,
            tree: vec![SyncNode { id: 4_290_236_987, parent: 0, attribute: 0 }],
            automatic: true,
            cloud_db_id: 0,
        };
        let bytes = render(&source, &[4_290_236_987], 1_789_685_972_345, &BTreeMap::new());
        assert_eq!(String::from_utf8(bytes.clone()).unwrap(), REFERENCE);
        // A later sync keeps the time the record already gave the playlist.
        let kept = parse(&bytes).unwrap().timestamps;
        let again = render(&source, &[4_290_236_987], 1_789_773_803_138, &kept);
        assert_eq!(String::from_utf8(again).unwrap(), REFERENCE);
    }

    #[test]
    fn the_captured_record_reads_back() {
        let record = parse(REFERENCE.as_bytes()).unwrap();
        assert_eq!(record.db_id, 1_912_725_212);
        assert!(record.automatic);
        assert_eq!(record.ticked, vec![4_290_236_987]);
        assert_eq!(record.timestamps.get(&4_290_236_987), Some(&1_789_685_972_345));
        assert_eq!(parse(b"not a record"), None);
        let bare = parse(b"<Sync DBID=\"5\" AutomaticSync=\"0\"></Sync>").unwrap();
        assert_eq!((bare.db_id, bare.automatic, bare.ticked.len()), (5, false, 0));
    }

    #[test]
    fn what_is_rendered_reads_back_with_the_folders_left_out() {
        let source = SyncSource {
            db_id: 9,
            tree: vec![
                SyncNode { id: 0x10, parent: 0, attribute: 1 },
                SyncNode { id: 0x30, parent: 0x10, attribute: 0 },
                SyncNode { id: 0x40, parent: 0, attribute: 4 },
            ],
            automatic: true,
            cloud_db_id: 0,
        };
        let record = parse(&render(&source, &[0x30, 0x40], 1, &BTreeMap::new())).unwrap();
        assert_eq!((record.db_id, record.automatic, record.ticked), (9, true, vec![0x30, 0x40]));
    }

    #[test]
    fn folders_above_a_ticked_playlist_are_listed_first_and_partially_checked() {
        let source = SyncSource {
            db_id: 7,
            tree: vec![
                SyncNode { id: 0x10, parent: 0, attribute: 1 },
                SyncNode { id: 0x20, parent: 0x10, attribute: 1 },
                SyncNode { id: 0x30, parent: 0x20, attribute: 0 },
                SyncNode { id: 0x40, parent: 0x10, attribute: 4 },
                SyncNode { id: 0x50, parent: 0, attribute: 0 }, // not ticked
            ],
            automatic: false,
            cloud_db_id: 0,
        };
        let text = String::from_utf8(render(&source, &[0x30, 0x40, 0x99], 5, &BTreeMap::new())).unwrap();
        let nodes: Vec<&str> = text.lines().filter(|l| l.contains("<NODE")).collect();
        assert_eq!(nodes.len(), 5, "root, two folders, two playlists; the unknown id is left out:\n{text}");
        assert!(nodes[1].contains("Id=\"10\" ParentId=\"0\" Attribute=\"1\"") && nodes[1].contains("CheckType=\"2\""));
        assert!(nodes[2].contains("Id=\"20\" ParentId=\"10\" Attribute=\"1\""));
        assert!(nodes[3].contains("Id=\"30\" ParentId=\"20\" Attribute=\"0\"") && nodes[3].contains("Timestamp=\"5\" CheckType=\"1\""));
        assert!(nodes[4].contains("Id=\"40\" ParentId=\"10\" Attribute=\"4\""));
        assert!(text.contains("AutomaticSync=\"0\""));
        assert!(!text.contains("Id=\"50\""));
    }
}
