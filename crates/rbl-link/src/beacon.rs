//! Our presence on the link: the packets that put `rekordbox` on a player's
//! source list, and the packets from the players that tell us who is there
//! and what they have loaded.
//!
//! Two sockets, two threads. The announce socket (UDP 50000) broadcasts our
//! keep-alive every 2.0 s and hears everyone else's. The status socket (UDP
//! 50002) broadcasts the mixer-style status every 200 ms, answers a player's
//! media query and its `46` handshake, greets a player the first time it
//! reports in, and reads every player's status packet — which is how a
//! track loaded from us is known: the player says so, naming our device
//! number as the track's source. Everything sent is what rekordbox 7.2.11
//! sent a CDJ-3000 (`rbl-prolink`'s tests hold the captured bytes).

use std::collections::HashMap;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use alphatheta_connect::status::types::PlayState;
use alphatheta_connect::status::utils::status_from_packet;
use alphatheta_connect::types::MediaSlot;
use parking_lot::Mutex;
use rbl_prolink::{
    announce_kind_name, connect_greeting, connect_identity, hex, link_handshake_reply, packet_kind,
    status_kind_name, AnnounceKind, DevicePropertyQuery, DevicePropertyResponse, DeviceTable,
    DeviceType, KeepAlive, MediaQuery, MediaResponse, NumberProbe, NumberReply, Status,
    DEVICE_IDENTITY_QUERY_KIND, DEVICE_PROPERTY_QUERY_KIND, LINK_HANDSHAKE_KIND,
    LOAD_TRACK_ACK_KIND, PLAYER_STATUS_KIND, REKORDBOX_NAME, SLOT_REKORDBOX, SLOT_REKORDBOX_LEGACY,
};

use crate::join::{self, Join};

/// How many bytes of a packet a trace line shows.
const TRACE_BYTES: usize = 64;

/// rekordbox's keep-alive interval, measured.
const KEEP_ALIVE_EVERY: Duration = Duration::from_millis(2000);
/// rekordbox's network monitor: once a second it checks the interface it
/// came up on is still there with the same address, and takes the link
/// down when it is not.
const NETWORK_MONITOR_EVERY: Duration = Duration::from_secs(1);
/// rekordbox's status interval, measured.
const STATUS_EVERY: Duration = Duration::from_millis(200);
/// How long a receive blocks before the thread looks at the clock again.
const POLL: Duration = Duration::from_millis(50);
/// A player that has not reported in for this long is no longer holding
/// anything of ours. Players send status at 5 Hz; keep-alives every 1.5 s.
const PLAYER_TIMEOUT: Duration = Duration::from_secs(6);

/// The largest packet either port carries: a CDJ-3000's status is 300
/// bytes, and the mixers' are longer still.
const DATAGRAM: usize = 2048;

/// Where the beacon runs and what it says about the library.
#[derive(Debug, Clone)]
pub struct BeaconConfig {
    /// The OS name of the interface `address` belongs to (`en0`,
    /// `Ethernet 2`), which the sockets are pinned to; `None` on loopback,
    /// where a test has nothing to pin to.
    pub interface: Option<String>,
    pub address: Ipv4Addr,
    pub broadcast: Ipv4Addr,
    pub mac: [u8; 6],
    /// Our announce port; 0 for any free one.
    pub announce_port: u16,
    /// Our status port; 0 for any free one.
    pub status_port: u16,
    /// The port players listen on for status, replies and the greeting:
    /// 50002 on the link. A test's player binds its own.
    pub player_port: u16,
    /// The port players listen on for beat packets: 50001 on the link. A
    /// test binds its own.
    pub beat_port: u16,
    /// This computer's name, as rekordbox puts it in the identity reply.
    pub computer_name: String,
}

/// The tempo-master state the app drives and the beat clock reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MasterState {
    /// We are the network's tempo master, broadcasting beats the others sync
    /// to.
    pub on: bool,
    /// The tempo we drive, × 100. Persists across turning master off and on.
    pub bpm_x100: u16,
    /// The beat within the bar, 1 to 4; the beat clock advances it.
    pub bar_beat: u8,
}

impl Default for MasterState {
    fn default() -> Self {
        // 120.00 BPM until the DJ nudges it or takes a player's tempo, as a
        // resting default; rekordbox shows the last value it held.
        Self {
            on: false,
            bpm_x100: 12_000,
            bar_beat: 1,
        }
    }
}

/// The slowest and fastest master tempo the nudge will reach, × 100
/// (40.00 to 300.00 BPM), so a runaway nudge cannot send a meaningless
/// tempo onto the link.
const MASTER_BPM_MIN: u16 = 4_000;
const MASTER_BPM_MAX: u16 = 30_000;

/// What the media response tells a player about the library, and what the
/// players' status tells the library. Read on every query rather than fixed
/// at start, so a reload behind us is reflected.
pub trait LibraryFacts: Send + Sync {
    fn track_count(&self) -> u16;
    fn playlist_count(&self) -> u16;
    /// A player has just loaded one of our tracks: once per load, not per
    /// status packet.
    fn track_loaded(&self, _track: u32) {}
}

/// A player as its packets describe it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent flags off one status packet"
)]
pub struct Player {
    pub number: u8,
    pub name: String,
    pub address: Ipv4Addr,
    pub kind: DeviceType,
    /// The id of the track it has loaded from us, if one.
    pub loaded: Option<u32>,
    pub playing: bool,
    pub master: bool,
    /// The player has SYNC on: it is tracking the master's tempo.
    pub sync: bool,
    /// The player is sitting at its cue point (play state Cued or Cuing).
    pub cued: bool,
    /// Tempo × 100 as the player reports it — its track's, at its pitch.
    pub bpm_x100: u32,
    pub last_seen: Instant,
}

