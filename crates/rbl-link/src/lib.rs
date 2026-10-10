//! Link export: the library served to players over Pro DJ Link, the way
//! rekordbox serves it.
//!
//! Three servers and a beacon, all measured against rekordbox 7.2.11 and a
//! CDJ-3000 (`docs/pre-release/design-notes/link-export-capture.md`) and
//! built to rekordbox's own join and file service as read out of its
//! binary (`docs/pre-release/rekordbox/link-export-internals.md`): the
//! join and the keep-alive and status packets that put us on the network
//! as `rekordbox` (`join`, `beacon`), the database server a player browses
//! (`rbl-dbserver`, fed by `catalog`), and the NFS server it reads the
//! audio file from (`rbl-nfs`, fed by `files`). `blobs` builds the
//! analysis replies out of the ANLZ files. [`LinkExport::start`] binds all
//! of it; nothing is announced or served until the join hears a player and
//! settles a number; dropping it unbinds.
//!
//! The player's side of the protocol comes from
//! [alphatheta-connect](https://github.com/chrisle/alphatheta-connect-rs):
//! interface discovery, and the status packet that says what a player has
//! loaded. Its remote-database and NFS clients are what the integration
//! tests browse this server with — an implementation that was written
//! against real players, not against this crate.

pub mod beacon;
mod interface_mode;
pub mod blobs;
pub mod catalog;
pub mod files;
pub mod join;
pub mod watch;

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use rbl_dbserver::session::CatalogHandler;
use rbl_index::Library;

pub use beacon::{LinkState, Player};
pub use catalog::{IndexCatalog, KeyNotation, KeyOrder, Played, Source};
pub use rbl_dbserver::catalog::{ArtistRole, Edit, HotCueColour, PlayerCue, RootCategory, Sort, TrackColumn};
pub use rbl_prolink::DeviceType;
pub use watch::Watcher;

/// The ports rekordbox uses, which a player expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ports {
    pub announce: u16,
    pub status: u16,
    /// The port-query service. Fixed: a player asks here first.
    pub query: u16,
    /// The database server. rekordbox picks an ephemeral one per session.
    pub database: u16,
    /// rekordbox's portmap: 50111, not the privileged 111.
    pub portmap: u16,
    /// mountd; ephemeral on rekordbox.
    pub mount: u16,
    pub nfs: u16,
}

impl Ports {
    /// rekordbox's own.
    pub const REKORDBOX: Self = Self {
        announce: rbl_prolink::PORT_ANNOUNCE,
        status: rbl_prolink::PORT_STATUS,
        query: rbl_dbserver::PORT_QUERY,
        database: 0,
        portmap: rbl_nfs::REKORDBOX_PORTMAP_PORT,
        mount: 0,
        nfs: rbl_nfs::NFS_PORT,
    };

    /// Every port ephemeral, for tests on loopback.
    pub const EPHEMERAL: Self = Self {
        announce: 0,
        status: 0,
        query: 0,
        database: 0,
        portmap: 0,
        mount: 0,
        nfs: 0,
    };
}

/// A network interface link export can run on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interface {
    /// The OS name: `en0`, `Ethernet 2`.
    pub name: String,
    pub address: Ipv4Addr,
    pub netmask: Ipv4Addr,
    pub mac: [u8; 6],
}

impl Interface {
    /// The subnet's directed broadcast, which is where the beacons go.
    pub fn broadcast(&self) -> Ipv4Addr {
        Ipv4Addr::from(u32::from(self.address) | !u32::from(self.netmask))
    }

    /// Loopback, for tests: no MAC, no other hosts.
    pub fn loopback() -> Self {
        Self {
            name: "lo0".to_owned(),
            address: Ipv4Addr::LOCALHOST,
            netmask: Ipv4Addr::new(255, 0, 0, 0),
            mac: [0; 6],
        }
    }
}

