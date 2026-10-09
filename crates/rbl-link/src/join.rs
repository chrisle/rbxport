//! Joining the link the way rekordbox 7.2.11 does, as a state machine with
//! no socket of its own: the beacon feeds it what arrives on the announce
//! port and sends what it hands back.
//!
//! From the decompilation of rekordbox's system manager
//! (`docs/pre-release/rekordbox/link-export-internals.md`, "Joining the
//! network, exactly"):
//!
//! 1. Nothing is announced into an empty network. The link comes up on the
//!    first keep-alive from a player or mixer ([`rbl_prolink::brings_link_up`]).
//! 2. Discovery: three first-stage claims (`00`), 100 ms apart.
//! 3. The number probe: a second-stage packet (`02`) every 100 ms for each
//!    of the six numbers rekordbox reserves — 17, 18, 41, 42, 43, 44 —
//!    six rounds through, skipping numbers a device has answered for with
//!    `03` (in use).
//! 4. The choice: 17 or 18, matching rekordbox's wired Link identity.
//!    With none free, an assign request (`02` subtype `01`) is sent up to six
//!    times, waiting for a `03` subtype `01` that accepts.
//! 5. Running: keep-alives every two seconds with the number, and a `03`
//!    for anyone probing it.
//!
//! There is no third-stage (`04`) claim on this path.

use std::time::{Duration, Instant};

use rbl_prolink::{
    number_in_use_reply_with_counter, rekordbox_assign_request, rekordbox_claim_stage1, rekordbox_claim_stage2,
    ConnectionMode, DeviceType, KeepAlive, NumberBlock, NumberProbe, NumberReply, NUMBER_REPLY_IN_USE, PROBE_SUBTYPE_ASSIGN,
    PROBE_SUBTYPE_PROBE, REKORDBOX_CLAIM_NUMBERS, REKORDBOX_NAME,
};

/// The discovery and probe timer, and the spacing of assign requests.
pub const TICK: Duration = Duration::from_millis(100);
/// First-stage claims sent.
const DISCOVERY_SENDS: u8 = 3;
/// Rounds through the six numbers.
const PROBE_ROUNDS: u8 = 6;
/// Assign requests sent before giving up.
const ASSIGN_SENDS: u8 = 6;
/// rekordbox's wired candidates, in the order it takes them.
const PREFERRED_CHOICES: [u8; 2] = [0x11, 0x12];

/// Where the join is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Listening; no player or mixer heard yet.
    Waiting,
    /// First-stage claims going out.
    Discovery { sent: u8 },
    /// Probing the six numbers, `round` from one, `index` into the six.
    Probing { round: u8, index: usize },
    /// Every number answered for; asking to be assigned one.
    Assigning { sent: u8 },
    /// On the link with a number.
    Running { number: u8 },
    /// Given up, with why.
    Failed(String),
}

/// What the machine wants sent: to the broadcast address unless a peer is
/// named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    pub packet: Vec<u8>,
    pub to: Option<std::net::Ipv4Addr>,
    /// For the log.
    pub what: &'static str,
}

/// The join, from the first keep-alive to a settled number.
#[derive(Debug)]
pub struct Join {
    mode: ConnectionMode,
    mac: [u8; 6],
    ip: std::net::Ipv4Addr,
    state: State,
    /// Which of the six numbers a device answered for, by index.
    in_use: [bool; 6],
    next_at: Instant,
    /// Runtime announcement identity is not database serving readiness.
    configured_number: u8,
    runtime_ip: std::net::Ipv4Addr,
    /// Network access keeps its selected address after runtime clearing.
    network_ip: Option<std::net::Ipv4Addr>,
    rejected: bool,
    /// V1's runtime +0x1a1 gate, distinct from selected interface mode.
    exclude_original_models: bool,
    /// V1 +0x1aa: the peer type of the first successful `LinkUp`, not the
    /// selected interface mode or the type of a later announcement.
    all_in_one_session: Option<bool>,
}