/// Everything the threads learn, for the app to read.
#[derive(Default)]
struct Shared {
    peers: DeviceTable,
    players: HashMap<u8, Player>,
    /// Present players already greeted, by address. An all-in-one may expose
    /// several player numbers here, so the address is forgotten only after
    /// its last identity leaves.
    greeted: Vec<Ipv4Addr>,
    /// Players heard on the announce port and not yet greeted; the status
    /// loop sends the greeting, from the port rekordbox sends it from.
    to_greet: Vec<Ipv4Addr>,
    /// Our tempo-master state; the beat clock and the status loop share it.
    master: MasterState,
    /// The join: nothing announced until a player is heard, then the number
    /// probe, then a number. `None` until the announce loop makes it.
    join: Option<Join>,
    /// Why the link went down, when it did: the interface lost its address,
    /// or the join failed.
    down: Option<String>,
}

impl Shared {
    fn forget_greeting(&mut self, address: Ipv4Addr) {
        self.greeted.retain(|greeted| *greeted != address);
        self.to_greet.retain(|pending| *pending != address);
    }

    fn remove_players_at(&mut self, address: Ipv4Addr) {
        let leaving: Vec<u8> = self
            .players
            .iter()
            .filter(|(_, player)| player.address == address)
            .map(|(number, _)| *number)
            .collect();
        for number in leaving {
            tracing::info!(number, %address, "device said goodbye; gone from the link");
            self.players.remove(&number);
        }
        self.forget_greeting(address);
    }

    fn expire_silent_players(&mut self) {
        let mut expired_addresses = Vec::new();
        self.players.retain(|number, player| {
            let alive = player.last_seen.elapsed() < PLAYER_TIMEOUT;
            if !alive {
                tracing::info!(
                    number,
                    name = %player.name,
                    address = %player.address,
                    "device silent for 6 s; gone from the link"
                );
                expired_addresses.push(player.address);
            }
            alive
        });
        for address in expired_addresses {
            let address_remains = self
                .players
                .values()
                .any(|player| player.address == address);
            if !address_remains {
                self.forget_greeting(address);
            }
        }
    }
}

/// The running beacon.
pub struct Beacon {
    stop: Arc<AtomicBool>,
    threads: Vec<std::thread::JoinHandle<()>>,
    shared: Arc<Mutex<Shared>>,
    announce_port: u16,
    status_port: u16,
    /// The status socket again, for commands sent from outside its loop.
    /// A command must leave from the port the player has us at, not from a
    /// fresh ephemeral one — that is the source it answers to.
    commands: UdpSocket,
    /// Where the players listen: 50002 on the link, a test's own port.
    player_port: u16,
    /// Our device number once the join settles it, `0` before: what every
    /// packet we send carries, and what the database server answers with.
    number: Arc<AtomicU8>,
}

/// The join and the link as the app sees them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkState {
    /// Listening for a player or mixer; nothing announced yet.
    Waiting,
    /// Probing for a device number.
    Joining,
    /// On the link as this number.
    Up { number: u8 },
    /// Off the link, with why.
    Down(String),
}

