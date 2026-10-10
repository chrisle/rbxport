//! The read-only tree a player sees, and the file handles that address it.
//!
//! A player can only reach a node by walking from the mount point, one name
//! at a time, and there are two ways a name resolves:
//!
//! - a node registered up front ([`Vfs::add_file`], [`Vfs::add_dir`]): the
//!   tree the tests and the fake player build, every file named;
//! - a folder allowed as a whole ([`Vfs::allow_folder`]): the library's
//!   folders, `/Volumes/SD/RB` and the like. Nothing under it exists in the
//!   tree until a player asks for it by name; then the host is asked whether
//!   that name is a file or a directory there, and a node is made for it.
//!   The folders above an allowed one resolve only along the way to it.
//!
//! Names are never joined onto a path and re-opened from the root: `.` and
//! `..` are answered from the tree, a name with a separator in it is not a
//! name, and a symlink is not followed, so a player cannot reach outside
//! what was registered or allowed. rekordbox's own file server goes
//! further and exports `/`, resolving every name against the host.
//!
//! Nodes grow while the server runs, so the table is behind a lock; a node's
//! index, and so its handle, never changes once issued.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use unicode_normalization::UnicodeNormalization as _;

/// A file handle is a fixed 32 opaque bytes in `NFSv2`.
pub const HANDLE_LEN: usize = 32;

/// The most a single `READ` may return: rekordbox's libFilSiNE caps reads
/// at 0xfc00 (`verification/link/rekordbox-re/filsine.c:419`).
/// [OBS] A physical CDJ-3000 requests 32 KB; capping successful replies at
/// 8 KB left unfetched audio ranges in the 2026-10-06 KILLA capture. Reply
/// with the requested count up to the vendor cap, even when UDP fragments.
pub const MAX_READ: usize = 0xfc00;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Directory,
    File,
}

#[derive(Debug, Clone)]
struct Node {
    name: String,
    kind: NodeKind,
    parent: usize,
    /// In the order they were made, which is the order a listing shows.
    children: Vec<usize>,
    /// The children by name, decomposed (NFD): a name a player read off a
    /// listing comes back decomposed, the way rekordbox's `UTF8-MAC`
    /// conversion sent it, and the host may hold it either way.
    by_name: HashMap<String, usize>,
    /// Where a file's bytes actually live; for a directory under an allowed
    /// folder, the host directory its names are asked of. A directory of the
    /// registered tree, or one on the way to an allowed folder, has none.
    source: Option<PathBuf>,
    /// Whether every child is in the tree: for a directory on the host,
    /// once a listing has read it; for one of the tree, once a listing has
    /// added the allowed folders that pass through it. A registered
    /// directory starts unlisted, because an allowed folder may run through
    /// it too: a track at `/Volumes/SD/a.mp3` registers `Volumes/SD`, and
    /// `Volumes/SD/Music` and `Volumes/Transcend` must still resolve (#282).
    listed: bool,
    /// Attributes of synthetic nodes. Host-backed nodes are stat'ed on
    /// every request, as libFilSiNE obtains the current host attributes.
    stat: Option<Stat>,
}

impl Node {
    fn new(name: &str, kind: NodeKind, parent: usize, source: Option<PathBuf>, stat: Option<Stat>, listed: bool) -> Self {
        Self {
            name: name.to_owned(),
            kind,
            parent,
            children: Vec::new(),
            by_name: HashMap::new(),
            source,
            listed,
            stat,
        }
    }
}

/// A name as the tree keys it: decomposed, so either form a player sends
/// finds the same child.
fn key(name: &str) -> String {
    name.nfd().collect()
}

/// A folder a player may reach everything under.
#[derive(Debug, Clone)]
struct Root {
    /// Its path inside the export, one component per entry.
    parts: Vec<String>,
    /// Where it is on the host.
    host: PathBuf,
}

