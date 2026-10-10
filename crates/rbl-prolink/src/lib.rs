//! Pro DJ Link packets.
//!
//! Every packet on the DJ Link network begins with the same ten-byte magic,
//! a one-byte kind, a subtype, and a twenty-byte device name. What follows
//! depends on the kind and the port it arrived on.
//!
//! # What is documented and what is not
//!
//! Layouts here follow the community protocol analysis (Deep Symmetry's
//! `djl-analysis`), which was derived from captures of real hardware. Values
//! specific to *rekordbox announcing itself as a media source* — device number
//! `0x11`, device type `0x04` — are recorded there but have not been verified
//! against a capture on this machine, so they are marked and must be confirmed
//! before anyone relies on a player accepting them.
//!
//! Nothing in this crate opens a socket; it is encoding and decoding only, so
//! it can be tested exhaustively without a network.

use std::net::Ipv4Addr;

use serde::Deserialize;

/// Every DJ Link packet starts with this.
pub const MAGIC: [u8; 10] = [0x51, 0x73, 0x70, 0x74, 0x31, 0x57, 0x6d, 0x4a, 0x4f, 0x4c];

/// Device names are a fixed twenty bytes, NUL-padded.
pub const NAME_LEN: usize = 20;
/// Offset of the device name.
pub const NAME_AT: usize = 0x0c;

/// Announcement and keep-alive.
pub const PORT_ANNOUNCE: u16 = 50_000;
/// Beats and mixer features.
pub const PORT_BEAT: u16 = 50_001;
/// Player status.
pub const PORT_STATUS: u16 = 50_002;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PacketError {
    #[error("packet is too short: {0} bytes")]
    TooShort(usize),
    #[error("not a DJ Link packet: wrong magic")]
    BadMagic,
    #[error("unexpected packet kind {0:#04x} for this port")]
    WrongKind(u8),
    #[error("unexpected packet subtype {0:#04x}")]
    WrongSubtype(u8),
    #[error("unexpected payload length {0}")]
    WrongLength(u16),
}

pub type Result<T> = std::result::Result<T, PacketError>;

/// Packet kinds seen on port 50000.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnounceKind {
    /// First-stage device number claim.
    ClaimStage1,
    /// Second-stage claim.
    ClaimStage2,
    /// Final-stage claim.
    ClaimFinal,
    /// Still present on the network.
    KeepAlive,
    /// Another device is claiming the same number.
    Conflict,
    /// Initial "I am here".
    Announce,
    Other(u8),
}

impl AnnounceKind {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0x00 => Self::ClaimStage1,
            0x02 => Self::ClaimStage2,
            0x04 => Self::ClaimFinal,
            0x06 => Self::KeepAlive,
            0x08 => Self::Conflict,
            0x0a => Self::Announce,
            other => Self::Other(other),
        }
    }

    pub fn to_u8(self) -> u8 {
        match self {
            Self::ClaimStage1 => 0x00,
            Self::ClaimStage2 => 0x02,
            Self::ClaimFinal => 0x04,
            Self::KeepAlive => 0x06,
            Self::Conflict => 0x08,
            Self::Announce => 0x0a,
            Self::Other(v) => v,
        }
    }
}

/// What kind of device is speaking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    Cdj,
    Mixer,
    /// rekordbox acting as a media source (measured: byte `0x34` = `04`).
    Rekordbox,
    /// Peer type `07`. [OBS] An XDJ-RX3 on firmware 1.20 announces it (seen
    /// on the wire from Linux). [UNKNOWN] Which other products announce it;
    /// "all-in-one" is a working name, not an established category.
    AllInOne,
    Other(u8),
}

impl DeviceType {
    pub fn to_u8(self) -> u8 {
        match self {
            Self::Cdj => 0x01,
            Self::Mixer => 0x02,
            Self::Rekordbox => 0x04,
            Self::AllInOne => 0x07,
            Self::Other(v) => v,
        }
    }

    pub fn from_u8(v: u8) -> Self {
        match v {
            0x01 => Self::Cdj,
            // `02` is the community-documented mixer; a DJM-V5 announces `03`
            // (measured on the wire 2026-09-13, device 33 at 192.168.1.66).
            // Both are mixers, so both decode as one — `to_u8` still says `02`.
            0x02 | 0x03 => Self::Mixer,
            0x04 => Self::Rekordbox,
            0x07 => Self::AllInOne,
            other => Self::Other(other),
        }
    }
}

/// The device number rekordbox takes when it announces itself (measured;
/// the CDJ-3000 lists it as `USB LINK17`).
pub const REKORDBOX_DEVICE_NUMBER: u8 = 0x11;

/// The name rekordbox announces.
pub const REKORDBOX_NAME: &str = "rekordbox";

/// How often a keep-alive goes out: rekordbox 7.2.11 sends one every 2.0 s
/// and a CDJ-3000 every 1.5 s (measured). A peer that has not been heard
/// from in three intervals is considered gone.
pub const KEEP_ALIVE_INTERVAL_MS: u64 = 2_000;

/// A parsed keep-alive (`kind 06`), the packet that says a device is present.
///
/// Layout, measured from rekordbox 7.2.11, a CDJ-3000 and Now Playing on the
/// same network (`docs/pre-release/design-notes/link-export-capture.md`):
/// after the name, `01`, a generation byte (`03` for rekordbox 7 and the
/// CDJ-3000, `02` for older players and virtual CDJs), the length `0036`,
/// the device number, `01`, the MAC, the IP, the peer count, a byte that is
/// `01` on rekordbox and `00` on a CDJ-3000, two zeros, the device type
/// (`04` rekordbox, `01` player, `02` mixer), and a final byte (`08` on
/// rekordbox, `64` on a CDJ-3000).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeepAlive {
    pub name: String,
    pub device_number: u8,
    pub device_type: DeviceType,
    pub mac: [u8; 6],
    pub ip: Ipv4Addr,
    /// Byte `0x30`: the count of *other* devices seen on the network, not
    /// counting this one (rekordbox 7.2.11 sent `0x02` with two peers present,
    /// captured 2026-09-12).
    pub peers: u8,
    /// Byte `0x21`: `03` for rekordbox 7 and CDJ-3000 class devices, `02`
    /// for the rest.
    pub generation: u8,
}

/// Byte length of a keep-alive.
pub const KEEP_ALIVE_LEN: usize = 0x36;

fn write_header(out: &mut Vec<u8>, kind: u8, subtype: u8, name: &str) {
    out.extend_from_slice(&MAGIC);
    out.push(kind);
    out.push(subtype);
    let mut padded = [0_u8; NAME_LEN];
    for (slot, byte) in padded.iter_mut().zip(name.as_bytes()) {
        *slot = *byte;
    }
    out.extend_from_slice(&padded);
}

