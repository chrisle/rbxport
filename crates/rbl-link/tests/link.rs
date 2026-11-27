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
use rbl_dbserver::{kind, Argument, Message as DbMessage};
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
    assert_eq!(root.len(), 9);
    assert_eq!(root[0][6], Argument::Number(0x81));
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

/// A one-playlist fixture whose members each carry a distinct synthetic
/// preview, served on loopback.
struct PlaylistServed {
    _dir: tempfile::TempDir,
    link: LinkExport,
    /// The playlist's `djmdPlaylist.ID`.
    playlist: u32,
    /// The members' `djmdContent.ID`s in playlist order, each with the
    /// `4402` blob its own `.DAT` must come back as.
    previews: Vec<(u32, Vec<u8>)>,
}

/// How many tracks the playlist holds: enough for several render windows
/// and for the right-aligned clamp of the last one.
const PLAYLIST_MEMBERS: usize = 5;

/// Builds the fixture: every member gets a `.DAT` whose 400 `PWAV` and 100
/// `PWV2` columns are one value, different per track, so a preview served
/// for the wrong track cannot pass as the right one. The expected reply is
/// built from those same columns by the blob encoder the server uses.
fn serve_playlist() -> PlaylistServed {
    let dir = tempfile::tempdir().unwrap();
    let shape = Shape {
        tracks: PLAYLIST_MEMBERS,
        playlists: 1,
        tracks_per_playlist: PLAYLIST_MEMBERS,
        history_sessions: 1,
        start_usn: 1000,
    };
    let location = fixture::build(dir.path(), shape).unwrap();
    let mut blobs = Vec::with_capacity(PLAYLIST_MEMBERS);
    for index in 0..PLAYLIST_MEMBERS {
        let step = u8::try_from(index).unwrap();
        // Heights 3, 6, 9, 12, 15 under one whiteness bit; PWV2 2, 4, 6, 8,
        // 10: all inside the ranges the encoder leaves unchanged.
        let pwav = vec![0x40 | (3 + 3 * step); 400];
        let pwv2 = vec![2 + 2 * step; 100];
        let analysis = format!("/PIONEER/USBANLZ/P001/{index:08X}/ANLZ0000.DAT");
        fixture::set_analysis_path(&location, index, &analysis).unwrap();
        let dat = rbl_anlz::resolve(&location.share_root, &analysis);
        fs::create_dir_all(dat.parent().unwrap()).unwrap();
        fs::write(
            &dat,
            rbl_anlz::AnlzBuilder::new()
                .waveform_preview(b"PWAV", &pwav)
                .waveform_preview(b"PWV2", &pwv2)
                .finish(),
        )
        .unwrap();
        blobs.push(rbl_link::blobs::waveform_preview_blob(&pwav, &pwv2));
    }

    let db = Db::open(location.clone(), OpenMode::ReadOnly).unwrap();
    let (library, _) = rbl_index::load(&db).unwrap();
    let library = Arc::new(library);
    // The oracle comes from the index, not from a rendered menu: the
    // members in playlist order, each tied back to its fixture track by id.
    let (playlist, previews) = {
        let playlists = library.playlists();
        let playlist = u32::try_from(playlists.ids[0]).unwrap();
        let previews = playlists.members[0]
            .iter()
            .map(|&row| {
                let id = u32::try_from(library.ids[row as usize]).unwrap();
                let index = (0..PLAYLIST_MEMBERS)
                    .find(|&index| fixture::track_id(index).parse() == Ok(id))
                    .unwrap();
                (id, blobs[index].clone())
            })
            .collect::<Vec<_>>();
        (playlist, previews)
    };
    assert_eq!(previews.len(), PLAYLIST_MEMBERS);

    let source = Arc::new(StaticSource {
        library,
        share_root: location.share_root.clone(),
    });
    let link = LinkExport::start(source, Interface::loopback(), Ports::EPHEMERAL).unwrap();
    bring_up(&link);
    PlaylistServed {
        _dir: dir,
        link,
        playlist,
        previews,
    }
}

/// Asks for a menu and returns its row count, leaving the rows for render.
fn open_menu(db: &mut Database, request: u16, args: &[u32]) -> u32 {
    let header = db.request(request, numbers(args)).unwrap();
    assert_eq!(header.len(), 1, "{header:?}");
    assert_eq!(header[0].kind, kind::MENU_HEADER, "{header:?}");
    let Argument::Number(count) = header[0].arguments[1] else {
        panic!()
    };
    count
}

