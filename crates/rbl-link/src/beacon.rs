//! Our presence on the link: the packets that put `rekordbox` on a player's
//! source list, and the packets from the players that tell us who is there
//! and what they have loaded.
//!
//! Two sockets, two threads. The announce socket (UDP 50000) broadcasts our
//! keep-alive every 2.0 s and hears everyone else's. The status socket (UDP
//! 50002) broadcasts the mixer-style status every 200 ms, answers a player's
//! media query and its device-settings request, greets a player the first time it
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
    announce_kind_name, connect_greeting, connect_identity, device_settings_response, hex,
    packet_kind, status_kind_name, AnnounceKind, DevicePropertyQuery, DevicePropertyResponse,
    DeviceTable, DeviceType, KeepAlive, MediaQuery, MediaResponse, NumberProbe, NumberReply,
    Status, DEVICE_IDENTITY_QUERY_KIND, DEVICE_PROPERTY_QUERY_KIND, DEVICE_SETTINGS_REQUEST_KIND,
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
    /// The selected interface's actual mask, not inferred from broadcast.
    pub netmask: Ipv4Addr,
    pub broadcast: Ipv4Addr,
    pub mac: [u8; 6],
    /// Selected-interface mode; never inferred from a peer or number.
    pub mode: rbl_prolink::ConnectionMode,
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
    fn device_settings(&self) -> rbl_prolink::DeviceSettings {
        rbl_prolink::DeviceSettings::default()
    }
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
    /// Shared with the database gate; rejection clears it before returning
    /// any outgoing announcement, not on the next announce-loop tick.
    assigned: Arc<AtomicU8>,
}

impl Shared {
    fn rejected(&self) -> bool { self.join.as_ref().is_some_and(Join::rejected) }

    fn clear_members(&mut self) {
        self.peers = DeviceTable::new();
        self.players.clear();
        self.greeted.clear();
        self.to_greet.clear();
    }

    fn forget_greeting(&mut self, address: Ipv4Addr) {
        self.greeted.retain(|greeted| *greeted != address);
        self.to_greet.retain(|pending| *pending != address);
    }

    fn remove_member(&mut self, number: u8) {
        let peer = self.peers.remove_number(number);
        let player = self.players.remove(&number);
        for address in peer.map(|peer| peer.ip).into_iter().chain(player.map(|player| player.address)) {
            if !self.players.values().any(|player| player.address == address)
                && !self.peers.peers().iter().any(|peer| peer.ip == address)
            {
                self.forget_greeting(address);
            }
        }
    }

    fn expire_peers(&mut self, now_ms: u64) {
        let expired = self.peers.expire_received(now_ms);
        for &number in &expired {
            // V3 timerFuncAging removes only these evidenced pairs. The
            // additional OPUS 11/12 identities do not own number 9's timer.
            if number == 9 || number == 11 {
                self.remove_member(number);
                self.remove_member(number + 1);
            }
        }
        if !expired.is_empty() {
            tracing::debug!(expired = expired.len(), "peers timed out of the keep-alive table");
        }
    }

    fn observe_player(&mut self, keep_alive: &KeepAlive, sender: Ipv4Addr) {
        self.players.entry(keep_alive.device_number).or_insert_with(|| Player {
            number: keep_alive.device_number,
            name: keep_alive.name.clone(),
            address: sender,
            kind: keep_alive.device_type,
            loaded: None,
            playing: false,
            master: false,
            sync: false,
            cued: false,
            bpm_x100: 0,
            last_seen: Instant::now(),
        });
        if let Some(player) = self.players.get_mut(&keep_alive.device_number) {
            player.last_seen = Instant::now();
            player.name.clone_from(&keep_alive.name);
            // Only the established matching payload/sender case is changed.
            // A disagreement still needs vendor evidence before a policy.
            if keep_alive.ip == sender {
                let old_address = player.address;
                player.address = keep_alive.ip;
                player.kind = keep_alive.device_type;
                if old_address != keep_alive.ip
                    && !self.players.values().any(|other| other.address == old_address)
                    && !self.peers.peers().iter().any(|peer| peer.ip == old_address)
                {
                    self.forget_greeting(old_address);
                }
            }
        }
    }

    fn synthesize_members(&mut self, primary: &KeepAlive, sender: Ipv4Addr, now_ms: u64) {
        // V1 readConfigNotify adds these identities only in running states.
        if !matches!(self.join.as_ref().map(Join::state), Some(join::State::Running { .. }))
            || !(1..=9).contains(&primary.device_type.to_u8())
        {
            return;
        }
        let numbers: &[u8] = match (primary.device_number, primary.name.as_str()) {
            (9, "OPUS-QUAD") => &[10, 11, 12],
            (9, _) => &[10],
            (11, _) => &[12],
            _ => &[],
        };
        let flags = self.peers.peers().iter().find(|peer| peer.device_number == primary.device_number)
            .and_then(|peer| peer.member_flags);
        for &number in numbers {
            let mut logical = primary.clone();
            logical.device_number = number;
            self.peers.observe_synthetic(&logical, now_ms, flags);
            self.observe_player(&logical, sender);
        }
    }

    fn rediscover(&mut self, discovery: &rbl_prolink::Discovery) {
        let leaving: Vec<_> = self.peers.peers().iter().filter(|peer| {
            let in_range = match discovery.device_type {
                1 => (1..=4).contains(&peer.device_number),
                2 | 3 => peer.device_number == 33,
                4 => [17, 18, 41, 42, 43, 44].contains(&peer.device_number),
                6 => (41..=44).contains(&peer.device_number),
                7 => (9..=12).contains(&peer.device_number),
                _ => false,
            };
            in_range && peer.mac == discovery.mac
        }).map(|peer| peer.device_number).collect();
        for number in leaving {
            self.remove_member(number);
            if discovery.device_type == 7 && (number == 9 || number == 11) {
                self.remove_member(number + 1);
            }
        }
    }