impl Beacon {
    /// Binds both ports and starts announcing.
    pub fn start(config: BeaconConfig, facts: Arc<dyn LibraryFacts>) -> io::Result<Self> {
        // Pinned to the chosen interface, so everything leaves from the
        // address the keep-alive announces. A player answers a command only
        // from that address: with two interfaces on the players' subnet the
        // OS otherwise routes our unicast out whichever it likes, and a
        // CDJ-3000 told to load a track from the other one does nothing.
        //
        // Shared, unlike rekordbox's, which holds 50000 exclusively: a
        // listener beside us — the CDJ-3000 emulator's test harness hears
        // announcements on a socket of its own — costs nothing, and
        // rekordbox already running still refuses us, since its bind is
        // the exclusive one.
        let pin = config
            .interface
            .as_deref()
            .map(|name| (name, config.address));
        let announce = shared_udp(config.announce_port, pin)?;
        let status = shared_udp(config.status_port, pin)?;
        for socket in [&announce, &status] {
            socket.set_broadcast(true)?;
            socket.set_read_timeout(Some(POLL))?;
        }

        // Ephemeral ports are known only now; the loops broadcast to them.
        let mut config = config;
        config.announce_port = announce.local_addr()?.port();
        config.status_port = status.local_addr()?.port();
        let (announce_port, status_port) = (config.announce_port, config.status_port);
        tracing::debug!(
            interface = config.interface.as_deref().unwrap_or("any"),
            address = %config.address,
            broadcast = %config.broadcast,
            mac = %hex(&config.mac, 6),
            announce_port,
            status_port,
            player_port = config.player_port,
            beat_port = config.beat_port,
            computer_name = %config.computer_name,
            "beacon bound"
        );

        // Kept before the loop takes ownership: a load command has to go out
        // from this same port, so the player sees it from the device it knows.
        let commands = status.try_clone()?;
        let player_port = config.player_port;

        let stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Mutex::new(Shared::default()));
        let number = Arc::new(AtomicU8::new(0));
        let mut threads = Vec::with_capacity(3);
        {
            let (stop, shared, config, number) = (
                Arc::clone(&stop),
                Arc::clone(&shared),
                config.clone(),
                Arc::clone(&number),
            );
            threads.push(std::thread::spawn(move || {
                announce_loop(&announce, &config, &stop, &shared, &number)
            }));
        }
        {
            let (stop, shared, number) =
                (Arc::clone(&stop), Arc::clone(&shared), Arc::clone(&number));
            let config = config.clone();
            threads.push(std::thread::spawn(move || {
                status_loop(&status, &config, &stop, &shared, &facts, &number)
            }));
        }
        // The beat clock broadcasts a beat on its own socket, tempo-locked,
        // only while we are master; a bind failure loses only the beats, not
        // the rest of LINK, so it falls back to nothing rather than aborting.
        if let Ok(beats) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).and_then(|s| {
            s.set_broadcast(true)?;
            Ok(s)
        }) {
            let (stop, shared, config, number) = (
                Arc::clone(&stop),
                Arc::clone(&shared),
                config.clone(),
                Arc::clone(&number),
            );
            threads.push(std::thread::spawn(move || {
                beat_clock(&beats, &config, &stop, &shared, &number)
            }));
        } else {
            tracing::warn!("beat clock socket could not bind; LINK master will not drive tempo");
        }
        Ok(Self {
            stop,
            threads,
            shared,
            announce_port,
            status_port,
            commands,
            player_port,
            number,
        })
    }

    /// Our device number, once the join has settled one.
    pub fn number(&self) -> Option<u8> {
        match self.number.load(Ordering::Relaxed) {
            0 => None,
            number => Some(number),
        }
    }

    /// The cell the number lives in, for the servers that answer with it.
    pub fn number_cell(&self) -> Arc<AtomicU8> {
        Arc::clone(&self.number)
    }

    /// Where the link is: waiting, joining, up, or down and why.
    pub fn link_state(&self) -> LinkState {
        let shared = self.shared.lock();
        if let Some(why) = &shared.down {
            return LinkState::Down(why.clone());
        }
        match shared.join.as_ref().map(Join::state) {
            None | Some(join::State::Waiting) => LinkState::Waiting,
            Some(join::State::Running { number }) => LinkState::Up { number: *number },
            Some(join::State::Failed(why)) => LinkState::Down(why.clone()),
            Some(_) => LinkState::Joining,
        }
    }

    pub const fn announce_port(&self) -> u16 {
        self.announce_port
    }

    pub const fn status_port(&self) -> u16 {
        self.status_port
    }

    /// Every player heard from, in device-number order.
    pub fn players(&self) -> Vec<Player> {
        let mut players: Vec<Player> = self.shared.lock().players.values().cloned().collect();
        players.sort_by_key(|p| p.number);
        players
    }

    /// Tells player `player_number` to load `track_id` from our library.
    /// Returns an error when the player is unknown or the packet cannot be sent.
    pub fn load_track(&self, player_number: u8, track_id: u32) -> io::Result<()> {
        let Some(number) = self.number() else {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "not on the link yet: no device number",
            ));
        };
        let address = player_address(&self.shared.lock().players, player_number);
        let Some(address) = address else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("player {player_number} is not on the link"),
            ));
        };
        let packet =
            rbl_prolink::load_track_command(REKORDBOX_NAME, number, player_number, track_id);
        let to = SocketAddr::V4(SocketAddrV4::new(address, self.player_port));
        let sent = self.commands.send_to(&packet, to)?;
        tracing::debug!(player_number, track_id, %address, sent, "load track sent");
        tracing::trace!(%to, bytes = %hex(&packet, packet.len()), "load track packet");
        Ok(())
    }

    /// Our tempo-master state, for the app to show.
    pub fn master_state(&self) -> MasterState {
        self.shared.lock().master
    }

    /// Become the network's tempo master, or resign. Becoming master keeps
    /// whatever BPM is set; the beat clock starts driving beats at once and
    /// the status packets say we are master.
    ///
    /// Asserting master is enough — no handoff protocol is needed. Verified
    /// live (2026-09-14) on a real CDJ-3000: a deck that was itself master
    /// yields the moment it sees our master status and follows our tempo,
    /// sending no `0x26`/`0x27` handoff of its own, and when we resign a
    /// synced, playing deck takes master over on its own.
    pub fn set_master(&self, on: bool) {
        let mut shared = self.shared.lock();
        shared.master.on = on;
        if on {
            // Start each master run on the downbeat.
            shared.master.bar_beat = 1;
        }
        tracing::info!(on, bpm_x100 = shared.master.bpm_x100, "link master");
    }

    /// Set the master tempo (× 100), clamped to a sane range. Used by the
    /// "take the current master's tempo" button and any direct set.
    pub fn set_master_bpm(&self, bpm_x100: u16) {
        let mut shared = self.shared.lock();
        shared.master.bpm_x100 = bpm_x100.clamp(MASTER_BPM_MIN, MASTER_BPM_MAX);
        tracing::debug!(
            asked = bpm_x100,
            set = shared.master.bpm_x100,
            "master tempo set"
        );
    }

    /// Nudge the master tempo by `delta_x100` (rekordbox's −/+ move it a whole
    /// BPM), clamped to the same range.
    pub fn nudge_master(&self, delta_x100: i32) {
        let mut shared = self.shared.lock();
        let next = i32::from(shared.master.bpm_x100) + delta_x100;
        let clamped = next.clamp(i32::from(MASTER_BPM_MIN), i32::from(MASTER_BPM_MAX));
        shared.master.bpm_x100 = u16::try_from(clamped).unwrap_or(MASTER_BPM_MIN);
        tracing::debug!(
            delta_x100,
            bpm_x100 = shared.master.bpm_x100,
            "master tempo nudged"
        );
    }

    /// The tempo a player on the link currently reports as master, × 100, or
    /// `None` when no player is master. What the "take the master's tempo"
    /// button reads.
    pub fn current_player_tempo(&self) -> Option<u16> {
        self.shared
            .lock()
            .players
            .values()
            .find(|p| p.master && p.bpm_x100 != 0)
            .map(|p| u16::try_from(p.bpm_x100).unwrap_or(u16::MAX))
    }

    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for thread in self.threads.drain(..) {
            drop(thread.join());
        }
        tracing::debug!("beacon stopped");
    }
}