/// Reads the device name from any DJ Link packet.
pub fn device_name(packet: &[u8]) -> Result<String> {
    if packet.len() < NAME_AT + NAME_LEN {
        return Err(PacketError::TooShort(packet.len()));
    }
    if packet.get(0..10) != Some(&MAGIC) {
        return Err(PacketError::BadMagic);
    }
    let raw = packet.get(NAME_AT..NAME_AT + NAME_LEN).unwrap_or(&[]);
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    Ok(String::from_utf8_lossy(raw.get(..end).unwrap_or(&[])).into_owned())
}

/// The device name of a status-port packet (50002), where the name follows
/// the kind byte directly, at `0x0b`.
pub fn status_device_name(packet: &[u8]) -> Result<String> {
    if packet.len() < NAME_AT - 1 + NAME_LEN {
        return Err(PacketError::TooShort(packet.len()));
    }
    if packet.get(0..10) != Some(&MAGIC) {
        return Err(PacketError::BadMagic);
    }
    let raw = packet
        .get(NAME_AT - 1..NAME_AT - 1 + NAME_LEN)
        .unwrap_or(&[]);
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    Ok(String::from_utf8_lossy(raw.get(..end).unwrap_or(&[])).into_owned())
}

/// The kind byte of any DJ Link packet.
pub fn packet_kind(packet: &[u8]) -> Result<u8> {
    if packet.len() <= 0x0a {
        return Err(PacketError::TooShort(packet.len()));
    }
    if packet.get(0..10) != Some(&MAGIC) {
        return Err(PacketError::BadMagic);
    }
    Ok(packet.get(0x0a).copied().unwrap_or(0))
}

/// What a packet kind on the announce port (50000) is called, for a log
/// line; `kind 0x..` for one nobody has named.
pub fn announce_kind_name(kind: u8) -> String {
    match AnnounceKind::from_u8(kind) {
        AnnounceKind::ClaimStage1 => "number claim 1".to_owned(),
        AnnounceKind::ClaimStage2 => "number claim 2".to_owned(),
        AnnounceKind::ClaimFinal => "number claim 3".to_owned(),
        AnnounceKind::KeepAlive => "keep-alive".to_owned(),
        AnnounceKind::Conflict => "number in use".to_owned(),
        AnnounceKind::Announce => "announce".to_owned(),
        AnnounceKind::Other(other) => format!("kind {other:#04x}"),
    }
}

/// What a packet kind on the status port (50002) or the beat port (50001)
/// is called, for a log line: the names the community analysis uses,
/// `kind 0x..` for one it has no name for.
pub fn status_kind_name(kind: u8) -> String {
    match kind {
        0x05 => "media query".to_owned(),
        0x06 => "media response".to_owned(),
        PLAYER_STATUS_KIND => "player status".to_owned(),
        DEVICE_IDENTITY_QUERY_KIND => "device identity query".to_owned(),
        0x11 => "device identity reply".to_owned(),
        LOAD_TRACK_KIND => "load track".to_owned(),
        LOAD_TRACK_ACK_KIND => "load track ack".to_owned(),
        MASTER_HANDOFF_REQUEST_KIND => "master handoff request".to_owned(),
        0x27 => "master handoff reply".to_owned(),
        BEAT_KIND => "beat".to_owned(),
        0x29 => "mixer status".to_owned(),
        0x2a => "sync control".to_owned(),
        DEVICE_PROPERTY_QUERY_KIND => "device property query".to_owned(),
        DEVICE_PROPERTY_RESPONSE_KIND => "device property response".to_owned(),
        DEVICE_SETTINGS_REQUEST_KIND => "device settings request".to_owned(),
        DEVICE_SETTINGS_RESPONSE_KIND => "device settings response".to_owned(),
        _ => format!("kind {kind:#04x}"),
    }
}

/// `packet` as hex pairs, for a trace line. Cut at `limit` bytes with a
/// `…`, so a status packet does not fill a line on its own.
pub fn hex(packet: &[u8], limit: usize) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(limit * 3 + 1);
    for (i, byte) in packet.iter().take(limit).enumerate() {
        if i > 0 {
            out.push(' ');
        }
        // Writing to a `String` cannot fail.
        let _ = write!(out, "{byte:02x}");
    }
    if packet.len() > limit {
        out.push('…');
    }
    out
}

impl KeepAlive {
    /// rekordbox's own keep-alive for a given address: device 17, type 4,
    /// generation 3, and the tail bytes rekordbox 7.2.11 sends.
    pub fn rekordbox(mac: [u8; 6], ip: Ipv4Addr, peers: u8) -> Self {
        Self::rekordbox_as(REKORDBOX_DEVICE_NUMBER, mac, ip, peers)
    }

    /// The same under the number the join settled on: 17 when it is free,
    /// 18 when another rekordbox holds 17.
    pub fn rekordbox_as(device_number: u8, mac: [u8; 6], ip: Ipv4Addr, peers: u8) -> Self {
        Self {
            name: REKORDBOX_NAME.to_owned(),
            device_number,
            device_type: DeviceType::Rekordbox,
            mac,
            ip,
            peers,
            generation: 0x03,
        }
    }

    /// Encodes the packet.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(KEEP_ALIVE_LEN);
        write_header(&mut out, AnnounceKind::KeepAlive.to_u8(), 0x00, &self.name);
        out.push(0x01);
        out.push(self.generation);
        out.extend_from_slice(&u16::try_from(KEEP_ALIVE_LEN).unwrap_or(0).to_be_bytes());
        out.push(self.device_number);
        out.push(0x01);
        out.extend_from_slice(&self.mac);
        out.extend_from_slice(&self.ip.octets());
        out.push(self.peers);
        let (after_peers, last) = match self.device_type {
            DeviceType::Rekordbox => (0x01, 0x08),
            _ => (0x00, 0x64),
        };
        out.extend_from_slice(&[after_peers, 0, 0]);
        out.push(self.device_type.to_u8());
        out.push(last);
        debug_assert_eq!(out.len(), KEEP_ALIVE_LEN);
        out
    }

    /// Decodes a keep-alive.
    pub fn decode(packet: &[u8]) -> Result<Self> {
        if packet.len() < KEEP_ALIVE_LEN {
            return Err(PacketError::TooShort(packet.len()));
        }
        let kind = packet_kind(packet)?;
        if AnnounceKind::from_u8(kind) != AnnounceKind::KeepAlive {
            return Err(PacketError::WrongKind(kind));
        }
        if packet[0x0b] != 0 {
            return Err(PacketError::WrongSubtype(packet[0x0b]));
        }
        let at = |i: usize| packet.get(i).copied().unwrap_or(0);
        let mut mac = [0_u8; 6];
        for (slot, byte) in mac.iter_mut().zip(packet.get(0x26..0x2c).unwrap_or(&[])) {
            *slot = *byte;
        }
        Ok(Self {
            name: device_name(packet)?,
            generation: at(0x21),
            device_number: at(0x24),
            device_type: DeviceType::from_u8(at(0x34)),
            mac,
            ip: Ipv4Addr::new(at(0x2c), at(0x2d), at(0x2e), at(0x2f)),
            peers: at(0x30),
        })
    }
}