/// What `stat` says about a file, as the `fattr` reports it: rekordbox's
/// libFilSiNE hands a player the host's own mode, owner, block size and
/// device (`docs/pre-release/rekordbox/link-export-internals.md`), so a
/// player sees the file as the host does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stat {
    pub size: u64,
    /// `st_mode`, the type bits included.
    pub mode: u32,
    pub nlink: u32,
    pub uid: u32,
    pub gid: u32,
    pub blocksize: u32,
    pub rdev: u32,
    pub blocks: u32,
    /// Seconds since the epoch.
    pub accessed: u32,
    pub modified: u32,
    pub changed: u32,
}

impl Stat {
    /// A plain readable file of `size` bytes, changed at `modified`: what a
    /// node given its size at insertion reports.
    pub fn plain(size: u64, modified: u32) -> Self {
        Self {
            size,
            mode: 0o100_444,
            nlink: 1,
            uid: 0,
            gid: 0,
            blocksize: 4096,
            rdev: 0,
            blocks: u32::try_from(size.div_ceil(512)).unwrap_or(u32::MAX),
            accessed: modified,
            modified,
            changed: modified,
        }
    }

    /// A directory of the tree. The directories here are the library's,
    /// not the host's, so they carry what rekordbox's export root showed a
    /// player in the capture: mode `041ed`, two links, 64 bytes.
    pub fn directory(modified: u32) -> Self {
        Self {
            size: 64,
            mode: 0o040_755,
            nlink: 2,
            uid: 0,
            gid: 0,
            blocksize: 4096,
            rdev: 0,
            blocks: 0,
            accessed: modified,
            modified,
            changed: modified,
        }
    }
}

fn seconds(time: std::io::Result<std::time::SystemTime>) -> u32 {
    time.ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| u32::try_from(d.as_secs()).unwrap_or(u32::MAX))
}

/// The host's `stat` of the file, or an empty plain file for one that
/// cannot be read: a missing file is listed with no size and fails on
/// `READ`, which is what a player expects of a moved track.
fn stat_file(path: &Path) -> Stat {
    let Ok(meta) = std::fs::metadata(path) else {
        return Stat::plain(0, 0);
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let low = |v: u64| u32::try_from(v).unwrap_or(u32::MAX);
        Stat {
            size: meta.len(),
            mode: meta.mode() & 0xffff,
            nlink: low(meta.nlink()),
            uid: meta.uid(),
            gid: meta.gid(),
            blocksize: low(meta.blksize()),
            rdev: low(meta.dev()),
            blocks: low(meta.blocks()),
            accessed: seconds(meta.accessed()),
            modified: seconds(meta.modified()),
            changed: u32::try_from(meta.ctime()).unwrap_or(0),
        }
    }
    #[cfg(not(unix))]
    {
        let mut stat = if meta.is_dir() {
            Stat::directory(seconds(meta.modified()))
        } else {
            Stat::plain(meta.len(), seconds(meta.modified()))
        };
        stat.accessed = seconds(meta.accessed());
        stat.changed = seconds(meta.created());
        stat
    }
}

/// An exported filesystem and everything reachable inside it.
#[derive(Debug)]
pub struct Vfs {
    /// The export name a player mounts, e.g. `/` on macOS or `/C/` on Windows.
    export: String,
    nodes: RwLock<Vec<Node>>,
    /// The folders allowed as a whole.
    roots: Vec<Root>,
    /// Where this export's file ids start: libFilSiNE numbers every node
    /// of every export from one table, so a handle names its export by its
    /// ids alone. Set when the export joins an [`Exports`].
    id_base: u32,
}

impl Clone for Vfs {
    fn clone(&self) -> Self {
        Self {
            export: self.export.clone(),
            nodes: RwLock::new(self.read().clone()),
            roots: self.roots.clone(),
            id_base: self.id_base,
        }
    }
}

