//! A stand-in player: the client half of the link protocols.
//!
//! It exists to exercise our servers end to end over loopback, and — once
//! rekordbox can be stopped — to record what the real software answers, so the
//! servers can be checked against a capture instead of against the spec alone.
//!
//! It is deliberately a client only. Pointing it at real rekordbox reads;
//! nothing here writes anything anywhere.

use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, UdpSocket};
use std::time::Duration;

use rbl_dbserver::{Argument, Message, PORT_QUERY_REQUEST};
use rbl_nfs::rpc::{self, Auth, Call, Reply};
use rbl_nfs::xdr::{Reader, Writer};
use rbl_nfs::{
    mount_proc, nfs_proc, nfs_status, portmap_proc, IPPROTO_UDP, PROGRAM_MOUNT, PROGRAM_NFS,
    PROGRAM_PORTMAP, VERSION_MOUNT, VERSION_NFS, VERSION_PORTMAP,
};

/// How long a single request waits before it is retried or given up on.
const TIMEOUT: Duration = Duration::from_secs(2);
/// A datagram reply never legitimately exceeds this: a 32 KB `READ` plus its
/// attributes is the largest, and it arrives reassembled from IP fragments.
const DATAGRAM: usize = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum CdjError {
    #[error("network: {0}")]
    Io(#[from] io::Error),
    #[error("the reply could not be decoded: {0}")]
    Rpc(#[from] rpc::RpcError),
    #[error("the reply could not be decoded: {0}")]
    Xdr(#[from] rbl_nfs::xdr::XdrError),
    #[error("the server refused the request: NFS status {0}")]
    Nfs(u32),
    #[error("the RPC call was not accepted (reply {reply}, accept {accept})")]
    Rejected { reply: u32, accept: u32 },
    #[error("the program is not registered with portmap")]
    NotRegistered,
    #[error("no export named {0:?}")]
    NoExport(String),
    #[error("the database server did not answer the port query")]
    NoDatabasePort,
    #[error("{0} is not a file that can be read")]
    NotAFile(String),
}

pub type Result<T> = std::result::Result<T, CdjError>;

/// An RPC client over one UDP socket.
#[derive(Debug)]
pub struct RpcClient {
    socket: UdpSocket,
    server: SocketAddr,
    xid: u32,
}

impl RpcClient {
    pub fn connect(server: SocketAddr) -> Result<Self> {
        // Bind on the same family as the server, on any free port.
        let any: IpAddr = if server.is_ipv4() {
            IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)
        } else {
            IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED)
        };
        let socket = UdpSocket::bind(SocketAddr::new(any, 0))?;
        socket.set_read_timeout(Some(TIMEOUT))?;
        Ok(Self {
            socket,
            server,
            xid: 1,
        })
    }

    /// The address this client sends to. Used when switching to another port
    /// that portmap has just named.
    pub const fn server(&self) -> SocketAddr {
        self.server
    }

    pub fn retarget(&mut self, port: u16) {
        self.server.set_port(port);
    }

    /// Makes one call and returns the procedure's results.
    pub fn call(
        &mut self,
        program: u32,
        version: u32,
        procedure: u32,
        arguments: &[u8],
    ) -> Result<Vec<u8>> {
        self.xid = self.xid.wrapping_add(1);
        let request = Call {
            xid: self.xid,
            program,
            version,
            procedure,
            // The stamp a real player sends. Our server ignores it; rekordbox
            // may not, so the recording path must look like a player.
            credential: Auth {
                flavor: rpc::AUTH_UNIX,
                body: unix_credential(),
            },
            verifier: Auth::null(),
            arguments,
        }
        .encode();

        self.socket.send_to(&request, self.server)?;

        // Replies to an earlier, timed-out call can still arrive; skip any
        // whose transaction id is not the one we are waiting for.
        let mut buffer = vec![0_u8; DATAGRAM];
        loop {
            let (len, _) = self.socket.recv_from(&mut buffer)?;
            let bytes = buffer.get(..len).unwrap_or(&[]);
            let Ok(reply) = Reply::decode(bytes) else {
                continue;
            };
            if reply.xid != self.xid {
                continue;
            }
            if !reply.is_success() {
                return Err(CdjError::Rejected {
                    reply: reply.reply_status,
                    accept: reply.accept_status,
                });
            }
            return Ok(reply.results.to_vec());
        }
    }
}

/// The `AUTH_UNIX` body a CDJ sends: a stamp, an empty machine name, and no ids.
fn unix_credential() -> Vec<u8> {
    let mut writer = Writer::with_capacity(24);
    writer
        .u32(rpc::CDJ_AUTH_STAMP)
        .string("")
        .u32(0) // uid
        .u32(0) // gid
        .u32(0); // no supplementary groups
    writer.into_bytes()
}

/// A mounted export, ready to be walked and read.
#[derive(Debug)]
pub struct Mounted {
    nfs: RpcClient,
    root: [u8; rbl_nfs::HANDLE_LEN],
}

/// Asks portmap for a program's port, mounts an export, and returns a client
/// positioned at its root.
pub fn mount(portmap: SocketAddr, export: &str) -> Result<Mounted> {
    let mut client = RpcClient::connect(portmap)?;

    let mount_port = get_port(&mut client, PROGRAM_MOUNT, VERSION_MOUNT)?;
    let nfs_port = get_port(&mut client, PROGRAM_NFS, VERSION_NFS)?;

    let mut mount_client = RpcClient::connect(SocketAddr::new(portmap.ip(), mount_port))?;
    let exports = list_exports(&mut mount_client)?;
    if !exports.iter().any(|name| name == export) {
        return Err(CdjError::NoExport(export.to_owned()));
    }

    let mut arguments = Writer::new();
    arguments.utf16(export);
    let results = mount_client.call(
        PROGRAM_MOUNT,
        VERSION_MOUNT,
        mount_proc::MNT,
        &arguments.into_bytes(),
    )?;
    let mut reader = Reader::new(&results);
    let status = reader.u32()?;
    if status != nfs_status::OK {
        return Err(CdjError::Nfs(status));
    }
    let mut root = [0_u8; rbl_nfs::HANDLE_LEN];
    root.copy_from_slice(reader.opaque_fixed(rbl_nfs::HANDLE_LEN)?);

    let nfs = RpcClient::connect(SocketAddr::new(portmap.ip(), nfs_port))?;
    Ok(Mounted { nfs, root })
}

fn get_port(client: &mut RpcClient, program: u32, version: u32) -> Result<u16> {
    let mut arguments = Writer::new();
    arguments.u32(program).u32(version).u32(IPPROTO_UDP).u32(0);
    let results = client.call(
        PROGRAM_PORTMAP,
        VERSION_PORTMAP,
        portmap_proc::GETPORT,
        &arguments.into_bytes(),
    )?;
    let port = Reader::new(&results).u32()?;
    u16::try_from(port)
        .ok()
        .filter(|p| *p != 0)
        .ok_or(CdjError::NotRegistered)
}

fn list_exports(client: &mut RpcClient) -> Result<Vec<String>> {
    let results = client.call(PROGRAM_MOUNT, VERSION_MOUNT, mount_proc::EXPORT, &[])?;
    let mut reader = Reader::new(&results);
    let mut names = Vec::new();
    while reader.u32()? == 1 {
        names.push(reader.utf16()?);
        // The group list, which we do not use.
        while reader.u32()? == 1 {
            reader.skip_opaque()?;
        }
    }
    Ok(names)
}

impl Mounted {
    pub const fn root(&self) -> &[u8; rbl_nfs::HANDLE_LEN] {
        &self.root
    }

    /// Looks up one name in one directory, exactly as a player does.
    pub fn lookup(
        &mut self,
        parent: &[u8; rbl_nfs::HANDLE_LEN],
        name: &str,
    ) -> Result<[u8; rbl_nfs::HANDLE_LEN]> {
        let mut arguments = Writer::new();
        arguments.opaque_fixed(parent).utf16(name);
        let results = self.nfs.call(
            PROGRAM_NFS,
            VERSION_NFS,
            nfs_proc::LOOKUP,
            &arguments.into_bytes(),
        )?;
        let mut reader = Reader::new(&results);
        let status = reader.u32()?;
        if status != nfs_status::OK {
            return Err(CdjError::Nfs(status));
        }
        let mut handle = [0_u8; rbl_nfs::HANDLE_LEN];
        handle.copy_from_slice(reader.opaque_fixed(rbl_nfs::HANDLE_LEN)?);
        Ok(handle)
    }

    /// Walks a whole path from the root.
    pub fn resolve(&mut self, path: &str) -> Result<[u8; rbl_nfs::HANDLE_LEN]> {
        let mut at = self.root;
        for part in path.split('/').filter(|part| !part.is_empty()) {
            at = self.lookup(&at, part)?;
        }
        Ok(at)
    }

    /// Lists a directory, following the cookie to the end.
    pub fn list(&mut self, handle: &[u8; rbl_nfs::HANDLE_LEN]) -> Result<Vec<String>> {
        let mut names = Vec::new();
        let mut cookie = 0_u32;
        loop {
            let mut arguments = Writer::new();
            arguments.opaque_fixed(handle).u32(cookie).u32(8192);
            let results = self.nfs.call(
                PROGRAM_NFS,
                VERSION_NFS,
                nfs_proc::READDIR,
                &arguments.into_bytes(),
            )?;
            let mut reader = Reader::new(&results);
            let status = reader.u32()?;
            if status != nfs_status::OK {
                return Err(CdjError::Nfs(status));
            }
            while reader.u32()? == 1 {
                reader.u32()?; // fileid
                names.push(reader.utf16()?);
                cookie = reader.u32()?;
            }
            if reader.u32()? == 1 {
                return Ok(names);
            }
        }
    }

    /// Fetches a whole file, 32 KB at a time, as a player loading a track does.
    pub fn read_file(&mut self, path: &str) -> Result<Vec<u8>> {
        let handle = self.resolve(path)?;
        let mut out = Vec::new();
        loop {
            let offset = u32::try_from(out.len()).unwrap_or(u32::MAX);
            let mut arguments = Writer::new();
            arguments
                .opaque_fixed(&handle)
                .u32(offset)
                .u32(u32::try_from(rbl_nfs::MAX_READ).unwrap_or(8192))
                .u32(0);
            let results = self.nfs.call(
                PROGRAM_NFS,
                VERSION_NFS,
                nfs_proc::READ,
                &arguments.into_bytes(),
            )?;
            let mut reader = Reader::new(&results);
            let status = reader.u32()?;
            if status == nfs_status::ISDIR {
                return Err(CdjError::NotAFile(path.to_owned()));
            }
            if status != nfs_status::OK {
                return Err(CdjError::Nfs(status));
            }
            // The attributes: seventeen 32-bit fields, the sixth the size.
            // A player reads up to the size it was told and no further —
            // rekordbox answers a read at the end with IO, not an empty
            // success.
            let mut size = 0_u32;
            for field in 0..17 {
                let value = reader.u32()?;
                if field == 5 {
                    size = value;
                }
            }
            let chunk = reader.opaque()?;
            out.extend_from_slice(chunk);
            if chunk.is_empty() || u32::try_from(out.len()).unwrap_or(u32::MAX) >= size {
                return Ok(out);
            }
        }
    }
}

/// A session with a database server.
#[derive(Debug)]
pub struct Database {
    stream: TcpStream,
    transaction: u32,
    pending: Vec<u8>,
}

/// Asks the port-query service where the database server is listening.
pub fn database_port(query: SocketAddr) -> Result<u16> {
    let mut stream = TcpStream::connect_timeout(&query, TIMEOUT)?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.write_all(PORT_QUERY_REQUEST)?;
    let mut answer = [0_u8; 2];
    stream
        .read_exact(&mut answer)
        .map_err(|_| CdjError::NoDatabasePort)?;
    Ok(u16::from_be_bytes(answer))
}

impl Database {
    /// Connects and completes the setup exchange.
    pub fn connect(address: SocketAddr, our_device: u8) -> Result<Self> {
        let stream = TcpStream::connect_timeout(&address, TIMEOUT)?;
        stream.set_read_timeout(Some(TIMEOUT))?;
        stream.set_nodelay(true)?;
        let mut session = Self {
            stream,
            transaction: 0,
            pending: Vec::new(),
        };
        // Greeting first, both ways, as a player does (measured).
        session.stream.write_all(rbl_dbserver::GREETING)?;
        let mut greeting = [0_u8; 5];
        session.stream.read_exact(&mut greeting)?;
        if greeting != rbl_dbserver::GREETING {
            return Err(CdjError::NoDatabasePort);
        }
        session.send(&rbl_dbserver::setup_request(our_device))?;
        session.receive()?;
        Ok(session)
    }

    pub fn send(&mut self, message: &Message) -> Result<()> {
        self.stream.write_all(&message.encode())?;
        Ok(())
    }

    /// Sends a request under the next transaction id and reads the reply.
    pub fn request(&mut self, kind: u16, arguments: Vec<Argument>) -> Result<Vec<Message>> {
        self.transaction = self.transaction.wrapping_add(1);
        self.send(&Message::new(self.transaction, kind, arguments))?;
        self.receive()
    }

    /// Reads until at least one whole message is available.
    pub fn receive(&mut self) -> Result<Vec<Message>> {
        let mut chunk = [0_u8; 8192];
        loop {
            let (messages, used) = Message::decode_all(&self.pending);
            if !messages.is_empty() {
                self.pending.drain(..used);
                return Ok(messages);
            }
            let len = self.stream.read(&mut chunk)?;
            if len == 0 {
                return Ok(Vec::new());
            }
            self.pending
                .extend_from_slice(chunk.get(..len).unwrap_or(&[]));
        }
    }
}