/// The status rekordbox broadcasts on port 50002 five times a second: the
/// mixer-style packet (`kind 29`), 56 bytes, carrying the master tempo and
/// a beat counter (measured from rekordbox 7.2.11; players show the tempo
/// as MASTER BPM).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub name: String,
    pub device_number: u8,
    pub bpm_x100: u16,
    /// 1 to 4, advancing with the beat; what the value means when nothing
    /// plays is `[UNKNOWN]` — rekordbox kept sending 1.
    pub beat: u8,
    /// We are the network's tempo master, driving the tempo the other
    /// players sync to. Sets two bytes rekordbox sets only as master (the
    /// status flag's master bit and the `Mm` master flag, measured
    /// 2026-09-14 from rekordbox 7.2 acting as master at 130.00 BPM).
    pub master: bool,
}

/// Byte length of a status packet.
pub const STATUS_LEN: usize = 0x38;

/// Status-port packets (50002) carry the name straight after the kind byte,
/// with no subtype: name at `0x0b`, then the fields.
fn write_status_header(out: &mut Vec<u8>, kind: u8, name: &str) {
    out.extend_from_slice(&MAGIC);
    out.push(kind);
    let mut padded = [0_u8; NAME_LEN];
    for (slot, byte) in padded.iter_mut().zip(name.as_bytes()) {
        *slot = *byte;
    }
    out.extend_from_slice(&padded);
}

impl Status {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(STATUS_LEN);
        write_status_header(&mut out, 0x29, &self.name);
        out.extend_from_slice(&[
            0x01,
            0x01,
            self.device_number,
            0x00,
            0x38,
            self.device_number,
        ]);
        // The status flag (byte `0x27`): `0xc0` playing but not master,
        // `0xe0` as tempo master — the master bit (`0x20`). The byte before
        // the tempo (`0x2c`) is `0x80` when a master tempo is being broadcast
        // and `0x00` when idle (measured: idle rekordbox sends `0x00` with
        // beat 0, playing rekordbox `0x80` with the beat). We broadcast a
        // tempo when a player on the link is master, or when we are.
        let flag = if self.master { 0xe0 } else { 0xc0 };
        let tempo_valid = if self.master || self.bpm_x100 != 0 {
            0x80
        } else {
            0x00
        };
        out.extend_from_slice(&[0x00, 0x00, flag, 0x00, 0x10, 0x00, 0x00, tempo_valid, 0x00]);
        out.extend_from_slice(&self.bpm_x100.to_be_bytes());
        // `Mm` (byte `0x34`) is `0x01` when this device is the tempo master
        // playing a rekordbox track, `0x00` otherwise.
        let mm = u8::from(self.master);
        out.extend_from_slice(&[0x00, 0x10, 0x00, 0x00, mm, 0x09, 0xff, self.beat]);
        debug_assert_eq!(out.len(), STATUS_LEN);
        out
    }
}

/// Byte length of the connect greeting.
pub const CONNECT_GREETING_LEN: usize = 0x30;

/// The packet rekordbox unicasts to a player's port 50002 when the player
/// connects (`kind 16`, 48 bytes; measured, meaning unknown).
pub fn connect_greeting(name: &str, device_number: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(CONNECT_GREETING_LEN);
    write_status_header(&mut out, 0x16, name);
    out.extend_from_slice(&[0x01, 0x01, device_number]);
    out.resize(CONNECT_GREETING_LEN, 0);
    out
}

/// Byte length of the device-identity reply.
pub const CONNECT_IDENTITY_LEN: usize = 296;

/// The kind a player sends its own identity with, on port 50002 (`10`,
/// 36 bytes: magic, the byte, the player's name). rekordbox answers it and
/// only then does the player carry on to the media query and the mount.
pub const DEVICE_IDENTITY_QUERY_KIND: u8 = 0x10;

/// rekordbox's answer to a player's `10` identity packet (`kind 11`,
/// 296 bytes, measured 2026-09-13 on the emulator's bridge): magic, the
/// byte, the 20-byte name, `01 01 <dev> 01 04 <dev> 01 00 00`, then the
/// computer's name in UTF-16BE, zero-padded. Without this reply the player
/// keeps re-sending its `10` and never lists us; it is the step the wire
/// captures had missed.
pub fn connect_identity(name: &str, device_number: u8, computer_name: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(CONNECT_IDENTITY_LEN);
    write_status_header(&mut out, 0x11, name);
    out.extend_from_slice(&[
        0x01,
        0x01,
        device_number,
        0x01,
        0x04,
        device_number,
        0x01,
        0x00,
        0x00,
    ]);
    for unit in computer_name.encode_utf16() {
        out.extend_from_slice(&unit.to_be_bytes());
    }
    out.resize(CONNECT_IDENTITY_LEN, 0);
    out
}

/// The startup ladder rekordbox 7.2.11 broadcasts before it settles into
/// keep-alives, measured on the wire (2026-09-12): three first-stage claims
/// (`00`), then a second-stage claim (`02`) for each of the six device
/// numbers it reserves — 0x11, 0x12, 0x29, 0x2a, 0x2b, 0x2c — repeated with
/// a counter 1..=6. A CDJ-3000 mounts rekordbox's library only after this
/// sequence, not from the generic virtual-CDJ ladder. `[OBS]` the numbers
/// and the shape; why rekordbox reserves that particular block is `[UNKNOWN]`.
pub const REKORDBOX_CLAIM_NUMBERS: [u8; 6] = [0x11, 0x12, 0x29, 0x2a, 0x2b, 0x2c];

/// The device types whose keep-alive brings rekordbox's link up: a player
/// (1), an older mixer (2), a DJM (3) or type 7; a keep-alive from anything
/// else does not. Original minor-0 CDJ-2000/CDJ-900 models require a known
/// wired mode (`readConfigNotify`, `linkUpFunc` in the decompilation).
pub fn brings_link_up(keep_alive: &KeepAlive) -> bool {
    brings_link_up_on(keep_alive, ConnectionMode::Unknown)
}

/// Selected-interface classification, independent of candidate device number.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionMode {
    Wired,
    Wireless,
    #[default]
    Unknown,
}

