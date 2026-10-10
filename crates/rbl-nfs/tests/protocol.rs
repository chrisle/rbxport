//! The NFS, mount and portmap programs, driven as a real player drives them.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;

use rbl_nfs::rpc::{self, Auth, Call, Reply};
use rbl_nfs::xdr::{Reader, Writer};
use rbl_nfs::{
    mount_proc, nfs_proc, nfs_status, portmap_proc, split_export, Exports, Handle, Server, Vfs,
    HANDLE_LEN, IPPROTO_TCP, IPPROTO_UDP, MAX_READ, PROGRAM_MOUNT, PROGRAM_NFS, PROGRAM_PORTMAP,
    VERSION_MOUNT, VERSION_NFS, VERSION_PORTMAP,
};

const NFS_PORT: u16 = 12049;
const MOUNT_PORT: u16 = 12005;

/// Builds a server over a temp dir holding two real files, plus an empty dir.
fn fixture() -> (tempfile::TempDir, Server) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("track.mp3"), vec![7_u8; 40_000]).unwrap();
    fs::write(dir.path().join("small.dat"), b"hello").unwrap();

    let mut vfs = Vfs::new("/");
    vfs.add_file(
        "Contents/ARTBAT/The Abyss.mp3",
        dir.path().join("track.mp3"),
        40_000,
        1_700_000_000,
    );
    vfs.add_file("PIONEER/rekordbox/export.pdb", dir.path().join("small.dat"), 5, 1_700_000_001);
    vfs.add_dir("PIONEER/USBANLZ");

    let mut exports = Exports::new();
    exports.insert(vfs);
    (dir, Server::new(exports, NFS_PORT, MOUNT_PORT))
}

/// Each call its own xid, as a client sends them: the server answers a
/// call it has seen before with the reply it gave then.
static XID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x1234);

fn call(program: u32, version: u32, procedure: u32, arguments: Vec<u8>) -> Vec<u8> {
    Call {
        xid: XID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        program,
        version,
        procedure,
        credential: Auth { flavor: rpc::AUTH_UNIX, body: vec![0; 8] },
        verifier: Auth::null(),
        arguments: &arguments,
    }
    .encode()
}

fn ask(server: &Server, program: u32, version: u32, procedure: u32, args: Vec<u8>) -> Vec<u8> {
    server.handle(&call(program, version, procedure, args)).expect("a reply")
}

fn ok_reader(reply: &[u8]) -> Reader<'_> {
    let parsed = Reply::decode(reply).unwrap();
    assert!(parsed.is_success(), "{parsed:?}");
    assert!(parsed.xid >= 0x1234, "the reply carries the call's xid");
    parsed.reader()
}

/// Mounts `/` and returns the root handle.
fn mount_root(server: &Server) -> Handle {
    let mut args = Writer::new();
    args.utf16("/");
    let reply = ask(server, PROGRAM_MOUNT, VERSION_MOUNT, mount_proc::MNT, args.into_bytes());
    let mut reader = ok_reader(&reply);
    assert_eq!(reader.u32().unwrap(), nfs_status::OK);
    Handle::from_slice(reader.opaque_fixed(HANDLE_LEN).unwrap()).unwrap()
}

fn lookup(server: &Server, parent: &Handle, name: &str) -> Result<Handle, u32> {
    let mut args = Writer::new();
    args.opaque_fixed(parent.as_bytes()).utf16(name);
    let reply = ask(server, PROGRAM_NFS, VERSION_NFS, nfs_proc::LOOKUP, args.into_bytes());
    let mut reader = ok_reader(&reply);
    let status = reader.u32().unwrap();
    if status != nfs_status::OK {
        return Err(status);
    }
    Ok(Handle::from_slice(reader.opaque_fixed(HANDLE_LEN).unwrap()).unwrap())
}

fn lookup_path(server: &Server, root: &Handle, path: &str) -> Result<Handle, u32> {
    let mut at = *root;
    for part in path.split('/').filter(|p| !p.is_empty()) {
        at = lookup(server, &at, part)?;
    }
    Ok(at)
}

// ---------------------------------------------------------------- XDR

#[test]
fn xdr_pads_every_field_to_four_bytes() {
    for (text, expected) in [("", 4), ("a", 8), ("abc", 8), ("abcd", 8), ("abcde", 12)] {
        let mut writer = Writer::new();
        writer.string(text);
        assert_eq!(writer.len(), expected, "{text:?}");
        assert_eq!(Reader::new(writer.as_slice()).string().unwrap(), text);
    }
}

#[test]
fn utf16_names_declare_a_byte_length_not_a_character_count() {
    let mut writer = Writer::new();
    writer.utf16("PIONEER");
    let bytes = writer.into_bytes();
    assert_eq!(&bytes[0..4], &14_u32.to_be_bytes(), "seven characters, fourteen bytes");
    assert_eq!(&bytes[4..8], &[b'P', 0, b'I', 0]);
    assert_eq!(Reader::new(&bytes).utf16().unwrap(), "PIONEER");
}

#[test]
fn an_odd_length_utf16_name_is_rejected_rather_than_truncated() {
    let mut writer = Writer::new();
    writer.opaque(&[b'A', 0, b'B']);
    assert!(Reader::new(writer.as_slice()).utf16().is_err());
}

#[test]
fn an_absurd_length_does_not_allocate() {
    let mut bytes = 0xffff_ffff_u32.to_be_bytes().to_vec();
    bytes.extend_from_slice(&[0; 4]);
    assert!(Reader::new(&bytes).opaque().is_err());
}

#[test]
fn a_truncated_field_is_an_error_at_every_cut() {
    let mut writer = Writer::new();
    writer.u32(1).utf16("Melodic Techno").opaque(&[1, 2, 3]);
    let bytes = writer.into_bytes();
    for cut in 0..bytes.len() {
        let mut reader = Reader::new(&bytes[..cut]);
        let outcome = reader.u32().and_then(|_| reader.utf16()).and_then(|_| {
            reader.opaque()?;
            Ok(())
        });
        assert!(outcome.is_err(), "cut at {cut} should not decode");
    }
}

// ---------------------------------------------------------------- RPC

#[test]
fn an_rpc_call_round_trips() {
    let arguments = vec![1, 2, 3, 4];
    let original = Call {
        xid: 99,
        program: PROGRAM_NFS,
        version: VERSION_NFS,
        procedure: nfs_proc::READ,
        credential: Auth { flavor: rpc::AUTH_UNIX, body: vec![9; 12] },
        verifier: Auth::null(),
        arguments: &arguments,
    };
    let bytes = original.encode();
    assert_eq!(Call::decode(&bytes).unwrap(), original);
}

#[test]
fn a_reply_is_never_mistaken_for_a_call() {
    let reply = rpc::accepted_empty(1, rpc::accept::SUCCESS);
    assert!(matches!(Call::decode(&reply), Err(rpc::RpcError::NotACall(_))));
}

#[test]
fn a_wrong_rpc_version_is_answered_with_the_version_we_speak() {
    let mut bytes = call(PROGRAM_NFS, VERSION_NFS, nfs_proc::NULL, vec![]);
    bytes[8..12].copy_from_slice(&3_u32.to_be_bytes());
    let (_dir, server) = fixture();
    let reply = server.handle(&bytes).expect("a rejection, not silence");
    let parsed = Reply::decode(&reply).unwrap();
    assert_eq!(parsed.reply_status, rpc::MSG_DENIED);
    assert_eq!(parsed.accept_status, rpc::reject::RPC_MISMATCH);
}

#[test]
fn garbage_is_dropped_rather_than_answered() {
    let (_dir, server) = fixture();
    assert!(server.handle(&[]).is_none());
    assert!(server.handle(&[0; 3]).is_none());
    assert!(server.handle(&[0xff; 40]).is_none());
}

#[test]
fn an_unknown_program_is_reported_unavailable() {
    let (_dir, server) = fixture();
    let reply = ask(&server, 999_999, 1, 0, vec![]);
    assert_eq!(Reply::decode(&reply).unwrap().accept_status, rpc::accept::PROG_UNAVAIL);
}