/// Addresses one node, laid out as rekordbox's libFilSiNE lays its handles
/// out: three big-endian file ids — the node's, its parent's, the mount
/// root's — then twenty zero bytes, the root's being its own id three times
/// (`docs/pre-release/rekordbox/link-export-internals.md`, "File handles").
/// Checked on the way back in: the three must agree with the tree, so a
/// handle from another export, or a made-up one, is refused. The twenty
/// bytes after them are not read: libFilSiNE's `tkfNtoHFhandle` converts the
/// three words only, and an XDJ-700 sends its own bytes there (#43).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Handle([u8; HANDLE_LEN]);

impl Handle {
    pub const fn as_bytes(&self) -> &[u8; HANDLE_LEN] {
        &self.0
    }

    pub fn from_slice(bytes: &[u8]) -> Option<Self> {
        let mut out = [0_u8; HANDLE_LEN];
        out.copy_from_slice(bytes.get(..HANDLE_LEN)?);
        Some(Self(out))
    }
}

/// The handle as the three file-id words and the trailing bytes in hex,
/// `00000001.00000001.00000001.0000…`, so a log line names exactly what a
/// player sent when a handle is refused.
impl std::fmt::Display for Handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (at, byte) in self.0.iter().enumerate() {
            if at == 4 || at == 8 || at == 12 {
                f.write_str(".")?;
            }
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Node attributes, as `NFSv2` reports them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attributes {
    pub kind: NodeKind,
    pub size: u64,
    pub fileid: u32,
    pub modified: u32,
    /// The rest of what the host says, for the `fattr`.
    pub stat: Stat,
}

/// What the host says a name under an allowed folder is.
enum HostEntry {
    Directory,
    File,
}

/// Whether a name is one component and nothing else: no separator, no NUL,
/// and not the two names the tree answers itself.
fn is_plain_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0'])
}

/// Asks the host what `name` is inside `dir`, without following a symlink
/// out of the export.
fn host_entry(dir: &Path, name: &str) -> Option<HostEntry> {
    let meta = std::fs::symlink_metadata(dir.join(name)).ok()?;
    if meta.is_dir() {
        Some(HostEntry::Directory)
    } else if meta.is_file() {
        Some(HostEntry::File)
    } else {
        None
    }
}

