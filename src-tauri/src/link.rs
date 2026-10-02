//! Link export in the app: LINK on and off, and what the players are doing.
//!
//! `rbl-link` owns the protocol; this is the glue — the library it reads is
//! the app's, reloaded or not, through a weak handle to the state, and the
//! players it hears are reported to the window as an event whenever they
//! change, no more than twice a second.
//!
//! LINK is refused while rekordbox runs: it holds every port a player looks
//! for, and two sources called `rekordbox` on one network would be worse
//! than one that says why it stays off.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use rbl_link::{Interface, LinkExport, Ports, Snapshot, Source};
use serde::Serialize;

use crate::state::AppState;

/// How often the players are looked at for a change worth reporting.
const REPORT_EVERY: Duration = Duration::from_millis(500);

/// A network interface LINK can run on, as the interface offers it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InterfaceDto {
    pub name: String,
    pub address: String,
    pub adapter: Option<String>,
    pub connection: Option<String>,
}

impl From<&Interface> for InterfaceDto {
    fn from(interface: &Interface) -> Self {
        let label = crate::network_labels::for_interface(&interface.name);
        Self { name: interface.name.clone(), address: interface.address.to_string(), adapter: label.adapter, connection: label.connection }
    }
}

/// A track a player has loaded from us, named for the window.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LoadedDto {
    pub id: String,
    pub title: String,
    pub artist: String,
}

/// A device heard on the network before LINK is on: enough to say "a player
/// is here", not what it has loaded.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PeerDto {
    pub number: u8,
    pub name: String,
    /// `player`, `mixer`, `rekordbox` or `device`.
    pub kind: String,
    pub address: String,
}

/// How a device type reads for the window.
fn kind_name(kind: rbl_link::DeviceType) -> &'static str {
    match kind {
        rbl_link::DeviceType::Cdj => "player",
        rbl_link::DeviceType::Mixer => "mixer",
        rbl_link::DeviceType::Rekordbox => "rekordbox",
        rbl_link::DeviceType::Other(_) => "device",
    }
}

impl PeerDto {
    fn from_player(player: &rbl_link::Player) -> Self {
        Self {
            number: player.number,
            name: player.name.clone(),
            kind: kind_name(player.kind).to_owned(),
            address: player.address.to_string(),
        }
    }
}

/// A player on the link, as the window shows it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools, reason = "the player's lamps, sent together")]
pub struct PlayerDto {
    pub number: u8,
    pub name: String,
    pub kind: String,
    pub address: String,
    pub loaded: Option<LoadedDto>,
    pub playing: bool,
    pub master: bool,
    /// The player has SYNC on.
    pub sync: bool,
    /// The player is sitting at its cue point (play state Cued or Cuing).
    pub cued: bool,
    /// The mixer's Link Cue button. Always false: it rides in the Touch Audio
    /// timing packet (kind 0x20, byte 0x28) on UDP 50004, which a mixer only
    /// unicasts to a device that asked for it with bit 5 of status byte 0xcd —
    /// past the 0x38 our status packet is. Reaching it means binding 50004 and
    /// growing that packet, not wiring up a field.
    pub link_cue: bool,
    /// The player has mounted the library: a track can be sent to it.
    /// rekordbox refuses a drag to a player until then.
    pub mounted: bool,
}

/// Whether LINK is on, on what, and who is listening.
// Not `Eq`: `master_bpm` is an `f64`. Callers compare `players`, which is.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LinkStatusDto {
    pub on: bool,
    /// Why it could not be turned on, when it could not.
    pub problem: Option<String>,
    pub interface: Option<InterfaceDto>,
    pub players: Vec<PlayerDto>,
    /// What LINK could run on, for the picker.
    pub interfaces: Vec<InterfaceDto>,
    /// We are the network's tempo master, driving the tempo the players sync
    /// to.
    pub master: bool,
    /// The master tempo we would drive, in BPM. Shown on the master control
    /// whether or not we are master, as rekordbox shows the last value.
    pub master_bpm: f64,
    /// Where the join is while on: `waiting` (nothing announced until a
    /// player or mixer is heard, as rekordbox does), `joining` (probing for
    /// a device number), `up`, or `down` (the interface lost its address;
    /// `problem` says so). `off` while off.
    pub state: String,
    /// The device number the join settled on — 17, or 18 when another
    /// rekordbox holds 17 — once it has.
    pub number: Option<u8>,
}

