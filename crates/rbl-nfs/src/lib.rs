//! A read-only NFS server, in the shape rekordbox presents one.
//!
//! Three RPC programs share one server: portmap, mount, and NFS version 2.
//! Everything here is a pure function from a request datagram to a reply
//! datagram, so the whole protocol is testable without a socket; binding the
//! sockets is the caller's job.
//!
//! # What differs from a standard NFS server
//!
//! - **Ports.** rekordbox answers portmap on **50111**, not 111, and NFS on
//!   2049 with no privileged port anywhere. Both are how the real software
//!   behaves, and a player finds the rest by asking portmap.
//! - **Names are UTF-16LE**, in both mount paths and file names.
//! - **Nothing is writable.** Every mutating procedure is refused with
//!   `NFSERR_ROFS` rather than left unimplemented, so a client that tries gets
//!   an answer it understands.

pub mod net;
pub mod rpc;
pub mod vfs;
pub mod xdr;

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub use vfs::{Attributes, Exports, Handle, NodeKind, Vfs, HANDLE_LEN, MAX_READ};
use xdr::{Reader, Writer};

/// The port rekordbox answers portmap on. A CDJ uses the standard 111; asking
/// rekordbox on 111 gets nothing, which is why this is not a fallback.
pub const REKORDBOX_PORTMAP_PORT: u16 = 50_111;
/// The standard portmap port, which hardware players use.
pub const STANDARD_PORTMAP_PORT: u16 = 111;
/// Where the NFS program itself listens.
pub const NFS_PORT: u16 = 2049;

pub const PROGRAM_PORTMAP: u32 = 100_000;
pub const PROGRAM_MOUNT: u32 = 100_005;
pub const PROGRAM_NFS: u32 = 100_003;
/// Portmap's DUMP procedure, the mapping table.
const PORTMAP_DUMP: u32 = 4;
/// The network lock manager and status monitor, which rekordbox drops.
pub const PROGRAM_NLM: u32 = 100_021;
pub const PROGRAM_NSM: u32 = 100_024;

pub const VERSION_PORTMAP: u32 = 2;
pub const VERSION_MOUNT: u32 = 1;
pub const VERSION_NFS: u32 = 2;

pub const IPPROTO_TCP: u32 = 6;
pub const IPPROTO_UDP: u32 = 17;

/// Portmap procedures.
pub mod portmap_proc {
    pub const NULL: u32 = 0;
    pub const SET: u32 = 1;
    pub const UNSET: u32 = 2;
    pub const GETPORT: u32 = 3;
}

/// Mount procedures.
pub mod mount_proc {
    pub const NULL: u32 = 0;
    pub const MNT: u32 = 1;
    pub const DUMP: u32 = 2;
    pub const UMNT: u32 = 3;
    pub const UMNTALL: u32 = 4;
    pub const EXPORT: u32 = 5;
    pub const EXPORTALL: u32 = 6;
}

/// What a call is, for a log line: `mount MNT`, `nfs READ`, `portmap
/// GETPORT`; a number for what has no name.
pub fn call_name(program: u32, procedure: u32) -> String {
    let (program_name, procedure_name) = match program {
        PROGRAM_PORTMAP => (
            "portmap",
            match procedure {
                portmap_proc::NULL => Some("NULL"),
                portmap_proc::GETPORT => Some("GETPORT"),
                _ => None,
            },
        ),
        PROGRAM_MOUNT => (
            "mount",
            match procedure {
                mount_proc::NULL => Some("NULL"),
                mount_proc::MNT => Some("MNT"),
                mount_proc::DUMP => Some("DUMP"),
                mount_proc::UMNT => Some("UMNT"),
                mount_proc::UMNTALL => Some("UMNTALL"),
                mount_proc::EXPORT => Some("EXPORT"),
                _ => None,
            },
        ),
        PROGRAM_NFS => (
            "nfs",
            match procedure {
                nfs_proc::NULL => Some("NULL"),
                nfs_proc::GETATTR => Some("GETATTR"),
                nfs_proc::SETATTR => Some("SETATTR"),
                nfs_proc::LOOKUP => Some("LOOKUP"),
                nfs_proc::READLINK => Some("READLINK"),
                nfs_proc::READ => Some("READ"),
                nfs_proc::WRITE => Some("WRITE"),
                nfs_proc::CREATE => Some("CREATE"),
                nfs_proc::REMOVE => Some("REMOVE"),
                nfs_proc::RENAME => Some("RENAME"),
                nfs_proc::LINK => Some("LINK"),
                nfs_proc::SYMLINK => Some("SYMLINK"),
                nfs_proc::MKDIR => Some("MKDIR"),
                nfs_proc::RMDIR => Some("RMDIR"),
                nfs_proc::READDIR => Some("READDIR"),
                nfs_proc::STATFS => Some("STATFS"),
                _ => None,
            },
        ),
        _ => ("program", None),
    };
    match (program, procedure_name) {
        (PROGRAM_PORTMAP | PROGRAM_MOUNT | PROGRAM_NFS, Some(name)) => format!("{program_name} {name}"),
        (PROGRAM_PORTMAP | PROGRAM_MOUNT | PROGRAM_NFS, None) => format!("{program_name} procedure {procedure}"),
        _ => format!("program {program} procedure {procedure}"),
    }
}

