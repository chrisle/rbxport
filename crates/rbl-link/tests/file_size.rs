//! Track-info byte counts and the independently served audio file.
#![allow(
    clippy::pedantic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]
use rbl_dbserver::catalog::Catalog;
use rbl_dbserver::net::{Handler, Session};
use rbl_dbserver::{kind, Argument, Message};
use rbl_link::catalog::{IndexCatalog, Played, Source};
use rbl_nfs::rpc::{self, Auth, Call, Reply};
use rbl_nfs::xdr::{Reader, Writer};
use rbl_nfs::{
    mount_proc, nfs_proc, nfs_status, Handle, Server, HANDLE_LEN, PROGRAM_MOUNT, PROGRAM_NFS,
    VERSION_MOUNT, VERSION_NFS,
};
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};
struct FixtureSource {
    library: Arc<rbl_index::Library>,
    detail: rbl_db::details::TrackDetails,
    share: PathBuf,
}
impl Source for FixtureSource {
    fn library(&self) -> Option<Arc<rbl_index::Library>> {
        Some(self.library.clone())
    }
    fn share_root(&self) -> PathBuf {
        self.share.clone()
    }
    fn details(&self, _: &str) -> Option<rbl_db::details::TrackDetails> {
        Some(self.detail.clone())
    }
}
fn numbers(k: u16, x: u32, args: &[u32]) -> Message {
    Message::new(x, k, args.iter().copied().map(Argument::Number).collect())
}
fn menu(s: &mut Box<dyn Session>, id: u32) -> (u32, String) {
    let header = s.handle(&numbers(kind::TRACK_INFO, 0x100, &[0x01010301, id]));
    let Argument::Number(n) = header[0].arguments[1] else {
        panic!()
    };
    assert_eq!(n, 7);
    let rows = s.handle(&numbers(
        kind::RENDER,
        0x101,
        &[0x01010301, 0, n, 0, n, 0xc, 1, 0],
    ));
    assert_eq!(rows.len(), 9);
    assert_eq!(rows[1].arguments[1], Argument::Number(1));
    let Argument::Number(size) = rows[5].arguments[0] else {
        panic!()
    };
    let Argument::String(ref path) = rows[5].arguments[3] else {
        panic!()
    };
    (size, path.clone())
}
static XID: AtomicU32 = AtomicU32::new(0x1234);
fn ask(s: &Server, p: u32, v: u32, f: u32, a: Vec<u8>) -> Vec<u8> {
    let call = Call {
        xid: XID.fetch_add(1, Ordering::Relaxed),
        program: p,
        version: v,
        procedure: f,
        credential: Auth {
            flavor: rpc::AUTH_UNIX,
            body: vec![0; 8],
        },
        verifier: Auth::null(),
        arguments: &a,
    };
    s.handle(&call.encode()).unwrap()
}
fn ok(b: &[u8]) -> Reader<'_> {
    let r = Reply::decode(b).unwrap();
    assert!(r.is_success());
    r.reader()
}
fn open(s: &Server, path: &str) -> Handle {
    let (export, relative) = rbl_link::files::split(path).unwrap();
    let mut a = Writer::new();
    a.utf16(&export);
    let b = ask(
        s,
        PROGRAM_MOUNT,
        VERSION_MOUNT,
        mount_proc::MNT,
        a.into_bytes(),
    );
    let mut r = ok(&b);
    assert_eq!(r.u32().unwrap(), nfs_status::OK);
    let mut h = Handle::from_slice(r.opaque_fixed(HANDLE_LEN).unwrap()).unwrap();
    for part in relative.split('/').filter(|p| !p.is_empty()) {
        let mut a = Writer::new();
        a.opaque_fixed(h.as_bytes()).utf16(part);
        let b = ask(
            s,
            PROGRAM_NFS,
            VERSION_NFS,
            nfs_proc::LOOKUP,
            a.into_bytes(),
        );
        let mut r = ok(&b);
        assert_eq!(r.u32().unwrap(), nfs_status::OK);
        h = Handle::from_slice(r.opaque_fixed(HANDLE_LEN).unwrap()).unwrap();
    }
    h
}
fn read(s: &Server, h: &Handle, offset: u32, count: u32) -> Result<(u32, Vec<u8>), u32> {
    let mut a = Writer::new();
    a.opaque_fixed(h.as_bytes()).u32(offset).u32(count).u32(0);
    let b = ask(s, PROGRAM_NFS, VERSION_NFS, nfs_proc::READ, a.into_bytes());
    let mut r = ok(&b);
    let status = r.u32().unwrap();
    if status != nfs_status::OK {
        return Err(status);
    }
    let mut size = 0;
    for i in 0..17 {
        let n = r.u32().unwrap();
        if i == 5 {
            size = n;
        }
    }
    let data = r.opaque().unwrap().to_vec();
    assert_eq!(r.remaining(), 0);
    Ok((size, data))
}
#[test]
fn track_info_size_tracks_a_key_tag_rewrite_without_changing_served_audio() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let location = rbl_db::fixture::build(
        &root.join("wire-library"),
        rbl_db::fixture::Shape {
            tracks: 1,
            playlists: 1,
            tracks_per_playlist: 1,
            history_sessions: 0,
            start_usn: 1000,
        },
    )
    .unwrap();
    let audio = root.join("wire-noise.mp3");
    fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../rbl-deck/tests/fixtures/seek-noise.mp3"),
        &audio,
    )
    .unwrap();
    rbl_db::fixture::point_at_audio(&location, 0, audio.to_str().unwrap(), 2).unwrap();
    let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadWrite).unwrap();
    let id = rbl_db::fixture::track_id(0);
    let initial = fs::metadata(&audio).unwrap().len();
    db.connection()
        .execute(
            "UPDATE djmdContent SET FileSize=?1,FileType=1 WHERE ID=?2",
            (i64::try_from(initial).unwrap(), &id),
        )
        .unwrap();
    let (library, _) = rbl_index::load(&db).unwrap();
    let library = Arc::new(library);
    let source = Arc::new(FixtureSource {
        library: library.clone(),
        detail: db.track_details(&id).unwrap().unwrap(),
        share: location.share_root,
    });
    let catalog = Arc::new(IndexCatalog::new(source, Played::default()));
    let mut session = rbl_dbserver::session::CatalogHandler::new(catalog.clone()).open();
    session.handle(&rbl_dbserver::setup_request(1));
    let before = menu(&mut session, id.parse().unwrap());
    assert_eq!(before.0 as u64, initial);
    let server = Server::new(rbl_link::files::exports(&library), 12049, 12005);
    let handle = open(&server, &before.1);
    let pcm = rbl_audio::decode_mono(&audio, None).unwrap();
    rbl_db::key_tag::write_key(&audio, "12A").unwrap();
    let after_pcm = rbl_audio::decode_mono(&audio, None).unwrap();
    assert_eq!(pcm.sample_rate, after_pcm.sample_rate);
    assert_eq!(pcm.samples, after_pcm.samples);
    let bytes = fs::read(&audio).unwrap();
    let after = menu(&mut session, id.parse().unwrap());
    assert_eq!(
        db.track_details(&id).unwrap().unwrap().file_size,
        initial,
        "the saved size has not been refreshed by the tag writer"
    );
    assert_ne!(
        initial as usize,
        bytes.len(),
        "the valid MP3 tag rewrite changes the container size"
    );
    for count in [2048, 16384, 32768] {
        let mut offset = 0u32;
        let mut all = Vec::new();
        loop {
            let (size, chunk) = read(&server, &handle, offset, count).unwrap();
            assert_eq!(size as usize, bytes.len());
            offset += chunk.len() as u32;
            let end = chunk.len() < count as usize;
            all.extend(chunk);
            if end {
                break;
            }
        }
        assert_eq!(all, bytes);
        assert_eq!(
            read(&server, &handle, bytes.len() as u32, count),
            Err(nfs_status::IO)
        );
    }
    fs::remove_file(&audio).unwrap();
    assert_eq!(read(&server, &handle, 0, 32768), Err(nfs_status::IO));
    assert_eq!(menu(&mut session, id.parse().unwrap()).0, initial as u32);
    assert!(catalog.track(id.parse().unwrap()).is_some());
    assert_eq!(
        after.0 as usize,
        bytes.len(),
        "track-info must advertise the current file that NFS serves"
    );
}

