//! Link export end to end: real sockets on loopback, our servers, and two
//! players — `rbl-fakecdj`, which speaks exactly what the capture showed,
//! and alphatheta-connect's remote-database client, which was written
//! against real rekordbox and real CDJs and has never seen this crate.
//!
//! Every port is ephemeral: rekordbox holds the real ones whenever it runs.
#![allow(
    clippy::pedantic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use std::fs;
use std::sync::Arc;

use alphatheta_connect::remotedb::fields::{read_field, Field, FieldType};
use alphatheta_connect::remotedb::message::{control_request, Message};
use alphatheta_connect::remotedb::{queries, Connection, LookupDescriptor, MenuTarget};
use alphatheta_connect::types::{Device, DeviceType as ClientDeviceType, MediaSlot, TrackType};
use rbl_db::fixture::{self, Shape};
use rbl_db::{Library as Db, OpenMode};
use rbl_dbserver::{kind, Argument};
use rbl_fakecdj::{database_port, mount, Database};
use rbl_link::{Interface, LinkExport, Ports, StaticSource};
use tokio::io::AsyncWriteExt;

const DAT: &[u8] = include_bytes!("fixtures/ANLZ0001.DAT");
const EXT: &[u8] = include_bytes!("fixtures/ANLZ0001.EXT");
const LINK_DEVICE_NUMBER: u8 = 0x11;

struct Served {
    _dir: tempfile::TempDir,
    link: LinkExport,
    /// The bytes of track 0's audio file.
    audio: Vec<u8>,
    /// Track 0's `djmdContent.ID`.
    track: u32,
    path: String,
}

/// A three-track fixture library with one real audio file and the captured
/// analysis files behind track 0, served on loopback.
fn serve() -> Served {
    let dir = tempfile::tempdir().unwrap();
    let audio: Vec<u8> = (0..100_000_u32).map(|i| (i % 253) as u8).collect();
    let audio_dir = dir.path().join("Music").join("Crate");
    fs::create_dir_all(&audio_dir).unwrap();
    let path = audio_dir.join("At Your Best.mp3");
    fs::write(&path, &audio).unwrap();

    let shape = Shape {
        tracks: 3,
        playlists: 1,
        tracks_per_playlist: 2,
        history_sessions: 1,
        start_usn: 1000,
    };
    let location = fixture::build(dir.path(), shape).unwrap();
    fixture::point_at_audio(&location, 0, path.to_str().unwrap(), 290).unwrap();
    let analysis = "/PIONEER/USBANLZ/P001/0000ABCD/ANLZ0000.DAT";
    fixture::set_analysis_path(&location, 0, analysis).unwrap();
    let dat = rbl_anlz::resolve(&location.share_root, analysis);
    fs::create_dir_all(dat.parent().unwrap()).unwrap();
    fs::write(&dat, DAT).unwrap();
    fs::write(rbl_anlz::sibling(&dat, "EXT"), EXT).unwrap();

    let db = Db::open(location.clone(), OpenMode::ReadOnly).unwrap();
    let (library, _) = rbl_index::load(&db).unwrap();
    let track = u32::try_from(library.ids[0]).unwrap();
    let source = Arc::new(StaticSource {
        library: Arc::new(library),
        share_root: location.share_root.clone(),
    });
    let link = LinkExport::start(source, Interface::loopback(), Ports::EPHEMERAL).unwrap();
    bring_up(&link);
    Served {
        _dir: dir,
        link,
        audio,
        track,
        path: path.to_str().unwrap().to_owned(),
    }
}

/// A CDJ-3000's keep-alive, the one the beacon tests use.
const CDJ_KEEP_ALIVE: &str =
    "5173707431576d4a4f4c060043444a2d333030300000000000000000000000000103003601012497ed0b4043c0a80198030000000164";