/// NFS version 2 procedures.
pub mod nfs_proc {
    pub const NULL: u32 = 0;
    pub const GETATTR: u32 = 1;
    pub const SETATTR: u32 = 2;
    pub const LOOKUP: u32 = 4;
    pub const READLINK: u32 = 5;
    pub const READ: u32 = 6;
    pub const WRITE: u32 = 8;
    pub const CREATE: u32 = 9;
    pub const REMOVE: u32 = 10;
    pub const RENAME: u32 = 11;
    pub const LINK: u32 = 12;
    pub const SYMLINK: u32 = 13;
    pub const MKDIR: u32 = 14;
    pub const RMDIR: u32 = 15;
    pub const READDIR: u32 = 16;
    pub const STATFS: u32 = 17;
}

/// NFS version 2 status codes.
pub mod nfs_status {
    pub const OK: u32 = 0;
    pub const PERM: u32 = 1;
    pub const NOENT: u32 = 2;
    pub const IO: u32 = 5;
    pub const ACCES: u32 = 13;
    pub const NOTDIR: u32 = 20;
    pub const ISDIR: u32 = 21;
    pub const ROFS: u32 = 30;
    pub const NAMETOOLONG: u32 = 63;
    pub const STALE: u32 = 70;
}

/// `NFSv2` file types.
mod file_type {
    pub const REGULAR: u32 = 1;
    pub const DIRECTORY: u32 = 2;
}

/// The longest single component a player may look up. `NFSv2`'s own limit.
pub const MAX_NAME: usize = 255;

/// A read-only NFS server over a set of exports.
#[derive(Debug)]
pub struct Server {
    exports: Exports,
    /// The port the NFS program is bound to, reported by portmap.
    nfs_port: u16,
    /// The port the mount program is bound to, reported by portmap.
    mount_port: u16,
    /// The host group each export is offered to, `<ip>/<netmask>`, as
    /// rekordbox names its own subnet in the mount EXPORT reply. A CDJ checks
    /// itself against this list and will not mount an export that offers none.
    export_host: Option<String>,
    /// Serializes host reads, as libFilSiNE holds `_tkvFSSem` while opening,
    /// seeking, reading, obtaining attributes, and closing a file.
    file_reads: Mutex<()>,
    /// Non-zero once the link is up. rekordbox adds its export list only
    /// on link-up, so until then the EXPORT reply lists nothing and a mount
    /// is refused; a server made without a gate is up from the start.
    up: Option<Arc<std::sync::atomic::AtomicU8>>,
    /// Who has each export mounted, by export name: rekordbox keeps a host
    /// list per export, filled by MNT and emptied by UMNT, and counts the
    /// mounts to know when a player is ready to be sent a track.
    mounts: Mutex<HashMap<String, Vec<Ipv4Addr>>>,
    /// The last replies, by who asked and the call's first 24 bytes (xid,
    /// message type, RPC version, program, version, procedure): a
    /// retransmitted call gets the same reply again rather than a second
    /// execution, as libFilSiNE's twenty-entry cache does.
    replies: Mutex<std::collections::VecDeque<(ReplyKey, Vec<u8>)>>,
}

/// What tells one call from another in the reply cache.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ReplyKey {
    receiver: Receiver,
    from: Ipv4Addr,
    port: u16,
    head: Vec<u8>,
}

/// Identity of one receiving socket for the lifetime of its serving loop.
/// Fresh tokens keep cache entries distinct even after a socket is rebound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Receiver(u64);

impl Default for Receiver {
    fn default() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }
}

/// How many replies are kept for retransmits.
const REPLY_CACHE: usize = 20;
/// The bytes of a call that identify it in the cache.
const REPLY_KEY_LEN: usize = 24;

impl Server {
    pub fn new(exports: Exports, nfs_port: u16, mount_port: u16) -> Self {
        Self {
            exports,
            nfs_port,
            mount_port,
            export_host: None,
            file_reads: Mutex::new(()),
            up: None,
            mounts: Mutex::new(HashMap::new()),
            replies: Mutex::new(std::collections::VecDeque::with_capacity(REPLY_CACHE)),
        }
    }