#[test]
fn a_wrong_program_version_names_the_version_we_serve() {
    let (_dir, server) = fixture();
    let reply = ask(&server, PROGRAM_NFS, 3, nfs_proc::NULL, vec![]);
    let parsed = Reply::decode(&reply).unwrap();
    assert_eq!(parsed.accept_status, rpc::accept::PROG_MISMATCH);
    let mut reader = parsed.reader();
    assert_eq!(reader.u32().unwrap(), VERSION_NFS);
    assert_eq!(reader.u32().unwrap(), VERSION_NFS);
}

// ---------------------------------------------------------------- portmap

#[test]
fn portmap_reports_where_each_program_listens() {
    let (_dir, server) = fixture();
    for (program, version, expected) in [(PROGRAM_NFS, VERSION_NFS, NFS_PORT), (PROGRAM_MOUNT, VERSION_MOUNT, MOUNT_PORT)] {
        let mut args = Writer::new();
        args.u32(program).u32(version).u32(IPPROTO_UDP).u32(0);
        let reply = ask(
            &server,
            PROGRAM_PORTMAP,
            VERSION_PORTMAP,
            portmap_proc::GETPORT,
            args.into_bytes(),
        );
        assert_eq!(ok_reader(&reply).u32().unwrap(), u32::from(expected));
    }
}

#[test]
fn portmap_reports_zero_for_tcp_and_for_programs_we_do_not_serve() {
    let (_dir, server) = fixture();
    for (program, protocol) in [(PROGRAM_NFS, IPPROTO_TCP), (100_024, IPPROTO_UDP)] {
        let mut args = Writer::new();
        args.u32(program).u32(2).u32(protocol).u32(0);
        let reply = ask(
            &server,
            PROGRAM_PORTMAP,
            VERSION_PORTMAP,
            portmap_proc::GETPORT,
            args.into_bytes(),
        );
        assert_eq!(ok_reader(&reply).u32().unwrap(), 0, "program {program}");
    }
}

#[test]
fn rekordbox_answers_portmap_on_its_own_port() {
    // Not 111. A client that assumes the standard port finds nothing, which is
    // the single most confusing thing about talking to rekordbox over NFS.
    assert_eq!(rbl_nfs::REKORDBOX_PORTMAP_PORT, 50_111);
    assert_eq!(rbl_nfs::STANDARD_PORTMAP_PORT, 111);
    assert_eq!(rbl_nfs::NFS_PORT, 2049);
}

#[test]
fn local_dynamic_portmap_registration_is_explicitly_unsupported() {
    // Source-proven local registration needs dynamic-map lifetime work. Do
    // not acknowledge it as successful while maintaining only static maps.
    let (_dir, server) = fixture();
    for procedure in [1_u32, 2] {
        let mut args = Writer::new();
        args.u32(PROGRAM_NFS).u32(2).u32(IPPROTO_UDP).u32(9999);
        let reply = ask(&server, PROGRAM_PORTMAP, VERSION_PORTMAP, procedure, args.into_bytes());
        assert_eq!(
            Reply::decode(&reply).unwrap().accept_status,
            rpc::accept::PROC_UNAVAIL,
            "portmap procedure {procedure} must be refused"
        );
    }
    // The NFS mapping still points at the real port, not the 9999 a SET tried.
    let mut args = Writer::new();
    args.u32(PROGRAM_NFS).u32(2).u32(IPPROTO_UDP).u32(0);
    let reply = ask(&server, PROGRAM_PORTMAP, VERSION_PORTMAP, portmap_proc::GETPORT, args.into_bytes());
    assert_eq!(ok_reader(&reply).u32().unwrap(), u32::from(NFS_PORT));
}

#[test]
fn getport_matches_program_service_version_and_transport_exactly() {
    let (_dir, server) = fixture();
    for (program, version, expected) in [(PROGRAM_NFS, VERSION_NFS, NFS_PORT),
        (PROGRAM_MOUNT, VERSION_MOUNT, MOUNT_PORT)] {
        for requested in [0, 1, 2, 3, u32::MAX] {
            for protocol in [IPPROTO_UDP, IPPROTO_TCP, 0] {
                let mut args = Writer::new();
                args.u32(program).u32(requested).u32(protocol).u32(0);
                let request = call(PROGRAM_PORTMAP, VERSION_PORTMAP, portmap_proc::GETPORT, args.into_bytes());
                let xid = Call::decode(&request).unwrap().xid;
                let reply = server.handle(&request).unwrap();
                let port = if requested == version && protocol == IPPROTO_UDP { u32::from(expected) } else { 0 };
                let mut exact = rpc::accepted(xid, rpc::accept::SUCCESS);
                exact.u32(port);
                assert_eq!(reply, exact.into_bytes());
            }
        }
    }
}

#[test]
fn remote_portmap_registration_returns_false_without_changing_maps() {
    let (_dir, server) = fixture();
    for from in [std::net::Ipv4Addr::new(127, 0, 0, 2), std::net::Ipv4Addr::new(192, 168, 1, 20)] {
        for procedure in [portmap_proc::SET, portmap_proc::UNSET] {
            let mut args = Writer::new();
            args.u32(PROGRAM_NFS).u32(VERSION_NFS).u32(IPPROTO_UDP).u32(9999);
            let request = call(PROGRAM_PORTMAP, VERSION_PORTMAP, procedure, args.into_bytes());
            let xid = Call::decode(&request).unwrap().xid;
            let mut expected = rpc::accepted(xid, rpc::accept::SUCCESS);
            expected.u32(0);
            assert_eq!(server.handle_from(&request, from, 40000), Some(expected.into_bytes()));
        }
    }
    let mut args = Writer::new();
    args.u32(PROGRAM_NFS).u32(VERSION_NFS).u32(IPPROTO_UDP).u32(0);
    let reply = ask(&server, PROGRAM_PORTMAP, VERSION_PORTMAP, portmap_proc::GETPORT, args.into_bytes());
    assert_eq!(ok_reader(&reply).u32().unwrap(), u32::from(NFS_PORT));
}

#[test]
fn mount_dump_is_unavailable_and_exportall_is_the_same_bounded_list() {
    let (_dir, server) = fixture();
    let request = call(PROGRAM_MOUNT, VERSION_MOUNT, mount_proc::DUMP, vec![]);
    let xid = Call::decode(&request).unwrap().xid;
    assert_eq!(server.handle(&request), Some(rpc::accepted_empty(xid, rpc::accept::PROC_UNAVAIL)));
    let export = ask(&server, PROGRAM_MOUNT, VERSION_MOUNT, mount_proc::EXPORT, vec![]);
    let mut export_all = ask(&server, PROGRAM_MOUNT, VERSION_MOUNT, mount_proc::EXPORTALL, vec![]);
    export_all[..4].copy_from_slice(&export[..4]);
    assert_eq!(export_all, export);
}

#[test]
fn malformed_specific_unmount_keeps_every_host_and_export() {
    let mut exports = Exports::new();
    exports.insert(Vfs::new("/"));
    exports.insert(Vfs::new("/other"));
    let server = Server::new(exports, NFS_PORT, MOUNT_PORT);
    let first = std::net::Ipv4Addr::new(192, 168, 1, 20);
    let second = std::net::Ipv4Addr::new(192, 168, 1, 21);
    for from in [first, second] {
        for path in ["/", "/other"] {
            let mut args = Writer::new();
            args.utf16(path);
            let request = call(PROGRAM_MOUNT, VERSION_MOUNT, mount_proc::MNT, args.into_bytes());
            let reply = server.handle_from(&request, from, 40000).unwrap();
            assert_eq!(ok_reader(&reply).u32().unwrap(), nfs_status::OK);
        }
    }
    for args in [vec![], vec![0, 0, 0, 8, b'/', 0, 0, 0], vec![0, 0, 0, 1, b'/', 0, 0, 0]] {
        let request = call(PROGRAM_MOUNT, VERSION_MOUNT, mount_proc::UMNT, args);
        let xid = Call::decode(&request).unwrap().xid;
        assert_eq!(server.handle_from(&request, first, 40000), Some(rpc::accepted_empty(xid, rpc::accept::SUCCESS)));
        assert!(server.is_mounted(first));
        assert!(server.is_mounted(second));
    }
    let mut args = Writer::new();
    args.utf16("/");
    let request = call(PROGRAM_MOUNT, VERSION_MOUNT, mount_proc::UMNT, args.into_bytes());
    server.handle_from(&request, first, 40000);
    assert!(server.is_mounted(first), "the same host still holds /other");
    assert!(server.is_mounted(second));
    let request = call(PROGRAM_MOUNT, VERSION_MOUNT, mount_proc::UMNTALL, vec![]);
    server.handle_from(&request, first, 40000);
    assert!(!server.is_mounted(first));
    assert!(server.is_mounted(second));
}

