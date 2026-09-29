//! READ-ONLY: walks a bounded sample of a running rekordbox Link Export menu
//! tree and writes the replies as JSON.
//!
//! `cargo run --release -p rbl-fakecdj --example menu_probe -- <host>`
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::pedantic,
    clippy::print_stdout,
    clippy::too_many_lines,
    clippy::unwrap_used
)]
// perf-ok: a read-only diagnostic run by hand; retaining the wire rows is its point.

use std::collections::{HashSet, VecDeque};
use std::error::Error;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

use rbl_dbserver::{kind, Argument, Message};
use rbl_fakecdj::{database_port, Database};
use serde_json::{json, Value};

const DEFAULT_CONTEXT: u32 = 0x0101_0301;
const ROOT_MAGIC: u32 = 0x05cf_ffff;

mod request {
    pub const GENRE: u16 = 0x1001;
    pub const ARTIST: u16 = 0x1002;
    pub const ALBUM: u16 = 0x1003;
    pub const TRACK: u16 = 0x1004;
    pub const BPM: u16 = 0x1006;
    pub const RATING: u16 = 0x1007;
    pub const YEAR: u16 = 0x1008;
    pub const LABEL: u16 = 0x100a;
    pub const COLOR: u16 = 0x100d;
    pub const TIME: u16 = 0x1010;
    pub const BITRATE: u16 = 0x1011;
    pub const HISTORY: u16 = 0x1012;
    pub const FILENAME: u16 = 0x1013;
    pub const KEY: u16 = 0x1014;
    pub const MATCHING: u16 = 0x1017;
    pub const HOT_CUE_BANK: u16 = 0x1018;
    pub const GENRE_ARTISTS: u16 = 0x1101;
    pub const ARTIST_ALBUMS: u16 = 0x1102;
    pub const ALBUM_TRACKS: u16 = 0x1103;
    pub const PLAYLIST: u16 = 0x1105;
    pub const BPM_DISTANCES: u16 = 0x1106;
    pub const RATING_TRACKS: u16 = 0x1107;
    pub const YEARS: u16 = 0x1108;
    pub const LABEL_ARTISTS: u16 = 0x110a;
    pub const COLOR_TRACKS: u16 = 0x110d;
    pub const TIME_TRACKS: u16 = 0x1110;
    pub const BITRATE_TRACKS: u16 = 0x1111;
    pub const HISTORY_TRACKS: u16 = 0x1112;
    pub const KEY_DISTANCES: u16 = 0x1114;
    pub const GENRE_ALBUMS: u16 = 0x1201;
    pub const ARTIST_TRACKS: u16 = 0x1202;
    pub const BPM_TRACKS: u16 = 0x1206;
    pub const YEAR_TRACKS: u16 = 0x1208;
    pub const LABEL_ALBUMS: u16 = 0x120a;
    pub const KEY_TRACKS: u16 = 0x1214;
    pub const GENRE_TRACKS: u16 = 0x1301;
    pub const ORIGINAL_ARTIST: u16 = 0x1302;
    pub const LABEL_TRACKS: u16 = 0x130a;
    pub const ORIGINAL_ARTIST_ALBUMS: u16 = 0x1402;
    pub const ORIGINAL_ARTIST_TRACKS: u16 = 0x1502;
    pub const REMIXER: u16 = 0x1602;
    pub const REMIXER_ALBUMS: u16 = 0x1702;
    pub const REMIXER_TRACKS: u16 = 0x1802;
}

#[derive(Debug, Clone)]
struct Options {
    host: IpAddr,
    device: u8,
    context: u32,
    sort: u32,
    page_size: u32,
    max_rows: u32,
    branch_rows: usize,
    max_depth: usize,
    max_menus: usize,
    output: Option<PathBuf>,
}