impl Drop for Beacon {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// A UDP socket other listeners may share, pinned to one interface when
/// `pin` names it (with the address on it), or on every interface.
///
/// Pinning is by interface rather than by binding to the address, because
/// on macOS and Linux a socket bound to one address hears no broadcasts,
/// and the keep-alives are broadcasts. Windows delivers broadcasts to such
/// a socket, and has no interface pin for them, so there the address is
/// bound.
fn shared_udp(port: u16, pin: Option<(&str, Ipv4Addr)>) -> io::Result<UdpSocket> {
    let socket = socket2::Socket::new(socket2::Domain::IPV4, socket2::Type::DGRAM, None)?;
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    socket.set_reuse_port(true)?;
    let bind_to = match pin {
        Some((_, address)) if cfg!(windows) => address,
        _ => Ipv4Addr::UNSPECIFIED,
    };
    socket.bind(&SocketAddr::V4(SocketAddrV4::new(bind_to, port)).into())?;
    if let Some((name, _)) = pin {
        pin_to_interface(&socket, name)?;
    }
    Ok(socket.into())
}

/// `IP_BOUND_IF`: sends leave by this interface, from its address, and only
/// what arrives on it is received. It takes the interface's index, which
/// the OS lists beside the name.
#[cfg(target_vendor = "apple")]
fn pin_to_interface(socket: &socket2::Socket, name: &str) -> io::Result<()> {
    let index = if_addrs::get_if_addrs()?
        .into_iter()
        .find(|i| i.name == name)
        .and_then(|i| i.index)
        .and_then(std::num::NonZeroU32::new)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("no network interface called {name}"),
            )
        })?;
    socket.bind_device_by_index_v4(Some(index))
}

/// `SO_BINDTODEVICE`, the same by name.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn pin_to_interface(socket: &socket2::Socket, name: &str) -> io::Result<()> {
    socket.bind_device(Some(name.as_bytes()))
}

/// Bound to the interface's address instead, above.
#[cfg(not(any(target_vendor = "apple", target_os = "linux", target_os = "android")))]
fn pin_to_interface(_socket: &socket2::Socket, _name: &str) -> io::Result<()> {
    Ok(())
}

fn now_ms(since: Instant) -> u64 {
    u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// The join, then keep-alives every two seconds; everyone else's packets
/// into the peer table; the interface watched once a second.
fn announce_loop(
    socket: &UdpSocket,
    config: &BeaconConfig,
    stop: &AtomicBool,
    shared: &Mutex<Shared>,
    number: &AtomicU8,
) {
    let started = Instant::now();
    let broadcast = SocketAddr::V4(SocketAddrV4::new(config.broadcast, config.announce_port));
    let mut buffer = [0_u8; DATAGRAM];
    shared.lock().join = Some(Join::new(config.mac, config.address, started));

    let mut next_keep_alive: Option<Instant> = None;
    let mut next_monitor = Instant::now() + NETWORK_MONITOR_EVERY;
    while !stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        // The join's packets when due, and the number the moment it settles.
        let outgoing = {
            let mut shared = shared.lock();
            let outgoing = shared.join.as_mut().and_then(|join| join.tick(now));
            let settled = shared.join.as_ref().and_then(Join::number).unwrap_or(0);
            if settled != number.load(Ordering::Relaxed) {
                number.store(settled, Ordering::Relaxed);
                next_keep_alive = (settled != 0).then_some(now);
            }
            if let Some(join::State::Failed(why)) = shared.join.as_ref().map(Join::state) {
                if shared.down.is_none() {
                    shared.down = Some(why.clone());
                }
            }
            outgoing
        };
        if let Some(out) = outgoing {
            send_announce(socket, &out, broadcast, config.announce_port);
        }
        // Keep-alives only with a number: rekordbox announces nothing into
        // an empty network, and nothing before its number is settled.
        if let Some(due) = next_keep_alive {
            if now >= due {
                next_keep_alive = Some(due + KEEP_ALIVE_EVERY);
                // The count of *other* devices we see, not counting ourselves:
                // captured 2026-09-12 against rekordbox 7.2.11, which sent 0x02
                // at keep-alive offset 0x30 with a deck and one other client on
                // the LAN (two peers), where rbxport had been sending 0x03.
                let peers = u8::try_from(shared.lock().peers.len()).unwrap_or(u8::MAX);
                let ours = number.load(Ordering::Relaxed);
                let packet =
                    KeepAlive::rekordbox_as(ours, config.mac, config.address, peers).encode();
                match socket.send_to(&packet, broadcast) {
                    Ok(_) => tracing::trace!(number = ours, peers, %broadcast, "keep-alive sent"),
                    Err(error) => tracing::warn!(%error, "keep-alive not sent"),
                }
            }
        }
        match socket.recv_from(&mut buffer) {
            Ok((len, from)) => {
                let reply = hear_announce(
                    buffer.get(..len).unwrap_or(&[]),
                    from,
                    config,
                    shared,
                    started,
                );
                if let Some(out) = reply {
                    send_announce(socket, &out, broadcast, config.announce_port);
                }
            }
            Err(error) if is_timeout(&error) => {}
            Err(error) => {
                tracing::error!(%error, "announce socket stopped; the link will not hear new devices");
                return;
            }
        }
        shared.lock().expire_silent_players();
        if Instant::now() >= next_monitor {
            next_monitor += NETWORK_MONITOR_EVERY;
            if let Some(why) = interface_lost(config) {
                let mut shared = shared.lock();
                if shared.down.is_none() {
                    tracing::error!("{why}; the link is down");
                    shared.down = Some(why);
                    if let Some(join) = shared.join.as_mut() {
                        join.reset(Instant::now());
                    }
                    number.store(0, Ordering::Relaxed);
                    next_keep_alive = None;
                }
            }
        }
    }
    tracing::debug!("announce loop stopped");
}

/// Sends what the join asks for: to the broadcast, or to the one device
/// named, on the announce port.
fn send_announce(socket: &UdpSocket, out: &join::Outgoing, broadcast: SocketAddr, port: u16) {
    let to = out
        .to
        .map_or(broadcast, |ip| SocketAddr::V4(SocketAddrV4::new(ip, port)));
    match socket.send_to(&out.packet, to) {
        Ok(_) => tracing::trace!(what = out.what, len = out.packet.len(), %to, "sent"),
        Err(error) => tracing::warn!(%error, what = out.what, %to, "not sent"),
    }
}