/// Until a player is heard rekordbox serves nothing: the port query goes
/// unanswered and the export list is empty. A keep-alive from a player
/// starts the join, which settles on 17 about four seconds later.
fn bring_up(link: &LinkExport) {
    use std::net::{Ipv4Addr, UdpSocket};
    use std::time::{Duration, Instant};
    assert_eq!(link.link_state(), rbl_link::LinkState::Waiting);
    assert!(
        database_port(link.query_address()).is_err(),
        "no port query answer before the link is up"
    );
    let player = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let packet: Vec<u8> = (0..CDJ_KEEP_ALIVE.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&CDJ_KEEP_ALIVE[i..i + 2], 16).unwrap())
        .collect();
    player
        .send_to(&packet, (Ipv4Addr::LOCALHOST, link.beacon_ports().0))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while link.link_state()
        != (rbl_link::LinkState::Up {
            number: LINK_DEVICE_NUMBER,
        })
    {
        assert!(
            Instant::now() < deadline,
            "the link did not come up: {:?}",
            link.link_state()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn numbers(args: &[u32]) -> Vec<Argument> {
    args.iter().map(|&n| Argument::Number(n)).collect()
}

/// The player's context word, as captured.
const CTX: u32 = 0x0101_0301;

/// Asks for a menu and renders it whole.
fn browse(db: &mut Database, request: u16, args: &[u32]) -> Vec<Vec<Argument>> {
    let header = db.request(request, numbers(args)).unwrap();
    assert_eq!(header[0].kind, kind::MENU_HEADER, "{header:?}");
    let Argument::Number(count) = header[0].arguments[1] else {
        panic!()
    };
    let mut items = Vec::new();
    let mut messages = db
        .request(kind::RENDER, numbers(&[CTX, 0, count, 0, count, 0xc, 1, 0]))
        .unwrap();
    loop {
        for message in messages.drain(..) {
            if message.kind == kind::MENU_FOOTER {
                return items;
            }
            if message.kind == kind::MENU_ITEM {
                items.push(message.arguments);
            }
        }
        messages = db.receive().unwrap();
    }
}

#[test]
fn a_player_browses_loads_and_reads_a_track_as_the_capture_shows() {
    let served = serve();

    // The port query names the database server.
    let port = database_port(served.link.query_address()).unwrap();
    assert_eq!(port, served.link.database_address().port());
    let mut db = Database::connect(served.link.database_address(), 1).unwrap();

    // The root menu and the collection.
    let root = browse(&mut db, kind::ROOT_MENU, &[CTX, 0, 0x5cf_ffff]);
    assert_eq!(root.len(), 20);
    assert_eq!(root[3][6], Argument::Number(0x80));
    let tracks = browse(&mut db, kind::TRACK_MENU, &[CTX, 0]);
    assert_eq!(tracks.len(), 3);
    let Argument::Number(first) = tracks[0][1] else {
        panic!()
    };
    assert_eq!(
        first, served.track,
        "alphabetical: 'At Your Best' before 'Track 001'"
    );

    // A playlist: the fixture's one list, with two tracks in its own order.
    let lists = browse(&mut db, kind::PLAYLIST_MENU, &[CTX, 0, 0, 1]);
    assert_eq!(lists.len(), 1);
    let Argument::Number(playlist) = lists[0][1] else {
        panic!()
    };
    let members = browse(&mut db, kind::PLAYLIST_MENU, &[CTX, 0, playlist, 0]);
    assert_eq!(members.len(), 2);
    assert_eq!(
        members[0][9],
        Argument::Number(1),
        "the first row is position 1"
    );

    // Track info carries the absolute path, which the player then opens over NFS.
    let info = browse(&mut db, kind::TRACK_INFO, &[CTX, served.track]);
    assert_eq!(info.len(), 7);
    assert_eq!(info[4][3], Argument::String(served.path.clone()));
    assert_eq!(
        info[4][0],
        Argument::Number(u32::try_from(served.audio.len()).unwrap()),
        "file size"
    );

    // The analysis blobs are the captured bytes for this track.
    let grid = db
        .request(kind::BEAT_GRID, numbers(&[CTX, served.track]))
        .unwrap();
    assert_eq!(grid[0].kind, kind::BEAT_GRID_REPLY);
    assert_eq!(
        grid[0].arguments[3],
        Argument::Blob(include_bytes!("fixtures/captured-beat-grid.bin").to_vec())
    );
    let preview = db
        .request(kind::WAVEFORM_PREVIEW, numbers(&[CTX, 0, served.track, 0]))
        .unwrap();
    assert_eq!(
        preview[0].arguments[3],
        Argument::Blob(include_bytes!("fixtures/captured-waveform-preview.bin").to_vec())
    );
    let tag = db
        .request(
            kind::ANLZ_TAG,
            numbers(&[CTX, served.track, u32::from_le_bytes(*b"PWV4"), 0x0054_5845]),
        )
        .unwrap();
    assert_eq!(
        tag[0].arguments[3],
        Argument::Blob(include_bytes!("fixtures/captured-tag-PWV4.bin").to_vec())
    );

    // A track the fixture never analysed has no waveform, and says so the
    // way rekordbox does rather than failing.
    let Argument::Number(other) = tracks[1][1] else {
        panic!()
    };
    let none = db.request(kind::BEAT_GRID, numbers(&[CTX, other])).unwrap();
    assert_eq!(none[0].arguments[1], Argument::Number(0x32));

    // The file, read the way a player reads it.
    let (export, relative) = rbl_link::files::split(&served.path).unwrap();
    let mut mounted = mount(served.link.portmap_address(), &export).unwrap();
    assert_eq!(mounted.read_file(&relative).unwrap(), served.audio);

    let snapshot = served.link.snapshot();
    assert_eq!(snapshot.database_port, port);
    served.link.stop();
}

#[test]
fn a_second_player_only_sees_its_own_list() {
    let served = serve();
    let mut one = Database::connect(served.link.database_address(), 1).unwrap();
    let mut two = Database::connect(served.link.database_address(), 2).unwrap();
    let artists = browse(&mut one, kind::ARTIST_MENU, &[CTX, 0]);
    let tracks = browse(&mut two, kind::TRACK_MENU, &[CTX, 0]);
    assert!(artists.is_empty(), "the fixture names no artists");
    assert_eq!(tracks.len(), 3);
    // Player one's menu is still the artist menu: rendering it again is empty,
    // not player two's tracks.
    let again = one
        .request(kind::RENDER, numbers(&[CTX, 0, 25, 0, 25, 0xc, 1, 0]))
        .unwrap();
    assert!(again.iter().all(|m| m.kind != kind::MENU_ITEM));
    served.link.stop();
}

/// The connection alphatheta-connect opens to rekordbox, minus the port
/// query it hard-codes to 12523.
///
/// A one-argument introduction selects the legacy reply and twelve-field
/// rows, as measured against rekordbox 7.2.11 on 2026-09-20.
async fn client_connection(served: &Served) -> (Connection, LookupDescriptor) {
    let mut socket = tokio::net::TcpStream::connect(served.link.database_address())
        .await
        .unwrap();
    socket
        .write_all(&Field::UInt32(1).to_bytes())
        .await
        .unwrap();
    let hello = read_field(&mut socket, FieldType::UInt32).await.unwrap();
    assert_eq!(hello.as_number(), Some(1));
    let intro = Message::with_transaction(
        0xffff_fffe,
        control_request::INTRODUCE,
        vec![Field::UInt32(3)],
    );
    socket.write_all(&intro.to_bytes()).await.unwrap();
    let reply = Message::from_stream(&mut socket, 0x4000).await.unwrap();
    assert_eq!(reply.message_type, 0x4000);
    assert_eq!(
        reply.args.get(1).and_then(Field::as_number),
        Some(u32::from(LINK_DEVICE_NUMBER)),
        "answered as device 17"
    );

    let rekordbox = Device::new(
        "rekordbox",
        LINK_DEVICE_NUMBER,
        ClientDeviceType::Rekordbox,
        [0; 6],
        std::net::Ipv4Addr::LOCALHOST,
    );
    let host = Device::new(
        "CDJ-3000",
        3,
        ClientDeviceType::Cdj,
        [0; 6],
        std::net::Ipv4Addr::LOCALHOST,
    );
    let descriptor = LookupDescriptor {
        menu_target: MenuTarget::Main,
        track_slot: MediaSlot::Rb,
        track_type: TrackType::Rb,
        target_device: rekordbox.clone(),
        host_device: host,
    };
    (Connection::new(rekordbox, socket), descriptor)
}

#[tokio::test]
async fn alphatheta_connects_client_reads_metadata_and_analysis_from_us() {
    let served = tokio::task::spawn_blocking(serve).await.unwrap();
    let (conn, d) = client_connection(&served).await;

    let track = queries::get_metadata(&conn, &d, served.track)
        .await
        .unwrap();
    // This client revision recognizes only a bare 0x0004 title row, while
    // legacy players receive the captured composite 0x2304 track row.
    assert_eq!(track.duration, 290.0);
    assert!((track.tempo - 128.0).abs() < 0.01, "{}", track.tempo);
    assert_eq!(track.comment, "");

    let path = queries::get_track_info(&conn, &d, served.track)
        .await
        .unwrap();
    assert_eq!(path, served.path);

    let grid = queries::get_beatgrid(&conn, &d, served.track)
        .await
        .unwrap();
    assert!(!grid.is_empty(), "the captured track's beats");
    assert!((grid[0].bpm - 78.08).abs() < 0.01, "{}", grid[0].bpm);

    let preview = queries::get_waveform_preview(&conn, &d, served.track)
        .await
        .unwrap();
    assert_eq!(preview.len(), 400);

    let detailed = queries::get_waveform_detailed(&conn, &d, served.track)
        .await
        .unwrap();
    assert!(!detailed.is_empty());

    let missing = queries::get_metadata(&conn, &d, 0xdead_beef).await.unwrap();
    assert_eq!(
        missing.duration, 0.0,
        "an unknown track is an empty menu, not an error"
    );

    conn.close().await;
    tokio::task::spawn_blocking(move || served.link.stop())
        .await
        .unwrap();
}