    /// Offers the exports only while `up` is non-zero: the link's device
    /// number, settled by the join.
    #[must_use]
    pub fn with_gate(mut self, up: Arc<std::sync::atomic::AtomicU8>) -> Self {
        self.up = Some(up);
        self
    }

    fn is_up(&self) -> bool {
        self.up.as_ref().is_none_or(|up| up.load(std::sync::atomic::Ordering::Relaxed) != 0)
    }

    /// Whether `host` is in the export's subnet — the one permission entry
    /// rekordbox adds, its own `<ip>/<netmask>`; every host is allowed when
    /// no group was set (loopback tests).
    fn allows(&self, host: Ipv4Addr) -> bool {
        let Some(group) = self.export_host.as_deref() else { return true };
        let Some((ip, mask)) = group.split_once('/') else { return true };
        let (Ok(ip), Ok(mask)) = (ip.parse::<Ipv4Addr>(), mask.parse::<Ipv4Addr>()) else { return true };
        (u32::from(host) ^ u32::from(ip)) & u32::from(mask) == 0
    }

    /// The hosts with an export mounted, each once.
    pub fn mounted_hosts(&self) -> Vec<Ipv4Addr> {
        let mounts = self.mounts.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut hosts: Vec<Ipv4Addr> = mounts.values().flatten().copied().collect();
        hosts.sort_unstable();
        hosts.dedup();
        hosts
    }

    /// Whether `host` has any export mounted: what gates a load command.
    pub fn is_mounted(&self, host: Ipv4Addr) -> bool {
        let mounts = self.mounts.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        mounts.values().any(|hosts| hosts.contains(&host))
    }

    /// Sets the host group the exports are offered to (`<ip>/<netmask>`), which
    /// a CDJ requires in the EXPORT reply before it will mount.
    #[must_use]
    pub fn with_export_host(mut self, host: impl Into<String>) -> Self {
        self.export_host = Some(host.into());
        self
    }

    pub fn exports(&self) -> &Exports {
        &self.exports
    }

    /// Answers one request datagram from an unnamed peer: loopback, for the
    /// tests.
    pub fn handle(&self, datagram: &[u8]) -> Option<Vec<u8>> {
        self.handle_from(datagram, Ipv4Addr::LOCALHOST, 0)
    }

    /// Answers one request datagram from `from:port`.
    ///
    /// `None` means "say nothing": the datagram was not an RPC call we can
    /// even address a reply to, or one of the lock-manager calls rekordbox
    /// drops without a word. A malformed call we *can* identify still gets a
    /// reply, because silence is what a client times out on. A call seen
    /// before from the same peer gets the reply it got then.
    pub fn handle_from(&self, datagram: &[u8], from: Ipv4Addr, port: u16) -> Option<Vec<u8>> {
        self.handle_on(datagram, from, port, Receiver(0))
    }

    /// Handles a datagram on the explicitly identified receiving socket.
    pub fn handle_on(&self, datagram: &[u8], from: Ipv4Addr, port: u16, receiver: Receiver) -> Option<Vec<u8>> {
        let key = datagram
            .get(..REPLY_KEY_LEN)
            .map(|head| ReplyKey { receiver, from, port, head: head.to_vec() });
        if let Some(key) = &key {
            let replies = self.replies.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some((_, reply)) = replies.iter().find(|(k, _)| k == key) {
                tracing::trace!(%from, port, "call seen before; its reply sent again");
                return Some(reply.clone());
            }
        }
        let reply = self.answer(datagram, from)?;
        if let Some(key) = key {
            let mut replies = self.replies.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if replies.len() >= REPLY_CACHE {
                replies.pop_front();
            }
            replies.push_back((key, reply.clone()));
        }
        Some(reply)
    }