/// rekordbox's network monitor: the interface the link came up on, still
/// there with the address it announced? `None` while it is; on loopback
/// (no interface named) nothing is watched.
fn interface_lost(config: &BeaconConfig) -> Option<String> {
    let name = config.interface.as_deref()?;
    // Every interface, loopback included: a test pins to lo0.
    let present = alphatheta_connect::utils::network_interfaces()
        .into_iter()
        .any(|i| i.name == name && i.address == config.address);
    (!present).then(|| format!("{name} no longer has the address {}", config.address))
}

/// A packet off the announce port: a keep-alive into the peer table, the
/// player list and the join; a probe or a reply to the join; a first-heard
/// player queued for the greeting. What the join wants sent back, if
/// anything.
fn hear_announce(
    packet: &[u8],
    from: SocketAddr,
    config: &BeaconConfig,
    shared: &Mutex<Shared>,
    started: Instant,
) -> Option<join::Outgoing> {
    let len = packet.len();
    let kind = packet_kind(packet).ok();
    tracing::trace!(
        %from,
        len,
        kind = %kind.map_or_else(|| "not a link packet".to_owned(), announce_kind_name),
        bytes = %hex(packet, TRACE_BYTES),
        "announce port received"
    );
    let SocketAddr::V4(from) = from else {
        return None;
    };
    match kind.map(AnnounceKind::from_u8) {
        Some(AnnounceKind::ClaimStage2) => {
            let probe = NumberProbe::decode(packet).ok()?;
            return shared
                .lock()
                .join
                .as_mut()
                .and_then(|join| join.hear_probe(&probe));
        }
        Some(AnnounceKind::Other(0x03)) => {
            if let Ok(reply) = NumberReply::decode(packet) {
                if let Some(join) = shared.lock().join.as_mut() {
                    join.hear_reply(&reply);
                }
            }
            return None;
        }
        // rekordbox drops every member, stops its timers and starts over
        // on a compatibility response (`readCompatiRes`).
        Some(AnnounceKind::Other(0x0b)) => {
            tracing::warn!(%from, "compatibility response; leaving the link and starting over");
            let mut shared = shared.lock();
            shared.peers = DeviceTable::new();
            shared.players.clear();
            shared.greeted.clear();
            shared.to_greet.clear();
            if let Some(join) = shared.join.as_mut() {
                join.reset(Instant::now());
            }
            return None;
        }
        // A disconnect names the device leaving.
        Some(AnnounceKind::Other(0x07 | 0x08)) => {
            let mut shared = shared.lock();
            shared.remove_players_at(*from.ip());
            return None;
        }
        Some(AnnounceKind::KeepAlive) => {}
        _ => {
            tracing::trace!(%from, len, "announce port packet is not one the join or the peer table reads");
            return None;
        }
    }
    if let Ok(keep_alive) = KeepAlive::decode(packet) {
        if keep_alive.ip == config.address && keep_alive.mac == config.mac {
            return None; // our own broadcast, echoed back
        }
        let mut shared = shared.lock();
        let now = now_ms(started);
        if let Some(join) = shared.join.as_mut() {
            join.hear_keep_alive(&keep_alive, Instant::now());
        }
        let known = shared
            .peers
            .peers()
            .iter()
            .any(|p| p.device_number == keep_alive.device_number);
        shared.peers.observe(&keep_alive, now);
        let expired = shared.peers.expire(now);
        if expired > 0 {
            tracing::debug!(expired, "peers timed out of the keep-alive table");
        }
        if !known {
            tracing::info!(
                number = keep_alive.device_number,
                name = %keep_alive.name,
                kind = ?keep_alive.device_type,
                ip = %keep_alive.ip,
                mac = %hex(&keep_alive.mac, 6),
                "new device on the link"
            );
        }
        // A device is listed from its keep-alive; its status
        // fills in the rest when it comes.
        shared
            .players
            .entry(keep_alive.device_number)
            .or_insert_with(|| Player {
                number: keep_alive.device_number,
                name: keep_alive.name.clone(),
                address: *from.ip(),
                kind: keep_alive.device_type,
                loaded: None,
                playing: false,
                master: false,
                sync: false,
                cued: false,
                bpm_x100: 0,
                last_seen: Instant::now(),
            });
        if let Some(player) = shared.players.get_mut(&keep_alive.device_number) {
            player.last_seen = Instant::now();
            player.name.clone_from(&keep_alive.name);
        }
        // A player is greeted when first heard: in the capture the
        // greeting is what the player's portmap query follows,
        // six milliseconds later.
        if keep_alive.device_type == DeviceType::Cdj
            && !shared.greeted.contains(from.ip())
            && !shared.to_greet.contains(from.ip())
        {
            tracing::debug!(player = %from.ip(), "player heard for the first time; greeting queued");
            shared.to_greet.push(*from.ip());
        }
    }
    None
}

