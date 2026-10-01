//! The link protocols end to end: real sockets, our servers, our fake player.
//!
//! Everything binds ephemeral loopback ports. rekordbox holds the real ones
//! (50000-50002, 2049, 50111, 12523) whenever it is running, and a test that
//! competed for them would only pass with the app under test closed.
#![allow(
    clippy::pedantic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use rbl_dbserver::net::{Bound as DbBound, Handler, Session};
use rbl_dbserver::{kind, menu_footer, menu_header, Argument, Message};
use rbl_fakecdj::{database_port, mount, CdjError, Database};
use rbl_nfs::net::Bound as NfsBound;
use rbl_nfs::{Exports, Vfs};

const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// Two real files, one deep path, one directory with nothing in it.
fn exports(dir: &std::path::Path) -> Exports {
    // A track large enough to need several reads, with content that would
    // expose an off-by-one at any chunk boundary.
    let track: Vec<u8> = (0..40_000_u32).map(|i| (i % 251) as u8).collect();
    fs::write(dir.join("track.mp3"), &track).unwrap();
    fs::write(dir.join("export.pdb"), b"PDB0").unwrap();

    let mut vfs = Vfs::new("/");
    vfs.add_file(
        "Contents/ARTBAT/The Abyss.mp3",
        dir.join("track.mp3"),
        track.len() as u64,
        1_700_000_000,
    );
    vfs.add_file(
        "PIONEER/rekordbox/export.pdb",
        dir.join("export.pdb"),
        4,
        1_700_000_001,
    );
    vfs.add_dir("PIONEER/USBANLZ");

    let mut exports = Exports::new();
    exports.insert(vfs);
    exports
}

#[test]
fn a_player_mounts_browses_and_fetches_a_whole_track() {
    let dir = tempfile::tempdir().unwrap();
    let track: Vec<u8> = (0..40_000_u32).map(|i| (i % 251) as u8).collect();
    let bound = NfsBound::start(exports(dir.path()), LOOPBACK, 0, 0, 0, None, None).unwrap();

    let mut mounted = mount(bound.portmap_address(), "/").unwrap();

    // The tree is what we exported, in order.
    let root = *mounted.root();
    assert_eq!(mounted.list(&root).unwrap(), vec!["Contents", "PIONEER"]);
    let pioneer = mounted.lookup(&root, "PIONEER").unwrap();
    assert_eq!(
        mounted.list(&pioneer).unwrap(),
        vec!["rekordbox", "USBANLZ"]
    );

    // A whole track comes back byte for byte across several 32 KB reads.
    let fetched = mounted.read_file("Contents/ARTBAT/The Abyss.mp3").unwrap();
    assert_eq!(fetched.len(), track.len());
    assert_eq!(fetched, track);

    assert_eq!(
        mounted.read_file("PIONEER/rekordbox/export.pdb").unwrap(),
        b"PDB0"
    );

    bound.shutdown();
}

#[test]
fn a_player_cannot_reach_outside_the_export_over_the_wire() {
    let dir = tempfile::tempdir().unwrap();
    let bound = NfsBound::start(exports(dir.path()), LOOPBACK, 0, 0, 0, None, None).unwrap();
    let mut mounted = mount(bound.portmap_address(), "/").unwrap();
    let root = *mounted.root();

    for name in ["..", "../..", "/etc", "etc"] {
        match mounted.lookup(&root, name) {
            // `..` at the root resolves to the root, which is not an escape.
            Ok(handle) => assert_eq!(handle, root, "{name} must not leave the export"),
            Err(CdjError::Nfs(status)) => assert_eq!(status, rbl_nfs::nfs_status::NOENT, "{name}"),
            Err(error) => panic!("{name}: {error}"),
        }
    }

    // And a directory cannot be read as a file.
    assert!(matches!(
        mounted.read_file("Contents"),
        Err(CdjError::NotAFile(_))
    ));

    bound.shutdown();
}

#[test]
fn mounting_an_export_that_is_not_offered_fails_before_any_lookup() {
    let dir = tempfile::tempdir().unwrap();
    let bound = NfsBound::start(exports(dir.path()), LOOPBACK, 0, 0, 0, None, None).unwrap();
    assert!(matches!(
        mount(bound.portmap_address(), "/C/"),
        Err(CdjError::NoExport(_))
    ));
    bound.shutdown();
}

/// A handler that answers a menu request with a header, some rows and a footer.
///
/// The row contents are ours, not a real menu's: what this proves is the
/// transport — framing, multiple messages per reply, and reassembly. The
/// menus themselves are `rbl_dbserver::session`, tested against the capture.
struct Menu(Arc<MenuRows>);

struct MenuRows {
    rows: Vec<String>,
    seen: AtomicUsize,
}

impl Handler for Menu {
    fn open(&self) -> Box<dyn Session> {
        Box::new(MenuSession(Arc::clone(&self.0)))
    }
}

struct MenuSession(Arc<MenuRows>);