#[test]
fn duplicate_mounts_are_scoped_to_the_actual_receiving_socket() {
    use std::net::{Ipv4Addr, UdpSocket};
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
    let (_dir, server) = fixture();
    let server = Arc::new(server);
    let stop = Arc::new(AtomicBool::new(false));
    let first = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let second = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let peer = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    peer.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
    let first_addr = first.local_addr().unwrap();
    let second_addr = second.local_addr().unwrap();
    let workers: Vec<_> = [first, second].into_iter().map(|socket| {
        let server = Arc::clone(&server);
        let stop = Arc::clone(&stop);
        std::thread::spawn(move || rbl_nfs::net::serve(&server, &socket, &stop).unwrap())
    }).collect();
    let exchange = |request: &[u8], receiver| {
        peer.send_to(request, receiver).unwrap();
        let mut response = [0; 256];
        let (length, source) = peer.recv_from(&mut response).unwrap();
        assert_eq!(source, receiver, "reply originates on the receiving socket");
        response[..length].to_vec()
    };
    let mut path = Writer::new();
    path.utf16("/");
    let mount = call(PROGRAM_MOUNT, VERSION_MOUNT, mount_proc::MNT, path.into_bytes());
    let original = exchange(&mount, first_addr);
    assert_eq!(ok_reader(&original).u32().unwrap(), nfs_status::OK);
    assert!(server.is_mounted(Ipv4Addr::LOCALHOST));
    let unmount = call(PROGRAM_MOUNT, VERSION_MOUNT, mount_proc::UMNTALL, vec![]);
    let xid = Call::decode(&unmount).unwrap().xid;
    assert_eq!(exchange(&unmount, first_addr), rpc::accepted_empty(xid, rpc::accept::SUCCESS));
    assert!(!server.is_mounted(Ipv4Addr::LOCALHOST));
    assert_eq!(exchange(&mount, first_addr), original);
    assert!(!server.is_mounted(Ipv4Addr::LOCALHOST), "true retransmission replays without repeating MNT");
    assert_eq!(exchange(&mount, second_addr), original);
    assert!(server.is_mounted(Ipv4Addr::LOCALHOST), "a different receiver executes independently");
    stop.store(true, Ordering::Relaxed);
    for worker in workers { worker.join().unwrap(); }
}

// ---------------------------------------------------------------- mount

#[test]
fn the_export_list_is_a_terminated_linked_list() {
    let (_dir, server) = fixture();
    let reply = ask(&server, PROGRAM_MOUNT, VERSION_MOUNT, mount_proc::EXPORT, vec![]);
    let mut reader = ok_reader(&reply);
    assert_eq!(reader.u32().unwrap(), 1, "one entry follows");
    assert_eq!(reader.utf16().unwrap(), "/");
    assert_eq!(reader.u32().unwrap(), 0, "no groups");
    assert_eq!(reader.u32().unwrap(), 0, "end of list");
    assert_eq!(reader.remaining(), 0);
}

#[test]
fn each_export_offers_the_host_group_a_cdj_checks_itself_against() {
    let (dir, server) = fixture();
    let server = server.with_export_host("192.168.1.14/255.255.255.0");
    let _ = &dir;
    let reply = ask(&server, PROGRAM_MOUNT, VERSION_MOUNT, mount_proc::EXPORT, vec![]);
    let mut reader = ok_reader(&reply);
    assert_eq!(reader.u32().unwrap(), 1, "one entry follows");
    assert_eq!(reader.utf16().unwrap(), "/");
    assert_eq!(reader.u32().unwrap(), 1, "a group follows");
    assert_eq!(reader.string().unwrap(), "192.168.1.14/255.255.255.0");
    assert_eq!(reader.u32().unwrap(), 0, "no more groups");
    assert_eq!(reader.u32().unwrap(), 0, "end of list");
    assert_eq!(reader.remaining(), 0);
}

#[test]
fn mounting_an_export_that_does_not_exist_says_so() {
    let (_dir, server) = fixture();
    let mut args = Writer::new();
    args.utf16("/D/");
    let reply = ask(&server, PROGRAM_MOUNT, VERSION_MOUNT, mount_proc::MNT, args.into_bytes());
    // `ACCES`, as rekordbox's libFilSiNE answers an unknown path.
    assert_eq!(ok_reader(&reply).u32().unwrap(), nfs_status::ACCES);
}

#[test]
fn unmounting_succeeds_even_though_we_hold_no_state() {
    let (_dir, server) = fixture();
    for procedure in [mount_proc::UMNT, mount_proc::UMNTALL, mount_proc::NULL] {
        let reply = ask(&server, PROGRAM_MOUNT, VERSION_MOUNT, procedure, vec![]);
        assert!(Reply::decode(&reply).unwrap().is_success());
    }
}

// ---------------------------------------------------------------- NFS

#[test]
fn a_player_can_walk_from_the_mount_point_to_a_track() {
    let (_dir, server) = fixture();
    let root = mount_root(&server);
    assert!(lookup_path(&server, &root, "Contents/ARTBAT/The Abyss.mp3").is_ok());
    assert!(lookup_path(&server, &root, "PIONEER/rekordbox/export.pdb").is_ok());
}

#[test]
fn a_name_that_is_not_there_is_noent_not_a_guess() {
    let (_dir, server) = fixture();
    let root = mount_root(&server);
    assert_eq!(lookup(&server, &root, "Nope").unwrap_err(), nfs_status::NOENT);
    // Including one that exists somewhere else in the tree.
    let contents = lookup(&server, &root, "Contents").unwrap();
    assert_eq!(lookup(&server, &contents, "PIONEER").unwrap_err(), nfs_status::NOENT);
}

#[test]
fn dot_dot_cannot_climb_out_of_the_export() {
    let (_dir, server) = fixture();
    let root = mount_root(&server);
    // Up from the root is the root, however many times it is asked for.
    let mut at = root;
    for _ in 0..8 {
        at = lookup(&server, &at, "..").unwrap();
    }
    assert_eq!(at, root);
    // And a deep path that climbs too far lands back at the root, not outside.
    let climbed = lookup_path(&server, &root, "Contents/ARTBAT/../../../../..").unwrap();
    assert_eq!(climbed, root);
}

#[test]
fn a_name_containing_a_separator_matches_nothing() {
    // The lookup takes one component, so a whole path as a "name" must fail
    // rather than being split and walked.
    let (_dir, server) = fixture();
    let root = mount_root(&server);
    for name in ["Contents/ARTBAT", "/etc/passwd", "../../etc/passwd", "Contents\\ARTBAT"] {
        assert_eq!(lookup(&server, &root, name).unwrap_err(), nfs_status::NOENT, "{name}");
    }
}

#[test]
fn looking_up_inside_a_file_is_notdir() {
    let (_dir, server) = fixture();
    let root = mount_root(&server);
    let file = lookup_path(&server, &root, "PIONEER/rekordbox/export.pdb").unwrap();
    assert_eq!(lookup(&server, &file, "anything").unwrap_err(), nfs_status::NOTDIR);
}

#[test]
fn a_forged_handle_is_stale_not_a_node() {
    let (_dir, server) = fixture();
    let root = mount_root(&server);

    // A handle is three file ids — the node's, its parent's, the root's —
    // and one whose parent is not the node's is not one we issued.
    let mut forged = *root.as_bytes();
    forged[7] = 5;
    let handle = Handle::from_slice(&forged).unwrap();
    let mut args = Writer::new();
    args.opaque_fixed(handle.as_bytes());
    let reply = ask(&server, PROGRAM_NFS, VERSION_NFS, nfs_proc::GETATTR, args.into_bytes());
    assert_eq!(ok_reader(&reply).u32().unwrap(), nfs_status::STALE);

    // So is one whose root id is not the root's, and one naming file id 0,
    // which no node has.
    let mut wrong_root = *root.as_bytes();
    wrong_root[11] = 2;
    let mut zero = *root.as_bytes();
    zero[3] = 0;
    for forged in [wrong_root, zero] {
        let mut args = Writer::new();
        args.opaque_fixed(&forged);
        let reply = ask(&server, PROGRAM_NFS, VERSION_NFS, nfs_proc::GETATTR, args.into_bytes());
        assert_eq!(ok_reader(&reply).u32().unwrap(), nfs_status::STALE);
    }
}