/// V2 `readReject`'s broadcast disconnect, using the pre-clear runtime
/// identity. There is no numbered-target field on the incoming rejection.
pub fn rejection_disconnect(number: u8, ip: Ipv4Addr) -> Vec<u8> {
    let mut out = Vec::with_capacity(41);
    write_header(&mut out, 0x08, 0, REKORDBOX_NAME);
    out.extend_from_slice(&[1, 3, 0, 41, number]);
    out.extend_from_slice(&ip.octets());
    out
}

/// Discovery request (`00/00`), before device-number assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovery {
    pub counter: u8,
    pub device_type: u8,
    pub mac: [u8; 6],
}

impl Discovery {
    pub fn decode(packet: &[u8]) -> Result<Self> {
        if packet.len() < 44 {
            return Err(PacketError::TooShort(packet.len()));
        }
        let kind = packet_kind(packet)?;
        if kind != 0 { return Err(PacketError::WrongKind(kind)); }
        if packet[11] != 0 { return Err(PacketError::WrongSubtype(packet[11])); }
        Ok(Self {
            counter: packet[36],
            device_type: packet[37],
            mac: [packet[38], packet[39], packet[40], packet[41], packet[42], packet[43]],
        })
    }
}

pub fn brings_link_up_on(keep_alive: &KeepAlive, mode: ConnectionMode) -> bool {
    let kind = keep_alive.device_type.to_u8();
    if ![1, 2, 3, 7].contains(&kind) {
        return false;
    }
    mode == ConnectionMode::Wired
        || !(keep_alive.generation == 0 && (keep_alive.name == "CDJ-2000" || keep_alive.name == "CDJ-900"))
}

/// One first-stage claim (`00`, 44 bytes): the counter, `04`, then the MAC.
pub fn rekordbox_claim_stage1(mac: [u8; 6], counter: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(0x2c);
    write_header(&mut out, 0x00, 0x00, REKORDBOX_NAME);
    out.extend_from_slice(&[0x01, 0x03, 0x00, 0x2c]);
    out.push(counter);
    out.push(0x04);
    out.extend_from_slice(&mac);
    debug_assert_eq!(out.len(), 0x2c);
    out
}

/// One second-stage claim (`02`, 50 bytes): the IP, the MAC, the number
/// being claimed, the counter, then `04 01`.
pub fn rekordbox_claim_stage2(mac: [u8; 6], ip: Ipv4Addr, number: u8, counter: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(0x32);
    write_header(&mut out, 0x02, 0x00, REKORDBOX_NAME);
    out.extend_from_slice(&[0x01, 0x03, 0x00, 0x32]);
    out.extend_from_slice(&ip.octets());
    out.extend_from_slice(&mac);
    out.push(number);
    out.push(counter);
    out.extend_from_slice(&[0x04, 0x01]);
    debug_assert_eq!(out.len(), 0x32);
    out
}

/// The subtype (byte `0x0b`) of a `02` packet: a probe of a number, a
/// request to be assigned one, or a block of numbers a device holds
/// (`docs/pre-release/rekordbox/link-export-internals.md`).
pub const PROBE_SUBTYPE_PROBE: u8 = 0x00;
pub const PROBE_SUBTYPE_ASSIGN: u8 = 0x01;
pub const PROBE_SUBTYPE_BLOCK: u8 = 0x02;

/// A device number probe (`02`, 50 bytes) as rekordbox 7.2.11 sends and
/// reads it: who is asking (IP, MAC), which number, which round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumberProbe {
    /// `PROBE_SUBTYPE_PROBE` or `PROBE_SUBTYPE_ASSIGN`.
    pub subtype: u8,
    pub name: String,
    pub ip: Ipv4Addr,
    pub mac: [u8; 6],
    /// The number being probed or asked for.
    pub number: u8,
    /// Byte `0x2f`: the round, counted from one.
    pub round: u8,
    pub device_type: DeviceType,
    /// Byte `0x31`: `01` when the sender takes any free number.
    pub auto_assign: bool,
}

/// Byte length of a number probe.
pub const NUMBER_PROBE_LEN: usize = 0x32;

impl NumberProbe {
    pub fn decode(packet: &[u8]) -> Result<Self> {
        if packet.len() < NUMBER_PROBE_LEN {
            return Err(PacketError::TooShort(packet.len()));
        }
        let kind = packet_kind(packet)?;
        if AnnounceKind::from_u8(kind) != AnnounceKind::ClaimStage2 {
            return Err(PacketError::WrongKind(kind));
        }
        let at = |i: usize| packet.get(i).copied().unwrap_or(0);
        if ![PROBE_SUBTYPE_PROBE, PROBE_SUBTYPE_ASSIGN].contains(&at(0x0b)) {
            return Err(PacketError::WrongSubtype(at(0x0b)));
        }
        let mut mac = [0_u8; 6];
        for (slot, byte) in mac.iter_mut().zip(packet.get(0x28..0x2e).unwrap_or(&[])) {
            *slot = *byte;
        }
        Ok(Self {
            subtype: at(0x0b),
            name: device_name(packet)?,
            ip: Ipv4Addr::new(at(0x24), at(0x25), at(0x26), at(0x27)),
            mac,
            number: at(0x2e),
            round: at(0x2f),
            device_type: DeviceType::from_u8(at(0x30)),
            auto_assign: at(0x31) == 1,
        })
    }
}

/// Subtype-2 occupancy request. It is not the 50-byte single-number probe.
/// [OBS] V1/V6 readIdBlkRequset consume masks at 0x30/0x33 and counter 0x43.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumberBlock {
    pub ip: Ipv4Addr,
    pub mac: [u8; 6],
    pub wired_mask: u8,
    pub wireless_mask: u8,
    pub counter: u8,
}

impl NumberBlock {
    pub fn decode(packet: &[u8]) -> Result<Self> {
        if packet.len() < 68 {
            return Err(PacketError::TooShort(packet.len()));
        }
        let kind = packet_kind(packet)?;
        if kind != 2 {
            return Err(PacketError::WrongKind(kind));
        }
        if packet[11] != PROBE_SUBTYPE_BLOCK {
            return Err(PacketError::WrongSubtype(packet[11]));
        }
        Ok(Self {
            ip: Ipv4Addr::new(packet[36], packet[37], packet[38], packet[39]),
            mac: [packet[40], packet[41], packet[42], packet[43], packet[44], packet[45]],
            wired_mask: packet[48],
            wireless_mask: packet[51],
            counter: packet[67],
        })
    }

    pub fn names(&self, number: u8, mode: ConnectionMode) -> bool {
        match mode {
            ConnectionMode::Wired if (17..=18).contains(&number) => self.wired_mask & (1 << (number - 17)) != 0,
            ConnectionMode::Wireless if (41..=44).contains(&number) => self.wireless_mask & (1 << (number - 41)) != 0,
            _ => false,
        }
    }
}