/// Renders one window of the menu at `CTX`'s location, as a player paging
/// a list does: the offset the header names and the rows that follow.
fn render_page(
    db: &mut Database,
    offset: u32,
    limit: u32,
    total: u32,
) -> (u32, Vec<Vec<Argument>>) {
    let mut messages = db
        .request(kind::RENDER, numbers(&[CTX, offset, limit, 0, total, 0xc, 1, 0]))
        .unwrap();
    let mut header = None;
    let mut rows = Vec::new();
    loop {
        assert!(!messages.is_empty(), "the session closed before the menu footer");
        let mut batch = messages.into_iter();
        while let Some(message) = batch.next() {
            if message.kind == kind::RENDER_HEADER {
                let Argument::Number(offset) = message.arguments[1] else {
                    panic!("the render header did not name its offset")
                };
                assert!(header.replace((message.transaction, offset)).is_none(), "duplicate render header");
                continue;
            }
            let (transaction, offset) = header.expect("rendered item or footer before the render header");
            assert_eq!(message.transaction, transaction, "message belongs to another request");
            match message.kind {
                kind::MENU_ITEM => rows.push(message.arguments),
                kind::MENU_FOOTER => {
                    assert!(batch.next().is_none(), "extra message after the render footer");
                    return (offset, rows);
                }
                other => panic!("{other:#06x} while rendering"),
            }
        }
        messages = db.receive().unwrap();
    }
}

/// The `4402` reply a preview blob goes out as: `[2004, 0, len, blob]`.
fn preview_reply(blob: &[u8]) -> Vec<Argument> {
    vec![
        Argument::Number(u32::from(kind::WAVEFORM_PREVIEW)),
        Argument::Number(0),
        Argument::Number(u32::try_from(blob.len()).unwrap()),
        Argument::Blob(blob.to_vec()),
    ]
}

/// The preview of `track`, asked for in the shape the browse test above
/// uses: `[ctx, 0, id, 0]`, the id in the third argument.
fn preview_of(db: &mut Database, track: u32) -> Vec<Argument> {
    let reply = db
        .request(kind::WAVEFORM_PREVIEW, numbers(&[CTX, 0, track, 0]))
        .unwrap();
    assert_eq!(reply.len(), 1, "{reply:?}");
    assert_eq!(reply[0].kind, kind::WAVEFORM_PREVIEW_REPLY);
    reply[0].arguments.clone()
}

/// One pass of a player through the playlist: in from the folder menu,
/// then `limit` rows a window until the end, each rendered row's id fed to
/// a preview request and checked against the oracle for that position.
fn page_playlist(db: &mut Database, served: &PlaylistServed, limit: u32, pass: usize) {
    let total = u32::try_from(served.previews.len()).unwrap();
    assert_eq!(open_menu(db, kind::PLAYLIST_MENU, &[CTX, 0, 0, 1]), 1);
    let (_, lists) = render_page(db, 0, 1, 1);
    assert_eq!(lists[0][1], Argument::Number(served.playlist));
    assert_eq!(open_menu(db, kind::PLAYLIST_MENU, &[CTX, 0, served.playlist, 0]), total);
    let mut seen = Vec::new();
    let mut offset = 0;
    while offset < total {
        let (at, rows) = render_page(db, offset, limit, total);
        let drawn = u32::try_from(rows.len()).unwrap();
        // The last window is right-aligned to keep its size, so the header's
        // offset, not the request's, says which rows these are.
        let count = limit.min(total);
        assert_eq!(
            (at, drawn), (offset.min(total - count), count),
            "pass {pass}: window for offset {offset}"
        );
        for (k, row) in rows.iter().enumerate() {
            let position = at + u32::try_from(k).unwrap();
            let (id, blob) = &served.previews[usize::try_from(position).unwrap()];
            let Argument::Number(rendered) = row[1] else {
                panic!()
            };
            assert_eq!(rendered, *id, "pass {pass}: the id at row {position}");
            assert_eq!(
                row[9],
                Argument::Number(position + 1),
                "pass {pass}: the playlist position at row {position}"
            );
            // What goes to `2004` is the id the row carried, not the oracle's.
            assert_eq!(
                preview_of(db, rendered),
                preview_reply(blob),
                "pass {pass}: the preview of row {position}"
            );
            seen.push(rendered);
        }
        offset += limit;
    }
    seen.sort_unstable();
    seen.dedup();
    let mut members: Vec<u32> = served.previews.iter().map(|(id, _)| *id).collect();
    members.sort_unstable();
    assert_eq!(seen, members, "pass {pass}: every member was rendered");
}