fn from_hex(hex: &str) -> Vec<u8> {
    (0..hex.len()).step_by(2).map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap()).collect()
}

/// [OBS] The mount reply handle RBXport sent an XDJ-700 (firmware 1.15) in
/// `xdj700_linux_rbxport.pcap` on #43, xid 1266.
const XDJ700_MOUNT_REPLY: &str = "0000000100000001000000010000000000000000000000000000000000000000";

/// [OBS] The handle the same XDJ-700 then sent in its first LOOKUP: the
/// three file ids it was given, then twenty bytes of its own. Linux capture
/// xid 1267 (`mnt`), Windows capture `xdj700_windows_rbxport.pcap` xid 1260
/// (`Users`).
const XDJ700_FIRST_LOOKUPS: [&str; 2] = [
    "0000000100000001000000010301000000001b58000000001104010002a33812",
    "0000000100000001000000010301000000001b58000000001104010004450197",
];

/// [OBS] The trailing bytes the XDJ-700 put on every LOOKUP handle when it
/// loaded a track from rekordbox, which answered each one
/// (`xdj700_windows_rekordbox.pcap` on #43, xids 1018-1024).
const XDJ700_TAIL_TO_REKORDBOX: &str = "0301000000001b5800000000110401000cce428f";

/// A handle as the XDJ-700 sends it back: ours, with its own bytes after
/// the three file ids.
fn as_xdj700_sends(handle: &Handle) -> Handle {
    let mut bytes = *handle.as_bytes();
    bytes[12..].copy_from_slice(&from_hex(XDJ700_TAIL_TO_REKORDBOX));
    Handle::from_slice(&bytes).unwrap()
}

#[test]
fn the_xdj700s_first_lookup_resolves_under_the_mount_handle() {
    // #43/#123: the XDJ-700 got STALE for this exact LOOKUP and showed
    // E-8302 (C658). rekordbox reads only the three file ids
    // (libFilSiNE `tkfNtoHFhandle`), so the bytes after them do not matter.
    let (_dir, server) = fixture();
    let root = mount_root(&server);
    assert_eq!(root.as_bytes().as_slice(), from_hex(XDJ700_MOUNT_REPLY));
    for sent in XDJ700_FIRST_LOOKUPS {
        let handle = Handle::from_slice(&from_hex(sent)).unwrap();
        assert!(lookup(&server, &handle, "Contents").is_ok(), "{sent}");
        let pioneer = lookup(&server, &handle, "PIONEER").unwrap();
        assert_eq!(pioneer, lookup(&server, &root, "PIONEER").unwrap(), "the same node, the clean handle");
    }
}

#[test]
fn the_xdj700_walks_to_a_track_and_reads_it_with_its_own_trailing_bytes() {
    let (_dir, server) = fixture();
    let mut at = mount_root(&server);
    for part in ["Contents", "ARTBAT", "The Abyss.mp3"] {
        at = lookup(&server, &as_xdj700_sends(&at), part).unwrap();
    }
    let sent = as_xdj700_sends(&at);
    let mut args = Writer::new();
    args.opaque_fixed(sent.as_bytes());
    let reply = ask(&server, PROGRAM_NFS, VERSION_NFS, nfs_proc::GETATTR, args.into_bytes());
    assert_eq!(ok_reader(&reply).u32().unwrap(), nfs_status::OK);
    assert_eq!(read_whole(&server, &sent), vec![7_u8; 40_000]);
}

#[test]
fn a_handle_displays_as_its_words_and_trailing_bytes() {
    // A stale-handle warning carries the handle a player sent, so a log
    // shows which of the three ids, or which trailing byte, differed from
    // the one the mount issued (#43).
    let (_dir, server) = fixture();
    let root = mount_root(&server);
    assert_eq!(
        root.to_string(),
        format!("00000001.00000001.00000001.{}", "00".repeat(HANDLE_LEN - 12))
    );
}

#[test]
fn attributes_describe_a_read_only_tree() {
    let (_dir, server) = fixture();
    let root = mount_root(&server);
    let file = lookup_path(&server, &root, "Contents/ARTBAT/The Abyss.mp3").unwrap();

    let mut args = Writer::new();
    args.opaque_fixed(file.as_bytes());
    let reply = ask(&server, PROGRAM_NFS, VERSION_NFS, nfs_proc::GETATTR, args.into_bytes());
    let mut reader = ok_reader(&reply);
    assert_eq!(reader.u32().unwrap(), nfs_status::OK);
    assert_eq!(reader.u32().unwrap(), 1, "regular file");
    // The host's own mode and owner, as rekordbox hands them to a player.
    assert_eq!(reader.u32().unwrap() & 0o170_000, 0o100_000, "a regular file's mode");
    assert_eq!(reader.u32().unwrap(), 1, "nlink");
    reader.u32().unwrap(); // uid: the host's
    reader.u32().unwrap(); // gid: the host's
    assert_eq!(reader.u32().unwrap(), 40_000, "size");
    reader.u32().unwrap(); // blocksize: the host's
    reader.u32().unwrap(); // rdev: the host's
    reader.u32().unwrap(); // blocks: the host's
    assert_eq!(reader.u32().unwrap(), 2, "fsid, as rekordbox reports it");

    // A directory reports the directory type and mode.
    let dir = lookup(&server, &root, "Contents").unwrap();
    let mut args = Writer::new();
    args.opaque_fixed(dir.as_bytes());
    let reply = ask(&server, PROGRAM_NFS, VERSION_NFS, nfs_proc::GETATTR, args.into_bytes());
    let mut reader = ok_reader(&reply);
    assert_eq!(reader.u32().unwrap(), nfs_status::OK);
    assert_eq!(reader.u32().unwrap(), 2, "directory");
    assert_eq!(reader.u32().unwrap(), 0o040_755, "rekordbox's export root showed 041ed");
}

/// Reads a whole file using the CDJ's 32 KB request size, continuing until
/// the read at EOF is answered `IO`.
fn read_whole(server: &Server, handle: &Handle) -> Vec<u8> {
    const PLAYER_READ: u32 = 32 * 1024;
    let mut out = Vec::new();
    loop {
        let mut args = Writer::new();
        args.opaque_fixed(handle.as_bytes())
            .u32(u32::try_from(out.len()).unwrap())
            .u32(PLAYER_READ)
            .u32(0);
        let reply = ask(server, PROGRAM_NFS, VERSION_NFS, nfs_proc::READ, args.into_bytes());
        let mut reader = ok_reader(&reply);
        let status = reader.u32().unwrap();
        if status == nfs_status::IO {
            return out;
        }
        assert_eq!(status, nfs_status::OK);
        for _ in 0..17 {
            reader.u32().unwrap(); // the attributes
        }
        let chunk = reader.opaque().unwrap();
        assert!(!chunk.is_empty(), "a read within the file carries bytes");
        out.extend_from_slice(chunk);
    }
}

#[test]
fn a_file_reads_back_byte_for_byte() {
    let (_dir, server) = fixture();
    let root = mount_root(&server);
    let file = lookup_path(&server, &root, "Contents/ARTBAT/The Abyss.mp3").unwrap();
    let data = read_whole(&server, &file);
    assert_eq!(data.len(), 40_000);
    assert!(data.iter().all(|b| *b == 7));

    let small = lookup_path(&server, &root, "PIONEER/rekordbox/export.pdb").unwrap();
    assert_eq!(read_whole(&server, &small), b"hello");
}