    fn answer(&self, datagram: &[u8], from: Ipv4Addr) -> Option<Vec<u8>> {
        let call = match rpc::Call::decode(datagram) {
            Ok(call) => call,
            Err(rpc::RpcError::Version(version)) => {
                // We can still read the xid: tell it which version we speak.
                let xid = u32::from_be_bytes([
                    *datagram.first()?,
                    *datagram.get(1)?,
                    *datagram.get(2)?,
                    *datagram.get(3)?,
                ]);
                tracing::warn!(xid, version, "RPC call of a version this server does not speak");
                return Some(rpc::rpc_mismatch(xid, rpc::RPC_VERSION, rpc::RPC_VERSION));
            }
            Err(error) => {
                tracing::debug!(%error, len = datagram.len(), "datagram is not an RPC call; ignored");
                return None;
            }
        };
        tracing::trace!(
            xid = call.xid,
            call = %call_name(call.program, call.procedure),
            version = call.version,
            args = call.arguments.len(),
            "RPC call"
        );

        let reply = match call.program {
            PROGRAM_PORTMAP => self.portmap(&call, from),
            PROGRAM_MOUNT => self.mount(&call, from),
            PROGRAM_NFS => self.nfs(&call),
            // The lock and status monitors: rekordbox says nothing at all
            // to these, and a client that asks stops asking.
            PROGRAM_NLM | PROGRAM_NSM => {
                tracing::debug!(xid = call.xid, program = call.program, "lock-manager call; dropped without a reply");
                return None;
            }
            _ => {
                tracing::warn!(xid = call.xid, program = call.program, "RPC call for a program this server does not have");
                rpc::accepted_empty(call.xid, rpc::accept::PROG_UNAVAIL)
            }
        };
        tracing::trace!(xid = call.xid, len = reply.len(), "RPC reply");
        Some(reply)
    }

    fn portmap(&self, call: &rpc::Call<'_>, from: Ipv4Addr) -> Vec<u8> {
        if call.version != VERSION_PORTMAP {
            return rpc::program_mismatch(call.xid, VERSION_PORTMAP, VERSION_PORTMAP);
        }
        match call.procedure {
            portmap_proc::NULL => rpc::accepted_empty(call.xid, rpc::accept::SUCCESS),
            // V7 registration is allowed only from exactly 127.0.0.1.
            // Local dynamic registration is not implemented: retain explicit
            // PROC_UNAVAIL there while answering remote refusal as false.
            portmap_proc::SET | portmap_proc::UNSET if from != Ipv4Addr::LOCALHOST => {
                let mut writer = rpc::accepted(call.xid, rpc::accept::SUCCESS);
                writer.u32(0);
                writer.into_bytes()
            }
            // Static mapping list; dynamic lifecycle and dump-length parity
            // remain separate evidence tasks.
            PORTMAP_DUMP => {
                let mut writer = rpc::accepted(call.xid, rpc::accept::SUCCESS);
                for (program, version, port) in [
                    (PROGRAM_PORTMAP, VERSION_PORTMAP, REKORDBOX_PORTMAP_PORT),
                    (PROGRAM_NFS, VERSION_NFS, self.nfs_port),
                    (PROGRAM_MOUNT, VERSION_MOUNT, self.mount_port),
                ] {
                    writer.some().u32(program).u32(version).u32(IPPROTO_UDP).u32(u32::from(port));
                }
                writer.none();
                writer.into_bytes()
            }
            portmap_proc::GETPORT => {
                let mut reader = call.reader();
                let (Ok(program), Ok(version), Ok(protocol)) =
                    (reader.u32(), reader.u32(), reader.u32())
                else {
                    return rpc::accepted_empty(call.xid, rpc::accept::GARBAGE_ARGS);
                };
                // Zero means "not registered", which is the correct answer for
                // a program we do not serve, and for TCP, which we do not bind.
                let port = if protocol == IPPROTO_UDP {
                    match program {
                        PROGRAM_NFS if version == VERSION_NFS => self.nfs_port,
                        PROGRAM_MOUNT if version == VERSION_MOUNT => self.mount_port,
                        _ => 0,
                    }
                } else {
                    0
                };
                tracing::trace!(xid = call.xid, program, protocol, port, "portmap GETPORT answered");
                let mut writer = rpc::accepted(call.xid, rpc::accept::SUCCESS);
                writer.u32(u32::from(port));
                writer.into_bytes()
            }
            _ => rpc::accepted_empty(call.xid, rpc::accept::PROC_UNAVAIL),
        }
    }