/// Status out five times a second, players' status in, the questions a
/// player asks on this port answered.
fn status_loop(
    socket: &UdpSocket,
    config: &BeaconConfig,
    stop: &AtomicBool,
    shared: &Mutex<Shared>,
    facts: &Arc<dyn LibraryFacts>,
    number: &AtomicU8,
) {
    // Status goes where the players listen, which on the link is the same
    // port we listen on.
    let broadcast = SocketAddr::V4(SocketAddrV4::new(config.broadcast, config.player_port));
    // rekordbox sends its STATUS from an EPHEMERAL source port, not from 50002
    // (measured on the wire: 51839/59681/…, a different one each time). A CDJ
    // may key its "this is a real rekordbox source" test off that, so status
    // goes out from an ephemeral socket while `socket` stays bound to 50002 for
    // receiving and for the unicast replies (which rekordbox does send from
    // 50002). Falls back to `socket` if the extra bind fails.
    let sender = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok();
    if let Some(s) = sender.as_ref() {
        let _ = s.set_broadcast(true);
    }
    let out = sender.as_ref().unwrap_or(socket);
    tracing::debug!(
        status_from = %out.local_addr().map_or_else(|e| e.to_string(), |a| a.to_string()),
        replies_from = %socket.local_addr().map_or_else(|e| e.to_string(), |a| a.to_string()),
        %broadcast,
        "status loop started"
    );
    let mut buffer = [0_u8; DATAGRAM];
    let mut next_send = Instant::now();
    let mut beat: u8 = 1;
    while !stop.load(Ordering::Relaxed) {
        // Nothing said and nothing answered until the join has a number:
        // rekordbox's status starts on LINKUP, and its receive path is gated
        // until then.
        let ours = number.load(Ordering::Relaxed);
        if ours == 0 {
            std::thread::sleep(POLL);
            next_send = Instant::now();
            continue;
        }
        if Instant::now() >= next_send {
            next_send += STATUS_EVERY;
            let packet = status_packet(&shared.lock(), ours, beat);
            beat = if beat >= 4 { 1 } else { beat + 1 };
            match out.send_to(&packet, broadcast) {
                Ok(_) => {
                    tracing::trace!(len = packet.len(), bytes = %hex(&packet, TRACE_BYTES), "status sent")
                }
                Err(error) => tracing::warn!(%error, "status not sent"),
            }
        }
        let pending: Vec<Ipv4Addr> = {
            let mut shared = shared.lock();
            let pending = std::mem::take(&mut shared.to_greet);
            shared.greeted.extend(pending.iter().copied());
            pending
        };
        for player in pending {
            let greeting = connect_greeting(REKORDBOX_NAME, ours);
            send(socket, &greeting, player, config.player_port, "greeting");
        }
        let (len, from) = match socket.recv_from(&mut buffer) {
            Ok(received) => received,
            Err(error) if is_timeout(&error) => continue,
            Err(error) => {
                tracing::error!(%error, "status socket stopped; players will not be answered");
                return;
            }
        };
        let packet = buffer.get(..len).unwrap_or(&[]);
        let SocketAddr::V4(from) = from else { continue };
        // Our own status comes back off the broadcast; its kind is one
        // nothing below handles.
        let Ok(kind) = packet_kind(packet) else {
            tracing::trace!(%from, len, bytes = %hex(packet, TRACE_BYTES), "status port received a non-link packet");
            continue;
        };
        if from.ip() != &config.address {
            tracing::trace!(
                %from,
                len,
                kind = %status_kind_name(kind),
                bytes = %hex(packet, TRACE_BYTES),
                "status port received"
            );
        }
        match kind {
            DEVICE_IDENTITY_QUERY_KIND => {
                // The player announces itself with `10`; rekordbox answers
                // with its own identity (`11`), and only then does the player
                // go on to the media query and the mount. Sent to the status
                // port, as rekordbox sends it, not the player's source port.
                tracing::debug!(player = %from.ip(), "device identity query; answering with ours");
                let identity = connect_identity(REKORDBOX_NAME, ours, &config.computer_name);
                send(
                    socket,
                    &identity,
                    *from.ip(),
                    config.player_port,
                    "identity",
                );
            }
            0x05 => answer_media_query(socket, packet, config, facts.as_ref(), ours),
            DEVICE_PROPERTY_QUERY_KIND => {
                answer_device_property_query(socket, packet, from, config, ours)
            }
            LINK_HANDSHAKE_KIND => {
                tracing::debug!(player = %from.ip(), "link handshake; answering");
                let reply = link_handshake_reply(REKORDBOX_NAME, ours);
                send(
                    socket,
                    &reply,
                    *from.ip(),
                    config.player_port,
                    "handshake reply",
                );
            }
            // A player that accepts a load command says so with `1a`; the
            // load itself shows up in its next status packets.
            LOAD_TRACK_ACK_KIND => {
                tracing::info!(from = %from.ip(), "player accepted a load track command");
            }
            PLAYER_STATUS_KIND => {
                hear_player_status(packet, from, socket, config, shared, facts, ours)
            }
            _ => {
                if from.ip() != &config.address {
                    tracing::trace!(%from, kind = %status_kind_name(kind), "status port packet not handled");
                }
            }
        }
    }
    tracing::debug!("status loop stopped");
}

/// Answers the RX3's source-eligibility request. The RX3 does not render a
/// registered rekordbox peer in SOURCE until this response marks its PC media
/// as detected.
fn answer_device_property_query(
    socket: &UdpSocket,
    packet: &[u8],
    from: SocketAddrV4,
    config: &BeaconConfig,
    ours: u8,
) {
    let query = match DevicePropertyQuery::decode(packet) {
        Ok(query) => query,
        Err(error) => {
            tracing::warn!(%error, len = packet.len(), "device property query could not be read");
            return;
        }
    };
    tracing::debug!(player = %from.ip(), requester = query.requester, "device property query; answering");
    let response = DevicePropertyResponse {
        name: REKORDBOX_NAME.to_owned(),
        device_number: ours,
    }
    .encode();
    send(
        socket,
        &response,
        *from.ip(),
        config.player_port,
        "device property response",
    );
}