/// rekordbox's request to be assigned `number` (`02` subtype `01`, 50
/// bytes), sent when every number it probes is taken; the same bytes as a
/// probe but for the subtype.
pub fn rekordbox_assign_request(mac: [u8; 6], ip: Ipv4Addr, number: u8, counter: u8) -> Vec<u8> {
    let mut out = rekordbox_claim_stage2(mac, ip, number, counter);
    if let Some(subtype) = out.get_mut(0x0b) {
        *subtype = PROBE_SUBTYPE_ASSIGN;
    }
    out
}

/// The answer to a probe of a number the answering device holds (`03`
/// subtype `00`, 39 bytes): the number at `0x24`, `01` at `0x26`. rekordbox
/// both sends this for its own number and reads it to mark a number in use
/// (`readIdUseRequest`, `readIdUseResponse`). Byte `0x25` echoes the request
/// counter; the convenience constructor below uses zero.
pub const NUMBER_IN_USE_LEN: usize = 0x27;

/// The status byte (`0x26`) of a `03` reply: `01` in use / accepted.
pub const NUMBER_REPLY_IN_USE: u8 = 0x01;

pub fn number_in_use_reply(name: &str, number: u8) -> Vec<u8> {
    number_in_use_reply_with_counter(name, number, 0)
}

/// [OBS] Both subtype-0 and subtype-2 requests echo their counter in 0x25.
pub fn number_in_use_reply_with_counter(name: &str, number: u8, counter: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(NUMBER_IN_USE_LEN);
    write_header(&mut out, 0x03, PROBE_SUBTYPE_PROBE, name);
    out.extend_from_slice(&[0x01, 0x03, 0x00, 0x27]);
    out.extend_from_slice(&[number, counter, NUMBER_REPLY_IN_USE]);
    debug_assert_eq!(out.len(), NUMBER_IN_USE_LEN);
    out
}

/// A `03` reply: to a probe (subtype `00`, the number is in use) or to an
/// assign request (subtype `01`: status `0` accepts the number at `0x24`,
/// `2` asks for a retry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumberReply {
    pub subtype: u8,
    pub name: String,
    pub number: u8,
    pub status: u8,
}

impl NumberReply {
    pub fn decode(packet: &[u8]) -> Result<Self> {
        if packet.len() < NUMBER_IN_USE_LEN {
            return Err(PacketError::TooShort(packet.len()));
        }
        let kind = packet_kind(packet)?;
        if kind != 0x03 {
            return Err(PacketError::WrongKind(kind));
        }
        let at = |i: usize| packet.get(i).copied().unwrap_or(0);
        Ok(Self {
            subtype: at(0x0b),
            name: device_name(packet)?,
            number: at(0x24),
            status: at(0x26),
        })
    }
}

/// rekordbox's whole startup ladder in order: `00`×3, then `02` for each
/// reserved number with counter 1..=6 — what a join sends when no device
/// answers a probe (`rbl-link`'s beacon skips the numbers that are answered
/// for).
pub fn rekordbox_startup_ladder(mac: [u8; 6], ip: Ipv4Addr) -> Vec<Vec<u8>> {
    let mut ladder = Vec::with_capacity(3 + REKORDBOX_CLAIM_NUMBERS.len() * 6);
    for counter in 1..=3 {
        ladder.push(rekordbox_claim_stage1(mac, counter));
    }
    // Counter-major, as rekordbox 7.2.11 sends it on a cold LINK-on (measured
    // 2026-09-13): all six numbers at counter 1, then all six at counter 2, and
    // so on — not each number's six counters in turn.
    for counter in 1..=6 {
        for &number in &REKORDBOX_CLAIM_NUMBERS {
            ladder.push(rekordbox_claim_stage2(mac, ip, number, counter));
        }
    }
    ladder
}

/// A player's question about one media slot on one device (`kind 05`, 48
/// bytes, unicast to port 50002): which device, which slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaQuery {
    pub name: String,
    /// The asking device's address, which the answer goes back to.
    pub from: Ipv4Addr,
    pub device_number: u8,
    pub slot: u8,
}

/// Byte length of a media query.
pub const MEDIA_QUERY_LEN: usize = 0x30;
/// The slot number for rekordbox's library: the `Rb` slot of a status packet,
/// what a current CDJ-3000 asks about in its media query and what rekordbox
/// 7.2 names as the source in its Load Track command (both measured on the
/// wire, 2026-09-13).
pub const SLOT_REKORDBOX: u8 = 0x04;
/// The slot number an older CDJ-3000 (EP122 firmware, the emulator) asks
/// about for rekordbox's library instead: the USB slot. rekordbox answers
/// either question, naming back whichever slot was asked.
pub const SLOT_REKORDBOX_LEGACY: u8 = 0x03;

impl MediaQuery {
    pub fn decode(packet: &[u8]) -> Result<Self> {
        if packet.len() < MEDIA_QUERY_LEN {
            return Err(PacketError::TooShort(packet.len()));
        }
        let kind = packet_kind(packet)?;
        if kind != 0x05 {
            return Err(PacketError::WrongKind(kind));
        }
        let at = |i: usize| packet.get(i).copied().unwrap_or(0);
        Ok(Self {
            name: status_device_name(packet)?,
            from: Ipv4Addr::new(at(0x24), at(0x25), at(0x26), at(0x27)),
            device_number: at(0x2b),
            slot: at(0x2f),
        })
    }
}

/// rekordbox's answer to a media query about its library (`kind 06`, 192
/// bytes, unicast back to the player's port 50002; measured 2026-09-12): the
/// name again in UTF-16BE, the track and playlist counts, and the fixed
/// bytes that say the tracks are rekordbox-analysed and there are settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaResponse {
    pub name: String,
    pub device_number: u8,
    /// The slot the player asked about, named back to it: `04` to a current
    /// CDJ-3000, `03` to the EP122 emulator. A player ignores an answer
    /// about a slot it did not ask about.
    pub slot: u8,
    pub tracks: u16,
    pub playlists: u16,
}

/// Byte length of a media response.
pub const MEDIA_RESPONSE_LEN: usize = 0xc0;