/// The interface the OS would send to `peer` from, out of `interfaces`:
/// with two interfaces on the players' subnet, the one its routing table
/// prefers for them. `None` when no route reaches `peer` or the address it
/// picks is none of theirs.
///
/// Asked of a socket, which connecting tells the local address it would
/// use; connecting a UDP socket sends nothing.
pub fn interface_toward(interfaces: &[Interface], peer: Ipv4Addr) -> Option<Interface> {
    let socket = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((peer, rbl_prolink::PORT_STATUS)).ok()?;
    let IpAddr::V4(local) = socket.local_addr().ok()?.ip() else {
        return None;
    };
    interfaces.iter().find(|i| i.address == local).cloned()
}

/// The interfaces a link network could be on: every IPv4 one that is not
/// loopback, in the order the OS lists them.
pub fn interfaces() -> Vec<Interface> {
    alphatheta_connect::utils::network_interfaces()
        .into_iter()
        .filter(|i| !i.internal)
        .map(|i| Interface {
            name: i.name,
            address: i.address,
            netmask: i.netmask,
            mac: i.mac,
        })
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    #[error("{0}")]
    Bind(String),
    #[error("the library has not loaded")]
    NoLibrary,
}

/// What the app shows about a running link session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub interface: Interface,
    pub database_port: u16,
    pub players: Vec<Player>,
    /// Our tempo-master state: whether we are master and at what BPM.
    pub master: beacon::MasterState,
    /// Where the join is: waiting for a player, probing, up as a number,
    /// or down and why.
    pub link: LinkState,
    /// The players that have mounted the library, by address: the ones a
    /// track can be sent to.
    pub mounted: Vec<Ipv4Addr>,
}

/// A running link export: the beacon and both servers, bound.
pub struct LinkExport {
    interface: Interface,
    beacon: beacon::Beacon,
    database: rbl_dbserver::net::Bound,
    files: rbl_nfs::net::Bound,
    catalog: Arc<IndexCatalog>,
}

struct Facts {
    source: Arc<dyn Source>,
}

impl beacon::LibraryFacts for Facts {
    fn track_count(&self) -> u16 {
        self.source
            .library()
            .map_or(0, |l| u16::try_from(l.len()).unwrap_or(u16::MAX))
    }
    fn playlist_count(&self) -> u16 {
        self.source.library().map_or(0, |l| {
            u16::try_from(l.playlists().len()).unwrap_or(u16::MAX)
        })
    }
    fn device_settings(&self) -> rbl_prolink::DeviceSettings {
        self.source.device_settings()
    }
}

