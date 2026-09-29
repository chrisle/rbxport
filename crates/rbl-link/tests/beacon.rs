//! The beacon against captured packets: a player's keep-alive, its media
//! query, its `46`, and its status with one of our tracks playing, all sent
//! from a socket standing in for the player, on loopback.
#![allow(
    clippy::pedantic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rbl_link::beacon::{Beacon, BeaconConfig, LibraryFacts};
use rbl_prolink::{packet_kind, DeviceType};

const CDJ_KEEP_ALIVE: &str =
    "5173707431576d4a4f4c060043444a2d333030300000000000000000000000000103003601012497ed0b4043c0a80198030000000164";
const MEDIA_QUERY: &str =
    "5173707431576d4a4f4c0543444a2d33303030000000000000000000000000010001000cc0a801980000001100000003";
const HANDSHAKE: &str =
    "5173707431576d4a4f4c4643444a2d333030300000000000000000000000000100010004010400e4";
const STATUS_PLAYING_OURS: &[u8] = include_bytes!("fixtures/cdj-status-playing-ours.bin");
const STATUS_EMPTY: &[u8] = include_bytes!("fixtures/cdj-status-empty.bin");
const LINK_DEVICE_NUMBER: u8 = 0x11;

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn status_playing_ours() -> Vec<u8> {
    let mut packet = STATUS_PLAYING_OURS.to_vec();
    packet[0x28] = LINK_DEVICE_NUMBER;
    packet
}

/// Library facts, remembering the tracks the beacon reports loaded.
#[derive(Default)]
struct Facts(Mutex<Vec<u32>>);
impl LibraryFacts for Facts {
    fn track_count(&self) -> u16 {
        38_681
    }
    fn playlist_count(&self) -> u16 {
        627
    }
    fn track_loaded(&self, track: u32) {
        self.0.lock().unwrap().push(track);
    }
}

/// A socket standing in for the player, and the beacon told to answer it
/// there, on the link as 17.
fn start() -> (Beacon, UdpSocket) {
    let (beacon, player, _) = start_on(None);
    join(&beacon, &player);
    (beacon, player)
}

/// Says nothing into an empty network: the player's keep-alive is what
/// starts the join, which settles on 17 about four seconds later.
fn join(beacon: &Beacon, player: &UdpSocket) {
    use rbl_link::LinkState;
    assert_eq!(beacon.link_state(), LinkState::Waiting);
    assert_eq!(beacon.number(), None);
    let announce = SocketAddrV4::new(Ipv4Addr::LOCALHOST, beacon.announce_port());
    player.send_to(&hex(CDJ_KEEP_ALIVE), announce).unwrap();
    wait_for_link(
        beacon,
        &LinkState::Up {
            number: LINK_DEVICE_NUMBER,
        },
    );
}