fn size_for(path: &std::path::Path, stored: Option<u64>) -> u32 {
    let mut library = rbl_index::testing::library_from(&[rbl_index::testing::TestTrack {
        id: 1,
        ..Default::default()
    }]);
    library.folder_path = Default::default();
    library.folder_path.push(path.to_str().unwrap());
    let library = Arc::new(library);
    let source = Arc::new(BoundarySource {
        library,
        details: stored.map(|file_size| rbl_db::details::TrackDetails {
            file_size,
            ..Default::default()
        }),
    });
    let mut session = rbl_dbserver::session::CatalogHandler::new(Arc::new(IndexCatalog::new(
        source,
        Played::default(),
    )))
    .open();
    session.handle(&rbl_dbserver::setup_request(1));
    menu(&mut session, 1).0
}
struct BoundarySource {
    library: Arc<rbl_index::Library>,
    details: Option<rbl_db::details::TrackDetails>,
}
impl Source for BoundarySource {
    fn library(&self) -> Option<Arc<rbl_index::Library>> {
        Some(self.library.clone())
    }
    fn share_root(&self) -> PathBuf {
        PathBuf::new()
    }
    fn details(&self, _: &str) -> Option<rbl_db::details::TrackDetails> {
        self.details.clone()
    }
}
#[test]
fn regular_empty_missing_and_non_file_sizes_keep_their_distinct_fallbacks() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("generated.mp3");
    fs::write(&file, b"generated bytes").unwrap();
    assert_eq!(size_for(&file, Some(900)), 15);
    assert_eq!(size_for(&file, Some(0)), 15);
    assert_eq!(size_for(&file, None), 15);
    fs::write(&file, []).unwrap();
    assert_eq!(
        size_for(&file, Some(900)),
        0,
        "a reachable empty file overrides a stale positive saved size"
    );
    fs::remove_file(&file).unwrap();
    assert_eq!(size_for(&file, Some(900)), 900);
    assert_eq!(size_for(&file, Some(0)), 0);
    assert_eq!(size_for(&file, None), 0);
    assert_eq!(
        size_for(&file, Some(u64::MAX)),
        u32::MAX,
        "saved size saturation is unchanged"
    );
    fs::create_dir(&file).unwrap();
    assert_eq!(
        size_for(&file, Some(900)),
        900,
        "directory metadata is not an audio file size"
    );
    assert_eq!(size_for(&file, None), 0);
}
#[cfg(unix)]
#[test]
fn regular_symlinks_follow_existing_path_semantics_and_dangling_links_fall_back() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("generated.mp3");
    let alias = temp.path().join("alias.mp3");
    fs::write(&file, b"generated bytes").unwrap();
    symlink(&file, &alias).unwrap();
    assert_eq!(size_for(&alias, Some(900)), 15);
    fs::remove_file(&file).unwrap();
    assert_eq!(size_for(&alias, Some(900)), 900);
    assert_eq!(size_for(&alias, None), 0);
}