    fn mount(&self, call: &rpc::Call<'_>, from: Ipv4Addr) -> Vec<u8> {
        if call.version != VERSION_MOUNT {
            return rpc::program_mismatch(call.xid, VERSION_MOUNT, VERSION_MOUNT);
        }
        match call.procedure {
            mount_proc::NULL => rpc::accepted_empty(call.xid, rpc::accept::SUCCESS),
            // rekordbox takes the caller off the export's host list; with
            // nothing held per mount here, the list is the whole of it.
            mount_proc::UMNT | mount_proc::UMNTALL => {
                let path = if call.procedure == mount_proc::UMNT {
                    let Ok(path) = call.reader().utf16() else {
                        // V7 emits a void success but does not remove hosts
                        // when decoding a specific UMNT path fails.
                        return rpc::accepted_empty(call.xid, rpc::accept::SUCCESS);
                    };
                    Some(path)
                } else {
                    None
                };
                let mut mounts = self.mounts.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                for (export, hosts) in mounts.iter_mut() {
                    if path.as_deref().is_none_or(|p| p == export) && hosts.contains(&from) {
                        hosts.retain(|h| *h != from);
                        tracing::info!(xid = call.xid, host = %from, %export, "player unmounted an export");
                    }
                }
                rpc::accepted_empty(call.xid, rpc::accept::SUCCESS)
            }
            mount_proc::MNT => {
                let mut reader = call.reader();
                let Ok(path) = reader.utf16() else {
                    tracing::warn!(xid = call.xid, "mount request with an unreadable path");
                    return rpc::accepted_empty(call.xid, rpc::accept::GARBAGE_ARGS);
                };
                let mut writer = rpc::accepted(call.xid, rpc::accept::SUCCESS);
                // Denied and unknown alike are `ACCES`, as libFilSiNE answers
                // them; before the link is up there is no export to mount.
                let export = self.is_up().then(|| self.exports.get(&path)).flatten();
                match export.and_then(|vfs| vfs.handle(vfs.root())) {
                    Some(handle) if self.allows(from) => {
                        let mut mounts = self.mounts.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                        let hosts = mounts.entry(path.clone()).or_default();
                        if !hosts.contains(&from) {
                            hosts.push(from);
                        }
                        let now: usize = mounts.values().map(Vec::len).sum();
                        tracing::info!(xid = call.xid, host = %from, %path, mounts = now, %handle, "player mounted an export");
                        writer.u32(nfs_status::OK).opaque_fixed(handle.as_bytes());
                    }
                    Some(_) => {
                        tracing::warn!(xid = call.xid, host = %from, %path, "mount from outside the export's subnet; refused");
                        writer.u32(nfs_status::ACCES);
                    }
                    None => {
                        tracing::warn!(xid = call.xid, host = %from, %path, up = self.is_up(), exports = ?self.exports.names(), "mount of a path that is not exported");
                        writer.u32(nfs_status::ACCES);
                    }
                }
                writer.into_bytes()
            }
            mount_proc::EXPORT | mount_proc::EXPORTALL => {
                tracing::debug!(
                    xid = call.xid,
                    exports = ?self.exports.names(),
                    host = self.export_host.as_deref().unwrap_or("-"),
                    up = self.is_up(),
                    "export list asked for"
                );
                let mut writer = rpc::accepted(call.xid, rpc::accept::SUCCESS);
                let names = if self.is_up() { self.exports.names() } else { Vec::new() };
                for name in names {
                    // Each entry is an optional-list link: present, the export
                    // name, then its group list. rekordbox 7.2.11 lists one
                    // group, `<ip>/<netmask>` for its own subnet (measured on
                    // the wire 2026-09-13), and a CDJ mounts nothing whose
                    // group list is empty — so the host is emitted as one
                    // group, in ASCII as rekordbox sends it.
                    writer.some().utf16(name);
                    if let Some(host) = &self.export_host {
                        writer.some().string(host).none();
                    } else {
                        writer.none();
                    }
                }
                writer.none();
                writer.into_bytes()
            }
            _ => {
                tracing::warn!(xid = call.xid, procedure = call.procedure, "mount procedure this server does not have");
                rpc::accepted_empty(call.xid, rpc::accept::PROC_UNAVAIL)
            }
        }
    }

    fn nfs(&self, call: &rpc::Call<'_>) -> Vec<u8> {
        if call.version != VERSION_NFS {
            return rpc::program_mismatch(call.xid, VERSION_NFS, VERSION_NFS);
        }
        match call.procedure {
            nfs_proc::NULL => rpc::accepted_empty(call.xid, rpc::accept::SUCCESS),
            nfs_proc::GETATTR => self.getattr(call),
            nfs_proc::LOOKUP => self.lookup(call),
            nfs_proc::READ => self.read(call),
            nfs_proc::READDIR => self.readdir(call),
            nfs_proc::STATFS => self.statfs(call),
            // Every mutating procedure, answered as libFilSiNE's stubs answer
            // it: STALE for the writes, ACCES for links and readlink — not
            // the ROFS a read-only server would say.
            nfs_proc::SETATTR
            | nfs_proc::WRITE
            | nfs_proc::CREATE
            | nfs_proc::REMOVE
            | nfs_proc::RENAME
            | nfs_proc::MKDIR
            | nfs_proc::RMDIR => Self::status_only(call.xid, nfs_status::STALE),
            nfs_proc::LINK | nfs_proc::SYMLINK | nfs_proc::READLINK => Self::status_only(call.xid, nfs_status::ACCES),
            _ => rpc::accepted_empty(call.xid, rpc::accept::PROC_UNAVAIL),
        }
    }

    fn status_only(xid: u32, status: u32) -> Vec<u8> {
        let mut writer = rpc::accepted(xid, rpc::accept::SUCCESS);
        writer.u32(status);
        writer.into_bytes()
    }