impl LinkStatusDto {
    pub fn off(problem: Option<String>) -> Self {
        Self {
            on: false,
            problem,
            interface: None,
            players: Vec::new(),
            interfaces: interfaces(),
            master: false,
            master_bpm: 120.0,
            state: "off".to_owned(),
            number: None,
        }
    }
}

/// The interfaces on offer, as the window lists them.
pub fn interfaces() -> Vec<InterfaceDto> {
    rbl_link::interfaces().iter().map(InterfaceDto::from).collect()
}

/// The app's library, as the link reads it. Weak so the state does not own
/// a session that owns the state.
struct StateSource(Weak<AppState>, Arc<dyn Fn(&'static str, u32) + Send + Sync>, bool);

impl Source for StateSource {
    fn alphabetical_keys(&self) -> bool { self.2 }
    fn edit(&self, edit: &rbl_link::Edit) -> bool {
        let Some(state) = self.0.upgrade() else { return false; };
        if let rbl_link::Edit::GridOffset { track, offset_ms } = edit {
            return match save_grid_offset(&state, &track.to_string(), *offset_ms) {
                Ok(()) => true,
                Err(error) => { tracing::warn!(%error, ?edit, "player grid edit refused"); false }
            };
        }
        let touched = match edit {
            rbl_link::Edit::Tag { .. } | rbl_link::Edit::ClearTags => crate::commands::Touched::TagList,
            rbl_link::Edit::Rating { track, .. } => crate::commands::Touched::Metadata(vec![track.to_string()]),
            rbl_link::Edit::GridOffset { .. } => unreachable!("handled above"),
            rbl_link::Edit::HotCueBankCue { .. } => crate::commands::Touched::Metadata(Vec::new()),
            // The catalog keeps the link session's history and writes it
            // through the methods below.
            rbl_link::Edit::HistoryAdd { .. } | rbl_link::Edit::HistoryRemove { .. } | rbl_link::Edit::HistoryDelete { .. } => return false,
        };
        let event = touched.event();
        let result = state.write_then(|writer| match edit {
            rbl_link::Edit::Tag { track, add: true } => writer.tag_list_add(&[track.to_string()]),
            rbl_link::Edit::Tag { track, add: false } => writer.tag_list_remove(&[track.to_string()]),
            rbl_link::Edit::ClearTags => writer.tag_list_clear(),
            rbl_link::Edit::Rating { track, stars } => writer.set_rating(&track.to_string(), *stars),
            rbl_link::Edit::HotCueBankCue { bank, cue } => writer.set_hot_cue_bank_cue(&bank.to_string(), &rbl_db::details::HotCueBankCue {
                slot: cue.slot,
                content: cue.content,
                in_ms: cue.in_ms,
                out_ms: cue.out_ms,
                color: cue.color,
                color_table_index: cue.color_table_index,
                active_loop: cue.active_loop,
                beat_loop_size: cue.beat_loop_size,
                cue_microsec: cue.cue_microsec,
            }),
            rbl_link::Edit::GridOffset { .. }
            | rbl_link::Edit::HistoryAdd { .. }
            | rbl_link::Edit::HistoryRemove { .. }
            | rbl_link::Edit::HistoryDelete { .. } => unreachable!("handled above"),
        }, |db, _| crate::commands::refresh_after_edit(&state, db, touched));
        match result {
            Ok(generation) => { (self.1)(event, generation); true }
            Err(error) => { tracing::warn!(%error, ?edit, "player library edit or refresh failed"); false }
        }
    }

    fn new_link_history(&self) -> Option<u32> {
        let id = self.history_write(Vec::new(), rbl_db::write::Writer::new_link_history)?;
        id.parse().ok()
    }

    fn add_to_history(&self, history: u32, track: u32) -> bool {
        let track = track.to_string();
        self.history_write(vec![track.clone()], |writer| writer.add_to_history(&history.to_string(), &track)).is_some()
    }

    fn remove_from_history(&self, history: u32, track: u32) -> bool {
        self.history_write(Vec::new(), |writer| writer.remove_from_history(&history.to_string(), &[track.to_string()])).is_some()
    }

    fn library(&self) -> Option<Arc<rbl_index::Library>> {
        self.0.upgrade()?.library().ok()
    }

    fn share_root(&self) -> std::path::PathBuf {
        self.0.upgrade().map(|state| state.share_root()).unwrap_or_default()
    }

    fn details(&self, id: &str) -> Option<rbl_db::details::TrackDetails> {
        let state = self.0.upgrade()?;
        state.read_db(|db| rbl_db::details::track_details(db.connection(), id)).ok().flatten()
    }

    fn hot_cue_banks(&self, parent: Option<u32>) -> Vec<rbl_db::details::HotCueBank> {
        let Some(state) = self.0.upgrade() else { return Vec::new() };
        state.read_db(|db| rbl_db::details::hot_cue_banks(db.connection(), parent)).unwrap_or_default()
    }

    fn hot_cue_bank_cues(&self, bank: u32) -> Vec<rbl_db::details::HotCueBankCue> {
        let Some(state) = self.0.upgrade() else { return Vec::new() };
        state.read_db(|db| rbl_db::details::hot_cue_bank_cues(db.connection(), bank)).unwrap_or_default()
    }

    fn hot_cue_bank_track_ids(&self, bank: u32) -> Vec<u32> {
        let Some(state) = self.0.upgrade() else { return Vec::new() };
        state.read_db(|db| rbl_db::details::hot_cue_bank_track_ids(db.connection(), bank)).unwrap_or_default()
    }

    fn matching_ids(&self, seed: u32) -> Vec<u32> {
        let Some(state) = self.0.upgrade() else { return Vec::new() };
        state
            .read_db(|db| rbl_db::details::matching_ids(db.connection(), seed))
            .unwrap_or_default()
    }

    fn artist_role_names(&self, role: rbl_link::ArtistRole) -> Vec<(u32, String)> {
        let Some(state) = self.0.upgrade() else { return Vec::new() };
        let role = match role {
            rbl_link::ArtistRole::Original => rbl_db::details::ArtistRole::Original,
            rbl_link::ArtistRole::Remixer => rbl_db::details::ArtistRole::Remixer,
        };
        state.read_db(|db| rbl_db::details::artist_role_names(db.connection(), role)).unwrap_or_default()
    }

    fn artist_role_track_ids(&self, role: rbl_link::ArtistRole, artist: u32) -> Vec<u32> {
        let Some(state) = self.0.upgrade() else { return Vec::new() };
        let role = match role {
            rbl_link::ArtistRole::Original => rbl_db::details::ArtistRole::Original,
            rbl_link::ArtistRole::Remixer => rbl_db::details::ArtistRole::Remixer,
        };
        state.read_db(|db| rbl_db::details::artist_role_track_ids(db.connection(), role, artist)).unwrap_or_default()
    }
}

impl StateSource {
    /// A write to the history tree for a player, then the histories read
    /// again — with the play counts of `tracks` — and the window told.
    fn history_write<T>(&self, tracks: Vec<String>, edit: impl FnOnce(&mut rbl_db::write::Writer) -> Result<T, rbl_db::DbError>) -> Option<T> {
        let state = self.0.upgrade()?;
        let touched = crate::commands::Touched::Histories(tracks);
        let event = touched.event();
        let result = state.write_then(edit, |db, value| {
            crate::commands::refresh_after_edit(&state, db, touched).map(|generation| (value, generation))
        });
        match result {
            Ok((value, generation)) => {
                (self.1)(event, generation);
                Some(value)
            }
            Err(error) => {
                tracing::warn!(%error, "player history edit or refresh failed");
                None
            }
        }
    }
}

/// Keep the correction in the analysis header, as rekordbox does. The beat
/// records remain the common baseline used by every player on the network.
fn save_grid_offset(state: &AppState, track: &str, offset_ms: i16) -> crate::error::AppResult<()> {
    use crate::error::{AppError, ErrorKind};
    let _edit_guard = state.edit_gate.lock();
    let _files = state.analysis_write.lock();
    let location = state.location()?;
    if let Some(reason) = rbl_db::write_refusal_reason(location.is_real_install,
        std::env::var_os("RB_LITE_TEST").is_some(), rbl_db::is_rekordbox_running()) {
        return Err(AppError::new(ErrorKind::ReadOnly, reason));
    }
    crate::file_journal::recover(state.backup_dir(), &location)?;
    let library = state.library()?;
    let row = library.row_of(track).ok_or_else(|| AppError::new(ErrorKind::NotFound, "Track not found"))?;
    let relative = library.analysis_path.get(row as usize);
    if relative.is_empty() { return Err(AppError::new(ErrorKind::NotFound, "Track has no analysis")); }
    let dat = rbl_anlz::resolve(&location.share_root, relative);
    let analysis = rbl_anlz::Anlz::read(&dat).map_err(|e| AppError::internal(e.to_string()))?;
    let bytes = analysis.with_grid_offset(offset_ms)
        .ok_or_else(|| AppError::new(ErrorKind::Malformed, "Track has no complete beat-grid header"))?;
    let journal = crate::file_journal::FileJournal::prepare(state.backup_dir(), &location, track,
        library.bpm_x100[row as usize], None, false, &[(dat, bytes)])?;
    if let Err(e) = journal.publish() { journal.rollback()?; return Err(e); }
    journal.commit()
}

/// A running LINK session: the export, and the thread that reports it.
pub struct Session {
    rx3_activation: Option<crate::rx3_link::Activation>,
    export: Option<LinkExport>,
    stop: Arc<AtomicBool>,
    reporter: Option<std::thread::JoinHandle<()>>,
}

impl Session {
    /// Turns LINK on for `interface`, or when none is named the interface
    /// the OS reaches a player already heard through. With a connected RX3
    /// that has not announced yet, its sole link-local USB interface is used.
    /// Otherwise the first interface is the fallback. Reports the players to
    /// `report` as they change.
    ///
    /// Blocking: binds seven sockets and walks every track's path.
    pub fn start<F>(state: &Arc<AppState>, interface: Option<&str>, alphabetical_keys: bool, report: F, library_changed: Arc<dyn Fn(&'static str, u32) + Send + Sync>) -> Result<Self, String>
    where
        F: Fn(LinkStatusDto) + Send + 'static,
    {
        let available = rbl_link::interfaces();
        tracing::debug!(
            interfaces = ?available.iter().map(|i| format!("{} {}/{}", i.name, i.address, i.netmask)).collect::<Vec<_>>(),
            "interfaces LINK could run on"
        );
        let rx3 = match crate::rx3_link::detect() {
            Ok(rx3) => rx3,
            Err(error) => {
                tracing::debug!(%error, "XDJ-RX3 MIDI detection unavailable");
                None
            }
        };
        let chosen = if let Some(name) = interface {
            available.iter().find(|i| i.name == name).cloned()
        } else {
            let peers = state.link_peers();
            let toward = peers
                .iter()
                .find_map(|peer| rbl_link::interface_toward(&available, peer.address).map(|i| (peer.address, i)));
            if let Some((peer, i)) = toward {
                tracing::debug!(interface = %i.name, %peer, "interface chosen: the one that reaches a device already heard");
                Some(i)
            } else if rx3.is_some() {
                let mut usb = available.iter().filter(|candidate| is_link_local(candidate.address));
                let first = usb.next().cloned();
                if usb.next().is_some() {
                    return Err("An XDJ-RX3 is connected, but more than one link-local network interface is available. Choose the RX3 USB interface in Settings > LINK.".to_owned());
                }
                if first.is_none() {
                    return Err("An XDJ-RX3 is connected, but its USB Link Export network interface has no 169.254.x.x address. Install the Link Export driver, then reconnect the rear USB-B cable.".to_owned());
                }
                tracing::debug!(interface = %first.as_ref().map_or("", |i| i.name.as_str()), "interface chosen: the XDJ-RX3 USB link-local interface");
                first
            } else {
                tracing::debug!(peers = peers.len(), "no device heard on any interface; taking the first");
                available.first().cloned()
            }
        }
        .ok_or_else(|| match interface {
            Some(name) => format!("No network interface called {name}."),
            None => "No network interface to run LINK on.".to_owned(),
        })?;
        tracing::info!(interface = %chosen.name, address = %chosen.address, "LINK running on an interface");

        let source: Arc<dyn Source> = Arc::new(StateSource(Arc::downgrade(state), library_changed, alphabetical_keys));
        let export = LinkExport::start(source, chosen, Ports::REKORDBOX).map_err(|e| e.to_string())?;
        let rx3_activation = match rx3 {
            Some(rx3) => match rx3.activate() {
                Ok(activation) => Some(activation),
                Err(error) => {
                    export.stop();
                    return Err(error);
                }
            },
            None => None,
        };

        let stop = Arc::new(AtomicBool::new(false));
        let weak = Arc::downgrade(state);
        let reporter = {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                let mut last: Option<(String, Option<u8>, Vec<PlayerDto>)> = None;
                while !stop.load(Ordering::Relaxed) {
                    std::thread::sleep(REPORT_EVERY);
                    let Some(state) = weak.upgrade() else { return };
                    let Some(status) = state.link_status() else { return };
                    // The link went down — the interface lost its address,
                    // or no device number could be had: LINK goes off with
                    // the reason, as rekordbox's LINKDOWN takes its button
                    // back to grey. The session is dropped on a thread of
                    // its own, since dropping it joins this one.
                    if status.state == "down" {
                        tracing::warn!(problem = status.problem.as_deref().unwrap_or(""), "the link went down; LINK off");
                        let session = state.set_link(None);
                        std::thread::spawn(move || drop(session));
                        report(LinkStatusDto::off(status.problem));
                        return;
                    }
                    let now = (status.state.clone(), status.number, status.players.clone());
                    if last.as_ref() != Some(&now) {
                        last = Some(now);
                        report(status);
                    }
                }
            })
        };
        Ok(Self { rx3_activation, export: Some(export), stop, reporter: Some(reporter) })
    }

    /// The session as the window shows it, with the loaded tracks named
    /// from `library`. The library is an argument rather than read from the
    /// state here: the caller holds the state's lock already.
    pub fn status(&self, library: Option<&rbl_index::Library>) -> LinkStatusDto {
        let Some(export) = &self.export else {
            return LinkStatusDto::off(None);
        };
        let snapshot = export.snapshot();
        let (state, number, problem) = match &snapshot.link {
            rbl_link::LinkState::Waiting => ("waiting", None, None),
            rbl_link::LinkState::Joining => ("joining", None, None),
            rbl_link::LinkState::Up { number } => ("up", Some(*number), None),
            rbl_link::LinkState::Down(why) => ("down", None, Some(why.clone())),
        };
        LinkStatusDto {
            on: true,
            problem,
            interface: Some(InterfaceDto::from(&snapshot.interface)),
            players: players(library, &snapshot),
            interfaces: interfaces(),
            master: snapshot.master.on,
            master_bpm: f64::from(snapshot.master.bpm_x100) / 100.0,
            state: state.to_owned(),
            number,
        }
    }

    /// Why the link went down, once it has: the app stops the session and
    /// shows the reason.
    pub fn down(&self) -> Option<String> {
        match self.export.as_ref()?.link_state() {
            rbl_link::LinkState::Down(why) => Some(why),
            _ => None,
        }
    }

    /// After an analysis run: the players get fresh files.
    pub fn analysis_changed(&self) {
        if let Some(export) = &self.export {
            export.analysis_changed();
        }
    }

    /// Tells a CDJ on the link to load a track from our library.
    ///
    /// The reason is carried back rather than logged: a drop that does nothing
    /// and says nothing is worse than one that says why.
    pub fn load_track(&self, player_number: u8, track_id: u32) -> Result<(), String> {
        let Some(export) = &self.export else {
            return Err("LINK is not running.".to_owned());
        };
        export
            .load_track(player_number, track_id)
            .map_err(|error| format!("The player could not be told to load that track: {error}"))
    }

    /// Become the network's tempo master, or resign.
    pub fn set_master(&self, on: bool) {
        if let Some(export) = &self.export {
            export.set_master(on);
        } else {
            tracing::debug!(on, "master asked while LINK is off; nothing to do");
        }
    }

    /// Nudge the master tempo (rekordbox's −/+ move it a whole BPM), in BPM.
    pub fn nudge_master(&self, delta_bpm: f64) {
        if let Some(export) = &self.export {
            #[allow(clippy::cast_possible_truncation)]
            let delta_x100 = (delta_bpm * 100.0).round() as i32;
            export.nudge_master(delta_x100);
        }
    }

    /// Take the current master player's tempo (rekordbox's ⟳); `false` when
    /// no player is master.
    pub fn take_master_tempo(&self) -> bool {
        self.export.as_ref().is_some_and(rbl_link::LinkExport::take_master_tempo)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        drop(self.rx3_activation.take());
        if let Some(export) = self.export.take() {
            export.stop();
        }
        if let Some(reporter) = self.reporter.take() {
            drop(reporter.join());
        }
    }
}

fn is_link_local(address: std::net::Ipv4Addr) -> bool {
    let [first, second, _, _] = address.octets();
    first == 169 && second == 254
}

#[cfg(test)]
mod rx3_interface_tests {
    use super::{is_link_local, players};
    use rbl_link::beacon::{MasterState, Player};
    use rbl_link::{Interface, LinkState, Snapshot};
    use rbl_prolink::DeviceType;
    use std::net::Ipv4Addr;
    use std::time::Instant;

    #[test]
    fn only_ipv4_link_local_addresses_are_rx3_usb_candidates() {
        assert!(is_link_local(Ipv4Addr::new(169, 254, 175, 153)));
        assert!(is_link_local(Ipv4Addr::new(169, 254, 0, 1)));
        assert!(!is_link_local(Ipv4Addr::new(192, 168, 1, 14)));
        assert!(!is_link_local(Ipv4Addr::new(169, 253, 255, 255)));
    }

    #[test]
    fn one_appliance_mount_makes_both_logical_decks_available() {
        let address = Ipv4Addr::new(169, 254, 20, 2);
        let player = |number, kind| Player {
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
        };
        let snapshot = Snapshot {
            interface: Interface::loopback(),
            database_port: 0,
            players: vec![
                player(1, DeviceType::Cdj),
                player(2, DeviceType::Cdj),
                player(33, DeviceType::Mixer),
            ],
            master: MasterState::default(),
            link: LinkState::Up { number: 17 },
            mounted: vec![address],
        };

        let shown = players(None, &snapshot);
        assert_eq!(shown.iter().map(|player| player.number).collect::<Vec<_>>(), [1, 2, 33]);
        assert!(shown.iter().all(|player| player.mounted));
    }
}

/// The players with their loaded tracks named from the library.
fn players(library: Option<&rbl_index::Library>, snapshot: &Snapshot) -> Vec<PlayerDto> {
    snapshot
        .players
        .iter()
        .map(|player| PlayerDto {
            number: player.number,
            name: player.name.clone(),
            kind: kind_name(player.kind).to_owned(),
            address: player.address.to_string(),
            loaded: player.loaded.map(|id| {
                let row = library.and_then(|l| l.row_of_id(u64::from(id)));
                LoadedDto {
                    id: id.to_string(),
                    title: row.zip(library).map(|(r, l)| l.title.get(r as usize).to_owned()).unwrap_or_default(),
                    artist: row.zip(library).map(|(r, l)| l.artist_name(r).to_owned()).unwrap_or_default(),
                }
            }),
            playing: player.playing,
            master: player.master,
            sync: player.sync,
            cued: player.cued,
            link_cue: false,
            mounted: snapshot.mounted.contains(&player.address),
        })
        .collect()
}

/// Starts the passive network watcher on the announce port, reporting peers
/// to `report`. Binding proves rekordbox is not holding the port. Returns
/// `None` when the port cannot be bound (rekordbox is running).
pub fn start_watcher<F>(report: F) -> Option<rbl_link::Watcher>
where
    F: Fn(Vec<PeerDto>) + Send + 'static,
{
    match rbl_link::Watcher::start(rbl_link::Ports::REKORDBOX.announce, move |players| {
        report(players.iter().map(PeerDto::from_player).collect());
    }) {
        Ok(watcher) => {
            tracing::debug!("watching the network for players");
            Some(watcher)
        }
        Err(error) => {
            tracing::debug!(%error, "network watcher not started (rekordbox may hold the port)");
            None
        }
    }
}

/// The peers heard so far, as the window shows them.
pub fn peers(state: &AppState) -> Vec<PeerDto> {
    state.link_peers().iter().map(PeerDto::from_player).collect()
}

/// Why LINK cannot start now, if it cannot: rekordbox holds the ports.
pub fn refusal() -> Option<String> {
    if rbl_db::is_rekordbox_running() {
        return Some("rekordbox is running and holds the link ports. Quit it to turn LINK on.".to_owned());
    }
    None
}

#[cfg(test)]
mod grid_offset_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn player_tag_edits_persist_and_refresh_without_replacing_the_track_index() {
        let dir = tempfile::tempdir().unwrap();
        let location = rbl_db::fixture::build(dir.path(), rbl_db::fixture::Shape::default()).unwrap();
        let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
        let (library, _) = rbl_index::load(&db).unwrap();
        let state = Arc::new(AppState::with_backups(dir.path().join("backups")));
        state.set_library(library, false, db.schema().db_version, 0, location);
        crate::backups::create(&state).unwrap();
        let original = state.library().unwrap();
        let notifications = Arc::new(std::sync::Mutex::new(Vec::new()));
        let received = notifications.clone();
        let source = StateSource(Arc::downgrade(&state), Arc::new(move |event, generation| {
            received.lock().unwrap().push((event, generation));
        }), false);
        let spec = |source| rbl_index::ViewSpec { source, sort: rbl_index::SortColumn::Title, descending: false, query: String::new(), filter: rbl_index::TrackFilter::default() };
        let id = rbl_db::fixture::track_id(1);
        let track = id.parse().unwrap();
        let row = original.row_of(&id).unwrap();
        for (edit, present) in [
            (rbl_link::Edit::ClearTags, false),
            (rbl_link::Edit::Tag { track, add: true }, true),
            (rbl_link::Edit::Tag { track, add: false }, false),
            (rbl_link::Edit::Tag { track, add: true }, true),
            (rbl_link::Edit::ClearTags, false),
        ] {
            let generation = state.summary().3;
            let (collection, ..) = state.open_view(&spec(rbl_index::TrackSource::Collection)).unwrap();
            let (tag_list, ..) = state.open_view(&spec(rbl_index::TrackSource::TagList)).unwrap();
            assert!(source.edit(&edit));
            assert!(Arc::ptr_eq(&original, &state.library().unwrap()));
            assert_eq!(original.tag_list().contains(&row), present);
            let reopened = state.open_read_only().unwrap();
            assert_eq!(rbl_index::reload_tag_list(&reopened, &original).unwrap(), original.tag_list());
            // Only the Tag List is stale: the list on screen keeps its view
            // and its pages rather than being fetched again.
            assert_eq!(state.summary().3, generation);
            assert!(state.view(collection).is_ok());
            assert!(state.view(tag_list).is_err());
            assert_eq!(notifications.lock().unwrap().last(), Some(&("tag-list:changed", generation)));
        }
        assert_eq!(notifications.lock().unwrap().len(), 5);
        let old_rating = original.rating[row as usize];
        let stars = (old_rating + 1) % 6;
        assert!(source.edit(&rbl_link::Edit::Rating { track, stars }));
        let updated = state.library().unwrap();
        assert_eq!(updated.rating[row as usize], stars);
        assert_eq!(updated.ids, original.ids, "cached analysis row identities stay valid");
        assert_eq!(original.rating[row as usize], old_rating, "old readers keep their snapshot");
        let db = state.open_read_only().unwrap();
        let (persisted, _) = rbl_index::load(&db).unwrap();
        assert_eq!(persisted.rating, updated.rating);
        assert_eq!(notifications.lock().unwrap().len(), 6);
        assert_eq!(notifications.lock().unwrap().last(), Some(&("library:changed", state.summary().3)));
    }

    #[test]
    fn a_players_plays_make_this_link_sessions_history_in_the_library() {
        use rbl_dbserver::catalog::{Catalog, Query, Row, Sort, TrackScope};
        let dir = tempfile::tempdir().unwrap();
        let location = rbl_db::fixture::build(dir.path(), rbl_db::fixture::Shape::default()).unwrap();
        let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
        let (library, _) = rbl_index::load(&db).unwrap();
        let state = Arc::new(AppState::with_backups(dir.path().join("backups")));
        state.set_library(library, false, db.schema().db_version, 0, location);
        crate::backups::create(&state).unwrap();
        let notifications = Arc::new(std::sync::Mutex::new(Vec::new()));
        let received = notifications.clone();
        let source = StateSource(Arc::downgrade(&state), Arc::new(move |event, generation| {
            received.lock().unwrap().push((event, generation));
        }), false);
        let catalog = rbl_link::IndexCatalog::new(Arc::new(source), rbl_link::Played::default());
        // The fixture's own histories are not the menu.
        assert!(!state.library().unwrap().histories().is_empty());
        assert_eq!(catalog.list(&Query::Histories), [] as [rbl_dbserver::catalog::Row; 0]);

        let id = rbl_db::fixture::track_id(1);
        let track: u32 = id.parse().unwrap();
        let row = state.library().unwrap().row_of(&id).unwrap() as usize;
        let before = state.library().unwrap().play_count[row];
        assert!(catalog.edit(&rbl_link::Edit::HistoryAdd { track }));
        let menu = catalog.list(&Query::Histories);
        let session = &match menu.as_slice() { [Row::Named { id, .. }] => *id, _ => 0 };
        assert_eq!(menu, vec![Row::Named { id: *session, name: format!("LINK HISTORY {}", rbl_core::time::local_date()) }]);
        let tracks = catalog.list(&Query::Tracks { scope: TrackScope::History(*session), sort: Sort::Default });
        assert_eq!(tracks, vec![Row::Track { id: track, position: 1 }]);
        assert_eq!(state.library().unwrap().play_count[row], before + 1);
        assert_eq!(notifications.lock().unwrap().last(), Some(&("library:changed", state.summary().3)));

        // Persisted: a fresh read of the database has the session and the play.
        let reopened = state.open_read_only().unwrap();
        let histories = rbl_index::reload_histories(&reopened, &state.library().unwrap()).unwrap();
        let index = histories.index_of(u64::from(*session)).unwrap();
        assert_eq!(histories.members[index].len(), 1);

        assert!(catalog.edit(&rbl_link::Edit::HistoryRemove { track }));
        assert_eq!(catalog.list(&Query::Tracks { scope: TrackScope::History(*session), sort: Sort::Default }), [] as [rbl_dbserver::catalog::Row; 0]);
    }

    #[test]
    fn matching_relations_reach_link_browse_from_the_live_database() {
        let dir = tempfile::tempdir().unwrap();
        let location = rbl_db::fixture::build(dir.path(), rbl_db::fixture::Shape::default()).unwrap();
        let first = rbl_db::fixture::track_id(1);
        let second = rbl_db::fixture::track_id(2);
        let stamp = rbl_core::time::now();
        let writable = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadWrite).unwrap();
        writable.connection().execute_batch(&format!(
            "CREATE TABLE djmdRecommendLike (ID TEXT PRIMARY KEY, ContentID1 TEXT, ContentID2 TEXT, rb_local_deleted INTEGER DEFAULT 0, created_at DATETIME NOT NULL, updated_at DATETIME NOT NULL); \
             INSERT INTO djmdRecommendLike VALUES ('fixture-match', '{first}', '{second}', 0, '{stamp}', '{stamp}');"
        )).unwrap();
        drop(writable);

        let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
        let (library, _) = rbl_index::load(&db).unwrap();
        let state = Arc::new(AppState::with_backups(dir.path().join("backups")));
        state.set_library(library, false, db.schema().db_version, 0, location);
        let source = StateSource(Arc::downgrade(&state), Arc::new(|_, _| {}), false);

        assert_eq!(source.matching_ids(first.parse().unwrap()), [second.parse::<u32>().unwrap()]);
    }

    #[test]
    fn player_grid_correction_survives_reopening_without_rewriting_beats() {
        let dir = tempfile::tempdir().unwrap();
        let location = rbl_db::fixture::build(dir.path(), rbl_db::fixture::Shape::default()).unwrap();
        let relative = "/PIONEER/USBANLZ/test/ANLZ0000.DAT";
        rbl_db::fixture::set_analysis_path(&location, 1, relative).unwrap();
        let dat = rbl_anlz::resolve(&location.share_root, relative);
        std::fs::create_dir_all(dat.parent().unwrap()).unwrap();
        let beats = vec![rbl_anlz::Beat { beat_number: 1, tempo_x100: 12800, time_ms: 500 }];
        let mut builder = rbl_anlz::AnlzBuilder::new();
        builder.path("/test.wav").beat_grid(&beats);
        let original = builder.finish();
        std::fs::write(&dat, &original).unwrap();
        let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
        let (library, _) = rbl_index::load(&db).unwrap();
        let state = AppState::with_backups(dir.path().join("backups"));
        state.set_library(library, false, db.schema().db_version, 0, location);
        crate::backups::create(&state).unwrap();
        let track = rbl_db::fixture::track_id(1);
        for offset in [234, -467, 0] {
            save_grid_offset(&state, &track, offset).unwrap();
            let reopened = rbl_anlz::Anlz::read(&dat).unwrap();
            assert_eq!(reopened.grid_offset(), Some(offset));
            assert_eq!(reopened.beat_grid(), Some(beats.clone()));
        }
        assert_eq!(std::fs::read(&dat).unwrap(), original);
        assert!(save_grid_offset(&state, "999999", 234).is_err());
    }
}