impl Options {
    fn parse() -> Result<Self, String> {
        let mut args = std::env::args().skip(1);
        let host = args
            .next()
            .ok_or_else(|| "usage: menu_probe <host> [--page-size N] [--max-rows N] [--branch-rows N] [--max-depth N] [--max-menus N] [--sort N] [--device N] [--output PATH]".to_owned())?
            .parse()
            .map_err(|e| format!("invalid host: {e}"))?;
        let mut options = Self {
            host,
            device: 1,
            context: DEFAULT_CONTEXT,
            sort: 0,
            page_size: 32,
            max_rows: 64,
            branch_rows: 1,
            max_depth: 4,
            max_menus: 64,
            output: None,
        };
        while let Some(flag) = args.next() {
            let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
            match flag.as_str() {
                "--page-size" => options.page_size = number(&flag, &value)?,
                "--max-rows" => options.max_rows = number(&flag, &value)?,
                "--branch-rows" => options.branch_rows = number(&flag, &value)?,
                "--max-depth" => options.max_depth = number(&flag, &value)?,
                "--max-menus" => options.max_menus = number(&flag, &value)?,
                "--sort" => options.sort = number(&flag, &value)?,
                "--device" => options.device = number(&flag, &value)?,
                "--output" => options.output = Some(PathBuf::from(value)),
                _ => return Err(format!("unknown option {flag}")),
            }
        }
        if options.page_size == 0 {
            return Err("--page-size must be greater than zero".to_owned());
        }
        options.context = (u32::from(options.device) << 24) | (DEFAULT_CONTEXT & 0x00ff_ffff);
        Ok(options)
    }
}

fn number<T>(flag: &str, value: &str) -> Result<T, String>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    value.parse().map_err(|e| format!("invalid {flag}: {e}"))
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Route {
    Root,
    Tracks,
    Genres,
    GenreArtists(u32),
    GenreAlbums(u32, u32),
    GenreTracks(u32, u32, u32),
    Artists,
    ArtistAlbums(u32),
    ArtistTracks(u32, u32),
    Albums,
    AlbumTracks(u32),
    Bpms,
    BpmDistances(u32),
    BpmTracks(u32, u32),
    Ratings,
    RatingTracks(u32),
    Decades,
    Years(u32),
    YearTracks(u32, u32),
    Labels,
    LabelArtists(u32),
    LabelAlbums(u32, u32),
    LabelTracks(u32, u32, u32),
    Colors,
    ColorTracks(u32),
    Times,
    TimeTracks(u32),
    Bitrates,
    BitrateTracks(u32),
    Histories,
    HistoryTracks(u32),
    Filenames,
    HotCueBanks,
    Matching(u32),
    Keys,
    KeyDistances(u32),
    KeyTracks(u32, u32),
    OriginalArtists,
    OriginalArtistAlbums(u32),
    OriginalArtistTracks(u32, u32),
    Remixers,
    RemixerAlbums(u32),
    RemixerTracks(u32, u32),
    PlaylistFolder(u32),
    PlaylistTracks(u32),
}