impl Session for MenuSession {
    fn handle(&mut self, message: &Message) -> Vec<Message> {
        let this = &self.0;
        this.seen.fetch_add(1, Ordering::Relaxed);
        match message.kind {
            kind::SETUP => vec![rbl_dbserver::setup_reply(message.transaction, 0x11)],
            kind::RENDER => {
                let mut out = vec![menu_header(
                    message.transaction,
                    u32::from(kind::RENDER),
                    u32::try_from(this.rows.len()).unwrap(),
                )];
                for (index, row) in this.rows.iter().enumerate() {
                    out.push(Message::new(
                        message.transaction,
                        kind::MENU_ITEM,
                        vec![
                            Argument::Number(u32::try_from(index).unwrap()),
                            Argument::String(row.clone()),
                        ],
                    ));
                }
                out.push(menu_footer(message.transaction));
                out
            }
            _ => vec![Message::new(
                message.transaction,
                kind::ERROR,
                vec![Argument::Number(0)],
            )],
        }
    }
}

fn menu_server(rows: Vec<String>) -> (Arc<MenuRows>, DbBound) {
    let shared = Arc::new(MenuRows {
        rows,
        seen: AtomicUsize::new(0),
    });
    let handler: Arc<dyn Handler> = Arc::new(Menu(Arc::clone(&shared)));
    let bound = DbBound::start(handler, LOOPBACK, 0, 0).unwrap();
    (shared, bound)
}

#[test]
fn a_player_finds_the_database_port_then_browses_a_menu() {
    let rows: Vec<String> = (0..40).map(|i| format!("Playlist {i:02}")).collect();
    let (handler, bound) = menu_server(rows.clone());

    // The port query is a separate service, exactly as on a real device.
    let port = database_port(bound.query_address()).unwrap();
    assert_eq!(port, bound.database_address().port());

    let mut session = Database::connect(SocketAddr::new(LOOPBACK, port), 2).unwrap();

    // The whole menu arrives as many messages, and reassembles in order.
    let mut received = session
        .request(kind::RENDER, vec![Argument::Number(0)])
        .unwrap();
    while received.len() < rows.len() + 2 {
        let more = session.receive().unwrap();
        assert!(!more.is_empty(), "the server stopped mid-menu");
        received.extend(more);
    }

    assert_eq!(received.len(), rows.len() + 2);
    assert_eq!(received.first().map(|m| m.kind), Some(kind::MENU_HEADER));
    assert_eq!(received.last().map(|m| m.kind), Some(kind::MENU_FOOTER));

    let names: Vec<String> = received
        .iter()
        .filter(|m| m.kind == kind::MENU_ITEM)
        .filter_map(|m| match m.arguments.get(1) {
            Some(Argument::String(name)) => Some(name.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(names, rows);

    // Setup plus the render: every message we sent was decoded, none twice.
    assert_eq!(handler.seen.load(Ordering::Relaxed), 2);

    bound.shutdown();
}

#[test]
fn a_menu_row_with_non_ascii_text_survives_the_wire() {
    let rows = vec![
        "Björk".to_owned(),
        "とんかつ".to_owned(),
        "🎧 Set".to_owned(),
    ];
    let (_handler, bound) = menu_server(rows.clone());
    let port = database_port(bound.query_address()).unwrap();
    let mut session = Database::connect(SocketAddr::new(LOOPBACK, port), 1).unwrap();

    let mut received = session.request(kind::RENDER, vec![]).unwrap();
    while received.len() < rows.len() + 2 {
        received.extend(session.receive().unwrap());
    }
    let names: Vec<String> = received
        .iter()
        .filter_map(|m| match m.arguments.get(1) {
            Some(Argument::String(name)) => Some(name.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(names, rows);

    bound.shutdown();
}

#[test]
fn two_players_are_served_at_the_same_time() {
    let (_handler, bound) = menu_server(vec!["One".to_owned()]);
    let port = database_port(bound.query_address()).unwrap();
    let address = SocketAddr::new(LOOPBACK, port);

    let mut first = Database::connect(address, 1).unwrap();
    let mut second = Database::connect(address, 2).unwrap();

    // Interleaved, so a server that serialised sessions would deadlock here.
    let a = first.request(kind::RENDER, vec![]).unwrap();
    let b = second.request(kind::RENDER, vec![]).unwrap();
    assert!(!a.is_empty());
    assert!(!b.is_empty());

    bound.shutdown();
}

#[test]
fn an_unknown_request_is_answered_with_an_error_not_silence() {
    let (_handler, bound) = menu_server(vec![]);
    let port = database_port(bound.query_address()).unwrap();
    let mut session = Database::connect(SocketAddr::new(LOOPBACK, port), 1).unwrap();

    let received = session
        .request(kind::ARTWORK, vec![Argument::Number(1)])
        .unwrap();
    assert_eq!(received.first().map(|m| m.kind), Some(kind::ERROR));

    bound.shutdown();
}

#[test]
fn the_servers_stop_cleanly_and_release_their_ports() {
    let dir = tempfile::tempdir().unwrap();
    let nfs = NfsBound::start(exports(dir.path()), LOOPBACK, 0, 0, 0, None, None).unwrap();
    let addresses = [
        nfs.portmap_address(),
        nfs.mount_address(),
        nfs.nfs_address(),
    ];
    nfs.shutdown();

    // Rebinding the same ports proves the sockets were actually released.
    for address in addresses {
        std::net::UdpSocket::bind(address)
            .unwrap_or_else(|error| panic!("{address} was not released: {error}"));
    }

    let (_handler, db) = menu_server(vec![]);
    let query = db.query_address();
    let database = db.database_address();
    db.shutdown();
    for address in [query, database] {
        std::net::TcpListener::bind(address)
            .unwrap_or_else(|error| panic!("{address} was not released: {error}"));
    }
}
