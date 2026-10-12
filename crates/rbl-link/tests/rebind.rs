//! LINK gives every port back when it stops, so it can start again on the
//! same ports without the app restarting. Issue #348: after a sync, LINK went
//! down when Wi-Fi was turned off for a cable, and turning it on again said a
//! port was "already in use — usually by rekordbox itself" until rbxport quit.
//!
//! Fixed port numbers on loopback, chosen free at the start, stand in for
//! rekordbox's fixed ones (12523, 50111, 2049), which a test must not take.
#![allow(
    clippy::pedantic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use std::net::{Ipv4Addr, TcpListener, UdpSocket};
use std::sync::Arc;

use rbl_db::fixture::{self, Shape};
use rbl_db::{Library as Db, OpenMode};
use rbl_link::{Interface, LinkError, LinkExport, Ports, StaticSource};

fn source(dir: &std::path::Path) -> Arc<StaticSource> {
    let shape = Shape {
        tracks: 2,
        playlists: 1,
        tracks_per_playlist: 1,
        history_sessions: 0,
        start_usn: 1000,
    };
    let location = fixture::build(dir, shape).unwrap();
    let db = Db::open(location.clone(), OpenMode::ReadOnly).unwrap();
    let (library, _) = rbl_index::load(&db).unwrap();
    Arc::new(StaticSource {
        library: Arc::new(library),
        share_root: location.share_root,
    })
}

fn free_tcp() -> u16 {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn free_udp() -> u16 {
    UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Every port fixed, as `Ports::REKORDBOX` is, but on numbers free here.
fn fixed_ports() -> Ports {
    Ports {
        announce: free_udp(),
        status: free_udp(),
        query: free_tcp(),
        database: free_tcp(),
        portmap: free_udp(),
        mount: free_udp(),
        nfs: free_udp(),
    }
}

/// The address the file server binds a port on: every address on macOS,
/// pinned to the interface; the interface's own address elsewhere.
fn file_server_bind() -> Ipv4Addr {
    if cfg!(target_vendor = "apple") {
        Ipv4Addr::UNSPECIFIED
    } else {
        Ipv4Addr::LOCALHOST
    }
}

#[test]
fn link_starts_again_on_the_same_ports_after_stopping() {
    let dir = tempfile::tempdir().unwrap();
    let source = source(dir.path());
    let ports = fixed_ports();
    for round in 0..3 {
        let link = LinkExport::start(source.clone(), Interface::loopback(), ports)
            .unwrap_or_else(|e| panic!("start {round}: {e}"));
        assert_eq!(link.query_address().port(), ports.query);
        assert_eq!(link.database_address().port(), ports.database);
        assert_eq!(link.portmap_address().port(), ports.portmap);
        link.stop();
    }
    // And nothing is left holding them once LINK is off.
    TcpListener::bind((Ipv4Addr::LOCALHOST, ports.query)).unwrap();
    TcpListener::bind((Ipv4Addr::LOCALHOST, ports.database)).unwrap();
    UdpSocket::bind((file_server_bind(), ports.portmap)).unwrap();
    UdpSocket::bind((file_server_bind(), ports.nfs)).unwrap();
}

#[test]
fn a_start_that_fails_part_way_leaves_no_port_bound() {
    let dir = tempfile::tempdir().unwrap();
    let source = source(dir.path());
    let ports = fixed_ports();

    // Something else holds the portmap port, as rekordbox does: the beacon
    // and the database server bind, then the file server cannot.
    let holder = UdpSocket::bind((file_server_bind(), ports.portmap)).unwrap();
    match LinkExport::start(source.clone(), Interface::loopback(), ports) {
        Err(LinkError::Bind(why)) => assert!(why.contains(&ports.portmap.to_string()), "{why}"),
        Err(other) => panic!("expected a bind error, got {other}"),
        Ok(_) => panic!("started with the portmap port taken"),
    }
    drop(holder);

    // At once, with nothing to wait on: the database server bound by the
    // failed start must already be gone.
    let link = LinkExport::start(source, Interface::loopback(), ports)
        .unwrap_or_else(|e| panic!("start after a failed start: {e}"));
    assert_eq!(link.query_address().port(), ports.query);
    link.stop();
}