impl LinkExport {
    /// Binds every port and starts serving `source`'s library on `interface`.
    ///
    /// The database and file servers listen only on the selected interface.
    /// LINK discovery still binds on every address and is interface-pinned,
    /// because macOS and Linux do not deliver broadcasts to a socket bound
    /// directly to one address.
    pub fn start(
        source: Arc<dyn Source>,
        interface: Interface,
        ports: Ports,
    ) -> Result<Self, LinkError> {
        let library = source.library().ok_or(LinkError::NoLibrary)?;
        let played = Played::default();
        let catalog = Arc::new(IndexCatalog::new(Arc::clone(&source), played.clone()));

        // Bound first, as rekordbox binds its link stack at launch; but the
        // beacon's join decides when they serve. Its device number, `0`
        // until settled, is what the database server answers with and what
        // opens the port query and the export list to players — rekordbox
        // adds its exports and starts its database server on link-up.
        let beacon = beacon::Beacon::start(
            beacon::BeaconConfig {
                interface: (!interface.address.is_loopback()).then(|| interface.name.clone()),
                address: interface.address,
                netmask: interface.netmask,
                broadcast: interface.broadcast(),
                mac: interface.mac,
                mode: interface_mode::selected(interface.mac),
                announce_port: ports.announce,
                status_port: ports.status,
                player_port: rbl_prolink::PORT_STATUS,
                beat_port: rbl_prolink::PORT_BEAT,
                computer_name: computer_name(),
            },
            Arc::new(Facts { source }),
        )
        .map_err(|e| LinkError::Bind(explain(&e, "UDP", ports.announce)))?;
        let number = beacon.number_cell();

        let handler: Arc<dyn rbl_dbserver::net::Handler> =
            Arc::new(CatalogHandler::new(catalog.clone()).with_device(Arc::clone(&number)));
        let listen_on = service_bind_address(&interface);
        let database =
            rbl_dbserver::net::Bound::start(handler, listen_on, ports.query, ports.database)
                .map_err(|e| LinkError::Bind(explain(&e, "TCP", ports.query)))?;
        // The mount EXPORT reply must offer the export to the player's subnet,
        // which rekordbox names as its own `<ip>/<netmask>`; without it a CDJ
        // mounts nothing. Loopback tests have no meaningful subnet, so skip it.
        let export_host = (!interface.address.is_loopback())
            .then(|| format!("{}/{}", interface.address, interface.netmask));
        let files = rbl_nfs::net::Bound::start(
            files::exports(&library),
            listen_on,
            ports.portmap,
            ports.mount,
            ports.nfs,
            export_host,
            Some(number),
        )
        .map_err(|e| LinkError::Bind(explain(&e, "UDP", ports.portmap)))?;

        tracing::info!(
            interface = %interface.name,
            address = %interface.address,
            database = %database.database_address(),
            nfs = %files.nfs_address(),
            "link export started"
        );
        Ok(Self {
            interface,
            beacon,
            database,
            files,
            catalog,
        })
    }

    /// The beacon's announce and status ports, as bound.
    pub fn beacon_ports(&self) -> (u16, u16) {
        (self.beacon.announce_port(), self.beacon.status_port())
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            interface: self.interface.clone(),
            database_port: self.database.database_address().port(),
            players: self.beacon.players(),
            master: self.beacon.master_state(),
            link: self.beacon.link_state(),
            mounted: self.files.mounted_hosts(),
        }
    }

    /// Where the join is.
    pub fn link_state(&self) -> LinkState {
        self.beacon.link_state()
    }

    /// Become the network's tempo master, or resign.
    pub fn set_master(&self, on: bool) {
        self.beacon.set_master(on);
    }

    /// Nudge the master tempo by `delta_x100` (rekordbox's −/+ is ±100).
    pub fn nudge_master(&self, delta_x100: i32) {
        self.beacon.nudge_master(delta_x100);
    }

    /// Take the tempo of whichever player is currently master as our master
    /// tempo (rekordbox's ⟳ "take the master's tempo" button); does nothing
    /// when no player is master.
    pub fn take_master_tempo(&self) -> bool {
        match self.beacon.current_player_tempo() {
            Some(bpm) => {
                self.beacon.set_master_bpm(bpm);
                true
            }
            None => false,
        }
    }

    /// The port-query service, for a client.
    pub fn query_address(&self) -> std::net::SocketAddr {
        self.database.query_address()
    }

    /// The database server, for a client that skips the port query.
    pub fn database_address(&self) -> std::net::SocketAddr {
        self.database.database_address()
    }

    /// Portmap, for a client.
    pub fn portmap_address(&self) -> std::net::SocketAddr {
        self.files.portmap_address()
    }

    /// After the library's analysis files change: the next request reads
    /// them again rather than serving what was parsed before.
    pub fn analysis_changed(&self) {
        self.catalog.forget_analysis();
    }

    /// Tells a CDJ to load a specific track from our library.
    ///
    /// Refused until the player has mounted the export, as rekordbox refuses
    /// a drag to a player (`NG mnt not complete`): a load command to a
    /// player with nothing mounted is a command it cannot follow.
    pub fn load_track(&self, player_number: u8, track_id: u32) -> std::io::Result<()> {
        let address = self
            .beacon
            .players()
            .into_iter()
            .find(|p| p.number == player_number)
            .map(|p| p.address);
        if let Some(address) = address {
            if !self.files.is_mounted(address) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotConnected,
                    format!("player {player_number} has not mounted the library yet"),
                ));
            }
        }
        self.beacon.load_track(player_number, track_id)
    }

    /// Unbinds everything and waits for the threads.
    pub fn stop(self) {
        self.beacon.stop();
        self.database.shutdown();
        self.files.shutdown();
        tracing::info!("link export stopped");
    }
}