/// #282: a file registered in a folder an allowed folder also runs through
/// (a track at `/Volumes/SD/a.mp3` beside `/Volumes/SD/Music/…`) must not
/// hide the allowed folder. [OBS] A CDJ-3000X walked `Volumes`, `SD`, then
/// got NOENT for `Music` and `Transcend` and showed E-8306 (capture on #43).
#[test]
fn a_registered_file_does_not_hide_an_allowed_folder_beside_it() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("Music");
    fs::create_dir_all(music.join("PioneerDJ")).unwrap();
    fs::write(music.join("PioneerDJ/one.mp3"), vec![3_u8; 70_000]).unwrap();
    let transcend = dir.path().join("Transcend");
    fs::create_dir_all(transcend.join("Ron")).unwrap();
    fs::write(transcend.join("Ron/two.mp3"), b"two").unwrap();
    fs::write(dir.path().join("loose.mp3"), b"loose").unwrap();

    let mut vfs = Vfs::new("/");
    vfs.add_file_unsized("Volumes/SD/loose.mp3", dir.path().join("loose.mp3"));
    vfs.allow_folder("Volumes/SD/Music", &music);
    vfs.allow_folder("Volumes/Transcend/Music", &transcend);
    let mut exports = Exports::new();
    exports.insert(vfs);
    let server = Server::new(exports, NFS_PORT, MOUNT_PORT);

    let root = mount_root(&server);
    let one = lookup_path(&server, &root, "Volumes/SD/Music/PioneerDJ/one.mp3").unwrap();
    assert_eq!(read_whole(&server, &one), vec![3_u8; 70_000]);
    let two = lookup_path(&server, &root, "Volumes/Transcend/Music/Ron/two.mp3").unwrap();
    assert_eq!(read_whole(&server, &two), b"two");
    let loose = lookup_path(&server, &root, "Volumes/SD/loose.mp3").unwrap();
    assert_eq!(read_whole(&server, &loose), b"loose");
    // Beside them, still nothing.
    assert_eq!(lookup_path(&server, &root, "Volumes/SD/Other").unwrap_err(), nfs_status::NOENT);
    assert_eq!(lookup_path(&server, &root, "Volumes/Other").unwrap_err(), nfs_status::NOENT);
}

#[test]
fn each_new_read_reopens_the_file_and_reports_its_current_attributes() {
    let (dir, server) = fixture();
    let root = mount_root(&server);
    let handle = lookup_path(&server, &root, "Contents/ARTBAT/The Abyss.mp3").unwrap();
    let read = || {
        let mut args = Writer::new();
        args.opaque_fixed(handle.as_bytes()).u32(0).u32(32_768).u32(0);
        let reply = ask(&server, PROGRAM_NFS, VERSION_NFS, nfs_proc::READ, args.into_bytes());
        let mut reader = ok_reader(&reply);
        assert_eq!(reader.u32().unwrap(), nfs_status::OK);
        let mut size = 0;
        for field in 0..17 {
            let value = reader.u32().unwrap();
            if field == 5 {
                size = value;
            }
        }
        (size, reader.opaque().unwrap().to_vec())
    };
    assert_eq!(read(), (40_000, vec![7_u8; 32_768]));
    // Replacing the path must not leave a retained descriptor or prefetch
    // window serving the old file, or cached attributes describing its size.
    let replacement = vec![9_u8; 16_384];
    fs::write(dir.path().join("replacement.mp3"), &replacement).unwrap();
    fs::remove_file(dir.path().join("track.mp3")).unwrap();
    fs::rename(dir.path().join("replacement.mp3"), dir.path().join("track.mp3")).unwrap();
    assert_eq!(read(), (16_384, replacement));
    fs::remove_file(dir.path().join("track.mp3")).unwrap();
    assert_read_edge_status(&server, &handle, 0, 32_768, nfs_status::IO);
}

#[test]
fn cdj_32_kib_reads_are_complete_before_the_end_of_the_file() {
    let (_dir, server) = fixture();
    let root = mount_root(&server);
    let file = lookup_path(&server, &root, "Contents/ARTBAT/The Abyss.mp3").unwrap();
    // [OBS] The physical CDJ requests 32 KiB. An 8 KiB success can leave
    // unfetched ranges; do not model the client as always filling them in.
    for offset in [0, 4096] {
        let mut args = Writer::new();
        args.opaque_fixed(file.as_bytes()).u32(offset).u32(32_768).u32(0);
        let reply = ask(&server, PROGRAM_NFS, VERSION_NFS, nfs_proc::READ, args.into_bytes());
        let mut reader = ok_reader(&reply);
        assert_eq!(reader.u32().unwrap(), nfs_status::OK);
        for _ in 0..17 {
            reader.u32().unwrap();
        }
        assert_eq!(reader.opaque().unwrap(), vec![7_u8; 32_768]);
        assert_eq!(reader.remaining(), 0);
    }
}

#[test]
fn a_read_is_capped_at_the_protocol_limit_however_much_is_asked_for() {
    let (_dir, server) = fixture();
    let root = mount_root(&server);
    let file = lookup_path(&server, &root, "Contents/ARTBAT/The Abyss.mp3").unwrap();
    let mut args = Writer::new();
    args.opaque_fixed(file.as_bytes()).u32(0).u32(u32::MAX).u32(u32::MAX);
    let reply = ask(&server, PROGRAM_NFS, VERSION_NFS, nfs_proc::READ, args.into_bytes());
    let mut reader = ok_reader(&reply);
    assert_eq!(reader.u32().unwrap(), nfs_status::OK);
    for _ in 0..17 {
        reader.u32().unwrap();
    }
    assert_eq!(MAX_READ, 0xfc00);
    assert_eq!(reader.opaque().unwrap().len(), 40_000.min(MAX_READ));
}

#[test]
fn reading_at_or_past_the_end_is_io_as_rekordbox_answers_it() {
    let (_dir, server) = fixture();
    let root = mount_root(&server);
    let file = lookup_path(&server, &root, "PIONEER/rekordbox/export.pdb").unwrap();
    for offset in [1_000_000, 40_000] {
        let mut args = Writer::new();
        args.opaque_fixed(file.as_bytes()).u32(offset).u32(4096).u32(0);
        let reply = ask(&server, PROGRAM_NFS, VERSION_NFS, nfs_proc::READ, args.into_bytes());
        assert_eq!(ok_reader(&reply).u32().unwrap(), nfs_status::IO, "offset {offset}");
    }
}

/// Isolated READ fixtures with deterministic attributes and real file bytes.
fn read_edge_fixture() -> (tempfile::TempDir, Server, Handle, Handle) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("nonempty.dat"), b"hello").unwrap();
    fs::write(dir.path().join("empty.dat"), b"").unwrap();
    let mut vfs = Vfs::new("/");
    let file = vfs.add_file("nonempty.dat", dir.path().join("nonempty.dat"), 5, 1_700_000_000);
    let empty = vfs.add_file("empty.dat", dir.path().join("empty.dat"), 0, 1_700_000_000);
    let handles = (vfs.handle(file).unwrap(), vfs.handle(empty).unwrap());
    let mut exports = Exports::new();
    exports.insert(vfs);
    (dir, Server::new(exports, NFS_PORT, MOUNT_PORT), handles.0, handles.1)
}

fn read_edge_arguments(handle: &Handle, offset: u32, count: u32) -> Vec<u8> {
    let mut args = Writer::new();
    args.opaque_fixed(handle.as_bytes()).u32(offset).u32(count).u32(0);
    args.into_bytes()
}

/// Explicit accepted RPC words, independent of the server's reply builder.
fn read_edge_envelope(xid: u32, accept_status: u32) -> Vec<u8> {
    [xid, 1, 0, 0, 0, accept_status].into_iter().flat_map(u32::to_be_bytes).collect()
}

fn assert_read_edge_status(server: &Server, handle: &Handle, offset: u32, count: u32, status: u32) {
    let request = call(PROGRAM_NFS, VERSION_NFS, nfs_proc::READ, read_edge_arguments(handle, offset, count));
    let xid = Call::decode(&request).unwrap().xid;
    let mut expected = read_edge_envelope(xid, 0);
    expected.extend_from_slice(&status.to_be_bytes());
    assert_eq!(server.handle(&request), Some(expected), "offset {offset}, count {count}");
}