impl Vfs {
    /// Creates an empty export. `export` is the name a player mounts.
    pub fn new(export: impl Into<String>) -> Self {
        let export = export.into();
        Self {
            id_base: 0,
            export,
            nodes: RwLock::new(vec![Node::new("", NodeKind::Directory, 0, None, Some(Stat::directory(0)), false)]),
            roots: Vec::new(),
        }
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, Vec<Node>> {
        // A poisoned lock means a panic mid-insert on another thread; the
        // table is append-only, so what is there is still whole.
        self.nodes.read().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, Vec<Node>> {
        self.nodes.write().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn export_name(&self) -> &str {
        &self.export
    }

    pub const fn root(&self) -> usize {
        0
    }

    pub fn len(&self) -> usize {
        self.read().len()
    }

    pub fn is_empty(&self) -> bool {
        // The root always exists, so a fresh tree has exactly one node.
        self.read().len() <= 1 && self.roots.is_empty()
    }

    /// Allows everything under a folder: `path` is its slash-separated place
    /// inside the export, `host` where it is on the host. A player reaching
    /// it by name gets whatever the host has there, one name at a time; the
    /// folders above it resolve only on the way to it.
    ///
    /// The same folder allowed twice is allowed once.
    pub fn allow_folder(&mut self, path: &str, host: impl Into<PathBuf>) {
        let parts: Vec<String> =
            path.split('/').filter(|part| !part.is_empty() && *part != "." && *part != "..").map(str::to_owned).collect();
        if parts.is_empty() || self.roots.iter().any(|root| root.parts == parts) {
            return;
        }
        self.roots.push(Root { parts, host: host.into() });
    }

    /// Adds a file at a slash-separated path inside the export, creating the
    /// directories it needs. Returns the node index.
    ///
    /// Empty and `.`/`..` components are dropped rather than honoured: a caller
    /// building a tree has no business asking for them, and silently resolving
    /// them is how an export grows a hole.
    pub fn add_file(&mut self, path: &str, source: impl Into<PathBuf>, size: u64, modified: u32) -> usize {
        self.add(path, Some((source.into(), Some(Stat::plain(size, modified)))), Stat::directory(modified))
    }

    /// Adds a file whose attributes are read from `source` on each request,
    /// rather than doing filesystem I/O while building the export.
    pub fn add_file_unsized(&mut self, path: &str, source: impl Into<PathBuf>) -> usize {
        self.add(path, Some((source.into(), None)), Stat::directory(0))
    }

    /// Adds an empty directory, for a tree that must show a folder with no files.
    pub fn add_dir(&mut self, path: &str) -> usize {
        self.add(path, None, Stat::directory(0))
    }

    fn add(&mut self, path: &str, file: Option<(PathBuf, Option<Stat>)>, dir_stat: Stat) -> usize {
        let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty() && *part != "." && *part != "..").collect();
        let nodes = self.nodes.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut at = 0;
        let (last, directories) = match file {
            Some(_) => match parts.split_last() {
                Some(split) => split,
                None => return at,
            },
            None => (&"", parts.as_slice()),
        };
        for part in directories {
            at = Self::insert(nodes, at, part, NodeKind::Directory, None, Some(dir_stat), false);
        }
        match file {
            Some((source, stat)) => Self::insert(nodes, at, last, NodeKind::File, Some(source), stat, true),
            None => at,
        }
    }

    /// The child of `parent` called `name`, made if it is not there.
    fn insert(
        nodes: &mut Vec<Node>,
        parent: usize,
        name: &str,
        kind: NodeKind,
        source: Option<PathBuf>,
        stat: Option<Stat>,
        listed: bool,
    ) -> usize {
        let k = key(name);
        if let Some(existing) = nodes.get(parent).and_then(|node| node.by_name.get(&k)) {
            return *existing;
        }
        let index = nodes.len();
        nodes.push(Node::new(name, kind, parent, source, stat, listed));
        if let Some(node) = nodes.get_mut(parent) {
            node.children.push(index);
            node.by_name.insert(k, index);
        }
        index
    }

    /// Looks up one name in one directory. This is the only way in.
    pub fn child(&self, parent: usize, name: &str) -> Option<usize> {
        {
            let nodes = self.read();
            let node = nodes.get(parent)?;
            if node.kind != NodeKind::Directory {
                return None;
            }
            // `.` and `..` are answered here rather than by name matching, so
            // they stay inside the export: `..` at the root is the root.
            match name {
                "." => return Some(parent),
                ".." => return Some(node.parent),
                _ => {}
            }
            if let Some(index) = node.by_name.get(&key(name)) {
                return Some(*index);
            }
            if node.listed {
                return None;
            }
        }
        if !is_plain_name(name) {
            return None;
        }
        // Not in the tree yet: what the allowed folders say, then the host.
        let mut nodes = self.write();
        let node = nodes.get(parent)?;
        if let Some(index) = node.by_name.get(&key(name)) {
            return Some(*index);
        }
        if let Some(dir) = node.source.clone() {
            // Under an allowed folder: the host decides. A name the player
            // decomposed may be composed on a volume that keeps names as
            // written.
            let composed: String = name.nfc().collect();
            let (found, as_named) = match host_entry(&dir, name) {
                Some(entry) => (entry, name),
                None if composed != name => (host_entry(&dir, &composed)?, composed.as_str()),
                None => return None,
            };
            let host = dir.join(as_named);
            return Some(match found {
                HostEntry::Directory => {
                    Self::insert(&mut nodes, parent, as_named, NodeKind::Directory, Some(host), None, false)
                }
                HostEntry::File => Self::insert(&mut nodes, parent, as_named, NodeKind::File, Some(host), None, true),
            });
        }
        // On the way to an allowed folder, or at it.
        let mut path = Self::path_of(&nodes, parent);
        path.push(key(name));
        let depth = path.len();
        let root = self.roots.iter().find(|root| {
            root.parts.len() >= depth && root.parts.iter().map(|p| key(p)).zip(&path).all(|(a, b)| a == *b)
        })?;
        let as_named = root.parts.get(depth - 1)?.clone();
        let host = (root.parts.len() == depth).then(|| root.host.clone());
        // A folder on the way has no host behind it and reports as a
        // directory of the tree; the allowed folder itself is the host's,
        // read when first asked.
        let stat = host.is_none().then(|| Stat::directory(0));
        Some(Self::insert(&mut nodes, parent, &as_named, NodeKind::Directory, host, stat, false))
    }

    /// The names from the root down to `index`, decomposed.
    fn path_of(nodes: &[Node], mut index: usize) -> Vec<String> {
        let mut parts = Vec::new();
        while index != 0 {
            let Some(node) = nodes.get(index) else { break };
            parts.push(key(&node.name));
            index = node.parent;
        }
        parts.reverse();
        parts
    }

    /// The children of a directory, complete: a directory on the host is
    /// read the first time, so a listing shows what is there and later
    /// lookups find the same nodes.
    pub fn children(&self, index: usize) -> Vec<usize> {
        {
            let nodes = self.read();
            let Some(node) = nodes.get(index) else { return Vec::new() };
            if node.listed || node.kind != NodeKind::Directory {
                return node.children.clone();
            }
        }
        let mut nodes = self.write();
        let Some(node) = nodes.get(index) else { return Vec::new() };
        if node.listed {
            return node.children.clone();
        }
        if let Some(dir) = node.source.clone() {
            let mut names: Vec<(String, HostEntry)> = std::fs::read_dir(&dir)
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .filter_map(|entry| {
                    let name = entry.file_name().to_str()?.to_owned();
                    if !is_plain_name(&name) {
                        return None;
                    }
                    let kind = host_entry(&dir, &name)?;
                    Some((name, kind))
                })
                .collect();
            names.sort_by(|a, b| a.0.cmp(&b.0));
            for (name, kind) in names {
                let host = dir.join(&name);
                match kind {
                    HostEntry::Directory => {
                        Self::insert(&mut nodes, index, &name, NodeKind::Directory, Some(host), None, false);
                    }
                    HostEntry::File => {
                        Self::insert(&mut nodes, index, &name, NodeKind::File, Some(host), None, true);
                    }
                }
            }
        } else {
            // On the way to the allowed folders: the next name of each that
            // passes through here.
            let path = Self::path_of(&nodes, index);
            let depth = path.len();
            let mut next: Vec<(String, Option<PathBuf>)> = Vec::new();
            let mut seen = HashSet::new();
            for root in &self.roots {
                if root.parts.len() <= depth || !root.parts.iter().map(|p| key(p)).zip(&path).all(|(a, b)| a == *b) {
                    continue;
                }
                let Some(name) = root.parts.get(depth) else { continue };
                if seen.insert(key(name)) {
                    next.push((name.clone(), (root.parts.len() == depth + 1).then(|| root.host.clone())));
                }
            }
            for (name, host) in next {
                Self::insert(&mut nodes, index, &name, NodeKind::Directory, host, Some(Stat::directory(0)), false);
            }
        }
        if let Some(node) = nodes.get_mut(index) {
            node.listed = true;
            node.children.clone()
        } else {
            Vec::new()
        }
    }

    /// The name of a node as it goes on the wire: decomposed (NFD) on Apple
    /// hosts, where rekordbox converts through `UTF8-MAC` both ways, and as
    /// it is elsewhere.
    pub fn wire_name(&self, index: usize) -> Option<String> {
        let name = self.name(index)?;
        if cfg!(target_vendor = "apple") {
            Some(name.nfd().collect())
        } else {
            Some(name)
        }
    }

    /// Walks a whole slash-separated path from the root, one name at a time.
    pub fn resolve(&self, path: &str) -> Option<usize> {
        let mut at = self.root();
        for part in path.split('/').filter(|part| !part.is_empty()) {
            at = self.child(at, part)?;
        }
        Some(at)
    }

    pub fn kind(&self, index: usize) -> Option<NodeKind> {
        self.read().get(index).map(|node| node.kind)
    }

    pub fn name(&self, index: usize) -> Option<String> {
        self.read().get(index).map(|node| node.name.clone())
    }

    /// The host path behind a node: a file's bytes, or the directory a
    /// host-backed folder reads its names from.
    pub fn source(&self, index: usize) -> Option<PathBuf> {
        self.read().get(index)?.source.clone()
    }

    pub fn attributes(&self, index: usize) -> Option<Attributes> {
        let nodes = self.read();
        let node = nodes.get(index)?;
        let stat = node.source.as_deref().map_or_else(
            || node.stat.unwrap_or_else(|| Stat::directory(0)),
            stat_file,
        );
        Some(Attributes {
            kind: node.kind,
            size: stat.size,
            fileid: self.fileid(index),
            modified: stat.modified,
            stat,
        })
    }

    /// The file id of a node: its place in the table of every export's
    /// nodes, from one, as `NFSv2` file ids are 32-bit and a file id of 0
    /// confuses some clients.
    fn fileid(&self, index: usize) -> u32 {
        u32::try_from(index + 1).ok().and_then(|i| i.checked_add(self.id_base)).unwrap_or(u32::MAX)
    }

    /// Builds the handle for a node.
    pub fn handle(&self, index: usize) -> Option<Handle> {
        let nodes = self.read();
        let node = nodes.get(index)?;
        let mut out = [0_u8; HANDLE_LEN];
        out.get_mut(0..4)?.copy_from_slice(&self.fileid(index).to_be_bytes());
        out.get_mut(4..8)?.copy_from_slice(&self.fileid(node.parent).to_be_bytes());
        out.get_mut(8..12)?.copy_from_slice(&self.fileid(self.root()).to_be_bytes());
        Some(Handle(out))
    }

    /// Resolves a handle back to a node by its three file-id words, rejecting
    /// ids we did not issue.
    ///
    /// Bytes 12..32 are ignored, as rekordbox ignores them: [OBS, static]
    /// libFilSiNE's `tkfNtoHFhandle` (`0x2744`) loads only the words at 0, 4
    /// and 8. [OBS, captures on #43] an XDJ-700 on firmware 1.15 echoes those
    /// three words but writes its own twenty bytes after them
    /// (`0301000000001b5800000000110401…`) in every LOOKUP, to rekordbox and
    /// to us alike; rekordbox answers, so a check of those bytes refuses a
    /// player rekordbox serves.
    pub fn node_of(&self, handle: &Handle) -> Option<usize> {
        let bytes = handle.as_bytes();
        let word = |at: usize| Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
        let (own, parent, root) = (word(0)?, word(4)?, word(8)?);
        let index = usize::try_from(own.checked_sub(self.id_base)?.checked_sub(1)?).ok()?;
        let nodes = self.read();
        let node = nodes.get(index)?;
        if parent != self.fileid(node.parent) || root != self.fileid(self.root()) {
            return None;
        }
        Some(index)
    }
}

/// The whole set of exports a player can mount.
#[derive(Debug, Default, Clone)]
pub struct Exports {
    by_name: HashMap<String, Vfs>,
}

/// File ids per export: every export's nodes are numbered from one table,
/// and an export grows as players look names up, so each gets a range of
/// its own rather than the count of the ones before it.
const IDS_PER_EXPORT: u32 = 1 << 24;

impl Exports {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an export, numbering its nodes in a range after every export
    /// already in.
    pub fn insert(&mut self, mut vfs: Vfs) {
        vfs.id_base = u32::try_from(self.by_name.len()).unwrap_or(u32::MAX).saturating_mul(IDS_PER_EXPORT);
        self.by_name.insert(vfs.export_name().to_owned(), vfs);
    }

    pub fn get(&self, name: &str) -> Option<&Vfs> {
        self.by_name.get(name)
    }

    /// Export names, sorted so a listing is stable between calls.
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.by_name.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }

    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }
}