impl Join {
    pub fn new(mac: [u8; 6], ip: std::net::Ipv4Addr, now: Instant) -> Self {
        Self {
            mode: ConnectionMode::Unknown,
            mac,
            ip,
            state: State::Waiting,
            in_use: [false; 6],
            next_at: now,
            configured_number: 17,
            runtime_ip: std::net::Ipv4Addr::UNSPECIFIED,
            network_ip: None,
            rejected: false,
            exclude_original_models: false,
            all_in_one_session: None,
        }
    }

    pub const fn state(&self) -> &State {
        &self.state
    }

    #[must_use]
    pub fn with_mode(mut self, mode: ConnectionMode) -> Self {
        self.mode = mode;
        self
    }

    #[cfg(test)]
    pub(crate) fn test_state(mut self, state: State) -> Self {
        self.state = state;
        self
    }

    /// Our number, once there is one.
    pub const fn number(&self) -> Option<u8> {
        match self.state {
            State::Running { number } => Some(number),
            _ => None,
        }
    }

    pub(crate) const fn rejected(&self) -> bool { self.rejected }
    #[cfg(test)]
    pub(crate) const fn network_initialized(&self) -> bool { self.network_ip.is_some() }
    pub(crate) const fn excludes_original_models(&self) -> bool { self.exclude_original_models }

    /// V5 messageReceived's guard precedes every announcement dispatch.
    /// Before `NetIF` setup there is no subnet restriction; Unknown does
    /// not initialize that cache under RBX's existing evidence boundary.
    pub(crate) fn announcement_sender_allowed(&self, sender: std::net::Ipv4Addr, mask: std::net::Ipv4Addr) -> bool {
        let own = self.network_ip.unwrap_or(std::net::Ipv4Addr::UNSPECIFIED);
        sender != own && (own.octets()[0] == 0
            || u32::from(sender) & u32::from(mask) == u32::from(own) & u32::from(mask))
    }

    /// V1 Running (6/8) excludes the other session's numbered deck range.
    pub(crate) fn allows_running_member(&self, number: u8) -> bool {
        if !matches!(self.state, State::Running { .. }) { return true; }
        match self.all_in_one_session {
            Some(false) => !(9..=12).contains(&number),
            Some(true) => !(1..=4).contains(&number),
            None => true, // No actual successful LinkUp history (test-only Running fixtures).
        }
    }

    pub(crate) fn reject(&mut self, now: Instant) -> Option<Outgoing> {
        if self.mode == ConnectionMode::Unknown { return None; }
        let number = self.number().unwrap_or(self.configured_number);
        let packet = rbl_prolink::rejection_disconnect(number, self.runtime_ip);
        let can_send = self.network_ip.is_some_and(|ip| ip.octets()[0] != 0);
        self.rejected = true;
        // V2 clears the runtime mode byte to zero, which V1 frameRead uses
        // for its outer model exclusion even when selected mode is wired.
        self.exclude_original_models = true;
        self.state = State::Waiting;
        self.runtime_ip = std::net::Ipv4Addr::UNSPECIFIED;
        self.in_use = [false; 6];
        self.next_at = now;
        can_send.then_some(Outgoing { packet, to: None, what: "rejection disconnect" })
    }

    /// Back to listening, as rekordbox goes on a compatibility response or
    /// a link-down: the number is given up.
    pub fn reset(&mut self, now: Instant) {
        // A compatibility reset is not evidence that the rejection latch
        // clears. Only a new RBX Join instance starts without that latch.
        if self.rejected { return; }
        self.state = State::Waiting;
        self.in_use = [false; 6];
        self.all_in_one_session = None;
        self.next_at = now;
    }