fn wait_for_link(beacon: &Beacon, wanted: &rbl_link::LinkState) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while beacon.link_state() != *wanted {
        assert!(
            Instant::now() < deadline,
            "the link did not reach {wanted:?}: {:?}",
            beacon.link_state()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The same, with the beacon's sockets pinned to `interface`.
fn start_on(interface: Option<String>) -> (Beacon, UdpSocket, Arc<Facts>) {
    let facts = Arc::new(Facts::default());
    let player = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    player
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let beacon = Beacon::start(
        BeaconConfig {
            interface,
            address: Ipv4Addr::LOCALHOST,
            broadcast: Ipv4Addr::LOCALHOST,
            mac: [0x00, 0xe0, 0x4c, 0xcf, 0x63, 0x2e],
            announce_port: 0,
            status_port: 0,
            player_port: player.local_addr().unwrap().port(),
            beat_port: player.local_addr().unwrap().port(),
            computer_name: "test-mac".to_owned(),
        },
        facts.clone(),
    )
    .unwrap();
    (beacon, player, facts)
}

/// Receives until a packet of `kind` arrives; the beacon's own broadcasts
/// are interleaved with the replies.
fn receive(player: &UdpSocket, kind: u8) -> Vec<u8> {
    let mut buffer = [0_u8; 2048];
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        let Ok((len, _)) = player.recv_from(&mut buffer) else {
            continue;
        };
        if packet_kind(&buffer[..len]) == Ok(kind) {
            return buffer[..len].to_vec();
        }
    }
    panic!("no packet of kind {kind:#x}");
}

fn wait_for(beacon: &Beacon, ready: impl Fn(&[rbl_link::Player]) -> bool) -> Vec<rbl_link::Player> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let players = beacon.players();
        if ready(&players) || Instant::now() > deadline {
            return players;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_player_is_listed_from_its_keep_alive_and_answered_on_its_status_port() {
    let (beacon, player, facts) = start_on(None);
    let status = SocketAddrV4::new(Ipv4Addr::LOCALHOST, beacon.status_port());

    join(&beacon, &player);
    let players = wait_for(&beacon, |p| !p.is_empty());
    assert_eq!(players.len(), 1);
    assert_eq!(players[0].name, "CDJ-3000");
    assert_eq!(players[0].number, 1);
    assert_eq!(players[0].kind, DeviceType::Cdj);
    assert_eq!(players[0].loaded, None);

    // A player heard for the first time is greeted, as rekordbox greets one.
    let greeting = receive(&player, 0x16);
    assert_eq!(greeting.len(), 0x30);
    assert_eq!(&greeting[0x0b..0x14], b"rekordbox");

    // The media query is answered with the library's counts, naming back
    // the slot the player asked about: the emulator's `03` and a current
    // CDJ-3000's `04` alike. The captured query names 192.168.1.152 as the
    // asker; ours has to name us.
    for slot in [
        rbl_prolink::SLOT_REKORDBOX_LEGACY,
        rbl_prolink::SLOT_REKORDBOX,
    ] {
        let mut query = hex(MEDIA_QUERY);
        query[0x24..0x28].copy_from_slice(&Ipv4Addr::LOCALHOST.octets());
        query[0x2b] = LINK_DEVICE_NUMBER;
        query[0x2f] = slot;
        player.send_to(&query, status).unwrap();
        let response = receive(&player, 0x06);
        assert_eq!(response.len(), 0xc0);
        assert_eq!(response[0x2b], slot, "the slot named back");
        assert_eq!(u16::from_be_bytes([response[0xa6], response[0xa7]]), 38_681);
        assert_eq!(u16::from_be_bytes([response[0xae], response[0xaf]]), 627);
    }

    player.send_to(&hex(HANDSHAKE), status).unwrap();
    let reply = receive(&player, 0x47);
    assert_eq!(reply.len(), 0x48);
    assert_eq!(reply[0x21], LINK_DEVICE_NUMBER);

    // The player's status says what it has loaded from us and whether it is
    // master.
    player.send_to(&status_playing_ours(), status).unwrap();
    let players = wait_for(&beacon, |p| p[0].loaded.is_some());
    assert_eq!(players[0].loaded, Some(17_181));
    assert!(players[0].playing);
    assert!(players[0].master);
    assert_eq!(players[0].bpm_x100, 12_539);

    // Our status now carries the master's tempo.
    let ours = receive(&player, 0x29);
    assert_eq!(u16::from_be_bytes([ours[0x2e], ours[0x2f]]), 12_539);

    // Unloading clears it; the load was reported once, not per status packet.
    player.send_to(STATUS_EMPTY, status).unwrap();
    let players = wait_for(&beacon, |p| p[0].loaded.is_none());
    assert_eq!(players[0].loaded, None);
    assert!(!players[0].playing);
    assert_eq!(*facts.0.lock().unwrap(), vec![17_181]);

    beacon.stop();
}

/// Pinned to an interface, the beacon still hears the player and answers
/// it: the pin is how a command leaves from the announced address on a
/// machine with two interfaces on the players' subnet, and it must not cost
/// the broadcasts. Loopback is the one interface every machine has.
#[test]
fn a_beacon_pinned_to_an_interface_still_hears_and_answers_a_player() {
    let loopback = if_addrs::get_if_addrs()
        .unwrap()
        .into_iter()
        .find(|i| i.is_loopback() && i.ip().is_ipv4())
        .map(|i| i.name)
        .expect("a loopback interface");
    let (beacon, player, _) = start_on(Some(loopback));
    let status = SocketAddrV4::new(Ipv4Addr::LOCALHOST, beacon.status_port());

    join(&beacon, &player);
    let players = wait_for(&beacon, |p| !p.is_empty());
    assert_eq!(players.len(), 1, "the keep-alive was heard through the pin");
    let greeting = receive(&player, 0x16);
    assert_eq!(&greeting[0x0b..0x14], b"rekordbox");

    let mut query = hex(MEDIA_QUERY);
    query[0x24..0x28].copy_from_slice(&Ipv4Addr::LOCALHOST.octets());
    query[0x2b] = LINK_DEVICE_NUMBER;
    player.send_to(&query, status).unwrap();
    assert_eq!(receive(&player, 0x06).len(), 0xc0);

    beacon.stop();
}

#[test]
#[ignore = "environment-sensitive: asserts a real join handshake completes inside a 3500-6000ms wall-clock window"]
fn nothing_is_said_until_a_player_is_heard_and_the_join_settles_on_seventeen() {
    let (beacon, player, _) = start_on(None);
    // Silence: no status for a second on an empty network.
    let mut buffer = [0_u8; 2048];
    player
        .set_read_timeout(Some(Duration::from_millis(1000)))
        .unwrap();
    assert!(
        player.recv_from(&mut buffer).is_err(),
        "a packet before any player was heard"
    );
    player
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();

    // The keep-alive starts the join: three claims and six rounds of six
    // probes at 100 ms, then 17 — about four seconds.
    let started = Instant::now();
    join(&beacon, &player);
    let took = started.elapsed();
    assert!(
        (Duration::from_millis(3500)..Duration::from_millis(6000)).contains(&took),
        "{took:?}"
    );
    assert_eq!(beacon.number(), Some(LINK_DEVICE_NUMBER));
    let status = receive(&player, 0x29);
    assert_eq!(status[0x21], LINK_DEVICE_NUMBER);
    beacon.stop();
}

#[test]
fn a_number_answered_for_is_left_to_its_holder() {
    let (beacon, player, _) = start_on(None);
    let announce = SocketAddrV4::new(Ipv4Addr::LOCALHOST, beacon.announce_port());
    player.send_to(&hex(CDJ_KEEP_ALIVE), announce).unwrap();
    // Another rekordbox holds 17: it answers every probe of it with `03`,
    // sent to the prober. The join skips 17 from then on and takes 18.
    let in_use = rbl_prolink::number_in_use_reply("rekordbox", LINK_DEVICE_NUMBER);
    let deadline = Instant::now() + Duration::from_millis(1500);
    while Instant::now() < deadline {
        player.send_to(&in_use, announce).unwrap();
        std::thread::sleep(Duration::from_millis(50));
    }
    wait_for_link(&beacon, &rbl_link::LinkState::Up { number: 0x12 });
    let status = receive(&player, 0x29);
    assert_eq!(status[0x21], 0x12, "the status carries the number taken");
    // And 18 is answered for when probed.
    let probe = rbl_prolink::rekordbox_claim_stage2(
        [1, 2, 3, 4, 5, 6],
        Ipv4Addr::new(127, 0, 0, 2),
        0x12,
        1,
    );
    player.send_to(&probe, announce).unwrap();
    // The reply goes to the prober's address on the announce port, which
    // on loopback is the beacon's own socket; what can be checked here is
    // that the beacon stays up and keeps its number.
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(beacon.number(), Some(0x12));
    beacon.stop();
}

#[test]
#[ignore = "environment-sensitive: asserts five real 200ms beacon intervals land inside 1200ms wall-clock"]
fn the_status_beacon_runs_at_five_hertz_with_no_tempo_until_a_master_reports() {
    let (beacon, player) = start();
    let first = receive(&player, 0x29);
    let started = Instant::now();
    assert_eq!(first.len(), 0x38);
    assert_eq!(&first[0x0b..0x14], b"rekordbox");
    assert_eq!(first[0x21], LINK_DEVICE_NUMBER);
    assert_eq!(
        u16::from_be_bytes([first[0x2e], first[0x2f]]),
        0,
        "no master yet"
    );
    // Four more within a second and a bit: 200 ms apart.
    for _ in 0..4 {
        receive(&player, 0x29);
    }
    assert!(
        started.elapsed() < Duration::from_millis(1200),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(
        beacon.players().len(),
        1,
        "the player whose keep-alive brought the link up"
    );
    beacon.stop();
}

/// Receives until a packet of `kind` arrives, reporting who sent it.
/// A status packet (`0x29`) whose master flag matches `want_master`, skipping
/// any stale ones still buffered from before a `set_master` call took effect —
/// the beacon keeps sending at 5 Hz, so up to one status built before the flag
/// flipped can already be in flight. Bounded by a deadline so a beacon that
/// never reaches the wanted state fails instead of hanging.
fn receive_status(player: &UdpSocket, want_master: bool) -> Vec<u8> {
    let mut buffer = [0_u8; 2048];
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        let Ok((len, _)) = player.recv_from(&mut buffer) else {
            continue;
        };
        let packet = &buffer[..len];
        if packet_kind(packet) == Ok(0x29) && (packet[0x27] == 0xe0) == want_master {
            return packet.to_vec();
        }
    }
    panic!("no status with master={want_master} arrived");
}
fn receive_from(player: &UdpSocket, kind: u8) -> (Vec<u8>, std::net::SocketAddr) {
    let mut buffer = [0_u8; 2048];
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if let Ok((len, from)) = player.recv_from(&mut buffer) {
            let packet = &buffer[..len];
            if packet_kind(packet) == Ok(kind) {
                return (packet.to_vec(), from);
            }
        }
    }
    panic!("no packet of kind {kind:#04x} arrived");
}

/// A load command reaches the player it names, from the port the player has
/// us at — not a fresh ephemeral one, which is not a source it answers to.
#[test]
fn a_load_command_reaches_the_player_from_our_status_port() {
    let (beacon, player) = start();
    let status = SocketAddrV4::new(Ipv4Addr::LOCALHOST, beacon.status_port());

    // The player has to be on the link before it can be told anything.
    player.send_to(&hex(CDJ_KEEP_ALIVE), status).unwrap();
    player.send_to(&status_playing_ours(), status).unwrap();
    wait_for(&beacon, |p| p.iter().any(|q| q.number == 1));

    beacon.load_track(1, 17_181).unwrap();
    let (packet, from) = receive_from(&player, 0x19);

    // It came from our status port, as rekordbox's own replies do.
    assert_eq!(
        from.port(),
        beacon.status_port(),
        "a command must leave from the port the player knows"
    );
    assert_eq!(packet.len(), rbl_prolink::LOAD_TRACK_LEN);
    assert_eq!(
        rbl_prolink::status_device_name(&packet).unwrap(),
        rbl_prolink::REKORDBOX_NAME
    );
    assert_eq!(
        packet[0x21],
        LINK_DEVICE_NUMBER,
        "sent as rekordbox"
    );
    assert_eq!(
        packet[0x28],
        LINK_DEVICE_NUMBER,
        "the track's source device"
    );
    assert_eq!(packet[0x29], rbl_prolink::SLOT_REKORDBOX);
    assert_eq!(&packet[0x2c..0x30], &17_181_u32.to_be_bytes());
    assert_eq!(packet[0x40], 0, "player 1, counted from zero");

    // A player that is not there is refused rather than silently dropped.
    assert!(beacon.load_track(9, 1).is_err());

    beacon.stop();
}

/// A beacon with its beat clock pointed at a socket the test owns, so the
/// beats it broadcasts as master can be read.
fn start_master() -> (Beacon, UdpSocket, UdpSocket) {
    let player = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    player
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let beats = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    beats
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let beacon = Beacon::start(
        BeaconConfig {
            interface: None,
            address: Ipv4Addr::LOCALHOST,
            broadcast: Ipv4Addr::LOCALHOST,
            mac: [0x00, 0xe0, 0x4c, 0xcf, 0x63, 0x2e],
            announce_port: 0,
            status_port: 0,
            player_port: player.local_addr().unwrap().port(),
            beat_port: beats.local_addr().unwrap().port(),
            computer_name: "test-mac".to_owned(),
        },
        Arc::new(Facts::default()),
    )
    .unwrap();
    join(&beacon, &player);
    (beacon, player, beats)
}

#[test]
fn as_master_the_beacon_drives_beats_and_says_it_is_master() {
    let (beacon, player, beats) = start_master();

    // Off, no beats are broadcast, and the status is not master.
    assert!(!beacon.master_state().on);
    beacon.set_master_bpm(12_800);
    beacon.set_master(true);
    assert!(beacon.master_state().on);

    // A beat packet arrives, byte-for-byte a master beat at 128.00 BPM with a
    // beat within the bar of 1..4.
    let beat = receive(&beats, rbl_prolink::BEAT_KIND);
    assert_eq!(beat.len(), rbl_prolink::BEAT_LEN);
    assert_eq!(
        rbl_prolink::status_device_name(&beat).unwrap(),
        rbl_prolink::REKORDBOX_NAME
    );
    let bar_beat = beat[0x5c];
    assert!((1..=4).contains(&bar_beat), "beat within the bar");
    assert_eq!(
        beat,
        rbl_prolink::beat_packet(
            rbl_prolink::REKORDBOX_NAME,
            LINK_DEVICE_NUMBER,
            12_800,
            bar_beat
        )
    );

    // The status now says we are master, at our tempo. Read one addressed to
    // the player's port on 50002, skipping any non-master status still in
    // flight from before set_master took effect.
    let s = receive_status(&player, true);
    assert_eq!(s[0x27], 0xe0, "the master status flag");
    assert_eq!(s[0x34], 0x01, "Mm master");
    assert_eq!(u16::from_be_bytes([s[0x2e], s[0x2f]]), 12_800);

    // Resigning stops the beats and clears the master flag.
    beacon.set_master(false);
    std::thread::sleep(Duration::from_millis(150));
    // Drain, then confirm no fresh beat arrives within a beat's time.
    beats
        .set_read_timeout(Some(Duration::from_millis(400)))
        .unwrap();
    let mut buffer = [0_u8; 2048];
    while beats.recv_from(&mut buffer).is_ok() {}
    assert!(
        beats.recv_from(&mut buffer).is_err(),
        "no beats once master is off"
    );
    let s = receive_status(&player, false);
    assert_eq!(s[0x27], 0xc0, "not master again");

    beacon.stop();
}