impl MediaResponse {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(MEDIA_RESPONSE_LEN);
        write_status_header(&mut out, 0x06, &self.name);
        out.extend_from_slice(&[0x01, 0x01, self.device_number]);
        out.extend_from_slice(
            &u16::try_from(MEDIA_RESPONSE_LEN - 0x24)
                .unwrap_or(0)
                .to_be_bytes(),
        );
        out.extend_from_slice(&u32::from(self.device_number).to_be_bytes());
        out.extend_from_slice(&u32::from(self.slot).to_be_bytes());
        // The name as the player shows it, UTF-16BE in a 64-byte field.
        let mut utf16: Vec<u8> = self
            .name
            .encode_utf16()
            .take(31)
            .flat_map(u16::to_be_bytes)
            .collect();
        utf16.resize(0x40, 0);
        out.extend_from_slice(&utf16);
        // Creation date and the rest: zero for rekordbox.
        out.resize(0xa6, 0);
        out.extend_from_slice(&self.tracks.to_be_bytes());
        // Colour none; tracks are rekordbox's; settings present.
        out.extend_from_slice(&[0x00, 0x00, 0x01, 0x01, 0x00, 0x00]);
        out.extend_from_slice(&self.playlists.to_be_bytes());
        out.resize(MEDIA_RESPONSE_LEN, 0);
        debug_assert_eq!(out.len(), MEDIA_RESPONSE_LEN);
        out
    }
}

/// The device-property request an XDJ-RX3 sends before it makes a rekordbox
/// PC visible in SOURCE (`kind 30`, 36 bytes, unicast to port 50002).
///
/// Unlike a CDJ-3000 media query, this packet does not name a media slot or
/// target device. The destination address identifies the rekordbox peer; the
/// packet names only the requesting logical deck.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevicePropertyQuery {
    pub name: String,
    pub requester: u8,
}

pub const DEVICE_PROPERTY_QUERY_KIND: u8 = 0x30;
pub const DEVICE_PROPERTY_QUERY_LEN: usize = 0x24;

impl DevicePropertyQuery {
    pub fn decode(packet: &[u8]) -> Result<Self> {
        if packet.len() < DEVICE_PROPERTY_QUERY_LEN {
            return Err(PacketError::TooShort(packet.len()));
        }
        let kind = packet_kind(packet)?;
        if kind != DEVICE_PROPERTY_QUERY_KIND {
            return Err(PacketError::WrongKind(kind));
        }
        Ok(Self {
            name: status_device_name(packet)?,
            requester: packet.get(0x21).copied().unwrap_or(0),
        })
    }
}

/// Rekordbox's answer to an RX3 device-property request (`kind 31`, 44
/// bytes, unicast to the RX3's status port). Firmware 1.19 uses this reply to
/// mark the PC-backed media detected; a registered peer without it stays out
/// of the SOURCE list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevicePropertyResponse {
    pub name: String,
    pub device_number: u8,
}

pub const DEVICE_PROPERTY_RESPONSE_KIND: u8 = 0x31;
pub const DEVICE_PROPERTY_RESPONSE_LEN: usize = 0x2c;

impl DevicePropertyResponse {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(DEVICE_PROPERTY_RESPONSE_LEN);
        write_status_header(&mut out, DEVICE_PROPERTY_RESPONSE_KIND, &self.name);
        out.extend_from_slice(&[
            0x01,
            0x03,
            self.device_number,
            0x00,
            0x08,
            0x06,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
        ]);
        debug_assert_eq!(out.len(), DEVICE_PROPERTY_RESPONSE_LEN);
        out
    }
}

/// A player's request for the media source's device display settings.
pub const DEVICE_SETTINGS_REQUEST_KIND: u8 = 0x46;
/// The response carrying the media source's device display settings.
pub const DEVICE_SETTINGS_RESPONSE_KIND: u8 = 0x47;
/// Byte length of a device-settings response.
pub const DEVICE_SETTINGS_RESPONSE_LEN: usize = 0x48;

/// Type of the overview waveform shown on a player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OverviewWaveform {
    #[default]
    Half,
    Full,
}

/// Color palette used for player waveforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WaveformColor {
    Blue,
    Rgb,
    #[default]
    #[serde(rename = "3band")]
    ThreeBand,
}

/// Notation used when a player displays musical keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyDisplay {
    #[default]
    Classic,
    Alphanumeric,
}

/// Position of the play marker in the scrolling waveform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WaveformPosition {
    #[default]
    Center,
    Left,
}

/// The complete decoded settings in the 32-byte `DEVSETTING` body.
///
/// Body bytes 9, 10, 12 and 13 carry the four user-facing values. The
/// remaining bytes are framing, reserved bytes or customization markers whose
/// semantics are not established. The encoder preserves rekordbox's captured
/// Link values for them rather than assigning speculative settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSettings {
    pub overview_waveform: OverviewWaveform,
    pub waveform_color: WaveformColor,
    pub key_display: KeyDisplay,
    pub waveform_position: WaveformPosition,
}

impl DeviceSettings {
    fn body(self) -> [u8; 32] {
        let mut body = [0; 32];
        body[0..4].copy_from_slice(&0x1234_5678_u32.to_le_bytes());
        body[4..8].copy_from_slice(&1_u32.to_le_bytes());
        body[8] = 1;
        body[9] = match self.overview_waveform {
            OverviewWaveform::Half => 1,
            OverviewWaveform::Full => 2,
        };
        body[10] = match self.waveform_color {
            WaveformColor::Blue => 1,
            WaveformColor::Rgb => 3,
            WaveformColor::ThreeBand => 4,
        };
        body[11] = 1;
        body[12] = match self.key_display {
            KeyDisplay::Classic => 1,
            KeyDisplay::Alphanumeric => 2,
        };
        body[13] = match self.waveform_position {
            WaveformPosition::Center => 1,
            WaveformPosition::Left => 2,
        };
        // The captured rekordbox Link response carries `2` here. USB files
        // vary at bytes 14-16 after category, sort and color-list edits, but
        // no stable meaning has been demonstrated for those markers.
        body[16] = 2;
        body
    }
}

/// rekordbox's response to a device-settings request (`kind 47`, 72 bytes,
/// unicast). The 32-byte body is the same logical structure stored in
/// `DEVSETTING.DAT`; integer fields use network byte order on Link.
pub fn device_settings_response(
    name: &str,
    device_number: u8,
    settings: DeviceSettings,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(DEVICE_SETTINGS_RESPONSE_LEN);
    write_status_header(&mut out, DEVICE_SETTINGS_RESPONSE_KIND, name);
    out.extend_from_slice(&[
        0x01,
        0x01,
        device_number,
        0x00,
        0x24,
        device_number,
        0x04,
        0x00,
        0x00,
    ]);
    let body = settings.body();
    out.extend_from_slice(&u32::from_le_bytes([body[0], body[1], body[2], body[3]]).to_be_bytes());
    out.extend_from_slice(&u32::from_le_bytes([body[4], body[5], body[6], body[7]]).to_be_bytes());
    out.extend_from_slice(&body[8..24]);
    out.extend_from_slice(&u16::from_le_bytes([body[24], body[25]]).to_be_bytes());
    out.extend_from_slice(&u16::from_le_bytes([body[26], body[27]]).to_be_bytes());
    out.extend_from_slice(
        &u32::from_le_bytes([body[28], body[29], body[30], body[31]]).to_be_bytes(),
    );
    debug_assert_eq!(out.len(), DEVICE_SETTINGS_RESPONSE_LEN);
    out
}