    /// A keep-alive from another device: the first from a player or mixer
    /// starts the join.
    pub fn hear_keep_alive(&mut self, keep_alive: &KeepAlive, now: Instant) {
        if self.rejected { return; }
        if keep_alive.ip == self.ip && keep_alive.mac == self.mac {
            return;
        }
        if !(1..=80).contains(&keep_alive.device_number) || !(1..=9).contains(&keep_alive.device_type.to_u8()) {
            return;
        }
        if matches!(self.state, State::Probing { .. }) {
            if let Some(index) = REKORDBOX_CLAIM_NUMBERS.iter().position(|number| *number == keep_alive.device_number) {
                self.in_use[index] = true;
            }
        }
        if self.state == State::Waiting
            && rbl_prolink::brings_link_up_on(keep_alive, ConnectionMode::Wired)
            && self.mode != ConnectionMode::Unknown
        {
            // V1 linkUpFunc sets the interface, IP and configured candidate
            // before it excludes original minor-0 players on wireless.
            self.network_ip = Some(self.ip);
            self.runtime_ip = self.ip;
            self.configured_number = if self.mode == ConnectionMode::Wireless { 41 } else { 17 };
            self.exclude_original_models = self.mode == ConnectionMode::Wireless;
        }
        if self.state == State::Waiting && rbl_prolink::brings_link_up_on(keep_alive, self.mode) {
            tracing::info!(
                number = keep_alive.device_number,
                name = %keep_alive.name,
                ip = %keep_alive.ip,
                "first player or mixer heard; joining the link"
            );
            self.state = State::Discovery { sent: 0 };
            self.all_in_one_session = Some(keep_alive.device_type == DeviceType::AllInOne);
            self.next_at = now;
        }
    }

    /// A `03` reply: to a probe, marks the number in use; to an assign
    /// request, settles or retries it.
    pub fn hear_reply(&mut self, reply: &NumberReply) {
        if self.rejected { return; }
        match (&self.state, reply.subtype) {
            (State::Probing { .. }, PROBE_SUBTYPE_PROBE) if reply.status == NUMBER_REPLY_IN_USE => {
                if let Some(index) = REKORDBOX_CLAIM_NUMBERS
                    .iter()
                    .position(|&n| n == reply.number)
                {
                    if !self.in_use[index] {
                        tracing::debug!(number = reply.number, holder = %reply.name, "device number in use");
                    }
                    self.in_use[index] = true;
                }
            }
            (State::Assigning { .. }, PROBE_SUBTYPE_ASSIGN) => match reply.status {
                0 => {
                    tracing::info!(number = reply.number, by = %reply.name, "device number assigned");
                    self.state = State::Running {
                        number: reply.number,
                    };
                }
                status => tracing::debug!(status, "assign request answered with a retry"),
            },
            _ => tracing::trace!(
                subtype = reply.subtype,
                status = reply.status,
                "number reply not for this state"
            ),
        }
    }

    /// A `02` from another device: a probe of our number, or a block that
    /// names it, is answered with `03` (in use).
    pub fn hear_probe(&mut self, probe: &NumberProbe) -> Option<Outgoing> {
        if self.rejected { return None; }
        let State::Running { number } = self.state else {
            return None;
        };
        if probe.ip == self.ip && probe.mac == self.mac {
            return None; // our own, echoed back
        }
        let names_ours = probe.subtype == PROBE_SUBTYPE_PROBE && probe.number == number;
        if !names_ours {
            return None;
        }
        tracing::debug!(number, prober = %probe.name, ip = %probe.ip, "our number probed; answering in use");
        Some(Outgoing {
            packet: number_in_use_reply_with_counter(REKORDBOX_NAME, number, probe.round),
            to: Some(probe.ip),
            what: "number in use",
        })
    }

    /// The distinct subtype-2 occupancy masks, valid only while running.
    pub fn hear_block(&mut self, block: &NumberBlock) -> Option<Outgoing> {
        if self.rejected { return None; }
        let State::Running { number } = self.state else {
            return None;
        };
        if (block.ip == self.ip && block.mac == self.mac) || !block.names(number, self.mode) {
            return None;
        }
        Some(Outgoing {
            packet: number_in_use_reply_with_counter(REKORDBOX_NAME, number, block.counter),
            to: Some(block.ip),
            what: "number in use",
        })
    }