#[test]
fn read_edge_zero_count_is_io_for_nonempty_and_empty_files() {
    let (dir, server, file, empty) = read_edge_fixture();
    assert_read_edge_status(&server, &file, 0, 0, nfs_status::IO);
    assert_read_edge_status(&server, &empty, 0, 0, nfs_status::IO);
    assert_read_edge_status(&server, &empty, 0, 4096, nfs_status::IO);
    // The first READ populates the nonempty file's read-ahead window; its
    // cached zero-length slice must still produce status-only IO.
    assert_read_edge_status(&server, &file, 2, 0, nfs_status::IO);
    assert_eq!(fs::read(dir.path().join("nonempty.dat")).unwrap(), b"hello");
    assert_eq!(fs::read(dir.path().join("empty.dat")).unwrap(), b"");
}

#[test]
fn read_edge_exact_eof_and_past_eof_have_status_only_io_replies() {
    let (_dir, server, file, _) = read_edge_fixture();
    for offset in [5, 6, u32::MAX] {
        for count in [0, 1, 4096] {
            assert_read_edge_status(&server, &file, offset, count, nfs_status::IO);
        }
    }
}

#[test]
fn read_edge_stale_handle_is_stale_even_when_count_is_zero() {
    let (_dir, server, _, _) = read_edge_fixture();
    let stale = Handle::from_slice(&[0xff; HANDLE_LEN]).unwrap();
    for count in [0, 4096] {
        assert_read_edge_status(&server, &stale, 0, count, nfs_status::STALE);
    }
}

#[test]
fn read_edge_positive_short_read_keeps_attributes_length_data_and_padding() {
    let (dir, server, file, _) = read_edge_fixture();
    // A previous zero-count read must not prevent a later successful read.
    assert_read_edge_status(&server, &file, 0, 0, nfs_status::IO);
    for count in [4096, u32::MAX] {
        let request = call(PROGRAM_NFS, VERSION_NFS, nfs_proc::READ, read_edge_arguments(&file, 2, count));
        let xid = Call::decode(&request).unwrap().xid;
        let actual = server.handle(&request);
        let mut expected = read_edge_envelope(xid, 0);
        // READ carries current host attributes, as GETATTR does; synthetic
        // insertion-time mode/owner/timestamps are not returned for a file
        // backed by the host. Keep checking the complete reply envelope.
        let mut args = Writer::new();
        args.opaque_fixed(file.as_bytes());
        let attributes = ask(&server, PROGRAM_NFS, VERSION_NFS, nfs_proc::GETATTR, args.into_bytes());
        expected.extend_from_slice(&attributes[24..96]);
        expected.extend_from_slice(&3_u32.to_be_bytes());
        expected.extend_from_slice(b"llo\0");
        assert_eq!(actual, Some(expected), "count {count}");
    }
    assert_eq!(fs::read(dir.path().join("nonempty.dat")).unwrap(), b"hello");
}

#[test]
fn read_edge_truncated_handle_offset_or_count_is_garbage_args() {
    let (_dir, server, file, _) = read_edge_fixture();
    let arguments = read_edge_arguments(&file, 0, 0);
    // The existing parser requires a full 32-byte handle and both u32
    // arguments. Obsolete total-count handling is outside this change.
    for cut in 0..HANDLE_LEN + 8 {
        let request = call(PROGRAM_NFS, VERSION_NFS, nfs_proc::READ, arguments[..cut].to_vec());
        let xid = Call::decode(&request).unwrap().xid;
        assert_eq!(
            server.handle(&request),
            Some(read_edge_envelope(xid, 4)),
            "truncation at {cut}"
        );
    }
    assert_read_edge_status(&server, &file, 0, 0, nfs_status::IO);
}

#[test]
fn reading_a_directory_is_isdir() {
    let (_dir, server) = fixture();
    let root = mount_root(&server);
    let mut args = Writer::new();
    args.opaque_fixed(root.as_bytes()).u32(0).u32(4096).u32(0);
    let reply = ask(&server, PROGRAM_NFS, VERSION_NFS, nfs_proc::READ, args.into_bytes());
    assert_eq!(ok_reader(&reply).u32().unwrap(), nfs_status::ISDIR);
}

/// Lists a directory, following the cookie until the server reports EOF.
fn list(server: &Server, handle: &Handle, count: u32) -> Vec<String> {
    let mut names = Vec::new();
    let mut cookie = 0_u32;
    for _ in 0..100 {
        let mut args = Writer::new();
        args.opaque_fixed(handle.as_bytes()).u32(cookie).u32(count);
        let reply = ask(server, PROGRAM_NFS, VERSION_NFS, nfs_proc::READDIR, args.into_bytes());
        let mut reader = ok_reader(&reply);
        assert_eq!(reader.u32().unwrap(), nfs_status::OK);
        while reader.u32().unwrap() == 1 {
            reader.u32().unwrap(); // fileid
            names.push(reader.utf16().unwrap());
            cookie = reader.u32().unwrap();
        }
        if reader.u32().unwrap() == 1 {
            return names;
        }
    }
    panic!("listing never reached the end");
}

#[test]
fn a_directory_lists_its_children() {
    let (_dir, server) = fixture();
    let root = mount_root(&server);
    assert_eq!(list(&server, &root, 8192), vec!["Contents", "PIONEER"]);

    let pioneer = lookup(&server, &root, "PIONEER").unwrap();
    assert_eq!(list(&server, &pioneer, 8192), vec!["rekordbox", "USBANLZ"]);

    let empty = lookup(&server, &pioneer, "USBANLZ").unwrap();
    assert!(list(&server, &empty, 8192).is_empty());
}

#[test]
fn a_listing_that_does_not_fit_resumes_from_its_cookie() {
    let mut vfs = Vfs::new("/");
    for i in 0..200 {
        vfs.add_file(&format!("Contents/track {i:03}.mp3"), "/dev/null", 0, 0);
    }
    let mut exports = Exports::new();
    exports.insert(vfs);
    let server = Server::new(exports, NFS_PORT, MOUNT_PORT);

    let root = mount_root(&server);
    let contents = lookup(&server, &root, "Contents").unwrap();
    // A small budget forces many round trips; the result must still be whole,
    // in order, and free of duplicates.
    let names = list(&server, &contents, 512);
    assert_eq!(names.len(), 200);
    assert_eq!(names.first().map(String::as_str), Some("track 000.mp3"));
    assert_eq!(names.last().map(String::as_str), Some("track 199.mp3"));
    assert_eq!(names, list(&server, &contents, 8192));
}

#[test]
fn listing_a_file_is_notdir() {
    let (_dir, server) = fixture();
    let root = mount_root(&server);
    let file = lookup_path(&server, &root, "PIONEER/rekordbox/export.pdb").unwrap();
    let mut args = Writer::new();
    args.opaque_fixed(file.as_bytes()).u32(0).u32(4096);
    let reply = ask(&server, PROGRAM_NFS, VERSION_NFS, nfs_proc::READDIR, args.into_bytes());
    assert_eq!(ok_reader(&reply).u32().unwrap(), nfs_status::NOTDIR);
}

#[test]
fn every_mutating_procedure_is_refused_as_read_only() {
    let (_dir, server) = fixture();
    let root = mount_root(&server);
    let mut args = Writer::new();
    args.opaque_fixed(root.as_bytes());
    let args = args.into_bytes();

    // As libFilSiNE's stubs answer them: STALE for the writes, ACCES for
    // links and readlink.
    for procedure in [
        nfs_proc::SETATTR,
        nfs_proc::WRITE,
        nfs_proc::CREATE,
        nfs_proc::REMOVE,
        nfs_proc::RENAME,
        nfs_proc::MKDIR,
        nfs_proc::RMDIR,
    ] {
        let reply = ask(&server, PROGRAM_NFS, VERSION_NFS, procedure, args.clone());
        assert_eq!(
            ok_reader(&reply).u32().unwrap(),
            nfs_status::STALE,
            "procedure {procedure} must be refused, not ignored"
        );
    }
    for procedure in [nfs_proc::LINK, nfs_proc::SYMLINK, nfs_proc::READLINK] {
        let reply = ask(&server, PROGRAM_NFS, VERSION_NFS, procedure, args.clone());
        assert_eq!(ok_reader(&reply).u32().unwrap(), nfs_status::ACCES, "procedure {procedure}");
    }
}