/// The kind of a player's status packet on port 50002.
pub const PLAYER_STATUS_KIND: u8 = 0x0a;

/// The kind of a beat packet, broadcast to port 50001 on each beat by the
/// tempo master.
pub const BEAT_KIND: u8 = 0x28;

/// The kind of a master-handoff request, sent to the current tempo master on
/// port 50001 to ask it to yield.
pub const MASTER_HANDOFF_REQUEST_KIND: u8 = 0x26;

/// Byte length of a beat packet.
pub const BEAT_LEN: usize = 0x60;

/// The beat packet the tempo master broadcasts to port 50001 on every beat:
/// `kind 28`, 96 bytes, saying when the next several beats and bars fall so
/// a synced player can lock to the master's tempo and downbeat.
///
/// Byte for byte what rekordbox 7.2 broadcasts as master (captured
/// 2026-09-14 at 130.00 BPM across a whole bar and verified live against
/// the packets rbxport itself puts on the wire; pinned in the tests).
///
/// `bar_beat` is the beat within the bar, 1 to 4 (the downbeat is 1). The
/// six timing fields are `floor(k · 6_000_000 / bpm_x100)` milliseconds
/// until the k-th upcoming beat, for k of 1, 2, 5−beat (the next bar), 4,
/// 9−beat (the bar after) and 8.
pub fn beat_packet(name: &str, device_number: u8, bpm_x100: u16, bar_beat: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(BEAT_LEN);
    write_status_header(&mut out, BEAT_KIND, name);
    out.extend_from_slice(&[0x01, 0x01, device_number, 0x00, 0x3c]);
    let beat = bar_beat.clamp(1, 4);
    let bpm = u32::from(bpm_x100.max(1));
    let offset = |k: u32| ((k * 6_000_000) / bpm).to_be_bytes();
    for k in [1, 2, u32::from(5 - beat), 4, u32::from(9 - beat), 8] {
        out.extend_from_slice(&offset(k));
    }
    out.extend_from_slice(&[0xff; 24]);
    // Pitch fixed at +0% (`0x00100000`); the tempo is carried by `bpm_x100`.
    out.extend_from_slice(&0x0010_0000_u32.to_be_bytes());
    out.extend_from_slice(&[0x00, 0x00]);
    out.extend_from_slice(&bpm_x100.to_be_bytes());
    out.extend_from_slice(&[beat, 0x00, 0x00, device_number]);
    debug_assert_eq!(out.len(), BEAT_LEN);
    out
}

/// The kind of the Load Track command rekordbox sends to a CDJ.
pub const LOAD_TRACK_KIND: u8 = 0x19;

/// The kind of a player's Load Track response; status semantics are unknown.
pub const LOAD_TRACK_ACK_KIND: u8 = 0x1a;

/// Raw fields of the evidenced 40-byte `MusicDD` response. No field is named
/// "accepted": neither its enum meaning nor request correlation is proved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadTrackResponse {
    pub name: String,
    pub fields: [u8; 4],
}

impl LoadTrackResponse {
    pub fn decode(packet: &[u8]) -> Result<Self> {
        if packet.len() < 40 { return Err(PacketError::TooShort(packet.len())); }
        let kind = packet_kind(packet)?;
        if kind != LOAD_TRACK_ACK_KIND { return Err(PacketError::WrongKind(kind)); }
        let length = u16::from_be_bytes([packet[34], packet[35]]);
        if length != 4 { return Err(PacketError::WrongLength(length)); }
        Ok(Self {
            name: status_device_name(packet)?,
            fields: [packet[36], packet[37], packet[38], packet[39]],
        })
    }
}

/// Byte length of a Load Track command.
pub const LOAD_TRACK_LEN: usize = 0x58;

/// The track type a player reports for a track in a rekordbox library, as in
/// its status packet.
pub const TRACK_TYPE_REKORDBOX: u8 = 0x01;

/// Tells a player to load a specific track from our library: `kind 19`,
/// 88 bytes, unicast to the player on port 50002.
///
/// Byte for byte what rekordbox 7.2 sends a CDJ-3000 (captured 2026-09-13,
/// pinned in the tests): the status-packet header, `01 01`, our number, the
/// length `0034` of what follows, our number again, then the track named as
/// coming from our own device's rekordbox slot as a rekordbox track. The
/// djl-analysis "Loading Tracks" figure differs in the two bytes rekordbox
/// sets (`01` at `0x20`, `32` at `0x4b`) and shows the USB slot, `03`, as
/// its example source; a real CDJ-3000 answers `03` from rekordbox's address
/// with a media query about slot `04` rather than a load, so the slot is not
/// a free choice.
///
/// The player answers `1a` within a few milliseconds and mounts our export
/// to fetch the track. It only does so for a command whose source address is
/// the one our keep-alives announce: sent from another address of the same
/// machine it does nothing at all (measured 2026-09-13 on a CDJ-3000).
///
/// `from_device` is our device number (`0x11` as rekordbox), `to_device` the
/// player number to load onto, `track_id` the library ID.
pub fn load_track_command(name: &str, from_device: u8, to_device: u8, track_id: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(LOAD_TRACK_LEN);
    write_status_header(&mut out, LOAD_TRACK_KIND, name);
    // 0x1f..0x24: 01, subtype, our number, then the length of what follows.
    out.extend_from_slice(&[0x01, 0x01, from_device, 0x00, 0x34]);
    // 0x24..0x2c: our number, padding, the source device, slot and track type.
    out.extend_from_slice(&[
        from_device,
        0x00,
        0x00,
        0x00,
        from_device,
        SLOT_REKORDBOX,
        TRACK_TYPE_REKORDBOX,
        0x00,
    ]);
    // 0x2c..0x30: the track.
    out.extend_from_slice(&track_id.to_be_bytes());
    out.resize(LOAD_TRACK_LEN, 0);
    out[0x33] = 0x32;
    // The player to load onto, counted from zero here; a player accepts the
    // command by address regardless.
    out[0x40] = to_device.saturating_sub(1);
    out[0x4b] = 0x32;
    debug_assert_eq!(out.len(), LOAD_TRACK_LEN);
    out
}

/// A device-number claim, sent three times in each of three stages at startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub stage: AnnounceKind,
    pub name: String,
    pub device_number: u8,
    pub mac: [u8; 6],
    pub ip: Ipv4Addr,
    /// 1..=3, which of the three repeats this is.
    pub repeat: u8,
    pub device_type: DeviceType,
}