    /// What to send now, if the timer is due.
    pub fn tick(&mut self, now: Instant) -> Option<Outgoing> {
        if self.rejected { return None; }
        if now < self.next_at {
            return None;
        }
        match self.state.clone() {
            State::Waiting | State::Running { .. } | State::Failed(_) => None,
            State::Discovery { sent } => {
                self.next_at = now + TICK;
                let counter = sent + 1;
                self.state = if counter >= DISCOVERY_SENDS {
                    State::Probing { round: 1, index: 0 }
                } else {
                    State::Discovery { sent: counter }
                };
                Some(Outgoing {
                    packet: rekordbox_claim_stage1(self.mac, counter),
                    to: None,
                    what: "first-stage claim",
                })
            }
            State::Probing { round, index } => {
                self.next_at = now + TICK;
                // The next free number from here in this round, or the next
                // round; six rounds through and the number is chosen.
                let mut round = round;
                let mut index = index;
                loop {
                    if round > PROBE_ROUNDS {
                        return self.choose(now);
                    }
                    if index >= REKORDBOX_CLAIM_NUMBERS.len() {
                        round += 1;
                        index = 0;
                        continue;
                    }
                    if self.in_use[index] {
                        index += 1;
                        continue;
                    }
                    break;
                }
                let number = REKORDBOX_CLAIM_NUMBERS[index];
                self.state = State::Probing {
                    round,
                    index: index + 1,
                };
                Some(Outgoing {
                    packet: rekordbox_claim_stage2(self.mac, self.ip, number, round),
                    to: None,
                    what: "number probe",
                })
            }
            State::Assigning { sent } => {
                if sent >= ASSIGN_SENDS {
                    let why = "every device number rekordbox reserves is in use, and nothing assigned one".to_owned();
                    tracing::error!("{why}");
                    self.state = State::Failed(why);
                    return None;
                }
                self.next_at = now + TICK;
                self.state = State::Assigning { sent: sent + 1 };
                Some(Outgoing {
                    packet: rekordbox_assign_request(self.mac, self.ip, PREFERRED_CHOICES[0], sent + 1),
                    to: None,
                    what: "assign request",
                })
            }
        }
    }