#[test]
fn statfs_reports_the_host_filesystems_figures() {
    let (_dir, server) = fixture();
    let root = mount_root(&server);
    let file = lookup_path(&server, &root, "PIONEER/rekordbox/export.pdb").unwrap();
    let mut args = Writer::new();
    args.opaque_fixed(file.as_bytes());
    let reply = ask(&server, PROGRAM_NFS, VERSION_NFS, nfs_proc::STATFS, args.into_bytes());
    let mut reader = ok_reader(&reply);
    assert_eq!(reader.u32().unwrap(), nfs_status::OK);
    // rekordbox hands a player the host's own transfer and block sizes and
    // counts (a mebibyte `tsize` on APFS, more than a READ carries).
    let tsize = reader.u32().unwrap();
    let bsize = reader.u32().unwrap();
    let blocks = reader.u32().unwrap();
    if cfg!(unix) {
        assert!(tsize > 0 && bsize > 0 && blocks > 0, "tsize {tsize} bsize {bsize} blocks {blocks}");
    }
}

#[test]
fn a_truncated_request_is_answered_rather_than_dropped() {
    // Silence is what a client times out on, so a call we can address must
    // always get a reply, even when its arguments are unusable.
    let (_dir, server) = fixture();
    for procedure in [nfs_proc::GETATTR, nfs_proc::LOOKUP, nfs_proc::READ, nfs_proc::READDIR] {
        for len in [0, 4, 16, 31] {
            let reply = ask(&server, PROGRAM_NFS, VERSION_NFS, procedure, vec![0; len]);
            let parsed = Reply::decode(&reply).unwrap();
            assert_eq!(parsed.accept_status, rpc::accept::GARBAGE_ARGS, "{procedure}/{len}");
        }
    }
}

#[test]
fn handles_survive_a_rebuild_of_the_same_tree() {
    // A player caches handles across reconnects. Rebuilding the same export
    // must hand back the same bytes, or every cached handle goes stale.
    let build = || {
        let mut vfs = Vfs::new("/");
        vfs.add_file("Contents/a.mp3", "/dev/null", 1, 0);
        vfs.add_file("Contents/b.mp3", "/dev/null", 1, 0);
        vfs
    };
    let (first, second) = (build(), build());
    for index in 0..first.len() {
        assert_eq!(first.handle(index), second.handle(index), "node {index}");
    }
}

#[test]
fn a_handle_from_another_export_is_not_accepted() {
    // Every export's nodes are numbered from one table, so a handle from
    // another export names ids this one does not have — or, for the first
    // export in a set, ids past the end of this one's.
    let mut exports = rbl_nfs::Exports::new();
    let mut a = Vfs::new("/A/");
    a.add_file("Contents/x.mp3", "/dev/null", 1, 0);
    let mut b = Vfs::new("/B/");
    b.add_file("Contents/y.mp3", "/dev/null", 1, 0);
    exports.insert(a);
    exports.insert(b);
    let server = Server::new(exports, 2049, 0);
    let a = server.exports().get("/A/").unwrap();
    let b = server.exports().get("/B/").unwrap();
    assert_ne!(a.handle(1), b.handle(1), "the same index in two exports is two handles");
    assert_eq!(a.node_of(&b.handle(1).unwrap()), None);
    assert_eq!(b.node_of(&a.handle(1).unwrap()), None);

    let (_dir, server) = fixture();
    let mut args = Writer::new();
    args.opaque_fixed(b.handle(1).unwrap().as_bytes());
    let reply = ask(&server, PROGRAM_NFS, VERSION_NFS, nfs_proc::GETATTR, args.into_bytes());
    assert_eq!(ok_reader(&reply).u32().unwrap(), nfs_status::STALE);
}

// ---------------------------------------------------------------- paths

#[test]
fn an_absolute_path_splits_into_the_export_and_the_path_within_it() {
    assert_eq!(
        split_export("/Users/chris/Music/track.mp3"),
        Some(("/".into(), "Users/chris/Music/track.mp3".into()))
    );
    assert_eq!(
        split_export("C:\\Users\\chris\\Music\\track.mp3"),
        Some(("/C/".into(), "Users/chris/Music/track.mp3".into()))
    );
    assert_eq!(
        split_export("d:/Music/track.mp3"),
        Some(("/D/".into(), "Music/track.mp3".into()))
    );
    assert_eq!(split_export("relative/path.mp3"), None);
}

#[test]
fn building_the_tree_ignores_traversal_in_a_path() {
    let mut vfs = Vfs::new("/");
    vfs.add_file("../../etc/passwd", "/dev/null", 1, 0);
    // The components that could climb out are dropped, not honoured.
    assert!(vfs.resolve("etc/passwd").is_some());
    assert_eq!(vfs.resolve(".."), Some(vfs.root()));
}

// ---------------------------------------------------------------- as rekordbox's libFilSiNE does

#[test]
fn a_call_seen_before_gets_the_reply_it_got_then() {
    let (_dir, server) = fixture();
    let root = mount_root(&server);
    let mut args = Writer::new();
    args.opaque_fixed(root.as_bytes());
    let bytes = call(PROGRAM_NFS, VERSION_NFS, nfs_proc::GETATTR, args.into_bytes());
    let first = server.handle(&bytes).unwrap();
    let again = server.handle(&bytes).unwrap();
    assert_eq!(first, again, "a retransmit is answered from the cache, not re-executed");
    // From another peer it is a new call.
    assert_eq!(server.handle_from(&bytes, std::net::Ipv4Addr::new(192, 168, 1, 152), 700).unwrap(), first);
}

#[test]
fn lock_manager_calls_are_dropped_without_a_reply() {
    let (_dir, server) = fixture();
    for program in [rbl_nfs::PROGRAM_NLM, rbl_nfs::PROGRAM_NSM] {
        assert!(server.handle(&call(program, 1, 0, Vec::new())).is_none(), "program {program}");
    }
    // Any other unknown program is still told it is unavailable.
    assert!(server.handle(&call(100_099, 1, 0, Vec::new())).is_some());
}

#[test]
fn the_export_list_is_empty_and_a_mount_refused_until_the_link_is_up() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.mp3"), b"x").unwrap();
    let mut vfs = Vfs::new("/");
    vfs.add_file("Music/a.mp3", dir.path().join("a.mp3"), 1, 0);
    let mut exports = Exports::new();
    exports.insert(vfs);
    let up = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0));
    let server = Server::new(exports, NFS_PORT, MOUNT_PORT).with_gate(up.clone());

    let reply = ask(&server, PROGRAM_MOUNT, VERSION_MOUNT, mount_proc::EXPORT, Vec::new());
    let mut reader = ok_reader(&reply);
    assert_eq!(reader.u32().unwrap(), 0, "no export listed before the link is up");
    let mut args = Writer::new();
    args.utf16("/");
    let reply = ask(&server, PROGRAM_MOUNT, VERSION_MOUNT, mount_proc::MNT, args.into_bytes());
    assert_eq!(ok_reader(&reply).u32().unwrap(), nfs_status::ACCES);

    up.store(0x11, std::sync::atomic::Ordering::Relaxed);
    let reply = ask(&server, PROGRAM_MOUNT, VERSION_MOUNT, mount_proc::EXPORT, Vec::new());
    let mut reader = ok_reader(&reply);
    assert_eq!(reader.u32().unwrap(), 1, "the export is listed once the link is up");
    assert_eq!(reader.utf16().unwrap(), "/");
    let root = mount_root(&server);
    assert!(server.is_mounted(std::net::Ipv4Addr::LOCALHOST));
    assert_eq!(server.mounted_hosts(), vec![std::net::Ipv4Addr::LOCALHOST]);

    // Unmounting takes the host off the list.
    let mut args = Writer::new();
    args.utf16("/");
    ask(&server, PROGRAM_MOUNT, VERSION_MOUNT, mount_proc::UMNT, args.into_bytes());
    assert!(!server.is_mounted(std::net::Ipv4Addr::LOCALHOST));
    let _ = root;
}