    /// Finds the export a handle belongs to. A handle carries no export id, so
    /// this asks each in turn — the tag check makes a wrong export reject it.
    fn locate(&self, handle: &Handle) -> Option<(&Vfs, usize)> {
        self.exports
            .names()
            .into_iter()
            .filter_map(|name| self.exports.get(name))
            .find_map(|vfs| vfs.node_of(handle).map(|index| (vfs, index)))
    }

    fn read_handle(reader: &mut Reader<'_>) -> Option<Handle> {
        Handle::from_slice(reader.opaque_fixed(HANDLE_LEN).ok()?)
    }

    fn getattr(&self, call: &rpc::Call<'_>) -> Vec<u8> {
        let mut reader = call.reader();
        let Some(handle) = Self::read_handle(&mut reader) else {
            return rpc::accepted_empty(call.xid, rpc::accept::GARBAGE_ARGS);
        };
        let Some((vfs, index)) = self.locate(&handle) else {
            tracing::warn!(xid = call.xid, handle = %handle, "getattr of a stale handle");
            return Self::status_only(call.xid, nfs_status::STALE);
        };
        let Some(attributes) = vfs.attributes(index) else {
            return Self::status_only(call.xid, nfs_status::STALE);
        };
        let mut writer = rpc::accepted(call.xid, rpc::accept::SUCCESS);
        writer.u32(nfs_status::OK);
        write_attributes(&mut writer, &attributes);
        writer.into_bytes()
    }

    fn lookup(&self, call: &rpc::Call<'_>) -> Vec<u8> {
        let mut reader = call.reader();
        let Some(handle) = Self::read_handle(&mut reader) else {
            return rpc::accepted_empty(call.xid, rpc::accept::GARBAGE_ARGS);
        };
        let Ok(name) = reader.utf16() else {
            return rpc::accepted_empty(call.xid, rpc::accept::GARBAGE_ARGS);
        };
        if name.len() > MAX_NAME {
            tracing::warn!(xid = call.xid, len = name.len(), "lookup of a name too long");
            return Self::status_only(call.xid, nfs_status::NAMETOOLONG);
        }
        let Some((vfs, parent)) = self.locate(&handle) else {
            tracing::warn!(xid = call.xid, %name, handle = %handle, "lookup under a stale handle");
            return Self::status_only(call.xid, nfs_status::STALE);
        };
        if vfs.kind(parent) != Some(NodeKind::Directory) {
            tracing::warn!(xid = call.xid, %name, "lookup under a file, not a directory");
            return Self::status_only(call.xid, nfs_status::NOTDIR);
        }
        let found = vfs
            .child(parent, &name)
            .and_then(|index| Some((vfs.handle(index)?, vfs.attributes(index)?)));
        let Some((child_handle, attributes)) = found else {
            tracing::trace!(xid = call.xid, parent = %vfs.name(parent).unwrap_or_default(), %name, "lookup found nothing");
            return Self::status_only(call.xid, nfs_status::NOENT);
        };
        tracing::trace!(xid = call.xid, %name, size = attributes.size, "lookup found a node");
        let mut writer = rpc::accepted(call.xid, rpc::accept::SUCCESS);
        writer.u32(nfs_status::OK).opaque_fixed(child_handle.as_bytes());
        write_attributes(&mut writer, &attributes);
        writer.into_bytes()
    }