    /// After the sixth round: take the first free wired identity, else ask.
    fn choose(&mut self, now: Instant) -> Option<Outgoing> {
        let free = PREFERRED_CHOICES.iter().copied().find(|choice| {
            REKORDBOX_CLAIM_NUMBERS
                .iter()
                .position(|n| n == choice)
                .is_some_and(|i| !self.in_use[i])
        });
        if let Some(number) = free {
            tracing::info!(number, "device number settled; on the link");
            self.state = State::Running { number };
            None
        } else {
            tracing::warn!("17 and 18 are in use; asking to be assigned a number");
            self.state = State::Assigning { sent: 0 };
            self.next_at = now;
            self.tick(now)
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use rbl_prolink::{DeviceType, number_in_use_reply};

    const MAC: [u8; 6] = [0, 0xe0, 0x4c, 0xcf, 0x63, 0x2e];
    const IP: std::net::Ipv4Addr = std::net::Ipv4Addr::new(192, 168, 1, 14);

    fn player() -> KeepAlive {
        KeepAlive {
            name: "CDJ-3000".to_owned(),
            device_number: 1,
            device_type: DeviceType::Cdj,
            mac: [0x24, 0x97, 0xed, 0x0b, 0x40, 0x43],
            ip: std::net::Ipv4Addr::new(192, 168, 1, 152),
            peers: 0,
            generation: 3,
        }
    }

    /// Runs the timer forward, collecting what goes out, until the state
    /// stops changing or `limit` packets have gone.
    fn run(join: &mut Join, mut now: Instant, limit: usize) -> (Vec<Outgoing>, Instant) {
        let mut out = Vec::new();
        while out.len() < limit {
            match join.tick(now) {
                Some(packet) => out.push(packet),
                None if matches!(
                    join.state(),
                    State::Running { .. } | State::Failed(_) | State::Waiting
                ) =>
                {
                    break
                }
                None => now += TICK,
            }
        }
        (out, now)
    }

    #[test]
    fn nothing_goes_out_until_a_player_is_heard() {
        let now = Instant::now();
        let mut join = Join::new(MAC, IP, now);
        assert_eq!(join.tick(now + Duration::from_secs(5)), None);
        assert_eq!(join.state(), &State::Waiting);

        let mut rekordbox = player();
        rekordbox.device_type = DeviceType::Rekordbox;
        join.hear_keep_alive(&rekordbox, now);
        assert_eq!(
            join.state(),
            &State::Waiting,
            "another rekordbox does not bring the link up"
        );

        let mut old = player();
        old.name = "CDJ-2000".to_owned();
        old.generation = 0;
        join.hear_keep_alive(&old, now);
        assert_eq!(
            join.state(),
            &State::Waiting,
            "a CDJ-2000 at minor version 0 is refused"
        );

        join.hear_keep_alive(&player(), now);
        assert_eq!(join.state(), &State::Discovery { sent: 0 });
    }

    #[test]
    fn the_join_is_three_claims_then_six_rounds_of_six_probes_then_seventeen() {
        let now = Instant::now();
        let mut join = Join::new(MAC, IP, now);
        join.hear_keep_alive(&player(), now);
        let (out, _) = run(&mut join, now, 100);
        assert_eq!(out.len(), 3 + 36);
        assert_eq!(out[0].packet, rekordbox_claim_stage1(MAC, 1));
        assert_eq!(out[2].packet, rekordbox_claim_stage1(MAC, 3));
        // Counter-major: all six numbers at round 1, then round 2.
        assert_eq!(out[3].packet, rekordbox_claim_stage2(MAC, IP, 0x11, 1));
        assert_eq!(out[8].packet, rekordbox_claim_stage2(MAC, IP, 0x2c, 1));
        assert_eq!(out[9].packet, rekordbox_claim_stage2(MAC, IP, 0x11, 2));
        assert_eq!(out[38].packet, rekordbox_claim_stage2(MAC, IP, 0x2c, 6));
        assert!(out.iter().all(|o| o.to.is_none()), "all broadcast");
        assert_eq!(join.state(), &State::Running { number: 0x11 });
        assert_eq!(join.number(), Some(0x11));
    }

    #[test]
    fn a_number_answered_for_is_skipped_and_eighteen_is_taken_when_seventeen_is_held() {
        let now = Instant::now();
        let mut join = Join::new(MAC, IP, now);
        join.hear_keep_alive(&player(), now);
        // Through discovery and the first probe of 17.
        let (out, now) = run(&mut join, now, 4);
        assert_eq!(out[3].packet[0x2e], 0x11);
        let reply = NumberReply::decode(&number_in_use_reply("rekordbox", 0x11)).unwrap();
        join.hear_reply(&reply);
        let (rest, _) = run(&mut join, now, 100);
        // The rest of round one without 17, then five rounds of five.
        assert_eq!(rest.len(), 5 + 5 * 5);
        assert!(
            rest.iter().all(|o| o.packet[0x2e] != 0x11),
            "17 is not probed again"
        );
        assert_eq!(join.state(), &State::Running { number: 0x12 });
    }

    #[test]
    fn with_all_preferred_numbers_held_it_asks_to_be_assigned_and_takes_the_answer() {
        let now = Instant::now();
        let mut join = Join::new(MAC, IP, now);
        join.hear_keep_alive(&player(), now);
        let (_, now) = run(&mut join, now, 3);
        for number in PREFERRED_CHOICES {
            join.hear_reply(
                &NumberReply::decode(&number_in_use_reply("rekordbox", number)).unwrap(),
            );
        }
        let (out, now) = run(&mut join, now, 4 * 6 + 1);
        assert_eq!(out.len(), 4 * 6 + 1);
        let request = &out[24];
        assert_eq!(request.what, "assign request");
        assert_eq!(request.packet[0x0b], PROBE_SUBTYPE_ASSIGN);
        assert_eq!(request.packet[0x2e], 0x11);
        assert_eq!(join.state(), &State::Assigning { sent: 1 });

        let mut accepted = number_in_use_reply("DJM-V10", 0x11);
        accepted[0x0b] = PROBE_SUBTYPE_ASSIGN;
        accepted[0x26] = 0;
        join.hear_reply(&NumberReply::decode(&accepted).unwrap());
        assert_eq!(join.state(), &State::Running { number: 0x11 });
        assert_eq!(join.tick(now + TICK), None);
    }

    #[test]
    fn unanswered_assign_requests_end_in_failure_after_six() {
        let now = Instant::now();
        let mut join = Join::new(MAC, IP, now);
        join.hear_keep_alive(&player(), now);
        let (_, now) = run(&mut join, now, 3);
        for number in PREFERRED_CHOICES {
            join.hear_reply(
                &NumberReply::decode(&number_in_use_reply("rekordbox", number)).unwrap(),
            );
        }
        let (out, _) = run(&mut join, now, 100);
        assert_eq!(out.iter().filter(|o| o.what == "assign request").count(), 6);
        assert!(matches!(join.state(), State::Failed(_)));
        assert_eq!(join.number(), None);
    }

    #[test]
    fn running_it_answers_a_probe_of_its_number_and_no_other() {
        let now = Instant::now();
        let mut join = Join::new(MAC, IP, now);
        join.hear_keep_alive(&player(), now);
        run(&mut join, now, 100);
        let other_mac = [1, 2, 3, 4, 5, 6];
        let other_ip = std::net::Ipv4Addr::new(192, 168, 1, 20);
        let probe =
            NumberProbe::decode(&rekordbox_claim_stage2(other_mac, other_ip, 0x11, 1)).unwrap();
        let answer = join.hear_probe(&probe).unwrap();
        assert_eq!(answer.to, Some(other_ip));
        assert_eq!(answer.packet, number_in_use_reply_with_counter("rekordbox", 0x11, 1));
        let reply = NumberReply::decode(&answer.packet).unwrap();
        assert_eq!(
            (reply.number, reply.status, reply.subtype),
            (0x11, NUMBER_REPLY_IN_USE, PROBE_SUBTYPE_PROBE)
        );

        let other =
            NumberProbe::decode(&rekordbox_claim_stage2(other_mac, other_ip, 0x12, 1)).unwrap();
        assert_eq!(join.hear_probe(&other), None);
        let ours = NumberProbe::decode(&rekordbox_claim_stage2(MAC, IP, 0x11, 1)).unwrap();
        assert_eq!(join.hear_probe(&ours), None, "our own probe echoed back");

        join.reset(now);
        assert_eq!(join.state(), &State::Waiting);
    }

    #[test]
    fn occupancy_masks_echo_counters_only_in_the_established_mode_and_state() {
        let now = Instant::now();
        let other_ip = std::net::Ipv4Addr::new(192, 168, 1, 20);
        for (mode, choices) in [(ConnectionMode::Wired, vec![17, 18]),
            (ConnectionMode::Wireless, vec![41, 42, 43, 44])] {
            for number in choices {
                for counter in [0, 1, 255] {
                    let mut join = Join::new(MAC, IP, now).with_mode(mode);
                    // Explicit state fixture: this is a running reply contract,
                    // not evidence of wireless candidate selection timing.
                    join.state = State::Running { number };
                    let mut request = rekordbox_claim_stage2([1; 6], other_ip, 0, 0);
                    request.resize(68, 0);
                    request[11] = rbl_prolink::PROBE_SUBTYPE_BLOCK;
                    request[34..36].copy_from_slice(&68_u16.to_be_bytes());
                    request[48] = if mode == ConnectionMode::Wired { 1 << (number - 17) } else { 0 };
                    request[51] = if mode == ConnectionMode::Wireless { 1 << (number - 41) } else { 0 };
                    request[67] = counter;
                    let block = NumberBlock::decode(&request).unwrap();
                    let before = join.state.clone();
                    let response = join.hear_block(&block).unwrap();
                    assert_eq!(response.to, Some(other_ip));
                    let mut expected = vec![0x51,0x73,0x70,0x74,0x31,0x57,0x6d,0x4a,0x4f,0x4c,3,0,
                        b'r',b'e',b'k',b'o',b'r',b'd',b'b',b'o',b'x'];
                    expected.resize(32, 0);
                    expected.extend_from_slice(&[1,3,0,39,number,counter,1]);
                    assert_eq!(response.packet, expected);
                    assert_eq!(join.state(), &before);
                    let mut absent = block.clone();
                    absent.wired_mask = 0;
                    absent.wireless_mask = 0;
                    assert_eq!(join.hear_block(&absent), None);
                    join.mode = ConnectionMode::Unknown;
                    assert_eq!(join.hear_block(&block), None);
                    join.mode = mode;
                    let mut echo = block.clone();
                    echo.ip = IP;
                    echo.mac = MAC;
                    assert_eq!(join.hear_block(&echo), None);
                    for state in [State::Waiting, State::Probing { round: 1, index: 0 }, State::Assigning { sent: 1 }] {
                        join.state = state;
                        assert_eq!(join.hear_block(&block), None);
                    }
                }
            }
        }
    }

    #[test]
    fn probing_keepalives_mark_all_candidates_without_a_probe_reply() {
        let now = Instant::now();
        let mut join = Join::new(MAC, IP, now).with_mode(ConnectionMode::Wired);
        join.hear_keep_alive(&player(), now);
        run(&mut join, now, 3);
        let mut packet = player();
        packet.device_type = DeviceType::Rekordbox;
        for number in [17, 41, 42, 43, 44] {
            packet.device_number = number;
            let decoded = KeepAlive::decode(&packet.encode()).unwrap();
            join.hear_keep_alive(&decoded, now);
            join.hear_keep_alive(&decoded, now);
        }
        assert_eq!(join.in_use, [true, false, true, true, true, true]);
        packet.device_number = 18;
        packet.ip = IP;
        packet.mac = MAC;
        join.hear_keep_alive(&packet, now);
        assert!(!join.in_use[1]);
        packet.ip = player().ip;
        packet.mac = player().mac;
        packet.device_type = DeviceType::Other(10);
        join.hear_keep_alive(&packet, now);
        assert!(!join.in_use[1]);
        let (out, _) = run(&mut join, now, 100);
        assert!(out.iter().all(|request| request.packet[46] == 18));
        assert_eq!(join.number(), Some(18));
    }

    #[test]
    fn original_players_join_only_on_a_known_wired_interface() {
        let now = Instant::now();
        for name in ["CDJ-2000", "CDJ-900"] {
            for mode in [ConnectionMode::Wired, ConnectionMode::Wireless, ConnectionMode::Unknown] {
                let mut join = Join::new(MAC, IP, now).with_mode(mode);
                let mut packet = player();
                packet.name = name.into();
                packet.generation = 0;
                join.hear_keep_alive(&KeepAlive::decode(&packet.encode()).unwrap(), now);
                if mode == ConnectionMode::Wired {
                    assert_eq!(join.state(), &State::Discovery { sent: 0 });
                    assert_eq!(join.tick(now).unwrap().packet, rekordbox_claim_stage1(MAC, 1));
                } else {
                    assert_eq!(join.state(), &State::Waiting);
                    packet.generation = 1;
                    join.hear_keep_alive(&packet, now);
                    assert_eq!(join.state(), &State::Discovery { sent: 0 });
                }
            }
        }
    }

    #[test]
    fn single_number_probe_echoes_every_counter_and_does_not_answer_assignment() {
        let now = Instant::now();
        let mut join = Join::new(MAC, IP, now).test_state(State::Running { number: 17 });
        let destination = std::net::Ipv4Addr::new(192, 168, 1, 20);
        for counter in [0, 1, 255] {
            let wire = rekordbox_claim_stage2([1; 6], destination, 17, counter);
            let probe = NumberProbe::decode(&wire).unwrap();
            let response = join.hear_probe(&probe).unwrap();
            assert_eq!(response.to, Some(destination));
            assert_eq!(response.packet, number_in_use_reply_with_counter(REKORDBOX_NAME, 17, counter));
            assert_eq!(response.packet[37], counter);
            assert_eq!(join.state(), &State::Running { number: 17 });
            let mut assign = wire;
            assign[11] = PROBE_SUBTYPE_ASSIGN;
            assert_eq!(join.hear_probe(&NumberProbe::decode(&assign).unwrap()), None);
        }
    }

    #[test]
    fn each_candidate_keepalive_sets_exactly_one_occupancy_bit() {
        let now = Instant::now();
        for (index, number) in REKORDBOX_CLAIM_NUMBERS.iter().copied().enumerate() {
            let mut join = Join::new(MAC, IP, now).test_state(State::Probing { round: 1, index: 0 });
            let mut announcement = player();
            announcement.device_number = number;
            join.hear_keep_alive(&KeepAlive::decode(&announcement.encode()).unwrap(), now);
            let mut expected = [false; 6];
            expected[index] = true;
            assert_eq!(join.in_use, expected);
            announcement.device_number = 33;
            join.hear_keep_alive(&announcement, now);
            assert_eq!(join.in_use, expected);
        }
    }

    #[test]
    fn terminal_rejection_retains_interface_and_candidate_but_not_runtime_or_occupancy() {
        let now = Instant::now();
        for mode in [ConnectionMode::Wired, ConnectionMode::Wireless] {
            let candidate = if mode == ConnectionMode::Wireless { 41 } else { 17 };
            let mut machine = Join::new(MAC, IP, now).with_mode(mode);
            assert_eq!(machine.configured_number, 17);
            assert_eq!(machine.runtime_ip, std::net::Ipv4Addr::UNSPECIFIED);
            assert!(!machine.network_initialized());
            machine.hear_keep_alive(&player(), now);
            assert_eq!(machine.configured_number, candidate);
            assert_eq!(machine.runtime_ip, IP);
            machine.in_use = [true; 6];
            machine.state = State::Running { number: 18 };
            let outgoing = machine.reject(now).unwrap();
            assert_eq!(outgoing.packet[36], 18);
            assert_eq!(outgoing.packet[37..], IP.octets());
            assert_eq!(machine.state, State::Waiting);
            assert_eq!(machine.runtime_ip, std::net::Ipv4Addr::UNSPECIFIED);
            assert_eq!(machine.in_use, [false; 6]);
            assert_eq!(machine.network_ip, Some(IP));
            assert_eq!(machine.configured_number, candidate);
            machine.reset(now);
            machine.hear_keep_alive(&player(), now);
            assert!(machine.tick(now + Duration::from_secs(60)).is_none());
            let response = NumberReply::decode(&number_in_use_reply(REKORDBOX_NAME, 17)).unwrap();
            machine.hear_reply(&response);
            let probe = NumberProbe::decode(&rekordbox_claim_stage2([1;6], IP, 18, 3)).unwrap();
            assert!(machine.hear_probe(&probe).is_none());
            assert_eq!(machine.in_use, [false; 6]);
            assert_eq!(machine.state, State::Waiting);
            let repeat = machine.reject(now).unwrap();
            assert_eq!(repeat.packet[36], candidate);
            assert_eq!(repeat.packet[37..], [0; 4]);
            assert!(machine.rejected());
            assert!(!Join::new(MAC, IP, now).with_mode(mode).rejected());
        }
    }
}
