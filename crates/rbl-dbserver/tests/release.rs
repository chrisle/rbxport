//! The database server gives its ports back when it stops, however it
//! stops. Issue #348: after the interface LINK ran on lost its address,
//! stopping waited forever on accept loops that only a connection could
//! wake, and the ports stayed taken until the app quit.
#![allow(
    clippy::pedantic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rbl_dbserver::net::{Bound, Handler, Session};

struct Silent;

impl Session for Silent {
    fn handle(&mut self, _message: &rbl_dbserver::Message) -> Vec<rbl_dbserver::Message> {
        Vec::new()
    }
}

impl Handler for Silent {
    fn open(&self) -> Box<dyn Session> {
        Box::new(Silent)
    }
}

const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

fn handler() -> Arc<dyn Handler> {
    Arc::new(Silent)
}

/// Binds `address` again, retrying for up to two seconds: the ports come
/// back once the accept threads notice the stop, not at the instant of it.
fn rebinds(address: SocketAddr) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match TcpListener::bind(address) {
            Ok(_) => return true,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => return false,
        }
    }
}

#[test]
fn dropping_the_server_without_shutdown_releases_both_ports() {
    // What `LinkExport::start` did when a later bind failed: the bound
    // database server was simply dropped.
    let bound = Bound::start(handler(), LOOPBACK, 0, 0).unwrap();
    let (query, database) = (bound.query_address(), bound.database_address());
    // Let the accept threads reach `accept`, as they long have by the time
    // a later bind fails in a real start.
    std::thread::sleep(Duration::from_millis(200));
    drop(bound);
    assert!(rebinds(query), "query port {query} still held after drop");
    assert!(rebinds(database), "database port {database} still held after drop");
}

#[test]
fn shutdown_releases_both_ports_and_the_same_ports_serve_again() {
    let bound = Bound::start(handler(), LOOPBACK, 0, 0).unwrap();
    let (query, database) = (bound.query_address(), bound.database_address());
    let started = Instant::now();
    bound.shutdown();
    assert!(started.elapsed() < Duration::from_secs(2), "shutdown took {:?}", started.elapsed());

    // The same fixed ports, twice over, as LINK off and on again does.
    for _ in 0..2 {
        let again = Bound::start(handler(), LOOPBACK, query.port(), database.port()).unwrap();
        assert_eq!(again.query_address(), query);
        TcpStream::connect_timeout(&database, Duration::from_secs(1)).unwrap();
        again.shutdown();
    }
}

#[test]
fn a_player_is_still_accepted_promptly() {
    // The accept loops poll for the stop; a player connecting must not wait
    // on that poll for long.
    let bound = Bound::start(handler(), LOOPBACK, 0, 0).unwrap();
    let mut stream = TcpStream::connect_timeout(&bound.database_address(), Duration::from_secs(1)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    std::io::Write::write_all(&mut stream, rbl_dbserver::GREETING).unwrap();
    let mut greeting = [0_u8; 5];
    let started = Instant::now();
    std::io::Read::read_exact(&mut stream, &mut greeting).unwrap();
    assert_eq!(&greeting[..], rbl_dbserver::GREETING);
    assert!(started.elapsed() < Duration::from_secs(1));
    bound.shutdown();
}

