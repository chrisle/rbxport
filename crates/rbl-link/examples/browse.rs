//! READ-ONLY: browses a running link export the way a player does — the
//! port query, the root menu, the TRACK menu's first rows, one track's
//! metadata and info, its beat grid, and the file over NFS — and prints
//! what came back and how long each step took.
//! `cargo run --release -p rbl-link --example browse [host]`
#![allow(
    clippy::pedantic,
    clippy::print_stdout,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]
// perf-ok: a read-only tool run by hand, not shipped code; printing is its point.

use std::net::{IpAddr, SocketAddr};
use std::time::Instant;

use rbl_dbserver::{kind, Argument};
use rbl_fakecdj::{database_port, mount, Database};

const CTX: u32 = 0x0101_0301;

fn numbers(args: &[u32]) -> Vec<Argument> {
    args.iter().map(|&n| Argument::Number(n)).collect()
}

fn browse(db: &mut Database, request: u16, args: &[u32], limit: u32) -> (u32, Vec<Vec<Argument>>) {
    let header = db.request(request, numbers(args)).unwrap();
    let Argument::Number(count) = header[0].arguments[1] else {
        panic!("{header:?}")
    };
    let take = count.min(limit);
    let mut items = Vec::new();
    let mut messages = db
        .request(kind::RENDER, numbers(&[CTX, 0, take, 0, count, 0xc, 1, 0]))
        .unwrap();
    loop {
        for message in messages.drain(..) {
            if message.kind == kind::MENU_FOOTER {
                return (count, items);
            }
            if message.kind == kind::MENU_ITEM {
                items.push(message.arguments);
            }
        }
        messages = db.receive().unwrap();
    }
}

fn print_menu(name: &str, count: u32, rows: &[Vec<Argument>]) {
    println!("{name}: {count} rows");
    for row in rows {
        println!("  id {:?} label {:?} type {:?}", row[1], row[3], row[6]);
    }
}

fn main() {
    let host: IpAddr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1".into())
        .parse()
        .unwrap();
    let t = Instant::now();
    let port = database_port(SocketAddr::new(host, rbl_dbserver::PORT_QUERY)).unwrap();
    let mut db = Database::connect(SocketAddr::new(host, port), 1).unwrap();
    println!("connected to {host}:{port} in {:?}", t.elapsed());

    let t = Instant::now();
    let (n, rows) = browse(&mut db, kind::ROOT_MENU, &[CTX, 0, 0x5cf_ffff], 25);
    let elapsed = t.elapsed();
    print_menu("root", n, &rows);
    println!("  read in {elapsed:?}");

    let t = Instant::now();
    let (n, rows) = browse(&mut db, kind::SORT_MENU, &[CTX, 0, 0], 25);
    let elapsed = t.elapsed();
    print_menu("sort", n, &rows);
    println!("  read in {elapsed:?}");

    let t = Instant::now();
    let (n, rows) = browse(&mut db, kind::TRACK_MENU, &[CTX, 0], 3);
    println!("TRACK: {n} rows, first 3 in {:?}", t.elapsed());
    for row in &rows {
        println!(
            "  {:?} {:?} key {:?} bpm {:?}",
            row[1], row[3], row[14], row[15]
        );
    }
    let Argument::Number(first) = rows[0][1] else {
        panic!()
    };

    let t = Instant::now();
    let (n, _) = browse(&mut db, kind::ARTIST_MENU, &[CTX, 0], 3);
    println!("ARTIST: {n} rows in {:?}", t.elapsed());
    let t = Instant::now();
    let (n, _) = browse(&mut db, kind::SEARCH, &[CTX, 0, 0, 0], 3);
    println!("SEARCH '': {n} rows in {:?}", t.elapsed());
    let t = Instant::now();
    let (n, _) = browse(&mut db, kind::PLAYLIST_MENU, &[CTX, 0, 0, 1], 3);
    println!("PLAYLIST root: {n} rows in {:?}", t.elapsed());
    let t = Instant::now();
    let (n, _) = browse(&mut db, kind::YEARS, &[CTX, 0], 3);
    println!("DATE ADDED: {n} years in {:?}", t.elapsed());

    let t = Instant::now();
    let (_, meta) = browse(&mut db, kind::METADATA, &[CTX, first], 16);
    println!("metadata: {} rows in {:?}", meta.len(), t.elapsed());
    let t = Instant::now();
    let (_, info) = browse(&mut db, kind::TRACK_INFO, &[CTX, first], 7);
    println!("track info: {} rows in {:?}", info.len(), t.elapsed());
    let path = match &info[4][3] {
        Argument::String(s) => s.clone(),
        other => panic!("{other:?}"),
    };
    println!("  path {path}");

    let t = Instant::now();
    let grid = db.request(kind::BEAT_GRID, numbers(&[CTX, first])).unwrap();
    let len = match &grid[0].arguments[3] {
        Argument::Blob(b) => b.len(),
        _ => 0,
    };
    println!("beat grid: {len} bytes in {:?}", t.elapsed());
    let t = Instant::now();
    let preview = db
        .request(kind::WAVEFORM_PREVIEW, numbers(&[CTX, 0, first, 0]))
        .unwrap();
    let len = match &preview[0].arguments[3] {
        Argument::Blob(b) => b.len(),
        _ => 0,
    };
    println!("waveform preview: {len} bytes in {:?}", t.elapsed());

    let t = Instant::now();
    let (export, relative) = rbl_link::files::split(&path).unwrap();
    let mut mounted = mount(
        SocketAddr::new(host, rbl_nfs::REKORDBOX_PORTMAP_PORT),
        &export,
    )
    .unwrap();
    let bytes = mounted.read_file(&relative).unwrap();
    println!("file: {} bytes over NFS in {:?}", bytes.len(), t.elapsed());
}