impl Route {
    fn query(&self, options: &Options) -> (u16, Vec<u32>) {
        let c = options.context;
        let s = options.sort;
        match *self {
            Self::Root => (kind::ROOT_MENU, vec![c, s, ROOT_MAGIC]),
            Self::Tracks => (request::TRACK, vec![c, s]),
            Self::Genres => (request::GENRE, vec![c, s]),
            Self::GenreArtists(g) => (request::GENRE_ARTISTS, vec![c, s, g]),
            Self::GenreAlbums(g, a) => (request::GENRE_ALBUMS, vec![c, s, g, a]),
            Self::GenreTracks(g, a, l) => (request::GENRE_TRACKS, vec![c, s, g, a, l]),
            Self::Artists => (request::ARTIST, vec![c, s]),
            Self::ArtistAlbums(a) => (request::ARTIST_ALBUMS, vec![c, s, a]),
            Self::ArtistTracks(a, l) => (request::ARTIST_TRACKS, vec![c, s, a, l]),
            Self::Albums => (request::ALBUM, vec![c, s]),
            Self::AlbumTracks(a) => (request::ALBUM_TRACKS, vec![c, s, a]),
            Self::Bpms => (request::BPM, vec![c, s]),
            Self::BpmDistances(bpm) => (request::BPM_DISTANCES, vec![c, s, bpm]),
            Self::BpmTracks(bpm, distance) => (request::BPM_TRACKS, vec![c, s, bpm, distance]),
            Self::Ratings => (request::RATING, vec![c, s]),
            Self::RatingTracks(r) => (request::RATING_TRACKS, vec![c, s, r]),
            Self::Decades => (request::YEAR, vec![c, s]),
            Self::Years(d) => (request::YEARS, vec![c, s, d]),
            Self::YearTracks(d, y) => (request::YEAR_TRACKS, vec![c, s, d, y]),
            Self::Labels => (request::LABEL, vec![c, s]),
            Self::LabelArtists(l) => (request::LABEL_ARTISTS, vec![c, s, l]),
            Self::LabelAlbums(l, a) => (request::LABEL_ALBUMS, vec![c, s, l, a]),
            Self::LabelTracks(l, a, b) => (request::LABEL_TRACKS, vec![c, s, l, a, b]),
            Self::Colors => (request::COLOR, vec![c, s]),
            Self::ColorTracks(color) => (request::COLOR_TRACKS, vec![c, s, color]),
            Self::Times => (request::TIME, vec![c, s]),
            Self::TimeTracks(time) => (request::TIME_TRACKS, vec![c, s, time]),
            Self::Bitrates => (request::BITRATE, vec![c, s]),
            Self::BitrateTracks(bitrate) => (request::BITRATE_TRACKS, vec![c, s, bitrate]),
            Self::Histories => (request::HISTORY, vec![c, s]),
            Self::HistoryTracks(h) => (request::HISTORY_TRACKS, vec![c, s, h]),
            Self::Filenames => (request::FILENAME, vec![c, s]),
            Self::HotCueBanks => (request::HOT_CUE_BANK, vec![c, s]),
            Self::Matching(track) => (request::MATCHING, vec![c, s, track]),
            Self::Keys => (request::KEY, vec![c, s]),
            Self::KeyDistances(k) => (request::KEY_DISTANCES, vec![c, s, k]),
            Self::KeyTracks(k, d) => (request::KEY_TRACKS, vec![c, s, k, d]),
            Self::OriginalArtists => (request::ORIGINAL_ARTIST, vec![c, s]),
            Self::OriginalArtistAlbums(a) => (request::ORIGINAL_ARTIST_ALBUMS, vec![c, s, a]),
            Self::OriginalArtistTracks(a, l) => (request::ORIGINAL_ARTIST_TRACKS, vec![c, s, a, l]),
            Self::Remixers => (request::REMIXER, vec![c, s]),
            Self::RemixerAlbums(r) => (request::REMIXER_ALBUMS, vec![c, s, r]),
            Self::RemixerTracks(r, a) => (request::REMIXER_TRACKS, vec![c, s, r, a]),
            Self::PlaylistFolder(id) => (request::PLAYLIST, vec![c, s, id, 1]),
            Self::PlaylistTracks(id) => (request::PLAYLIST, vec![c, s, id, 0]),
        }
    }