/// The player has gone back to the folder menu, replacing the playlist at
/// this location, and now asks for the previews of the rows it just left:
/// all at once, in reverse, without waiting. Each must be answered once,
/// under its own transaction, with that row's bytes; the menu is untouched.
fn late_previews_after_leaving(db: &mut Database, served: &PlaylistServed, base: u32) {
    assert_eq!(open_menu(db, kind::PLAYLIST_MENU, &[CTX, 0, 0, 1]), 1);
    // Transaction `base + k` asks for the member at `k`.
    let transactions: Vec<u32> = (0..u32::try_from(served.previews.len()).unwrap())
        .map(|k| base + k)
        .collect();
    for (transaction, (id, _)) in transactions.iter().zip(&served.previews).rev() {
        db.send(&DbMessage::new(
            *transaction,
            kind::WAVEFORM_PREVIEW,
            numbers(&[CTX, 0, *id, 0]),
        ))
        .unwrap();
    }
    let mut replies = Vec::new();
    while replies.len() < served.previews.len() {
        let batch = db.receive().unwrap();
        assert!(!batch.is_empty(), "the session closed with previews outstanding");
        replies.extend(batch);
    }
    // One reply under each transaction sent: the right count with one
    // answered twice and another never is still a lost preview.
    let mut answered: Vec<u32> = replies.iter().map(|reply| reply.transaction).collect();
    answered.sort_unstable();
    assert_eq!(answered, transactions, "{replies:?}");
    for reply in replies {
        let (id, blob) = &served.previews[usize::try_from(reply.transaction - base).unwrap()];
        assert_eq!(reply.kind, kind::WAVEFORM_PREVIEW_REPLY, "late preview of {id}");
        assert_eq!(reply.arguments, preview_reply(blob), "late preview of {id}");
    }
    let (_, lists) = render_page(db, 0, 1, 1);
    assert_eq!(
        lists[0][1],
        Argument::Number(served.playlist),
        "the folder menu is still what renders here"
    );
}

/// #768: blank previews reported after browsing a playlist again and
/// again. Over the wire, a preview depends only on the id a row carried
/// and the track's `.DAT`, never on the menu that is open: this feeds the
/// ids of successive rendered positions into `2004`, re-enters the
/// playlist across menu replacement three times with two window sizes,
/// lets a second player draw the same list through the shared caches in
/// between, and asks late, pipelined previews for rows already left. The
/// detail waveform (`2904`, `PWV3` from the `.EXT`) is a separate reply
/// and is not asserted here.
#[test]
fn repeated_playlist_paging_feeds_each_rows_id_to_its_own_preview() {
    let served = serve_playlist();
    // Previews that could be mistaken for one another would prove nothing.
    let mut blobs: Vec<&Vec<u8>> = served.previews.iter().map(|(_, blob)| blob).collect();
    blobs.sort();
    blobs.dedup();
    assert_eq!(blobs.len(), served.previews.len(), "the fixture's previews differ");

    let mut one = Database::connect(served.link.database_address(), 1).unwrap();
    let mut two = Database::connect(served.link.database_address(), 2).unwrap();
    let total = u32::try_from(served.previews.len()).unwrap();
    for (pass, limit) in [2_u32, 3, 2].into_iter().enumerate() {
        page_playlist(&mut one, &served, limit, pass);
        // The other player draws the whole list between passes; the menus
        // and parsed files it shares with player one must not cross over.
        assert_eq!(open_menu(&mut two, kind::PLAYLIST_MENU, &[CTX, 0, served.playlist, 0]), total);
        let (_, rows) = render_page(&mut two, 0, total, total);
        assert_eq!(rows.len(), served.previews.len());
        for (row, (id, blob)) in rows.iter().zip(&served.previews) {
            let Argument::Number(rendered) = row[1] else {
                panic!()
            };
            assert_eq!(rendered, *id, "pass {pass}: player two's row for {id}");
            assert_eq!(
                preview_of(&mut two, rendered),
                preview_reply(blob),
                "pass {pass}: player two's preview of {id}"
            );
        }
        late_previews_after_leaving(
            &mut one,
            &served,
            0x7680_0000 + u32::try_from(pass).unwrap() * 0x100,
        );
        // Player one has returned to the root. Player two can still render
        // its existing playlist menu without opening it again.
        let (_, still_open) = render_page(&mut two, 0, total, total);
        assert_eq!(still_open, rows, "pass {pass}: player one's menu change leaked");
    }
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
    assert_eq!(track.title, "At Your Best");
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