impl Claim {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(0x30);
        write_header(&mut out, self.stage.to_u8(), 0x00, &self.name);
        out.push(self.repeat);
        out.push(self.device_type.to_u8());
        // Stage 1 carries only the MAC; stages 2 and 3 add the address being
        // claimed, so they are four bytes longer and declare it.
        if self.stage == AnnounceKind::ClaimStage1 {
            out.extend_from_slice(&0x002c_u16.to_be_bytes());
            out.extend_from_slice(&self.mac);
        } else {
            out.extend_from_slice(&0x0032_u16.to_be_bytes());
            out.extend_from_slice(&self.ip.octets());
            out.extend_from_slice(&self.mac);
            out.push(self.device_number);
            out.push(0x01);
        }
        out
    }
}

/// The initial "I am here" announcement (`kind 0a`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announcement {
    pub name: String,
    pub device_type: DeviceType,
}

impl Announcement {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(0x25);
        write_header(&mut out, AnnounceKind::Announce.to_u8(), 0x00, &self.name);
        out.push(0x01);
        out.push(self.device_type.to_u8());
        out.extend_from_slice(&0x0025_u16.to_be_bytes());
        out.push(0x01);
        out
    }
}

/// A device heard on the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    pub name: String,
    pub device_number: u8,
    pub device_type: DeviceType,
    pub ip: Ipv4Addr,
    /// Hardware identity advertised by the keep-alive.
    pub mac: [u8; 6],
    /// Raw membership flag bytes from keep-alive offsets 0x25/0x35.
    /// Their meanings are unknown; generic typed observations have no raw flags.
    pub member_flags: Option<[u8; 2]>,
    /// Milliseconds since this device was last heard from.
    pub last_seen_ms: u64,
}

/// Tracks who is on the network.
///
/// A device that has gone quiet for three keep-alive intervals is dropped: a
/// player unplugged mid-set stops announcing rather than saying goodbye.
#[derive(Debug, Default)]
pub struct DeviceTable {
    peers: Vec<Peer>,
    /// Pending per-slot ageing clocks, armed only by received keep-alives.
    /// Explicit membership removal does not stop these timers (V1/V2);
    /// synthesis neither arms nor refreshes them, even after recreation.
    received_at: std::collections::HashMap<u8, u64>,
}

/// How long a peer may be silent before it is considered gone.
pub const PEER_TIMEOUT_MS: u64 = KEEP_ALIVE_INTERVAL_MS * 3;

impl DeviceTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a keep-alive at a given time.
    pub fn observe(&mut self, keep_alive: &KeepAlive, now_ms: u64) {
        self.record(keep_alive, now_ms, true, None);
        self.received_at.insert(keep_alive.device_number, now_ms);
    }

    /// Records a received keep-alive with its validated raw membership flags.
    pub fn observe_with_flags(&mut self, keep_alive: &KeepAlive, now_ms: u64, flags: [u8; 2]) {
        self.record(keep_alive, now_ms, true, Some(flags));
        self.received_at.insert(keep_alive.device_number, now_ms);
    }

    /// Records a synthesized identity without arming or refreshing its timer.
    /// Any pending slot timer survives, including after member recreation.
    pub fn observe_synthetic(&mut self, keep_alive: &KeepAlive, now_ms: u64, flags: Option<[u8; 2]>) {
        self.record(keep_alive, now_ms, false, flags);
    }

    fn record(&mut self, keep_alive: &KeepAlive, now_ms: u64, received: bool, flags: Option<[u8; 2]>) {
        if let Some(existing) = self
            .peers
            .iter_mut()
            .find(|p| p.device_number == keep_alive.device_number)
        {
            existing.name.clear();
            existing.name.push_str(&keep_alive.name);
            existing.device_type = keep_alive.device_type;
            existing.ip = keep_alive.ip;
            existing.mac = keep_alive.mac;
            existing.member_flags = flags;
            if received || !self.received_at.contains_key(&keep_alive.device_number) {
                existing.last_seen_ms = now_ms;
            }
            return;
        }
        self.peers.push(Peer {
            name: keep_alive.name.clone(),
            device_number: keep_alive.device_number,
            device_type: keep_alive.device_type,
            ip: keep_alive.ip,
            mac: keep_alive.mac,
            member_flags: flags,
            last_seen_ms: if received { now_ms } else {
                self.received_at.get(&keep_alive.device_number).copied().unwrap_or(now_ms)
            },
        });
    }

    /// Drops peers that have gone quiet, returning how many were removed.
    pub fn expire(&mut self, now_ms: u64) -> usize {
        self.expire_received(now_ms).len()
    }

    /// Removes active members whose received per-slot keep-alive clock expired.
    /// Returns their numbers so Link can apply its evidenced paired removal.
    pub fn expire_received(&mut self, now_ms: u64) -> Vec<u8> {
        // V3 timerFuncAging stops the indexed timer before checking whether
        // its current slot is active. Consume inactive deadlines too, or a
        // later virtual recreation would inherit an already-fired timer.
        let mut expired_slots = Vec::new();
        self.received_at.retain(|number, seen| {
            if now_ms.saturating_sub(*seen) > PEER_TIMEOUT_MS {
                expired_slots.push(*number);
                false
            } else { true }
        });
        let expired: Vec<_> = self.peers.iter()
            .filter(|peer| expired_slots.contains(&peer.device_number))
            .map(|peer| peer.device_number).collect();
        for number in &expired {
            self.remove_number(*number);
        }
        expired
    }

    /// A live logical identity with no pending received keep-alive timer.
    /// A recreated virtual identity can still inherit an armed slot timer.
    pub fn is_synthetic(&self, number: u8) -> bool {
        self.peers.iter().any(|peer| peer.device_number == number)
            && !self.received_at.contains_key(&number)
    }

    pub fn peers(&self) -> &[Peer] {
        &self.peers
    }

    /// Removes exactly one logical member, independently of shared IPs.
    /// Its pending slot timer remains armed until expiry or session clearing.
    pub fn remove_number(&mut self, number: u8) -> Option<Peer> {
        let index = self.peers.iter().position(|peer| peer.device_number == number)?;
        Some(self.peers.remove(index))
    }

    pub fn len(&self) -> usize {
        self.peers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    /// A device number not already in use, for claiming one at startup.
    pub fn free_device_number(&self, preferred: u8) -> u8 {
        if !self.peers.iter().any(|p| p.device_number == preferred) {
            return preferred;
        }
        // rekordbox-style numbers live above the player range.
        (0x11..=0x20)
            .find(|candidate| !self.peers.iter().any(|p| p.device_number == *candidate))
            .unwrap_or(preferred)
    }
}
