//! Watching the Pro DJ Link network without joining it.
//!
//! On startup the app wants to know who is on the network — a player or a
//! mixer — so it can offer LINK, but it must not announce as `rekordbox`
//! until the user turns LINK on (a source that announces and cannot answer
//! is worse than one that stays quiet). So this binds the announce port
//! read-only, shared with anything else on it, and reports the devices it
//! hears. Nothing is transmitted.

use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rbl_prolink::{DeviceTable, KeepAlive};

use crate::beacon::Player;

const POLL: Duration = Duration::from_millis(200);
/// Peers are reported no more often than this, however chatty the network.
const REPORT_EVERY: Duration = Duration::from_millis(500);
const DATAGRAM: usize = 512;

/// A running watcher.
pub struct Watcher {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    peers: Arc<Mutex<Vec<Player>>>,
}

impl Watcher {
    /// Binds the announce port (shared) and reports peers until stopped.
    pub fn start<F>(announce_port: u16, mut report: F) -> io::Result<Self>
    where
        F: FnMut(Vec<Player>) + Send + 'static,
    {
        let socket = shared_udp(announce_port)?;
        socket.set_broadcast(true)?;
        socket.set_read_timeout(Some(POLL))?;
        tracing::debug!(
            port = socket.local_addr().map_or(announce_port, |a| a.port()),
            "watching the announce port"
        );

        let stop = Arc::new(AtomicBool::new(false));
        let peers = Arc::new(Mutex::new(Vec::new()));
        let thread = {
            let (stop, peers) = (Arc::clone(&stop), Arc::clone(&peers));
            std::thread::spawn(move || {
                let mut table = DeviceTable::new();
                let started = Instant::now();
                let mut last_report = Instant::now();
                let mut buffer = [0_u8; DATAGRAM];
                while !stop.load(Ordering::Relaxed) {
                    let now = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                    if let Ok((len, from)) = socket.recv_from(&mut buffer) {
                        let packet = buffer.get(..len).unwrap_or(&[]);
                        tracing::trace!(%from, len, bytes = %rbl_prolink::hex(packet, 64), "announce port received");
                        if let Ok(keep_alive) = KeepAlive::decode(packet) {
                            if !table
                                .peers()
                                .iter()
                                .any(|p| p.device_number == keep_alive.device_number)
                            {
                                tracing::debug!(
                                    number = keep_alive.device_number,
                                    name = %keep_alive.name,
                                    kind = ?keep_alive.device_type,
                                    ip = %keep_alive.ip,
                                    "device heard on the network"
                                );
                            }
                            table.observe(&keep_alive, now);
                        }
                    }
                    let expired = table.expire(now);
                    if expired > 0 {
                        tracing::debug!(expired, "devices silent too long; gone from the network");
                    }
                    if last_report.elapsed() >= REPORT_EVERY {
                        last_report = Instant::now();
                        let list: Vec<Player> = table
                            .peers()
                            .iter()
                            .map(|p| Player {
                                number: p.device_number,
                                name: p.name.clone(),
                                address: p.ip,
                                kind: p.device_type,
                                loaded: None,
                                playing: false,
                                master: false,
                                sync: false,
                                cued: false,
                                bpm_x100: 0,
                                last_seen: Instant::now(),
                            })
                            .collect();
                        (*peers.lock()).clone_from(&list);
                        report(list);
                    }
                }
            })
        };
        Ok(Self {
            stop,
            thread: Some(thread),
            peers,
        })
    }

    /// The devices heard, as of the last report.
    pub fn peers(&self) -> Vec<Player> {
        self.peers.lock().clone()
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            drop(thread.join());
        }
        tracing::debug!("announce port watcher stopped");
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// A UDP socket others may share (rekordbox holds 50000 exclusively, so a
/// bind that succeeds is proof rekordbox is not running).
fn shared_udp(port: u16) -> io::Result<UdpSocket> {
    let socket = socket2::Socket::new(socket2::Domain::IPV4, socket2::Type::DGRAM, None)?;
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    socket.set_reuse_port(true)?;
    socket.bind(&SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port)).into())?;
    Ok(socket.into())
}