#[test]
fn a_mount_from_outside_the_export_subnet_is_refused() {
    let (_dir, server) = fixture();
    let server = server.with_export_host("192.168.1.14/255.255.255.0");
    let mut args = Writer::new();
    args.utf16("/");
    let bytes = call(PROGRAM_MOUNT, VERSION_MOUNT, mount_proc::MNT, args.into_bytes());
    let inside = server.handle_from(&bytes, std::net::Ipv4Addr::new(192, 168, 1, 152), 700).unwrap();
    assert_eq!(ok_reader(&inside).u32().unwrap(), nfs_status::OK);
    let outside = server.handle_from(&bytes, std::net::Ipv4Addr::new(10, 0, 0, 5), 700).unwrap();
    assert_eq!(ok_reader(&outside).u32().unwrap(), nfs_status::ACCES);
    assert_eq!(server.mounted_hosts(), vec![std::net::Ipv4Addr::new(192, 168, 1, 152)]);
}

#[test]
fn a_decomposed_name_finds_the_composed_file_and_listings_go_out_decomposed_on_apple() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("e.mp3"), b"x").unwrap();
    let mut vfs = Vfs::new("/");
    vfs.add_file("Music/caf\u{e9}.mp3", dir.path().join("e.mp3"), 1, 0);
    let mut exports = Exports::new();
    exports.insert(vfs);
    let server = Server::new(exports, NFS_PORT, MOUNT_PORT);
    let root = mount_root(&server);
    let music = lookup(&server, &root, "Music").unwrap();
    assert!(lookup(&server, &music, "cafe\u{301}.mp3").is_ok(), "NFD, as a player sends a name it read");
    assert!(lookup(&server, &music, "caf\u{e9}.mp3").is_ok(), "NFC, as the library wrote it");
    let listed = list(&server, &music, 8192);
    let expected = if cfg!(target_vendor = "apple") { "cafe\u{301}.mp3" } else { "caf\u{e9}.mp3" };
    assert_eq!(listed, vec![expected]);
}

#[test]
fn readdir_cookies_are_file_ids_and_an_unknown_one_is_io() {
    let (_dir, server) = fixture();
    let root = mount_root(&server);
    let mut args = Writer::new();
    args.opaque_fixed(root.as_bytes()).u32(0).u32(8192);
    let reply = ask(&server, PROGRAM_NFS, VERSION_NFS, nfs_proc::READDIR, args.into_bytes());
    let mut reader = ok_reader(&reply);
    assert_eq!(reader.u32().unwrap(), nfs_status::OK);
    assert_eq!(reader.u32().unwrap(), 1, "an entry");
    let fileid = reader.u32().unwrap();
    reader.utf16().unwrap();
    assert_eq!(reader.u32().unwrap(), fileid, "the cookie is the entry's file id");

    let mut args = Writer::new();
    args.opaque_fixed(root.as_bytes()).u32(0xdead).u32(8192);
    let reply = ask(&server, PROGRAM_NFS, VERSION_NFS, nfs_proc::READDIR, args.into_bytes());
    assert_eq!(ok_reader(&reply).u32().unwrap(), nfs_status::IO);
}

#[test]
fn portmap_dumps_its_three_mappings() {
    let (_dir, server) = fixture();
    let reply = ask(&server, PROGRAM_PORTMAP, VERSION_PORTMAP, 4, Vec::new());
    let mut reader = ok_reader(&reply);
    let mut seen = Vec::new();
    while reader.u32().unwrap() == 1 {
        let program = reader.u32().unwrap();
        reader.u32().unwrap(); // version
        reader.u32().unwrap(); // protocol
        seen.push((program, reader.u32().unwrap()));
    }
    assert_eq!(seen.len(), 3);
    assert!(seen.contains(&(PROGRAM_NFS, u32::from(NFS_PORT))));
    assert!(seen.contains(&(PROGRAM_MOUNT, u32::from(MOUNT_PORT))));
}

// ---------------------------------------------------------------- allowed folders

/// A server over a temp dir with one folder allowed as a whole: what the
/// library's folders look like to a player. The folder holds a track, a
/// subfolder with another, a file the library does not know, and a symlink
/// out; beside it sits a file that must stay out of reach.
fn folder_fixture() -> (tempfile::TempDir, Server, String) {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("Music");
    fs::create_dir_all(music.join("Artist/Album")).unwrap();
    fs::write(music.join("Artist/Album/one.mp3"), vec![1_u8; 3_000]).unwrap();
    fs::write(music.join("Artist/loose.mp3"), b"loose").unwrap();
    fs::write(music.join("Caf\u{e9}.wav"), b"composed on disk").unwrap();
    fs::write(dir.path().join("beside.mp3"), b"beside the folder").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.path().join("beside.mp3"), music.join("link.mp3")).unwrap();

    let (export, folder) = split_export(music.to_str().unwrap()).unwrap();
    let mut vfs = Vfs::new(export);
    vfs.allow_folder(&folder, &music);
    let mut exports = Exports::new();
    exports.insert(vfs);
    (dir, Server::new(exports, NFS_PORT, MOUNT_PORT), folder)
}

#[test]
fn a_track_under_an_allowed_folder_is_found_and_read_without_being_registered() {
    let (_dir, server, folder) = folder_fixture();
    let root = mount_root(&server);
    let one = lookup_path(&server, &root, &format!("{folder}/Artist/Album/one.mp3")).unwrap();
    assert_eq!(read_whole(&server, &one), vec![1_u8; 3_000]);
    let loose = lookup_path(&server, &root, &format!("{folder}/Artist/loose.mp3")).unwrap();
    assert_eq!(read_whole(&server, &loose), b"loose");
    // Asked twice, it is the same node and the same handle.
    assert_eq!(lookup_path(&server, &root, &format!("{folder}/Artist/Album/one.mp3")).unwrap(), one);
}

#[test]
fn nothing_beside_or_above_an_allowed_folder_resolves() {
    let (dir, server, folder) = folder_fixture();
    let root = mount_root(&server);
    let (_, beside) = split_export(dir.path().join("beside.mp3").to_str().unwrap()).unwrap();
    assert_eq!(lookup_path(&server, &root, &beside), Err(nfs_status::NOENT));
    // The folders on the way exist, but only the way is in them: the temp
    // dir's parent is not listed, and a real name in it is not found.
    let (parent, _) = folder.rsplit_once('/').unwrap();
    let way = lookup_path(&server, &root, parent).unwrap();
    assert_eq!(list(&server, &way, 8192), vec![folder.rsplit_once('/').unwrap().1]);
    assert_eq!(lookup(&server, &way, "beside.mp3"), Err(nfs_status::NOENT));
    // `..` from the allowed folder is the folder on the way, not the host's parent.
    let allowed = lookup_path(&server, &root, &folder).unwrap();
    assert_eq!(lookup(&server, &allowed, ".."), Ok(way));
    assert!(lookup(&server, &way, "..").is_ok());
    // A name that is a path is not a name.
    assert_eq!(lookup(&server, &allowed, "Artist/Album"), Err(nfs_status::NOENT));
}

#[cfg(unix)]
#[test]
fn a_symlink_in_an_allowed_folder_is_not_followed() {
    let (_dir, server, folder) = folder_fixture();
    let root = mount_root(&server);
    assert_eq!(lookup_path(&server, &root, &format!("{folder}/link.mp3")), Err(nfs_status::NOENT));
    let allowed = lookup_path(&server, &root, &folder).unwrap();
    let names = list(&server, &allowed, 8192);
    assert!(!names.iter().any(|n| n == "link.mp3"), "{names:?}");
}

#[test]
fn an_allowed_folder_lists_what_the_host_has_and_a_decomposed_name_finds_a_composed_file() {
    let (_dir, server, folder) = folder_fixture();
    let root = mount_root(&server);
    let allowed = lookup_path(&server, &root, &folder).unwrap();
    let names = list(&server, &allowed, 8192);
    assert!(names.iter().any(|n| n == "Artist"), "{names:?}");
    // The listing sends the name decomposed on Apple hosts, as rekordbox
    // does; a player asks for it that way and gets the file.
    let decomposed = "Cafe\u{301}.wav";
    let cafe = lookup(&server, &allowed, decomposed).unwrap();
    assert_eq!(read_whole(&server, &cafe), b"composed on disk");
    assert_eq!(lookup(&server, &allowed, "Caf\u{e9}.wav"), Ok(cafe));
}