    fn read(&self, call: &rpc::Call<'_>) -> Vec<u8> {
        let mut reader = call.reader();
        let Some(handle) = Self::read_handle(&mut reader) else {
            return rpc::accepted_empty(call.xid, rpc::accept::GARBAGE_ARGS);
        };
        let (Ok(offset), Ok(count)) = (reader.u32(), reader.u32()) else {
            return rpc::accepted_empty(call.xid, rpc::accept::GARBAGE_ARGS);
        };
        let Some((vfs, index)) = self.locate(&handle) else {
            tracing::warn!(xid = call.xid, offset, count, handle = %handle, "read through a stale handle");
            return Self::status_only(call.xid, nfs_status::STALE);
        };
        if vfs.kind(index) == Some(NodeKind::Directory) {
            tracing::warn!(xid = call.xid, "read of a directory");
            return Self::status_only(call.xid, nfs_status::ISDIR);
        }
        let Some(source) = vfs.source(index) else {
            tracing::warn!(xid = call.xid, "read of a node with no file behind it");
            return Self::status_only(call.xid, nfs_status::STALE);
        };

        let wanted = (count as usize).min(MAX_READ);
        if offset == 0 {
            tracing::debug!(xid = call.xid, file = %source.display(), "player started reading a file");
        }
        // A file that vanished between the export and the read is the normal
        // case here, not an I/O fault worth distinguishing.
        let guard = self.file_reads.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut file = match File::open(&source) {
            Ok(file) => file,
            Err(error) => {
                tracing::warn!(xid = call.xid, file = %source.display(), %error, "open for read failed");
                return Self::status_only(call.xid, nfs_status::IO);
            }
        };
        let started = std::time::Instant::now();
        let data = match read_at(&mut file, u64::from(offset), wanted) {
            Ok(data) => data,
            Err(error) => {
                tracing::warn!(xid = call.xid, file = %source.display(), offset, wanted, %error, "read failed");
                return Self::status_only(call.xid, nfs_status::IO);
            }
        };
        // [OBS] libFilSiNE `_tkfFSReadFile` (filsine.c:2266–2329) answers
        // IO for every zero-byte fread, including a requested count of zero.
        if data.is_empty() {
            tracing::trace!(xid = call.xid, offset, "read at the end of the file; IO, as rekordbox answers");
            return Self::status_only(call.xid, nfs_status::IO);
        }
        let Some(attributes) = vfs.attributes(index) else {
            return Self::status_only(call.xid, nfs_status::STALE);
        };
        // [STATIC] `_tkfFSReadFile` obtains attributes after fread, then
        // closes the file before the RPC reply is constructed.
        drop(file);
        drop(guard);
        tracing::trace!(xid = call.xid, offset, wanted, got = data.len(), elapsed_us = started.elapsed().as_micros(), "read served");

        let mut writer = rpc::accepted(call.xid, rpc::accept::SUCCESS);
        writer.u32(nfs_status::OK);
        write_attributes(&mut writer, &attributes);
        writer.opaque(&data);
        writer.into_bytes()
    }

    fn readdir(&self, call: &rpc::Call<'_>) -> Vec<u8> {
        let mut reader = call.reader();
        let Some(handle) = Self::read_handle(&mut reader) else {
            return rpc::accepted_empty(call.xid, rpc::accept::GARBAGE_ARGS);
        };
        let (Ok(cookie), Ok(count)) = (reader.u32(), reader.u32()) else {
            return rpc::accepted_empty(call.xid, rpc::accept::GARBAGE_ARGS);
        };
        let Some((vfs, index)) = self.locate(&handle) else {
            tracing::warn!(xid = call.xid, handle = %handle, "readdir of a stale handle");
            return Self::status_only(call.xid, nfs_status::STALE);
        };
        if vfs.kind(index) != Some(NodeKind::Directory) {
            return Self::status_only(call.xid, nfs_status::NOTDIR);
        }

        let children = vfs.children(index);

        // The cookie is the file id of the last entry sent, as libFilSiNE
        // sets it, so a resumed listing continues after that child; zero
        // starts over, and a cookie naming no child is IO, as there.
        let start = if cookie == 0 {
            0
        } else if let Some(position) =
            children.iter().position(|child| vfs.attributes(*child).is_some_and(|a| a.fileid == cookie))
        {
            position + 1
        } else {
            tracing::warn!(xid = call.xid, cookie, "readdir cookie names no child; IO");
            return Self::status_only(call.xid, nfs_status::IO);
        };
        let mut writer = rpc::accepted(call.xid, rpc::accept::SUCCESS);
        writer.u32(nfs_status::OK);
        let budget = (count as usize).clamp(512, 8192);
        let mut at = start;
        while let Some(child) = children.get(at) {
            let (Some(name), Some(attributes)) = (vfs.wire_name(*child), vfs.attributes(*child)) else {
                break;
            };
            // Entry size: present flag, fileid, name (length + padded UTF-16LE),
            // cookie. Stop before overrunning what the client asked for.
            let entry_len = 4 + 4 + 4 + xdr::padded(name.encode_utf16().count() * 2) + 4;
            if writer.len() + entry_len + 8 > budget {
                break;
            }
            at += 1;
            writer.some().u32(attributes.fileid).utf16(&name).u32(attributes.fileid);
        }
        let eof = at >= children.len();
        tracing::trace!(
            xid = call.xid,
            directory = %vfs.name(index).unwrap_or_default(),
            cookie,
            sent = at - start,
            of = children.len(),
            eof,
            "readdir served"
        );
        writer.none().u32(u32::from(eof));
        writer.into_bytes()
    }