/// The library services are unicast: bind them to the address advertised to
/// players, rather than exposing them on the host's unrelated interfaces.
fn service_bind_address(interface: &Interface) -> IpAddr {
    IpAddr::V4(interface.address)
}

/// A fixed library, for tests and tools.
pub struct StaticSource {
    pub library: Arc<Library>,
    pub share_root: std::path::PathBuf,
}

impl Source for StaticSource {
    fn library(&self) -> Option<Arc<Library>> {
        Some(Arc::clone(&self.library))
    }
    fn share_root(&self) -> std::path::PathBuf {
        self.share_root.clone()
    }
    fn details(&self, _id: &str) -> Option<rbl_db::details::TrackDetails> {
        None
    }
}

/// Why a port could not be bound, in words worth showing.
fn explain(error: &std::io::Error, protocol: &str, port: u16) -> String {
    match error.kind() {
        std::io::ErrorKind::AddrInUse => {
            format!("{protocol} port {port} is already in use — usually by rekordbox itself. Quit it to turn LINK on.")
        }
        std::io::ErrorKind::PermissionDenied => {
            format!("Not allowed to bind {protocol} port {port}.")
        }
        _ => format!("Could not bind {protocol} port {port}: {error}"),
    }
}

/// This computer's name, for the identity reply a player asks for. Windows
/// hands it out in the environment; elsewhere `hostname` does, trimmed of any
/// domain. Falls back to `rekordbox` — the reply's presence is what a player
/// waits on, not the exact name.
fn computer_name() -> String {
    if let Ok(name) = std::env::var("COMPUTERNAME") {
        if !name.is_empty() {
            return name;
        }
    }
    if let Ok(output) = std::process::Command::new("hostname").output() {
        if let Ok(name) = String::from_utf8(output.stdout) {
            let name = name.trim().split('.').next().unwrap_or("").trim();
            if !name.is_empty() {
                return name.to_owned();
            }
        }
    }
    rbl_prolink::REKORDBOX_NAME.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NotationSource(KeyNotation);

    impl Source for NotationSource {
        fn library(&self) -> Option<Arc<Library>> {
            None
        }

        fn key_notation(&self) -> KeyNotation {
            self.0
        }

        fn share_root(&self) -> std::path::PathBuf {
            std::path::PathBuf::new()
        }

        fn details(&self, _id: &str) -> Option<rbl_db::details::TrackDetails> {
            None
        }
    }

    #[test]
    fn library_services_bind_to_the_selected_interface_address() {
        let interface = Interface {
            name: "link0".into(),
            address: Ipv4Addr::new(192, 168, 22, 7),
            netmask: Ipv4Addr::new(255, 255, 255, 0),
            mac: [0; 6],
        };

        assert_eq!(
            service_bind_address(&interface),
            IpAddr::V4(interface.address)
        );
    }

    #[test]
    fn link_device_settings_follow_the_catalog_key_notation() {
        for (notation, display) in [
            (KeyNotation::Classic, rbl_prolink::KeyDisplay::Classic),
            (
                KeyNotation::Alphanumeric,
                rbl_prolink::KeyDisplay::Alphanumeric,
            ),
        ] {
            let facts = Facts {
                source: Arc::new(NotationSource(notation)),
            };

            assert_eq!(
                beacon::LibraryFacts::device_settings(&facts).key_display,
                display
            );
        }
    }
}