    fn child(&self, row: &[Argument]) -> Option<Self> {
        let id = row_number(row, 1)?;
        match *self {
            Self::Genres => Some(Self::GenreArtists(id)),
            Self::GenreArtists(g) => Some(Self::GenreAlbums(g, id)),
            Self::GenreAlbums(g, a) => Some(Self::GenreTracks(g, a, id)),
            Self::Artists => Some(Self::ArtistAlbums(id)),
            Self::ArtistAlbums(a) => Some(Self::ArtistTracks(a, id)),
            Self::Albums => Some(Self::AlbumTracks(id)),
            Self::Bpms => Some(Self::BpmDistances(id)),
            Self::BpmDistances(bpm) => Some(Self::BpmTracks(bpm, id)),
            Self::Ratings => Some(Self::RatingTracks(id)),
            Self::Decades => Some(Self::Years(id)),
            Self::Years(d) => Some(Self::YearTracks(d, id)),
            Self::Labels => Some(Self::LabelArtists(id)),
            Self::LabelArtists(l) => Some(Self::LabelAlbums(l, id)),
            Self::LabelAlbums(l, a) => Some(Self::LabelTracks(l, a, id)),
            Self::Colors => Some(Self::ColorTracks(id)),
            Self::Times => Some(Self::TimeTracks(id)),
            Self::Bitrates => Some(Self::BitrateTracks(id)),
            Self::Histories => Some(Self::HistoryTracks(id)),
            Self::Keys => Some(Self::KeyDistances(id)),
            Self::KeyDistances(k) => Some(Self::KeyTracks(k, id)),
            Self::OriginalArtists => Some(Self::OriginalArtistAlbums(id)),
            Self::OriginalArtistAlbums(a) => Some(Self::OriginalArtistTracks(a, id)),
            Self::Remixers => Some(Self::RemixerAlbums(id)),
            Self::RemixerAlbums(r) => Some(Self::RemixerTracks(r, id)),
            Self::PlaylistFolder(_) => match row_number(row, 6)? {
                0x01 => Some(Self::PlaylistFolder(id)),
                0x08 => Some(Self::PlaylistTracks(id)),
                _ => None,
            },
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
struct Node {
    path: Vec<String>,
    route: Route,
    depth: usize,
}

struct MenuReply {
    total: u32,
    unavailable: bool,
    header: Vec<Message>,
    rows: Vec<Vec<Argument>>,
    pages: Vec<Value>,
}

fn numbers(args: &[u32]) -> Vec<Argument> {
    args.iter().map(|&n| Argument::Number(n)).collect()
}

fn browse(
    db: &mut Database,
    request_kind: u16,
    args: &[u32],
    options: &Options,
) -> Result<MenuReply, Box<dyn Error>> {
    let header = db.request(request_kind, numbers(args))?;
    let unavailable = header.iter().any(|message| message.kind == kind::ERROR);
    let total = header
        .iter()
        .find(|message| message.kind == kind::MENU_HEADER)
        .and_then(|message| row_number(&message.arguments, 1))
        .or_else(|| unavailable.then_some(0))
        .ok_or_else(|| {
            format!(
                "{} returned no menu header: {header:?}",
                kind::name(request_kind)
            )
        })?;
    let wanted = if options.max_rows == 0 {
        total
    } else {
        total.min(options.max_rows)
    };
    let mut rows = Vec::new();
    let mut pages = Vec::new();
    while u32::try_from(rows.len()).unwrap_or(u32::MAX) < wanted {
        let offset = u32::try_from(rows.len()).unwrap_or(u32::MAX);
        let count = options.page_size.min(wanted - offset);
        let render_args = [options.context, offset, count, 0, total, 0x0c, 1, 0];
        let mut messages = db.request(kind::RENDER, numbers(&render_args))?;
        let mut page_rows = 0_u32;
        loop {
            let mut footer = false;
            for message in messages.drain(..) {
                match message.kind {
                    kind::MENU_ITEM => {
                        rows.push(message.arguments);
                        page_rows += 1;
                    }
                    kind::MENU_FOOTER => footer = true,
                    _ => {}
                }
            }
            if footer {
                break;
            }
            messages = db.receive()?;
            if messages.is_empty() {
                return Err("the database server closed during a render".into());
            }
        }
        pages.push(json!({
            "offset": offset,
            "requested": count,
            "received": page_rows,
            "arguments": render_args,
        }));
        if page_rows == 0 {
            break;
        }
    }
    Ok(MenuReply {
        total,
        unavailable,
        header,
        rows,
        pages,
    })
}

fn row_number(row: &[Argument], index: usize) -> Option<u32> {
    match row.get(index) {
        Some(Argument::Number(value)) => Some(*value),
        _ => None,
    }
}

fn row_label(row: &[Argument]) -> String {
    match row.get(3) {
        Some(Argument::String(value)) => value.trim_matches(['\u{fffa}', '\u{fffb}']).to_owned(),
        _ => "?".to_owned(),
    }
}

fn argument_json(argument: &Argument) -> Value {
    match argument {
        Argument::Number(value) => json!({
            "type": "number",
            "value": value,
            "hex": format!("{value:#010x}"),
        }),
        Argument::String(value) => json!({"type": "string", "value": value}),
        Argument::Blob(value) => json!({
            "type": "blob",
            "length": value.len(),
            "hex": hex(value),
        }),
    }
}

fn message_json(message: &Message) -> Value {
    json!({
        "transaction": message.transaction,
        "kind": message.kind,
        "kind_hex": format!("{:#06x}", message.kind),
        "arguments": message.arguments.iter().map(argument_json).collect::<Vec<_>>(),
    })
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    bytes.iter().fold(
        String::with_capacity(bytes.len().saturating_mul(2)),
        |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        },
    )
}

fn selected_rows(rows: &[Vec<Argument>], limit: usize) -> Vec<&Vec<Argument>> {
    let all = rows
        .iter()
        .filter(|row| row_number(row, 1) == Some(u32::MAX));
    let concrete = rows
        .iter()
        .filter(|row| row_number(row, 1) != Some(u32::MAX))
        .take(limit);
    all.chain(concrete).collect()
}

fn root_route(row: &[Argument]) -> Option<Route> {
    match row_number(row, 6)? {
        0x80 => Some(Route::Genres),
        0x81 => Some(Route::Artists),
        0x82 => Some(Route::Albums),
        0x83 => Some(Route::Tracks),
        0x84 => Some(Route::PlaylistFolder(0)),
        0x85 => Some(Route::Bpms),
        0x86 => Some(Route::Ratings),
        0x87 => Some(Route::Decades),
        0x88 => Some(Route::Remixers),
        0x89 => Some(Route::Labels),
        0x8a => Some(Route::OriginalArtists),
        0x8b => Some(Route::Keys),
        0x8e => Some(Route::Colors),
        0x92 => Some(Route::Times),
        0x93 => Some(Route::Bitrates),
        0x94 => Some(Route::Filenames),
        0x95 => Some(Route::Histories),
        0x98 => Some(Route::HotCueBanks),
        // MATCHING is relative to a track. This is a known seed in the copied
        // library, which makes the diagnostic deterministic for this capture.
        0xaa => Some(Route::Matching(245_834_041)),
        _ => None,
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let options = Options::parse().map_err(|message| format!("{message}\n"))?;
    let port = database_port(SocketAddr::new(options.host, rbl_dbserver::PORT_QUERY))?;
    let mut db = Database::connect(SocketAddr::new(options.host, port), options.device)?;
    let mut queue = VecDeque::from([Node {
        path: vec!["ROOT".to_owned()],
        route: Route::Root,
        depth: 0,
    }]);
    let mut seen = HashSet::new();
    let mut menus = Vec::new();
    let mut unsupported = Vec::new();

    while let Some(node) = queue.pop_front() {
        if menus.len() >= options.max_menus {
            break;
        }
        let (request_kind, args) = node.route.query(&options);
        if !seen.insert((request_kind, args.clone())) {
            continue;
        }
        let reply = browse(&mut db, request_kind, &args, &options)?;
        let rows_json = reply
            .rows
            .iter()
            .map(|row| row.iter().map(argument_json).collect::<Vec<_>>())
            .collect::<Vec<_>>();
        menus.push(json!({
            "path": node.path,
            "depth": node.depth,
            "request": {
                "kind": request_kind,
                "kind_hex": format!("{request_kind:#06x}"),
                "name": kind::name(request_kind),
                "arguments": args,
            },
            "response_header": reply.header.iter().map(message_json).collect::<Vec<_>>(),
            "total_rows": reply.total,
            "unavailable": reply.unavailable,
            "captured_rows": reply.rows.len(),
            "truncated": reply.rows.len() < usize::try_from(reply.total).unwrap_or(usize::MAX),
            "pages": reply.pages,
            "rows": rows_json,
        }));

        if node.depth >= options.max_depth {
            continue;
        }
        let branch_rows = if node.route == Route::Root {
            reply.rows.iter().collect()
        } else {
            selected_rows(&reply.rows, options.branch_rows)
        };
        for row in branch_rows {
            let route = if node.route == Route::Root {
                root_route(row)
            } else {
                node.route.child(row)
            };
            let Some(route) = route else {
                if node.route == Route::Root {
                    unsupported.push(json!({
                        "label": row_label(row),
                        "id": row_number(row, 1),
                        "item_type": row_number(row, 6),
                    }));
                }
                continue;
            };
            let mut path = node.path.clone();
            path.push(row_label(row));
            queue.push_back(Node {
                path,
                route,
                depth: node.depth + 1,
            });
        }
    }

    let capture = serde_json::to_string_pretty(&json!({
        "host": options.host.to_string(),
        "database_port": port,
        "device": options.device,
        "context": format!("{:#010x}", options.context),
        "limits": {
            "page_size": options.page_size,
            "max_rows_per_menu": options.max_rows,
            "branch_rows": options.branch_rows,
            "max_depth": options.max_depth,
            "max_menus": options.max_menus,
        },
        "menus": menus,
        "unsupported_root_items": unsupported,
        "menu_limit_reached": seen.len() >= options.max_menus,
    }))?;
    if let Some(path) = &options.output {
        std::fs::write(path, capture)?;
        println!("wrote {}", path.display());
    } else {
        println!("{capture}");
    }
    Ok(())
}