    fn statfs(&self, call: &rpc::Call<'_>) -> Vec<u8> {
        let mut reader = call.reader();
        let Some(handle) = Self::read_handle(&mut reader) else {
            return rpc::accepted_empty(call.xid, rpc::accept::GARBAGE_ARGS);
        };
        let Some((vfs, index)) = self.locate(&handle) else {
            tracing::warn!(xid = call.xid, handle = %handle, "statfs of a stale handle");
            return Self::status_only(call.xid, nfs_status::STALE);
        };
        // The host filesystem's own figures, as libFilSiNE reports them:
        // `tsize` its preferred I/O size (a mebibyte on APFS, which is more
        // than a READ may carry), the block size and counts from `statfs`
        // of the file behind the node — or of the export's root, where a
        // directory of the tree has no file behind it.
        let host = vfs
            .source(index)
            .or_else(|| Some(PathBuf::from(vfs.export_name())))
            .and_then(|path| host_statfs(&path));
        let (tsize, bsize, blocks, bfree, bavail) = host.unwrap_or((u32::try_from(MAX_READ).unwrap_or(8192), 4096, 0, 0, 0));
        let mut writer = rpc::accepted(call.xid, rpc::accept::SUCCESS);
        writer.u32(nfs_status::OK).u32(tsize).u32(bsize).u32(blocks).u32(bfree).u32(bavail);
        writer.into_bytes()
    }
}

/// `statfs` of the host path: `tsize`, `bsize`, `blocks`, `bfree`,
/// `bavail`, each in its low 32 bits, as libFilSiNE reports them.
#[cfg(unix)]
fn host_statfs(path: &Path) -> Option<(u32, u32, u32, u32, u32)> {
    let stat = nix::sys::statfs::statfs(path).ok()?;
    // The figures' types differ by platform (signed on some, wider on
    // others); each is taken in its low 32 bits, as libFilSiNE takes them.
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation, clippy::cast_lossless, clippy::unnecessary_cast)]
    let low = |v: i128| (v as u64 & u64::from(u32::MAX)) as u32;
    #[allow(clippy::cast_lossless, clippy::unnecessary_cast)]
    Some((
        low(stat.optimal_transfer_size() as i128),
        low(stat.block_size() as i128),
        low(stat.blocks() as i128),
        low(stat.blocks_free() as i128),
        low(stat.blocks_available() as i128),
    ))
}

/// Windows has no `statfs`; the figures fall back to what the caller
/// substitutes.
#[cfg(not(unix))]
fn host_statfs(_path: &Path) -> Option<(u32, u32, u32, u32, u32)> {
    None
}

/// Writes an `NFSv2` `fattr`: seventeen 32-bit fields, no padding. The
/// figures are the host's, as libFilSiNE hands them out, with `fsid` 2.
fn write_attributes(writer: &mut Writer, attributes: &Attributes) {
    let kind = match attributes.kind {
        NodeKind::Directory => file_type::DIRECTORY,
        NodeKind::File => file_type::REGULAR,
    };
    let stat = &attributes.stat;
    let size = u32::try_from(attributes.size).unwrap_or(u32::MAX);
    writer
        .u32(kind)
        .u32(stat.mode)
        .u32(stat.nlink)
        .u32(stat.uid)
        .u32(stat.gid)
        .u32(size)
        .u32(stat.blocksize)
        .u32(stat.rdev)
        .u32(stat.blocks)
        .u32(2) // fsid
        .u32(attributes.fileid);
    for seconds in [stat.accessed, stat.modified, stat.changed] {
        writer.u32(seconds).u32(0);
    }
}

/// Reads at most `len` bytes from `offset`. A short read at the end of the
/// file is not an error; NFS signals the end by returning fewer bytes.
fn read_at(file: &mut File, offset: u64, len: usize) -> std::io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(offset))?;
    let mut out = vec![0_u8; len];
    let mut filled = 0;
    while filled < len {
        match file.read(out.get_mut(filled..).unwrap_or(&mut []))? {
            0 => break,
            n => filled += n,
        }
    }
    out.truncate(filled);
    Ok(out)
}

/// Splits an absolute path into the export a player mounts and the path within
/// it, following rekordbox's own convention.
///
/// macOS exports `/`, so `/Users/x/a.mp3` mounts `/` and reads `Users/x/a.mp3`.
/// Windows exports the drive, so `C:\Users\x\a.mp3` mounts `/C/` and reads
/// `Users/x/a.mp3`.
pub fn split_export(path: &str) -> Option<(String, String)> {
    let mut chars = path.chars();
    let (Some(drive), Some(':'), Some(separator)) = (chars.next(), chars.next(), chars.next())
    else {
        return path
            .strip_prefix('/')
            .map(|rest| ("/".to_owned(), rest.replace('\\', "/")));
    };
    if separator != '/' && separator != '\\' {
        return None;
    }
    let rest: String = chars.collect();
    Some((
        format!("/{}/", drive.to_ascii_uppercase()),
        rest.replace('\\', "/"),
    ))
}