/// A player's status packet: what it has loaded, whether it plays, whether
/// it is master; the greeting on its first one.
fn hear_player_status(
    packet: &[u8],
    from: SocketAddrV4,
    socket: &UdpSocket,
    config: &BeaconConfig,
    shared: &Mutex<Shared>,
    facts: &Arc<dyn LibraryFacts>,
    ours: u8,
) {
    let len = packet.len();
    let state = match status_from_packet(packet) {
        Ok(Some(state)) => state,
        Ok(None) => {
            tracing::trace!(%from, len, "status packet not a player's; ignored");
            return;
        }
        Err(error) => {
            tracing::warn!(%from, len, %error, "status packet could not be read");
            return;
        }
    };
    tracing::trace!(
        %from,
        device = state.device_id,
        track_id = state.track_id,
        track_device = state.track_device_id,
        track_slot = ?state.track_slot,
        play_state = ?state.play_state,
        master = state.is_master,
        bpm = ?state.track_bpm,
        pitch = state.effective_pitch,
        "player status"
    );
    let mut shared = shared.lock();
    if !shared.greeted.contains(from.ip()) {
        // The first status from a player is what rekordbox
        // answers with the greeting `[ASSUME]`; it sent one just
        // before the player's portmap query.
        tracing::debug!(player = %from.ip(), "first status from a player; greeting it");
        shared.greeted.push(*from.ip());
        let greeting = connect_greeting(REKORDBOX_NAME, ours);
        send(
            socket,
            &greeting,
            *from.ip(),
            config.player_port,
            "greeting",
        );
    }
    // The player names the source device and the slot; a track
    // of ours is one it took from our device number. The slot byte says
    // `Rb` for a rekordbox source in the community analysis, but
    // the player's media query about us asks for slot 3 (USB), so
    // the slot is not relied on.
    let from_us = state.track_device_id == ours
        && matches!(state.track_slot, MediaSlot::Rb | MediaSlot::Usb)
        && state.track_id != 0;
    let playing = matches!(state.play_state, PlayState::Playing | PlayState::Looping);
    // The status packet does not name the device kind; the
    // keep-alive does. Read it from the peer table by number so a
    // mixer is shown as a mixer, not a player.
    let kind = peer_kind(&shared, state.device_id);
    let player = shared.players.entry(state.device_id).or_insert_with(|| {
        let name = rbl_prolink::status_device_name(packet).unwrap_or_default();
        tracing::debug!(number = state.device_id, %name, address = %from.ip(), "player listed from its status");
        Player {
            number: state.device_id,
            name,
            address: *from.ip(),
            kind,
            loaded: None,
            playing: false,
            master: false,
            sync: false,
            cued: false,
            bpm_x100: 0,
            last_seen: Instant::now(),
        }
    });
    let loaded = from_us.then_some(state.track_id);
    let cued = matches!(state.play_state, PlayState::Cued | PlayState::Cuing);
    if player.loaded != loaded {
        tracing::debug!(
            number = player.number,
            was = player.loaded,
            now = loaded,
            track_device = state.track_device_id,
            track_slot = ?state.track_slot,
            "player's loaded track of ours changed"
        );
        if let Some(track) = loaded {
            facts.track_loaded(track);
        }
    }
    if player.playing != playing {
        tracing::debug!(number = player.number, playing, play_state = ?state.play_state, "player play state changed");
    }
    if player.master != state.is_master {
        tracing::debug!(
            number = player.number,
            master = state.is_master,
            "player master state changed"
        );
    }
    player.kind = kind;
    player.loaded = loaded;
    player.playing = playing;
    player.master = state.is_master;
    player.sync = state.is_sync;
    player.cued = cued;
    player.bpm_x100 = tempo_x100(state.track_bpm, state.effective_pitch);
    player.last_seen = Instant::now();
}

/// The status packet to broadcast now. When we are master we say so, at our
/// own tempo and the beat clock's beat. Otherwise we echo the master player's
/// tempo with a beat that free-runs at the status rate (rekordbox echoes the
/// master's; with no master on the link it is `[UNKNOWN]`, so zero). The
/// status flag and Mm say which of the two this is.
fn status_packet(shared: &Shared, ours: u8, free_beat: u8) -> Vec<u8> {
    let master = shared.master;
    let (bpm_x100, beat, we_master) = if master.on {
        (master.bpm_x100, master.bar_beat, true)
    } else {
        let mirror = shared
            .players
            .values()
            .find(|p| p.master)
            .map_or(0, |p| u16::try_from(p.bpm_x100).unwrap_or(u16::MAX));
        (mirror, if mirror == 0 { 0 } else { free_beat }, false)
    };
    Status {
        name: REKORDBOX_NAME.to_owned(),
        device_number: ours,
        bpm_x100,
        beat,
        master: we_master,
    }
    .encode()
}

/// Broadcasts a beat packet on each beat while we are master, and advances
/// the shared bar beat 1 → 2 → 3 → 4. Idle, it waits.
///
/// The next beat is scheduled from the last rather than from a fresh sleep,
/// so the tempo does not drift with the OS's sleep granularity; a nudge is
/// picked up on the next beat because the interval is read each time.
fn beat_clock(
    socket: &UdpSocket,
    config: &BeaconConfig,
    stop: &AtomicBool,
    shared: &Mutex<Shared>,
    number: &AtomicU8,
) {
    let to = SocketAddr::V4(SocketAddrV4::new(config.broadcast, config.beat_port));
    let mut next_beat = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        let (on, bpm_x100, bar_beat) = {
            let s = shared.lock();
            (s.master.on, s.master.bpm_x100, s.master.bar_beat)
        };
        let now = Instant::now();
        let ours = number.load(Ordering::Relaxed);
        if !on || ours == 0 {
            // The first beat on becoming master falls at once.
            next_beat = now;
            std::thread::sleep(POLL);
            continue;
        }
        if now < next_beat {
            std::thread::sleep((next_beat - now).min(POLL));
            continue;
        }
        let packet = rbl_prolink::beat_packet(REKORDBOX_NAME, ours, bpm_x100, bar_beat);
        match socket.send_to(&packet, to) {
            Ok(_) => tracing::trace!(bpm_x100, bar_beat, %to, "beat sent"),
            Err(error) => tracing::warn!(%error, "beat not sent"),
        }
        shared.lock().master.bar_beat = if bar_beat >= 4 { 1 } else { bar_beat + 1 };
        // 60000/bpm ms a beat; bpm is × 100, so 6_000_000 / bpm_x100 ms.
        let interval = Duration::from_millis(6_000_000 / u64::from(bpm_x100.max(1)));
        next_beat += interval;
        // Behind by more than a beat (a tempo jump, or the thread was
        // starved): resync rather than fire a burst to catch up.
        if next_beat < now {
            next_beat = now + interval;
        }
    }
}