    fn expire_silent_players(&mut self) {
        let mut expired_addresses = Vec::new();
        let peers = &self.peers;
        self.players.retain(|number, player| {
            // A synthetic-only member has no independent vendor timer. Its
            // removal follows numbered/rediscovery/paired-primary teardown.
            let alive = peers.is_synthetic(*number) || player.last_seen.elapsed() < PLAYER_TIMEOUT;
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
        let initial = Shared::default();
        let number = Arc::clone(&initial.assigned);
        let shared = Arc::new(Mutex::new(initial));
        let mut threads = Vec::with_capacity(3);
        {
            let (stop, shared, config, number) = (
                Arc::clone(&stop),
                Arc::clone(&shared),
                config.clone(),
                Arc::clone(&number),
            );
            threads.push(std::thread::spawn(move || {
                announce_loop(&announce, &config, &stop, &shared, &number);
            }));
        }
        {
            let (stop, shared, number) =
                (Arc::clone(&stop), Arc::clone(&shared), Arc::clone(&number));
            let config = config.clone();
            threads.push(std::thread::spawn(move || {
                status_loop(&status, &config, &stop, &shared, &facts, &number);
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
                beat_clock(&beats, &config, &stop, &shared, &number);
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
    shared.lock().join = Some(Join::new(config.mac, config.address, started).with_mode(config.mode));

    let mut next_keep_alive: Option<Instant> = None;
    let mut next_monitor = Instant::now() + NETWORK_MONITOR_EVERY;
    while !stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        // The join's packets when due, and the number the moment it settles.
        let outgoing = {
            let mut shared = shared.lock();
            // Membership ageing must run even when no new packet arrives.
            // Keep the current timeout separate from status/player activity.
            shared.expire_peers(now_ms(started));
            let outgoing = shared.join.as_mut().and_then(|join| join.tick(now));
            let settled = shared.join.as_ref().and_then(Join::number).unwrap_or(0);
            if settled != number.load(Ordering::Relaxed) {
                number.store(settled, Ordering::Relaxed);
                next_keep_alive = (settled != 0).then_some(now);
            }
            // Rejection already cleared the shared number synchronously.
            // Do not leave the old keep-alive schedule armed just because
            // the next tick observes two equal zero values.
            if settled == 0 { next_keep_alive = None; }
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
    // V5 messageReceived rejects own/off-subnet senders using cached NetIF
    // before dispatch, not just in the rejection handler. Fresh/Unknown
    // sessions retain their uninitialized cache boundary.
    if !shared.lock().join.as_ref().map_or(*from.ip() != Ipv4Addr::UNSPECIFIED, |join|
        join.announcement_sender_allowed(*from.ip(), config.netmask)) {
        return None;
    }
    if kind != Some(9) && shared.lock().rejected() { return None; }
    // V1 frameRead checks runtime mode, initially 0xff. Known wireless
    // becomes active only on the first LinkUp attempt; Unknown keeps RBX's
    // conservative existing exclusion without pretending it is vendor 0xff.
    let original_model = packet.get(0x21) == Some(&0)
        && rbl_prolink::device_name(packet).is_ok_and(|name| name == "CDJ-2000" || name == "CDJ-900");
    let model_gate = match config.mode {
        rbl_prolink::ConnectionMode::Wired | rbl_prolink::ConnectionMode::Wireless =>
            shared.lock().join.as_ref().is_some_and(Join::excludes_original_models),
        rbl_prolink::ConnectionMode::Unknown => true,
    };
    if model_gate && original_model {
        return None;
    }
    match kind.map(AnnounceKind::from_u8) {
        Some(AnnounceKind::ClaimStage1) => {
            let discovery = rbl_prolink::Discovery::decode(packet).ok()?;
            shared.lock().rediscover(&discovery);
            return None;
        }
        Some(AnnounceKind::ClaimStage2) => {
            if packet.get(11) == Some(&rbl_prolink::PROBE_SUBTYPE_BLOCK) {
                let block = rbl_prolink::NumberBlock::decode(packet).ok()?;
                return shared.lock().join.as_mut().and_then(|join| join.hear_block(&block));
            }
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
        Some(AnnounceKind::Other(0x09)) => {
            // RBX safe common-header bounds policy; V1 dispatches 09 without
            // a target/subtype or running-only condition. Declared length is
            // not a source-established receive policy.
            if packet.len() < 36 || config.mode == rbl_prolink::ConnectionMode::Unknown {
                return None;
            }
            let mut shared = shared.lock();
            let join = shared.join.get_or_insert_with(||
                Join::new(config.mac, config.address, Instant::now()).with_mode(config.mode));
            let outgoing = join.reject(Instant::now());
            shared.assigned.store(0, Ordering::Relaxed);
            shared.clear_members();
            shared.master = MasterState { on: false, bpm_x100: 0, bar_beat: 0 };
            shared.down = Some("announcement rejected; restart Link to recover".into());
            return outgoing;
        }
        // rekordbox drops every member, stops its timers and starts over
        // on a compatibility response (`readCompatiRes`).
        Some(AnnounceKind::Other(0x0b)) => {
            let mut shared = shared.lock();
            // V2 readCompatiRes returns immediately in the idle state.
            if shared.join.as_ref().is_none_or(|join| *join.state() == join::State::Waiting) {
                return None;
            }
            tracing::warn!(%from, "compatibility response; leaving the link and starting over");
            shared.clear_members();
            if let Some(join) = shared.join.as_mut() {
                join.reset(Instant::now());
            }
            return None;
        }
        // A disconnect names the device leaving.
        Some(AnnounceKind::Other(0x07) | AnnounceKind::Conflict) => {
            let number = *packet.get(0x24)?;
            if !(1..=80).contains(&number) {
                return None;
            }
            let mut shared = shared.lock();
            if !matches!(shared.join.as_ref().map(Join::state), Some(
                join::State::Discovery { .. } | join::State::Probing { .. }
                | join::State::Assigning { .. } | join::State::Running { .. }
            )) || !shared.peers.peers().iter().any(|peer| peer.device_number == number) {
                return None;
            }
            shared.remove_member(number);
            if number == 9 || number == 11 {
                shared.remove_member(number + 1);
            }
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
        // The first wireless original-player attempt initializes runtime
        // identity above but fails linkUpFunc's model gate. Do not admit it
        // to membership or greet it while still Waiting.
        if config.mode == rbl_prolink::ConnectionMode::Wireless && original_model {
            return None;
        }
        if shared.join.as_ref().is_some_and(|join| !join.allows_running_member(keep_alive.device_number)) {
            return None;
        }
        let known = shared
            .peers
            .peers()
            .iter()
            .any(|p| p.device_number == keep_alive.device_number);
        // KeepAlive::decode has validated both offsets. Preserve these opaque
        // member flags through synthesis without guessing their meanings.
        shared.peers.observe_with_flags(&keep_alive, now, [packet[0x25], packet[0x35]]);
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
        shared.observe_player(&keep_alive, *from.ip());
        shared.synthesize_members(&keep_alive, *from.ip(), now);
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
                    tracing::trace!(len = packet.len(), bytes = %hex(&packet, TRACE_BYTES), "status sent");
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
        // A receive begun before rejection must not revive membership or
        // answer from the stale pre-rejection assigned-number snapshot.
        if shared.lock().rejected() { continue; }
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
                answer_device_property_query(socket, packet, from, config, ours);
            }
            DEVICE_SETTINGS_REQUEST_KIND => {
                tracing::debug!(player = %from.ip(), "device settings requested; answering");
                let reply = device_settings_response(REKORDBOX_NAME, ours, facts.device_settings());
                send(
                    socket,
                    &reply,
                    *from.ip(),
                    config.player_port,
                    "device settings response",
                );
            }
            // Preserve raw response fields without assigning unknown enum
            // semantics. Only later status can establish the loaded track.
            LOAD_TRACK_ACK_KIND => {
                if let Ok(response) = rbl_prolink::LoadTrackResponse::decode(packet) {
                    tracing::info!(from = %from.ip(), name = %response.name, fields = ?response.fields,
                        "player replied to a load track command");
                } else {
                    tracing::debug!(%from, len = packet.len(), "malformed load track response ignored");
                }
            }
            PLAYER_STATUS_KIND => {
                hear_player_status(packet, from, socket, config, shared, facts, ours);
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
    if shared.rejected() { return; }
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
        let mut state = shared.lock();
        if !state.rejected() {
            state.master.bar_beat = if bar_beat >= 4 { 1 } else { bar_beat + 1 };
        }
        drop(state);
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
        shared.players.get_mut(&1).unwrap().last_seen = Instant::now()
            .checked_sub(PLAYER_TIMEOUT + Duration::from_millis(1))
            .unwrap();

        shared.expire_silent_players();

        assert!(shared.greeted.contains(&address));
        assert_eq!(shared.players.len(), 1);

        shared.players.get_mut(&2).unwrap().last_seen = Instant::now()
            .checked_sub(PLAYER_TIMEOUT + Duration::from_millis(1))
            .unwrap();
        shared.expire_silent_players();

        assert!(shared.players.is_empty());
        assert!(!shared.greeted.contains(&address));
    }

    #[test]
    fn goodbye_preserves_the_other_logical_identity_and_shared_greeting() {
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

        shared.remove_member(1);

        assert_eq!(shared.players.len(), 1);
        assert!(shared.players.contains_key(&2));
        assert!(shared.greeted.contains(&address));
        assert!(shared.to_greet.contains(&address));
        shared.remove_member(2);

        assert!(shared.players.is_empty());
        assert!(!shared.greeted.contains(&address));
        assert!(!shared.to_greet.contains(&address));
    }

    fn config() -> BeaconConfig {
        BeaconConfig {
            interface: None,
            address: Ipv4Addr::LOCALHOST,
            netmask: Ipv4Addr::new(255, 0, 0, 0),
            broadcast: Ipv4Addr::LOCALHOST,
            mac: [0; 6],
            mode: rbl_prolink::ConnectionMode::Wired,
            announce_port: 50000,
            status_port: 50002,
            player_port: 50002,
            beat_port: 50001,
            computer_name: "fixture".into(),
        }
    }

    fn keep_alive(number: u8, ip: Ipv4Addr) -> KeepAlive {
        let mut packet = KeepAlive::rekordbox_as(number, [1, 2, 3, 4, 5, 6], ip, 0);
        packet.name = "CDJ-3000".into();
        packet.device_type = DeviceType::Cdj;
        packet
    }

    fn members(numbers: &[u8], now: Instant) -> Shared {
        let address = Ipv4Addr::new(169, 254, 20, 2);
        let mut shared = Shared {
            greeted: vec![address],
            to_greet: vec![address],
            join: Some(Join::new([0; 6], Ipv4Addr::LOCALHOST, now)),
            ..Shared::default()
        };
        for &number in numbers {
            let packet = keep_alive(number, address);
            shared.peers.observe(&packet, 0);
            shared.players.insert(number, player(number, address, DeviceType::Cdj));
            shared.join.as_mut().unwrap().hear_keep_alive(&packet, now);
        }
        shared
    }

    #[test]
    fn numbered_disconnect_covers_both_kinds_pairs_sender_and_state_guards() {
        let now = Instant::now();
        for kind in [7, 8] {
            for (leaving, remaining) in [(1, vec![2, 9, 10, 11, 12]), (9, vec![1, 2, 11, 12]),
                (10, vec![1, 2, 9, 11, 12]), (11, vec![1, 2, 9, 10]), (12, vec![1, 2, 9, 10, 11])] {
                let shared = Mutex::new(members(&[1, 2, 9, 10, 11, 12], now));
                // Complete synthetic 41-byte vendor-shaped disconnect. Its
                // sender is intentionally unrelated to the numbered member.
                let mut wire = keep_alive(leaving, Ipv4Addr::new(169, 254, 20, 2)).encode();
                wire[10] = kind;
                wire[34..36].copy_from_slice(&41_u16.to_be_bytes());
                wire.truncate(41);
                let sender = SocketAddr::from((Ipv4Addr::new(10, 1, 2, 3), 50000));
                for cut in 0..37 {
                    assert!(hear_announce(&wire[..cut], sender, &config(), &shared, now).is_none());
                    assert_eq!(shared.lock().peers.len(), 6);
                }
                assert!(hear_announce(&wire, sender, &config(), &shared, now).is_none());
                let state = shared.lock();
                let mut actual: Vec<_> = state.players.keys().copied().collect();
                actual.sort_unstable();
                assert_eq!(actual, remaining);
                assert_eq!(state.peers.peers().iter().map(|p| p.device_number).collect::<Vec<_>>(), remaining);
                assert!(state.greeted.contains(&Ipv4Addr::new(169, 254, 20, 2)));
            }
        }
        let shared = Mutex::new(members(&[9, 10], now));
        let sender = SocketAddr::from((Ipv4Addr::new(10, 1, 2, 3), 50000));
        let mut wire = keep_alive(9, Ipv4Addr::new(169, 254, 20, 2)).encode();
        wire[10] = 7;
        for number in [0, 81, 255, 1] {
            wire[36] = number;
            hear_announce(&wire, sender, &config(), &shared, now);
            assert_eq!(shared.lock().peers.len(), 2);
        }
        wire[36] = 9;
        shared.lock().join.as_mut().unwrap().reset(now);
        hear_announce(&wire, sender, &config(), &shared, now);
        assert_eq!(shared.lock().peers.len(), 2);
        shared.lock().join.as_mut().unwrap().hear_keep_alive(&keep_alive(9, Ipv4Addr::new(169, 254, 20, 2)), now);
        hear_announce(&wire, sender, &config(), &shared, now);
        let state = shared.lock();
        assert!(state.players.is_empty());
        assert!(state.peers.is_empty());
        assert_eq!(state.greeted, Vec::<Ipv4Addr>::new());
        assert_eq!(state.to_greet, Vec::<Ipv4Addr>::new());
    }

    #[test]
    fn matching_sender_address_change_updates_command_destination_and_peer_mac() {
        let now = Instant::now();
        let old = Ipv4Addr::new(169, 254, 20, 2);
        let new = Ipv4Addr::new(169, 254, 20, 3);
        let shared = Mutex::new(members(&[1, 2], now));
        let mut announcement = keep_alive(1, new);
        announcement.mac = [6, 5, 4, 3, 2, 1];
        let wire = announcement.encode();
        for _ in 0..2 {
            hear_announce(&wire, SocketAddr::from((new, 50000)), &config(), &shared, now);
            let state = shared.lock();
            assert_eq!(player_address(&state.players, 1), Some(new));
            assert_eq!(player_address(&state.players, 2), Some(old));
            let peer = state.peers.peers().iter().find(|p| p.device_number == 1).unwrap();
            assert_eq!(peer.ip, new);
            assert_eq!(peer.mac, announcement.mac);
            assert!(state.greeted.contains(&old));
        }
        // The old greeting is forgotten only when its final member moves.
        announcement.device_number = 2;
        hear_announce(&announcement.encode(), SocketAddr::from((new, 50000)), &config(), &shared, now);
        assert!(!shared.lock().greeted.contains(&old));
    }

    #[test]
    fn periodic_peer_expiry_needs_no_input_and_keeps_player_activity_separate() {
        let now = Instant::now();
        let mut shared = members(&[1, 2], now);
        let ip = Ipv4Addr::new(169, 254, 20, 2);
        shared.peers.observe(&keep_alive(2, ip), 1000);
        shared.expire_peers(rbl_prolink::PEER_TIMEOUT_MS);
        assert_eq!(shared.peers.len(), 2);
        shared.expire_peers(rbl_prolink::PEER_TIMEOUT_MS + 1);
        assert_eq!(shared.peers.peers()[0].device_number, 2);
        // Status/player timestamps do not extend the independent peer clock.
        shared.players.get_mut(&2).unwrap().last_seen = Instant::now();
        shared.expire_peers(rbl_prolink::PEER_TIMEOUT_MS + 1001);
        assert!(shared.peers.is_empty());
        assert_eq!(shared.players.len(), 2);
        assert_eq!(KeepAlive::decode(&KeepAlive::rekordbox_as(17, [0; 6], Ipv4Addr::LOCALHOST,
            u8::try_from(shared.peers.len()).unwrap()).encode()).unwrap().peers, 0);
    }

    #[test]
    fn idle_compatibility_response_does_not_destroy_members_or_greetings() {
        let now = Instant::now();
        let shared = Mutex::new(members(&[1], now));
        shared.lock().join.as_mut().unwrap().reset(now);
        let mut wire = keep_alive(1, Ipv4Addr::new(169, 254, 20, 2)).encode();
        wire[10] = 0x0b;
        assert!(hear_announce(&wire, SocketAddr::from((Ipv4Addr::new(10, 0, 0, 1), 50000)),
            &config(), &shared, now).is_none());
        let state = shared.lock();
        assert_eq!(state.peers.len(), 1);
        assert_eq!(state.players.len(), 1);
        assert_eq!(state.greeted.len(), 1);
        assert_eq!(state.to_greet.len(), 1);
        assert_eq!(state.join.as_ref().unwrap().state(), &join::State::Waiting);
    }

    #[test]
    fn xdj_az_usb_two_slot_survives_status_parsing() {
        let mut packet = vec![0_u8; 0xcd];
        packet[..rbl_prolink::MAGIC.len()].copy_from_slice(&rbl_prolink::MAGIC);
        packet[0x29] = 0x07;

        let status = status_from_packet(&packet).unwrap().unwrap();
        assert_eq!(status.track_slot, MediaSlot::Unknown07);
    }

    #[test]
    fn numbered_disconnect_state_guards_cover_the_whole_join_machine() {
        let now = Instant::now();
        let ip = Ipv4Addr::new(169, 254, 20, 2);
        let mut wire = keep_alive(1, ip).encode();
        wire[10] = 7;
        for (state, accepted) in [
            (join::State::Waiting, false),
            (join::State::Discovery { sent: 0 }, true),
            (join::State::Probing { round: 1, index: 0 }, true),
            (join::State::Assigning { sent: 1 }, true),
            (join::State::Running { number: 17 }, true),
            (join::State::Failed("fixture".into()), false),
        ] {
            let mut state_fixture = members(&[1, 2], now);
            state_fixture.join = Some(Join::new([0; 6], Ipv4Addr::LOCALHOST, now).test_state(state));
            let shared = Mutex::new(state_fixture);
            assert!(hear_announce(&wire, SocketAddr::from((ip, 50000)), &config(), &shared, now).is_none());
            assert_eq!(shared.lock().players.contains_key(&1), !accepted);
            assert_eq!(shared.lock().players.len(), if accepted { 1 } else { 2 });
        }
        let mut state_fixture = members(&[1], now);
        state_fixture.join = None;
        let shared = Mutex::new(state_fixture);
        hear_announce(&wire, SocketAddr::from((ip, 50000)), &config(), &shared, now);
        assert_eq!(shared.lock().players.len(), 1);
    }

    #[test]
    fn changed_address_is_used_by_the_actual_load_command_socket() {
        let now = Instant::now();
        let shared = Arc::new(Mutex::new(members(&[1], now)));
        let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        receiver.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let commands = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let command_source = commands.local_addr().unwrap();
        let beacon = Beacon {
            stop: Arc::new(AtomicBool::new(false)),
            threads: Vec::new(),
            shared: Arc::clone(&shared),
            announce_port: 0,
            status_port: command_source.port(),
            commands,
            player_port: receiver.local_addr().unwrap().port(),
            number: Arc::new(AtomicU8::new(17)),
        };
        let changed = keep_alive(1, Ipv4Addr::LOCALHOST).encode();
        assert!(hear_announce(&changed, SocketAddr::from((Ipv4Addr::LOCALHOST, 50000)),
            &config(), &shared, now).is_none());
        beacon.load_track(1, 0x1234).unwrap();
        let mut bytes = [0; 256];
        let (length, source) = receiver.recv_from(&mut bytes).unwrap();
        assert_eq!(source, command_source);
        assert_eq!(bytes[..length], rbl_prolink::load_track_command(REKORDBOX_NAME, 17, 1, 0x1234));
    }

    #[test]
    fn wireless_original_model_gate_applies_before_announcement_dispatch() {
        let now = Instant::now();
        // Keep the sender on the selected loopback /8 so this exercises
        // the model gate, not V5's earlier cached-NetIF subnet guard.
        let ip = Ipv4Addr::new(127, 0, 0, 2);
        for mode in [rbl_prolink::ConnectionMode::Wired, rbl_prolink::ConnectionMode::Wireless,
            rbl_prolink::ConnectionMode::Unknown] {
            for name in ["CDJ-2000", "CDJ-900"] {
                let shared = Mutex::new(members(&[1], now));
                let mut configuration = config();
                configuration.mode = mode;
                let mut initialized = Join::new(configuration.mac, configuration.address, now).with_mode(mode);
                initialized.hear_keep_alive(&keep_alive(1, ip), now);
                shared.lock().join = Some(initialized);
                let mut wire = rbl_prolink::rekordbox_claim_stage1([1,2,3,4,5,6], 1);
                wire[12..32].fill(0);
                wire[12..12 + name.len()].copy_from_slice(name.as_bytes());
                wire[33] = 0;
                wire[37] = 1;
                hear_announce(&wire, SocketAddr::from((ip, 50000)), &configuration, &shared, now);
                assert_eq!(shared.lock().peers.is_empty(), mode == rbl_prolink::ConnectionMode::Wired);
            }
        }
    }

    #[test]
    fn rediscovery_removes_only_the_matching_mac_in_its_evidenced_type_range() {
        let now = Instant::now();
        let ip = Ipv4Addr::new(169, 254, 20, 2);
        let numbers = [1, 2, 3, 4, 5, 6, 9, 10, 11, 12, 17, 18, 33, 41, 42, 43, 44];
        for (kind, removed) in [(1, vec![1,2,3,4]), (2,vec![33]), (3,vec![33]),
            (4,vec![17,18,41,42,43,44]), (6,vec![41,42,43,44]), (7,vec![9,10,11,12]), (5,vec![])] {
            let shared = Mutex::new(members(&numbers, now));
            let mut wire = rbl_prolink::rekordbox_claim_stage1([1,2,3,4,5,6], 1);
            wire[37] = kind;
            for cut in 0..44 {
                hear_announce(&wire[..cut], SocketAddr::from((ip, 50000)), &config(), &shared, now);
                assert_eq!(shared.lock().peers.len(), numbers.len());
            }
            let before = shared.lock().join.as_ref().unwrap().state().clone();
            assert!(hear_announce(&wire, SocketAddr::from((ip, 50000)), &config(), &shared, now).is_none());
            let state = shared.lock();
            let remaining: Vec<_> = numbers.into_iter().filter(|number| !removed.contains(number)).collect();
            assert_eq!(state.peers.peers().iter().map(|p| p.device_number).collect::<Vec<_>>(), remaining);
            assert_eq!(state.players.len(), remaining.len());
            assert_eq!(state.join.as_ref().unwrap().state(), &before);
            assert!(state.greeted.contains(&ip), "out-of-range shared-IP survivors retain greeting");
            drop(state);
            // Fresh registration succeeds; an unrelated MAC cannot remove it.
            let number = removed.first().copied().unwrap_or(1);
            hear_announce(&keep_alive(number, ip).encode(), SocketAddr::from((ip, 50000)), &config(), &shared, now);
            wire[38] ^= 0xff;
            hear_announce(&wire, SocketAddr::from((ip, 50000)), &config(), &shared, now);
            assert!(shared.lock().players.contains_key(&number));
            wire[11] = 1;
            wire[38] ^= 0xff;
            hear_announce(&wire, SocketAddr::from((ip, 50000)), &config(), &shared, now);
            assert!(shared.lock().players.contains_key(&number));
        }
    }

    #[test]
    fn running_multideck_keepalives_add_only_the_established_identities_and_flags() {
        let now = Instant::now();
        let ip = Ipv4Addr::new(169, 254, 20, 2);
        for (number, name, expected) in [(9, "XDJ-RX3", vec![9, 10]),
            (11, "XDJ-RX3", vec![11, 12]), (9, "OPUS-QUAD", vec![9, 10, 11, 12]),
            (9, "OPUS-QUAD-X", vec![9, 10]), (11, "OPUS-QUAD", vec![11, 12])] {
            let mut fixture = members(&[1, 2], now);
            fixture.join = Some(Join::new([0; 6], Ipv4Addr::LOCALHOST, now)
                .test_state(join::State::Running { number: 17 }));
            let shared = Mutex::new(fixture);
            let mut primary = keep_alive(number, ip);
            primary.name = name.into();
            primary.device_type = DeviceType::AllInOne;
            let mut wire = primary.encode();
            wire[0x25] = 0xa5;
            wire[0x35] = 0x5a;
            for _ in 0..2 {
                assert!(hear_announce(&wire, SocketAddr::from((ip, 50000)), &config(), &shared, now).is_none());
            }
            let state = shared.lock();
            let mut all = vec![1, 2];
            all.extend_from_slice(&expected);
            assert_eq!(state.peers.peers().iter().map(|peer| peer.device_number).collect::<Vec<_>>(), all);
            assert_eq!(state.players.len(), all.len());
            assert_eq!(state.join.as_ref().unwrap().state(), &join::State::Running { number: 17 });
            assert_eq!(KeepAlive::decode(&KeepAlive::rekordbox_as(17, [0; 6], Ipv4Addr::LOCALHOST,
                u8::try_from(state.peers.len()).unwrap()).encode()).unwrap().peers, u8::try_from(all.len()).unwrap());
            for number in expected {
                let peer = state.peers.peers().iter().find(|peer| peer.device_number == number).unwrap();
                assert_eq!((&peer.name, peer.ip, peer.mac, peer.device_type, peer.member_flags),
                    (&primary.name, ip, primary.mac, primary.device_type, Some([0xa5, 0x5a])));
                assert_eq!(state.players[&number].address, ip);
                assert_eq!(state.peers.is_synthetic(number), number != primary.device_number);
            }
            assert!(state.greeted.contains(&ip), "the unrelated same-IP members retain ownership");
        }
    }

    #[test]
    fn multideck_synthesis_is_inert_before_running_and_for_malformed_keepalives() {
        let now = Instant::now();
        let ip = Ipv4Addr::new(169, 254, 20, 2);
        let mut primary = keep_alive(9, ip);
        primary.name = "OPUS-QUAD".into();
        let wire = primary.encode();
        for state in [join::State::Waiting, join::State::Discovery { sent: 1 },
            join::State::Probing { round: 1, index: 0 }, join::State::Assigning { sent: 1 },
            join::State::Failed("fixture".into())] {
            let mut fixture = members(&[], now);
            fixture.join = Some(Join::new([0; 6], Ipv4Addr::LOCALHOST, now).test_state(state));
            let shared = Mutex::new(fixture);
            hear_announce(&wire, SocketAddr::from((ip, 50000)), &config(), &shared, now);
            assert_eq!(shared.lock().peers.peers().iter().map(|peer| peer.device_number).collect::<Vec<_>>(), vec![9]);
        }
        let mut fixture = members(&[], now);
        fixture.join = Some(Join::new([0; 6], Ipv4Addr::LOCALHOST, now)
            .test_state(join::State::Running { number: 17 }));
        let shared = Mutex::new(fixture);
        for cut in 0..wire.len() {
            hear_announce(&wire[..cut], SocketAddr::from((ip, 50000)), &config(), &shared, now);
            assert!(shared.lock().peers.is_empty());
        }
        let mut invalid = wire;
        invalid[11] = 1;
        hear_announce(&invalid, SocketAddr::from((ip, 50000)), &config(), &shared, now);
        assert!(shared.lock().peers.is_empty());
    }

    #[test]
    fn multideck_expiry_uses_received_timers_and_only_the_proven_pairs() {
        let now = Instant::now();
        let ip = Ipv4Addr::new(169, 254, 20, 2);
        let mut state = members(&[], now);
        state.join = Some(Join::new([0; 6], Ipv4Addr::LOCALHOST, now)
            .test_state(join::State::Running { number: 17 }));
        let mut primary = keep_alive(9, ip);
        primary.name = "OPUS-QUAD".into();
        state.peers.observe_with_flags(&primary, 0, [0xa5, 0x5a]);
        state.observe_player(&primary, ip);
        state.synthesize_members(&primary, ip, 0);
        for number in [10, 11, 12] {
            state.players.get_mut(&number).unwrap().last_seen = now.checked_sub(PLAYER_TIMEOUT + Duration::from_secs(1)).unwrap();
        }
        state.expire_silent_players();
        assert_eq!(state.players.len(), 4, "synthetic members have no independent activity expiry");
        state.expire_peers(rbl_prolink::PEER_TIMEOUT_MS);
        assert_eq!(state.peers.len(), 4);
        state.expire_peers(rbl_prolink::PEER_TIMEOUT_MS + 1);
        assert_eq!(state.peers.peers().iter().map(|peer| peer.device_number).collect::<Vec<_>>(), vec![11, 12]);
        assert_eq!(state.players.len(), 2);
        assert!(state.greeted.contains(&ip), "remaining OPUS identities retain shared resources");
        state.expire_peers(u64::MAX);
        assert_eq!(state.peers.len(), 2, "no invented all-four OPUS ageing rule");
        primary.device_number = 11;
        state.peers.observe_with_flags(&primary, 10_000, [1, 2]);
        state.observe_player(&primary, ip);
        state.synthesize_members(&primary, ip, 10_000);
        state.expire_peers(16_001);
        assert!(state.peers.is_empty());
        assert!(state.players.is_empty());
        assert_eq!(state.greeted, Vec::<Ipv4Addr>::new());
        assert_eq!(state.to_greet, Vec::<Ipv4Addr>::new());
    }

    #[test]
    fn multideck_identity_refresh_preserves_existing_received_timer_and_unrelated_peers() {
        let now = Instant::now();
        let old = Ipv4Addr::new(169, 254, 20, 2);
        let new = Ipv4Addr::new(169, 254, 20, 3);
        let mut state = members(&[1, 11], now);
        state.join = Some(Join::new([0; 6], Ipv4Addr::LOCALHOST, now)
            .test_state(join::State::Running { number: 17 }));
        let mut primary = keep_alive(9, new);
        primary.name = "OPUS-QUAD".into();
        primary.mac = [7; 6];
        state.peers.observe_with_flags(&primary, 5_000, [0xa5, 0x5a]);
        state.observe_player(&primary, new);
        state.synthesize_members(&primary, new, 5_000);
        let eleven = state.peers.peers().iter().find(|peer| peer.device_number == 11).unwrap();
        assert_eq!((eleven.ip, eleven.mac, eleven.last_seen_ms), (new, [7; 6], 0));
        assert_eq!(state.players[&1].address, old);
        assert_eq!(state.players[&11].address, new);
        state.peers.observe(&keep_alive(1, old), 5_000);
        state.expire_peers(6_001);
        assert!(!state.players.contains_key(&11));
        assert!(!state.players.contains_key(&12));
        assert!(state.players.contains_key(&9));
        assert!(state.players.contains_key(&10));
        assert!(state.greeted.contains(&old));
    }

    #[test]
    fn synthesized_opus_members_follow_numbered_rediscovery_and_reset_removal_paths() {
        let now = Instant::now();
        let ip = Ipv4Addr::new(169, 254, 20, 2);
        for removal in [7, 0, 0x0b] {
            let mut state = members(&[], now);
            state.join = Some(Join::new([0; 6], Ipv4Addr::LOCALHOST, now)
                .test_state(join::State::Running { number: 17 }));
            let shared = Mutex::new(state);
            let mut primary = keep_alive(9, ip);
            primary.name = "OPUS-QUAD".into();
            primary.device_type = DeviceType::AllInOne;
            hear_announce(&primary.encode(), SocketAddr::from((ip, 50000)), &config(), &shared, now);
            assert_eq!(shared.lock().peers.len(), 4);
            let mut packet = if removal == 0 {
                let mut discovery = rbl_prolink::rekordbox_claim_stage1(primary.mac, 1);
                discovery[37] = 7;
                discovery
            } else {
                let mut packet = primary.encode();
                packet[10] = removal;
                packet
            };
            assert!(hear_announce(&packet, SocketAddr::from((ip, 50000)), &config(), &shared, now).is_none());
            if removal == 7 {
                let state = shared.lock();
                assert_eq!(state.peers.peers().iter().map(|peer| peer.device_number).collect::<Vec<_>>(), vec![11, 12]);
                assert_eq!(state.players.len(), 2);
                assert!(state.greeted.contains(&ip));
                drop(state);
                packet[36] = 11;
                hear_announce(&packet, SocketAddr::from((ip, 50000)), &config(), &shared, now);
            }
            let state = shared.lock();
            assert!(state.peers.is_empty());
            assert!(state.players.is_empty());
            assert_eq!(state.greeted, Vec::<Ipv4Addr>::new());
            assert_eq!(state.to_greet, Vec::<Ipv4Addr>::new());
        }
    }

    fn rejection_config(mode: rbl_prolink::ConnectionMode) -> BeaconConfig {
        BeaconConfig { address: Ipv4Addr::new(192, 168, 50, 2),
            netmask: Ipv4Addr::new(255, 255, 255, 0),
            broadcast: Ipv4Addr::new(192, 168, 50, 255), mode, ..config() }
    }

    /// Acquire through the actual first keepalive and discovery/probe timers,
    /// rather than injecting Running and losing `LinkUp`'s session history.
    fn acquired(cfg: &BeaconConfig, first: &KeepAlive, now: Instant) -> Mutex<Shared> {
        let shared = Mutex::new(Shared {
            join: Some(Join::new(cfg.mac, cfg.address, now).with_mode(cfg.mode)),
            ..Shared::default()
        });
        hear_announce(&first.encode(), SocketAddr::from((first.ip, 50000)), cfg, &shared, now);
        assert!(matches!(shared.lock().join.as_ref().unwrap().state(), join::State::Discovery { .. }));
        let tick_start = Instant::now();
        for tick in 0..50 {
            shared.lock().join.as_mut().unwrap().tick(tick_start + join::TICK * tick);
            if shared.lock().join.as_ref().unwrap().number().is_some() { break; }
        }
        assert_eq!(shared.lock().join.as_ref().unwrap().number(), Some(17));
        shared
    }

    #[test]
    fn acquired_session_classification_gates_coexistence_before_membership_and_synthesis() {
        let now = Instant::now();
        for mode in [rbl_prolink::ConnectionMode::Wired, rbl_prolink::ConnectionMode::Wireless,
            rbl_prolink::ConnectionMode::Unknown] {
        let cfg = rejection_config(mode);
        for all_in_one in [false, true] {
            let ip = Ipv4Addr::new(192, 168, 50, 10);
            let mut first = keep_alive(if all_in_one { 9 } else { 1 }, ip);
            if all_in_one { first.device_type = DeviceType::AllInOne; first.name = "XDJ-RX3".into(); }
            let shared = acquired(&cfg, &first, now);
            let before = shared.lock().peers.peers().to_vec();
            let greetings = shared.lock().to_greet.clone();
            for number in if all_in_one { 1..=4 } else { 9..=12 } {
                let mut other = keep_alive(number, Ipv4Addr::new(192, 168, 50, 20));
                // Even a later different device type must not reclassify the session.
                if !all_in_one { other.device_type = DeviceType::AllInOne; other.name = "OPUS-QUAD".into(); }
                hear_announce(&other.encode(), SocketAddr::from((other.ip, 50000)), &cfg, &shared, now);
                assert_eq!(shared.lock().peers.peers(), before);
                assert_eq!(shared.lock().players.len(), before.len());
                assert_eq!(shared.lock().to_greet, greetings);
                assert_eq!(shared.lock().join.as_ref().unwrap().number(), Some(17));
            }
            // The original family remains admissible, with the bounded OPUS synthesis.
            let mut accepted = first.clone();
            if all_in_one { accepted.name = "OPUS-QUAD".into(); }
            hear_announce(&accepted.encode(), SocketAddr::from((ip, 50000)), &cfg, &shared, now);
            let expected = if all_in_one { vec![9,10,11,12] } else { vec![1] };
            assert_eq!(shared.lock().peers.peers().iter().map(|p| p.device_number).collect::<Vec<_>>(), expected);
        }
        }
    }

    #[test]
    fn initialized_netif_guards_all_announcement_dispatch_not_member_ownership() {
        let now = Instant::now();
        let cfg = rejection_config(rbl_prolink::ConnectionMode::Wired);
        let first = keep_alive(1, Ipv4Addr::new(192, 168, 50, 10));
        let mut discovery = rbl_prolink::rekordbox_claim_stage1(first.mac, 1);
        discovery[37] = 1;
        let mut disconnect = disconnect_bytes(1, first.ip);
        disconnect[10] = 7;
        let mut conflict = disconnect.clone(); conflict[10] = 8;
        let mut compatibility = first.encode(); compatibility[10] = 0x0b;
        let mut block = rbl_prolink::rekordbox_claim_stage2([9;6], first.ip, 17, 1);
        block[11] = 2; block.resize(68, 0); block[34..36].copy_from_slice(&68_u16.to_be_bytes());
        block[48] = 1; block[67] = 0xa5;
        assert!(rbl_prolink::NumberBlock::decode(&block).unwrap().names(17, cfg.mode));
        let probe = rbl_prolink::rekordbox_claim_stage2([9;6], first.ip, 17, 1);
        let new_member = keep_alive(2, Ipv4Addr::new(192,168,50,20));
        let changed_address = keep_alive(1, new_member.ip);
        let frames = [discovery, disconnect.clone(), conflict, compatibility,
            new_member.encode(), changed_address.encode(), block.clone(), probe.clone(), rejection_frame()];
        for sender in [cfg.address, Ipv4Addr::new(192,168,51,10)] {
            for frame in &frames {
                let shared = acquired(&cfg, &first, now);
                let before = shared.lock().peers.peers().to_vec();
                assert!(hear_announce(frame, SocketAddr::from((sender,50000)), &cfg, &shared, now).is_none());
                assert_eq!(shared.lock().peers.peers(), before, "kind {} sender {sender}", frame[10]);
                assert_eq!(shared.lock().players.len(), 1);
                assert_eq!(shared.lock().join.as_ref().unwrap().number(), Some(17));
                assert!(!shared.lock().rejected());
            }
        }
        // The common guard does not authenticate the disconnect payload's owner.
        let shared = acquired(&cfg, &first, now);
        hear_announce(&disconnect, SocketAddr::from((Ipv4Addr::new(192,168,50,99),50000)), &cfg, &shared, now);
        assert!(shared.lock().peers.is_empty());
        assert!(shared.lock().players.is_empty());
        // Same-subnet occupancy and single-number probes keep their exact
        // response bytes, echoed counters and advertised unicast target.
        for (frame,counter) in [(block,0xa5),(probe,1)] {
            let shared = acquired(&cfg,&first,now);
            let out = hear_announce(&frame,SocketAddr::from((Ipv4Addr::new(192,168,50,99),50000)),&cfg,&shared,now).unwrap();
            assert_eq!(out.packet,rbl_prolink::number_in_use_reply_with_counter(REKORDBOX_NAME,17,counter));
            assert_eq!(out.to,Some(first.ip));
            assert_eq!(shared.lock().peers.len(),1);
        }
        let shared = acquired(&cfg,&first,now);
        hear_announce(&changed_address.encode(),SocketAddr::from((changed_address.ip,50000)),&cfg,&shared,now);
        assert_eq!(shared.lock().peers.peers()[0].ip,changed_address.ip);
        assert_eq!(shared.lock().players[&1].address,changed_address.ip);
    }

    #[test]
    fn netif_guard_preserves_fresh_unknown_and_cached_after_reset_boundaries() {
        let now = Instant::now();
        for mode in [rbl_prolink::ConnectionMode::Wired, rbl_prolink::ConnectionMode::Wireless,
            rbl_prolink::ConnectionMode::Unknown] {
            let cfg = rejection_config(mode);
            let first = keep_alive(1, Ipv4Addr::new(192,168,50,10));
            let disconnect = disconnect_bytes(1, first.ip);
            for sender in [cfg.address, Ipv4Addr::new(10,1,2,3)] {
                let mut fresh = members(&[1], now);
                fresh.join = Some(Join::new(cfg.mac, cfg.address, now).with_mode(mode)
                    .test_state(join::State::Discovery { sent: 0 }));
                let shared = Mutex::new(fresh);
                // No cached NetIF yet: neither own selected IP nor off-subnet is filtered.
                hear_announce(&disconnect, SocketAddr::from((sender,50000)), &cfg, &shared, now);
                assert!(shared.lock().peers.is_empty());
            }
            let shared = acquired(&cfg, &first, now);
            shared.lock().join.as_mut().unwrap().reset(now);
            let other = keep_alive(2, Ipv4Addr::new(10,1,2,3));
            hear_announce(&other.encode(), SocketAddr::from((other.ip,50000)), &cfg, &shared, now);
            if mode == rbl_prolink::ConnectionMode::Unknown {
                assert_eq!(shared.lock().peers.len(), 2);
                assert!(matches!(shared.lock().join.as_ref().unwrap().state(), join::State::Discovery { .. }));
            } else {
                assert_eq!(shared.lock().peers.len(), 1);
                assert_eq!(*shared.lock().join.as_ref().unwrap().state(), join::State::Waiting);
            }
        }
    }

    #[test]
    fn netif_guard_filters_number_replies_and_keepalive_occupancy_during_real_probing() {
        let now = Instant::now();
        let cfg = rejection_config(rbl_prolink::ConnectionMode::Wired);
        let first = keep_alive(1,Ipv4Addr::new(192,168,50,10));
        let reply = rbl_prolink::number_in_use_reply_with_counter("rekordbox",17,0xa5);
        assert_eq!(reply.len(),39);
        let occupied = KeepAlive::rekordbox_as(17,[9;6],Ipv4Addr::new(192,168,50,99),0).encode();
        for frame in [&reply,&occupied] {
        for (sender,number) in [(cfg.address,17),(Ipv4Addr::new(192,168,51,10),17),
            (Ipv4Addr::new(192,168,50,99),18)] {
            let shared = Mutex::new(Shared {
                join: Some(Join::new(cfg.mac,cfg.address,now).with_mode(cfg.mode)), ..Shared::default()
            });
            hear_announce(&first.encode(),SocketAddr::from((first.ip,50000)),&cfg,&shared,now);
            let tick_start = Instant::now();
            for tick in 0..3 { shared.lock().join.as_mut().unwrap().tick(tick_start + join::TICK * tick); }
            assert!(matches!(shared.lock().join.as_ref().unwrap().state(),join::State::Probing { .. }));
            hear_announce(frame,SocketAddr::from((sender,50000)),&cfg,&shared,now);
            for tick in 3..50 { shared.lock().join.as_mut().unwrap().tick(tick_start + join::TICK * tick); }
            assert_eq!(shared.lock().join.as_ref().unwrap().number(),Some(number),"{sender}");
            let admitted = number == 18 && frame[10] == 6;
            assert_eq!(shared.lock().peers.len(),if admitted { 2 } else { 1 });
            assert_eq!(shared.lock().players.contains_key(&17),admitted);
        }
        }
    }

    #[test]
    fn session_classification_allows_eleven_pair_and_reclassifies_only_on_next_successful_linkup() {
        let now = Instant::now();
        for mode in [rbl_prolink::ConnectionMode::Wired,rbl_prolink::ConnectionMode::Wireless,
            rbl_prolink::ConnectionMode::Unknown] {
            let cfg = rejection_config(mode);
            let ip = Ipv4Addr::new(192,168,50,10);
            let mut first = keep_alive(9,ip); first.device_type = DeviceType::AllInOne; first.name = "XDJ-RX3".into();
            let shared = acquired(&cfg,&first,now);
            // The number gate permits 11/12 even if this later peer's type
            // differs from the original type7; it cannot reclassify LinkUp.
            let second = keep_alive(11,Ipv4Addr::new(192,168,50,20));
            let mut wire = second.encode(); wire[0x25] = 0xa5; wire[0x35] = 0x5a;
            hear_announce(&wire,SocketAddr::from((second.ip,50000)),&cfg,&shared,now);
            assert_eq!(shared.lock().peers.peers().iter().map(|p|p.device_number).collect::<Vec<_>>(),vec![9,11,12]);
            for number in [11,12] {
                let state = shared.lock();
                let peer = state.peers.peers().iter().find(|p|p.device_number==number).unwrap();
                assert_eq!(peer.member_flags,Some([0xa5,0x5a]));
                assert_eq!(peer.ip,second.ip);
                assert_eq!(state.players[&number].address,second.ip);
            }
            hear_announce(&keep_alive(1,ip).encode(),SocketAddr::from((ip,50000)),&cfg,&shared,now);
            assert!(!shared.lock().players.contains_key(&1));
            let mut reset = first.encode(); reset[10] = 0x0b;
            hear_announce(&reset,SocketAddr::from((ip,50000)),&cfg,&shared,now);
            assert!(shared.lock().peers.is_empty());
            assert_eq!(*shared.lock().join.as_ref().unwrap().state(),join::State::Waiting);
            hear_announce(&keep_alive(1,ip).encode(),SocketAddr::from((ip,50000)),&cfg,&shared,now);
            for tick in 0..50 { shared.lock().join.as_mut().unwrap().tick(now + join::TICK * tick); }
            assert_eq!(shared.lock().join.as_ref().unwrap().number(),Some(17));
            hear_announce(&first.encode(),SocketAddr::from((ip,50000)),&cfg,&shared,now);
            assert_eq!(shared.lock().peers.peers().iter().map(|p|p.device_number).collect::<Vec<_>>(),vec![1]);
            assert_eq!(shared.lock().players.len(),1);
        }
    }

    #[test]
    fn failed_wireless_original_attempt_does_not_classify_the_later_successful_session() {
        let now = Instant::now();
        let cfg = rejection_config(rbl_prolink::ConnectionMode::Wireless);
        let ip = Ipv4Addr::new(192,168,50,10);
        let mut first = keep_alive(1, ip); first.name = "CDJ-2000".into(); first.generation = 0;
        let shared = Mutex::new(Shared {
            join: Some(Join::new(cfg.mac,cfg.address,now).with_mode(cfg.mode)), ..Shared::default()
        });
        hear_announce(&first.encode(), SocketAddr::from((ip,50000)), &cfg, &shared, now);
        assert_eq!(*shared.lock().join.as_ref().unwrap().state(), join::State::Waiting);
        assert!(shared.lock().peers.is_empty());
        first = keep_alive(9, ip); first.device_type = DeviceType::AllInOne; first.name = "OPUS-QUAD".into();
        hear_announce(&first.encode(), SocketAddr::from((ip,50000)), &cfg, &shared, now);
        for tick in 0..50 { shared.lock().join.as_mut().unwrap().tick(now + join::TICK * tick); }
        assert_eq!(shared.lock().join.as_ref().unwrap().number(),Some(17));
        hear_announce(&keep_alive(1,ip).encode(), SocketAddr::from((ip,50000)), &cfg, &shared, now);
        assert!(!shared.lock().players.contains_key(&1));
        assert_eq!(shared.lock().peers.peers().iter().map(|p|p.device_number).collect::<Vec<_>>(), vec![9]);
    }

    #[test]
    fn slot_deadlines_survive_disconnect_rediscovery_and_paired_removal_before_recreation() {
        // Make wall-derived receipt deliberately nonzero: controlled slot
        // deadlines below must not depend on the acquisition helper's runtime.
        let now = Instant::now().checked_sub(Duration::from_millis(10)).unwrap();
        let cfg = rejection_config(rbl_prolink::ConnectionMode::Wired);
        let ip = Ipv4Addr::new(192,168,50,10);
        let mut primary = keep_alive(9,ip);
        primary.device_type = DeviceType::AllInOne; primary.name = "XDJ-RX3".into();
        let mut secondary = primary.clone(); secondary.device_number = 10;
        for removal in ["disconnect", "rediscovery", "paired_disconnect", "paired_expiry"] {
            let shared = acquired(&cfg, &primary, now);
            let secondary_seen = if removal == "paired_expiry" { 1_000 } else { 0 };
            {
                let mut state = shared.lock();
                state.peers.observe_with_flags(&primary,0,[1,2]);
                state.peers.observe_with_flags(&secondary,secondary_seen,[0xa5,0x5a]);
                state.observe_player(&secondary,ip);
                state.greeted.push(ip);
            }
            match removal {
                "rediscovery" => {
                    let mut wire = rbl_prolink::rekordbox_claim_stage1(primary.mac,1); wire[37] = 7;
                    assert_eq!(rbl_prolink::Discovery::decode(&wire).unwrap().device_type,7);
                    hear_announce(&wire,SocketAddr::from((ip,50000)),&cfg,&shared,now);
                }
                "paired_expiry" => { shared.lock().expire_peers(6_001); }
                _ => {
                    let wire = disconnect_bytes(if removal == "disconnect" { 10 } else { 9 },ip);
                    hear_announce(&wire,SocketAddr::from((ip,50000)),&cfg,&shared,now);
                }
            }
            assert!(!shared.lock().peers.peers().iter().any(|peer|peer.device_number==10),"{removal}");
            assert!(!shared.lock().players.contains_key(&10));
            assert_eq!(shared.lock().greeted.contains(&ip),removal == "disconnect");
            let recreated_at = if removal == "paired_expiry" { 6_500 } else { 1_000 };
            {
                let mut state = shared.lock();
                state.peers.observe_with_flags(&primary,recreated_at,[1,2]);
                state.observe_player(&primary,ip);
                state.synthesize_members(&primary,ip,recreated_at);
                let recreated = state.peers.peers().iter().find(|peer|peer.device_number==10).unwrap();
                assert_eq!(recreated.last_seen_ms,secondary_seen,"{removal}");
                assert_eq!(recreated.member_flags,Some([1,2]));
                assert!(!state.peers.is_synthetic(10));
                let deadline = secondary_seen + rbl_prolink::PEER_TIMEOUT_MS;
                state.expire_peers(deadline);
                assert!(state.peers.peers().iter().any(|peer|peer.device_number==10));
                state.expire_peers(deadline + 1);
                assert!(!state.peers.peers().iter().any(|peer|peer.device_number==10),"{removal}");
                // Retain issue08's distinct ordinary Player activity policy;
                // secondary peer expiry is not a new Player timer policy.
                state.players.get_mut(&10).unwrap().last_seen = now.checked_sub(PLAYER_TIMEOUT + Duration::from_millis(1)).unwrap();
                state.expire_silent_players();
                assert!(!state.players.contains_key(&10));
                assert!(state.players.contains_key(&9));
                assert_eq!(state.greeted.contains(&ip),removal == "disconnect");
            }
        }
    }

    #[test]
    fn inactive_primary_deadline_does_not_remove_a_new_secondary_and_session_clear_stops_all_timers() {
        let now = Instant::now();
        let mut shared = members(&[],now);
        let ip = Ipv4Addr::new(169,254,20,2);
        let primary = keep_alive(9,ip);
        let secondary = keep_alive(10,ip);
        shared.peers.observe(&primary,0);
        shared.remove_member(9);
        shared.peers.observe_synthetic(&secondary,1_000,None);
        shared.observe_player(&secondary,ip);
        shared.expire_peers(6_001);
        assert_eq!(shared.peers.peers().iter().map(|p|p.device_number).collect::<Vec<_>>(),vec![10]);
        assert!(shared.players.contains_key(&10));
        shared.peers.observe(&secondary,10_000);
        shared.remove_member(10); // pending inactive timer is still session state
        shared.clear_members();
        shared.peers.observe_synthetic(&secondary,11_000,None);
        assert!(shared.peers.is_synthetic(10));
        assert_eq!(shared.peers.expire_received(u64::MAX), [] as [u8;0]);
    }

    fn rejection_frame() -> Vec<u8> {
        let mut wire = keep_alive(1, Ipv4Addr::new(192, 168, 50, 10)).encode();
        wire[10] = 9;
        wire[11] = 0x7f; // V1 has no rejection subtype condition.
        wire[34..36].copy_from_slice(&[0xff, 0xff]); // no invented length policy
        wire.truncate(36); // common-header RBX safety boundary, no target
        wire
    }

    fn disconnect_bytes(number: u8, ip: Ipv4Addr) -> Vec<u8> {
        let mut bytes = vec![0x51,0x73,0x70,0x74,0x31,0x57,0x6d,0x4a,0x4f,0x4c,8,0,
            b'r',b'e',b'k',b'o',b'r',b'd',b'b',b'o',b'x',0,0,0,0,0,0,0,0,0,0,0,
            1,3,0,41,number];
        bytes.extend_from_slice(&ip.octets());
        bytes
    }

    struct EmptyCatalog;
    impl rbl_dbserver::catalog::Catalog for EmptyCatalog {
        fn list(&self, _: &rbl_dbserver::catalog::Query) -> Vec<rbl_dbserver::catalog::Row> { Vec::new() }
        fn track_row(&self, _: u32, _: Option<rbl_dbserver::catalog::TrackColumn>) -> Option<rbl_dbserver::item::TrackRow> { None }
        fn track(&self, _: u32) -> Option<rbl_dbserver::catalog::TrackDetails> { None }
        fn artwork(&self, _: u32) -> Option<Vec<u8>> { None }
        fn item_artwork(&self, _: u32) -> Option<Vec<u8>> { None }
        fn analysis(&self, _: u32, _: &rbl_dbserver::catalog::Analysis) -> Option<Vec<u8>> { None }
    }

    #[test]
    fn rejection_clears_every_store_and_database_readiness_in_all_known_mode_states() {
        use rbl_dbserver::net::Handler;
        let now = Instant::now();
        let sender = Ipv4Addr::new(192, 168, 50, 10);
        for mode in [rbl_prolink::ConnectionMode::Wired, rbl_prolink::ConnectionMode::Wireless] {
            let cfg = rejection_config(mode);
            let candidate = if mode == rbl_prolink::ConnectionMode::Wireless { 41 } else { 17 };
            let running = if mode == rbl_prolink::ConnectionMode::Wireless { 44 } else { 18 };
            for state in [join::State::Waiting, join::State::Discovery { sent: 2 },
                join::State::Probing { round: 4, index: 2 }, join::State::Assigning { sent: 3 },
                join::State::Running { number: running }, join::State::Failed("fixture".into())] {
                let assigned = if matches!(state, join::State::Running { .. }) { running } else { 0 };
                let mut fixture = members(&[1, 9, 10, 11, 12, 33], now);
                let mut initialized = Join::new(cfg.mac, cfg.address, now).with_mode(mode);
                initialized.hear_keep_alive(&keep_alive(1, sender), now);
                fixture.join = Some(initialized.test_state(state.clone()));
                fixture.master = MasterState { on: true, bpm_x100: 12_300, bar_beat: 3 };
                fixture.assigned.store(assigned, Ordering::Relaxed);
                let cell = Arc::clone(&fixture.assigned);
                let handler = rbl_dbserver::session::CatalogHandler::new(Arc::new(EmptyCatalog)).with_device(cell.clone());
                assert_eq!(handler.open_ready().is_some(), assigned != 0);
                let shared = Mutex::new(fixture);
                let out = hear_announce(&rejection_frame(), SocketAddr::from((sender, 50000)), &cfg, &shared, now).unwrap();
                assert_eq!(out.packet, disconnect_bytes(if assigned == 0 { candidate } else { running }, cfg.address), "{mode:?} {state:?}");
                assert_eq!(out.to, None);
                // The gate is already zero before the caller sends this output.
                assert_eq!(cell.load(Ordering::Relaxed), 0);
                assert!(handler.open_ready().is_none());
                let snapshot = shared.lock();
                assert!(snapshot.rejected());
                assert!(snapshot.down.as_ref().unwrap().contains("rejected"));
                assert_eq!(snapshot.join.as_ref().unwrap().state(), &join::State::Waiting);
                assert!(snapshot.peers.is_empty());
                assert!(snapshot.players.is_empty());
                assert_eq!(snapshot.greeted, Vec::<Ipv4Addr>::new());
                assert_eq!(snapshot.to_greet, Vec::<Ipv4Addr>::new());
                assert_eq!(snapshot.master, MasterState { on: false, bpm_x100: 0, bar_beat: 0 });
                drop(snapshot);
                let repeat = hear_announce(&rejection_frame(), SocketAddr::from((sender, 50000)), &cfg, &shared, now).unwrap();
                assert_eq!(repeat.packet, disconnect_bytes(candidate, Ipv4Addr::UNSPECIFIED));

                let mut frames = vec![keep_alive(1, sender).encode(), rbl_prolink::rekordbox_claim_stage1([1;6], 1),
                    rbl_prolink::rekordbox_claim_stage2([1;6], sender, running, 5),
                    rbl_prolink::number_in_use_reply(REKORDBOX_NAME, running)];
                for kind in [4, 7, 8, 0x0b] {
                    let mut frame = keep_alive(1, sender).encode(); frame[10] = kind; frames.push(frame);
                }
                for frame in frames {
                    assert!(hear_announce(&frame, SocketAddr::from((sender, 50000)), &cfg, &shared, now).is_none());
                }
                let mut snapshot = shared.lock();
                snapshot.join.as_mut().unwrap().reset(now);
                assert!(snapshot.join.as_mut().unwrap().tick(now + Duration::from_secs(60)).is_none());
                assert!(snapshot.rejected());
                assert!(snapshot.peers.is_empty() && snapshot.players.is_empty());
                assert!(snapshot.greeted.is_empty() && snapshot.to_greet.is_empty());
                assert_eq!(cell.load(Ordering::Relaxed), 0);
            }
        }
    }

    #[test]
    fn fresh_rejection_is_terminal_without_an_outgoing_datagram_and_unknown_is_unchanged() {
        let now = Instant::now();
        let sender = Ipv4Addr::new(10, 9, 8, 7); // before NetIF setup, no subnet guard
        for mode in [rbl_prolink::ConnectionMode::Wired, rbl_prolink::ConnectionMode::Wireless,
            rbl_prolink::ConnectionMode::Unknown] {
            let cfg = rejection_config(mode);
            let mut fixture = members(&[1], now);
            fixture.join = Some(Join::new(cfg.mac, cfg.address, now).with_mode(mode));
            let shared = Mutex::new(fixture);
            assert!(hear_announce(&rejection_frame(), SocketAddr::from((sender, 50000)), &cfg, &shared, now).is_none());
            let mut snapshot = shared.lock();
            let known = mode != rbl_prolink::ConnectionMode::Unknown;
            assert_eq!(snapshot.rejected(), known);
            assert_eq!(snapshot.peers.is_empty(), known);
            assert_eq!(snapshot.players.is_empty(), known);
            assert_eq!(snapshot.down.is_some(), known);
            assert!(snapshot.join.as_mut().unwrap().tick(now + Duration::from_secs(60)).is_none());
            drop(snapshot);
            assert!(hear_announce(&rejection_frame(), SocketAddr::from((sender, 50000)), &cfg, &shared, now).is_none());
            // A new object is RBX's bounded explicit recovery policy.
            let replacement = Mutex::new(Shared { join: Some(Join::new(cfg.mac, cfg.address, now).with_mode(mode)), ..Shared::default() });
            hear_announce(&keep_alive(1, sender).encode(), SocketAddr::from((sender, 50000)), &cfg, &replacement, now);
            assert!(!replacement.lock().rejected());
            assert!(matches!(replacement.lock().join.as_ref().unwrap().state(), join::State::Discovery { .. }));
        }
    }

    #[test]
    fn rejection_header_and_network_sender_guards_are_scoped_and_preserve_state() {
        let now = Instant::now();
        let cfg = rejection_config(rbl_prolink::ConnectionMode::Wired);
        let on_subnet = Ipv4Addr::new(192, 168, 50, 10);
        for initialized in [false, true] {
            for sender in [Ipv4Addr::UNSPECIFIED, cfg.address, Ipv4Addr::new(192, 168, 51, 10), on_subnet] {
                let mut join = Join::new(cfg.mac, cfg.address, now).with_mode(cfg.mode);
                if initialized { join.hear_keep_alive(&keep_alive(1, on_subnet), now); }
                let shared = Mutex::new(Shared { join: Some(join), ..Shared::default() });
                for cut in 0..36 {
                    assert!(hear_announce(&rejection_frame()[..cut], SocketAddr::from((sender, 50000)), &cfg, &shared, now).is_none());
                    assert!(!shared.lock().rejected());
                }
                let mut invalid = rejection_frame(); invalid[0] ^= 0xff;
                assert!(hear_announce(&invalid, SocketAddr::from((sender, 50000)), &cfg, &shared, now).is_none());
                assert!(!shared.lock().rejected());
                let accepted = if initialized { sender == on_subnet } else { sender != Ipv4Addr::UNSPECIFIED };
                let out = hear_announce(&rejection_frame(), SocketAddr::from((sender, 50000)), &cfg, &shared, now);
                assert_eq!(shared.lock().rejected(), accepted);
                assert_eq!(out.is_some(), initialized && accepted);
            }
        }
    }

    #[test]
    fn wireless_original_failed_linkup_retains_pre_rejection_identity_without_membership() {
        let now = Instant::now();
        let cfg = rejection_config(rbl_prolink::ConnectionMode::Wireless);
        let sender = Ipv4Addr::new(192, 168, 50, 10);
        for name in ["CDJ-2000", "CDJ-900"] {
            let shared = Mutex::new(Shared { join: Some(Join::new(cfg.mac, cfg.address, now).with_mode(cfg.mode)), ..Shared::default() });
            let mut original = keep_alive(1, sender);
            original.name = name.into(); original.generation = 0;
            hear_announce(&original.encode(), SocketAddr::from((sender, 50000)), &cfg, &shared, now);
            let snapshot = shared.lock();
            assert_eq!(snapshot.join.as_ref().unwrap().state(), &join::State::Waiting);
            assert!(snapshot.join.as_ref().unwrap().network_initialized());
            assert!(snapshot.peers.is_empty() && snapshot.players.is_empty());
            assert!(snapshot.greeted.is_empty() && snapshot.to_greet.is_empty());
            assert_eq!(snapshot.assigned.load(Ordering::Relaxed), 0);
            drop(snapshot);
            let out = hear_announce(&rejection_frame(), SocketAddr::from((sender, 50000)), &cfg, &shared, now).unwrap();
            assert_eq!(out.packet, disconnect_bytes(41, cfg.address));
            let repeat = hear_announce(&rejection_frame(), SocketAddr::from((sender, 50000)), &cfg, &shared, now).unwrap();
            assert_eq!(repeat.packet, disconnect_bytes(41, Ipv4Addr::UNSPECIFIED));
        }
    }

    #[test]
    fn rejection_disconnect_output_uses_configured_broadcast_destination() {
        let now = Instant::now();
        let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        receiver.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
        let port = receiver.local_addr().unwrap().port();
        let sender = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let mut cfg = config(); cfg.announce_port = port;
        let remote = Ipv4Addr::new(127, 0, 0, 2);
        let mut join = Join::new(cfg.mac, cfg.address, now).with_mode(cfg.mode);
        join.hear_keep_alive(&keep_alive(1, remote), now);
        let shared = Mutex::new(Shared { join: Some(join.test_state(join::State::Running { number: 18 })), ..Shared::default() });
        let out = hear_announce(&rejection_frame(), SocketAddr::from((remote, port + 1)), &cfg, &shared, now).unwrap();
        assert_eq!(out.to, None);
        send_announce(&sender, &out, SocketAddr::from((cfg.broadcast, port)), port);
        let mut buffer = [0; 100];
        let (length, from) = receiver.recv_from(&mut buffer).unwrap();
        assert_eq!(buffer[..length], disconnect_bytes(18, cfg.address));
        assert_eq!(from, sender.local_addr().unwrap());
    }

    #[test]
    fn fresh_known_mode_rejection_sends_no_udp_in_the_real_announce_loop() {
        for mode in [rbl_prolink::ConnectionMode::Wired, rbl_prolink::ConnectionMode::Wireless] {
            let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            receiver.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
            let announce = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            announce.set_read_timeout(Some(POLL)).unwrap();
            let destination = announce.local_addr().unwrap();
            let mut cfg = rejection_config(mode);
            cfg.broadcast = Ipv4Addr::LOCALHOST;
            cfg.announce_port = receiver.local_addr().unwrap().port();
            let stop = Arc::new(AtomicBool::new(false));
            let shared = Arc::new(Mutex::new(Shared::default()));
            let cell = Arc::clone(&shared.lock().assigned);
            let (thread_stop, thread_shared, thread_cell) = (stop.clone(), shared.clone(), cell.clone());
            let worker = std::thread::spawn(move || announce_loop(&announce, &cfg, &thread_stop, &thread_shared, &thread_cell));
            receiver.send_to(&rejection_frame(), destination).unwrap();
            let deadline = Instant::now() + Duration::from_secs(1);
            while !shared.lock().rejected() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            // Subsequent ordinary input and repeated rejection still have no
            // initialized network interface through which to broadcast.
            receiver.send_to(&keep_alive(1, Ipv4Addr::LOCALHOST).encode(), destination).unwrap();
            receiver.send_to(&rejection_frame(), destination).unwrap();
            let mut bytes = [0; 100];
            let observed = receiver.recv_from(&mut bytes);
            stop.store(true, Ordering::Relaxed);
            worker.join().unwrap();
            assert!(shared.lock().rejected());
            assert!(shared.lock().down.is_some());
            assert_eq!(cell.load(Ordering::Relaxed), 0);
            assert!(matches!(observed, Err(ref error) if is_timeout(error)));
        }
    }

    #[test]
    fn a_late_player_status_cannot_revive_terminal_membership_or_report_a_load() {
        struct Loads(std::sync::atomic::AtomicUsize);
        impl LibraryFacts for Loads {
            fn track_count(&self) -> u16 { 1 }
            fn playlist_count(&self) -> u16 { 0 }
            fn track_loaded(&self, _: u32) { self.0.fetch_add(1, Ordering::Relaxed); }
        }
        let now = Instant::now();
        let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        receiver.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let mut cfg = config(); cfg.player_port = receiver.local_addr().unwrap().port();
        let sender = Ipv4Addr::new(127, 0, 0, 2);
        let from = SocketAddrV4::new(sender, 50002);
        let mut join = Join::new(cfg.mac, cfg.address, now).with_mode(cfg.mode);
        join.hear_keep_alive(&keep_alive(1, sender), now);
        let mut fixture = Shared { join: Some(join.test_state(join::State::Running { number: 17 })), ..Shared::default() };
        fixture.assigned.store(17, Ordering::Relaxed);
        // Use localhost for the observed status sender so its greeting can
        // be received without adding another OS loopback alias.
        fixture.greeted.push(Ipv4Addr::LOCALHOST);
        let shared = Mutex::new(fixture);
        let counter = Arc::new(Loads(std::sync::atomic::AtomicUsize::new(0)));
        let facts: Arc<dyn LibraryFacts> = counter.clone();
        let mut packet = include_bytes!("../tests/fixtures/cdj-status-playing-ours.bin").to_vec();
        packet[0x28] = 17;
        assert!(status_from_packet(&packet).unwrap().is_some());
        assert!(hear_announce(&rejection_frame(), SocketAddr::V4(from), &cfg, &shared, now).is_some());
        // Stale `ours` is exactly the snapshot StatusLoop could have taken
        // before the rejection cleared its shared assigned-number cell.
        hear_player_status(&packet, SocketAddrV4::new(Ipv4Addr::LOCALHOST, 50002), &socket, &cfg, &shared, &facts, 17);
        let snapshot = shared.lock();
        assert!(snapshot.players.is_empty() && snapshot.peers.is_empty());
        assert!(snapshot.greeted.is_empty() && snapshot.to_greet.is_empty());
        drop(snapshot);
        let mut buffer = [0; 100];
        assert!(matches!(receiver.recv_from(&mut buffer), Err(ref error) if is_timeout(error)));
        // A recording facts instance observes the callback without touching
        // any actual library; prove the same captured status can otherwise
        // report a load using a fresh non-rejected object below.
        assert_eq!(counter.0.load(Ordering::Relaxed), 0);
        let replacement = Mutex::new(Shared::default());
        hear_player_status(&packet, SocketAddrV4::new(Ipv4Addr::LOCALHOST, 50002), &socket, &cfg, &replacement, &facts, 17);
        assert_eq!(counter.0.load(Ordering::Relaxed), 1);
        assert_eq!(replacement.lock().players.len(), 1);
    }

    #[test]
    fn rejection_runtime_model_gate_is_distinct_from_selected_mode_and_cached_candidate() {
        let now = Instant::now();
        let sender = Ipv4Addr::new(192, 168, 50, 10);
        for name in ["CDJ-2000", "CDJ-900"] {
            let mut original = rejection_frame();
            original[12..32].fill(0);
            original[12..12 + name.len()].copy_from_slice(name.as_bytes());
            original[33] = 0;
            for mode in [rbl_prolink::ConnectionMode::Wired, rbl_prolink::ConnectionMode::Wireless] {
                let cfg = rejection_config(mode);
                // Constructor runtime mode 0xff permits original-named 09
                // even on selected wireless; no initialized IF means no UDP.
                let fresh = Mutex::new(Shared { join: Some(Join::new(cfg.mac, cfg.address, now).with_mode(mode)), ..Shared::default() });
                assert!(!fresh.lock().join.as_ref().unwrap().excludes_original_models());
                assert!(hear_announce(&original, SocketAddr::from((sender, 50000)), &cfg, &fresh, now).is_none());
                assert!(fresh.lock().rejected());
                assert!(fresh.lock().join.as_ref().unwrap().excludes_original_models());

                let mut join = Join::new(cfg.mac, cfg.address, now).with_mode(mode);
                join.hear_keep_alive(&keep_alive(1, sender), now);
                let active = Mutex::new(Shared { join: Some(join), ..Shared::default() });
                let out = hear_announce(&original, SocketAddr::from((sender, 50000)), &cfg, &active, now);
                assert_eq!(out.is_some(), mode == rbl_prolink::ConnectionMode::Wired);
                if mode == rbl_prolink::ConnectionMode::Wireless {
                    // Runtime wireless exclusion precedes 09 dispatch.
                    assert!(!active.lock().rejected());
                    assert!(hear_announce(&rejection_frame(), SocketAddr::from((sender, 50000)), &cfg, &active, now).is_some());
                }
                assert!(active.lock().rejected());
                // readReject clears the runtime byte to zero, so original
                // repeats are excluded even after a wired first rejection.
                assert!(hear_announce(&original, SocketAddr::from((sender, 50000)), &cfg, &active, now).is_none());
                let modern = hear_announce(&rejection_frame(), SocketAddr::from((sender, 50000)), &cfg, &active, now).unwrap();
                let candidate = if mode == rbl_prolink::ConnectionMode::Wireless { 41 } else { 17 };
                assert_eq!(modern.packet, disconnect_bytes(candidate, Ipv4Addr::UNSPECIFIED));
            }
        }
    }
}