/// Answers a player asking what is in our rekordbox slot with the library's
/// counts, naming back whichever slot number it used for us — `04` from a
/// current CDJ-3000, `03` from the EP122 emulator — as rekordbox does. A
/// question about any other device or slot is not ours to answer.
fn answer_media_query(
    socket: &UdpSocket,
    packet: &[u8],
    config: &BeaconConfig,
    facts: &dyn LibraryFacts,
    ours: u8,
) {
    let query = match MediaQuery::decode(packet) {
        Ok(query) => query,
        Err(error) => {
            tracing::warn!(%error, len = packet.len(), "media query could not be read");
            return;
        }
    };
    if query.device_number != ours || ![SLOT_REKORDBOX, SLOT_REKORDBOX_LEGACY].contains(&query.slot)
    {
        tracing::trace!(
            from = %query.from,
            device = query.device_number,
            slot = query.slot,
            "media query about another device or slot; not ours to answer"
        );
        return;
    }
    let (tracks, playlists) = (facts.track_count(), facts.playlist_count());
    tracing::debug!(from = %query.from, slot = query.slot, tracks, playlists, "media query about our slot; answering");
    let response = MediaResponse {
        name: REKORDBOX_NAME.to_owned(),
        device_number: ours,
        slot: query.slot,
        tracks,
        playlists,
    }
    .encode();
    send(
        socket,
        &response,
        query.from,
        config.player_port,
        "media response",
    );
}

/// The device kind of the peer with `number`, from the keep-alive table, or a
/// player until its keep-alive has been heard.
fn peer_kind(shared: &Shared, number: u8) -> DeviceType {
    shared
        .peers
        .peers()
        .iter()
        .find(|p| p.device_number == number)
        .map_or(DeviceType::Cdj, |p| p.device_type)
}

/// Resolve a logical player independently of its address. All-in-one units
/// expose several player numbers from the same IPv4 address.
fn player_address(players: &HashMap<u8, Player>, number: u8) -> Option<Ipv4Addr> {
    players.get(&number).map(|player| player.address)
}

/// The tempo a player is playing at, ×100: its track's BPM at its pitch,
/// or 0 with nothing loaded.
fn tempo_x100(track_bpm: Option<f64>, pitch_percent: f64) -> u32 {
    let Some(bpm) = track_bpm else { return 0 };
    let x100 = (bpm * 100.0 * (1.0 + pitch_percent / 100.0)).round();
    if x100.is_finite() && x100 > 0.0 && x100 < f64::from(u32::MAX) {
        // In range and rounded: the cast is exact.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            x100 as u32
        }
    } else {
        0
    }
}

fn send(socket: &UdpSocket, packet: &[u8], to: Ipv4Addr, port: u16, what: &str) {
    match socket.send_to(packet, SocketAddr::V4(SocketAddrV4::new(to, port))) {
        Ok(_) => tracing::trace!(what, %to, port, len = packet.len(), "sent"),
        Err(error) => tracing::warn!(%error, %to, what, "not sent"),
    }
}

fn is_timeout(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod all_in_one_tests {
    use super::*;

    fn player(number: u8, address: Ipv4Addr, kind: DeviceType) -> Player {
        Player {
            number,
            name: format!("device {number}"),
            address,
            kind,
            loaded: None,
            playing: false,
            master: false,
            sync: false,
            cued: false,
            bpm_x100: 0,
            last_seen: Instant::now(),
        }
    }

    #[test]
    fn logical_decks_at_one_address_remain_separate_load_destinations() {
        let address = Ipv4Addr::new(169, 254, 20, 2);
        let players = HashMap::from([
            (1, player(1, address, DeviceType::Cdj)),
            (2, player(2, address, DeviceType::Cdj)),
            (33, player(33, address, DeviceType::Mixer)),
        ]);

        assert_eq!(player_address(&players, 1), Some(address));
        assert_eq!(player_address(&players, 2), Some(address));
        assert_eq!(player_address(&players, 33), Some(address));
        assert_eq!(player_address(&players, 3), None);
    }

    #[test]
    fn an_all_in_one_can_be_greeted_again_after_every_identity_times_out() {
        let address = Ipv4Addr::new(169, 254, 20, 2);
        let mut shared = Shared {
            greeted: vec![address],
            players: HashMap::from([
                (1, player(1, address, DeviceType::Cdj)),
                (2, player(2, address, DeviceType::Cdj)),
            ]),
            ..Shared::default()
        };
        shared.players.get_mut(&1).unwrap().last_seen =
            Instant::now() - PLAYER_TIMEOUT - Duration::from_millis(1);

        shared.expire_silent_players();

        assert!(shared.greeted.contains(&address));
        assert_eq!(shared.players.len(), 1);

        shared.players.get_mut(&2).unwrap().last_seen =
            Instant::now() - PLAYER_TIMEOUT - Duration::from_millis(1);
        shared.expire_silent_players();

        assert!(shared.players.is_empty());
        assert!(!shared.greeted.contains(&address));
    }

    #[test]
    fn goodbye_forgets_every_identity_and_the_greeting_at_an_address() {
        let address = Ipv4Addr::new(169, 254, 20, 2);
        let mut shared = Shared {
            greeted: vec![address],
            to_greet: vec![address],
            players: HashMap::from([
                (1, player(1, address, DeviceType::Cdj)),
                (2, player(2, address, DeviceType::Cdj)),
            ]),
            ..Shared::default()
        };

        shared.remove_players_at(address);

        assert!(shared.players.is_empty());
        assert!(!shared.greeted.contains(&address));
        assert!(!shared.to_greet.contains(&address));
    }

    #[test]
    fn xdj_az_usb_two_slot_survives_status_parsing() {
        let mut packet = vec![0_u8; 0xcd];
        packet[..rbl_prolink::MAGIC.len()].copy_from_slice(&rbl_prolink::MAGIC);
        packet[0x29] = 0x07;

        let status = status_from_packet(&packet).unwrap().unwrap();
        assert_eq!(status.track_slot, MediaSlot::Unknown07);
    }
}
