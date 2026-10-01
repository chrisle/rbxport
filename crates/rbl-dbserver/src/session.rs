//! One player's conversation with the database server.
//!
//! A menu request is answered with a row count; the rows follow on a render
//! request that names an offset and a limit, so the session keeps the menu
//! it was last asked for. Every layout here is the one rekordbox 7.2.11
//! sent a CDJ-3000 (`docs/pre-release/design-notes/link-export-capture.md`).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::catalog::{
    Analysis, ArtistRole, Catalog, Edit, HotCueBankCue, Query, Row, Sort, TrackDetails, TrackScope, UsbCue,
};
use crate::item::{item_type, root_menu, sort_menu, track_flags, Item};
use crate::net::{Handler, Session};
use crate::{Argument, Message, keys, kind, menu_footer, menu_header, setup_reply};

/// Our device number on the link when nothing has settled one: rekordbox's
/// first choice, so a player treats us as it treats rekordbox.
pub const DEVICE: u8 = 0x11;

/// Serves a catalog to every player that connects.
pub struct CatalogHandler {
    catalog: Arc<dyn Catalog>,
    /// The device number the beacon's join settled on — 17, or 18 when
    /// another rekordbox holds 17 — and `0` while there is none, which is
    /// also what keeps the port query unanswered until the link is up.
    device: Arc<AtomicU8>,
}

impl CatalogHandler {
    pub fn new(catalog: Arc<dyn Catalog>) -> Self {
        Self {
            catalog,
            device: Arc::new(AtomicU8::new(DEVICE)),
        }
    }

    /// Answers with the number in `device` rather than the fixed 17.
    #[must_use]
    pub fn with_device(mut self, device: Arc<AtomicU8>) -> Self {
        self.device = device;
        self
    }
}

impl Handler for CatalogHandler {
    fn open(&self) -> Box<dyn Session> {
        let device = match self.device.load(Ordering::Relaxed) {
            0 => DEVICE,
            number => number,
        };
        Box::new(LinkSession::new(Arc::clone(&self.catalog)).as_device(device))
    }

    fn serving(&self) -> bool {
        self.device.load(Ordering::Relaxed) != 0
    }
}

/// The length of rekordbox's user-info blob.
const USER_INFO_LEN: usize = 160;

/// The rows of the menu a player last asked for, ready to render.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Menu {
    Root,
    SortOptions(Vec<crate::catalog::Sort>),
    Selectors(Vec<Item>),
    Keys,
    RelatedKeys(u32),
    Library {
        query: Query,
        rows: Vec<Row>,
    },
    Metadata(Box<TrackDetails>),
    TrackInfo(Box<TrackDetails>),
    /// What the player delivers to KUVO about a track it loaded.
    DeliveryInfo(Box<TrackDetails>),
    /// A request with no rows: `3007`, MATCHING, an unknown track.
    Empty,
}

impl Menu {
    fn len(&self) -> u32 {
        let n = match self {
            Self::Root => root_menu().len(),
            Self::SortOptions(sorts) => sorts.len(),
            Self::Selectors(items) => items.len(),
            Self::Keys => keys::NAMES.len(),
            Self::RelatedKeys(_) => 3,
            Self::Library { rows, .. } => rows.len(),
            Self::Metadata(_) => 16,
            Self::TrackInfo(_) => 7,
            Self::DeliveryInfo(_) => 13,
            Self::Empty => 0,
        };
        u32::try_from(n).unwrap_or(u32::MAX)
    }
}

pub struct LinkSession {
    catalog: Arc<dyn Catalog>,
    menus: HashMap<u8, Menu>,
    /// The player's device number, from its setup message.
    player: u8,
    extended: bool,
    filter: crate::filter::TrackFilter,
    /// Our own, in the setup reply.
    device: u8,
}

impl LinkSession {
    pub fn new(catalog: Arc<dyn Catalog>) -> Self {
        Self {
            catalog,
            menus: HashMap::new(),
            player: 0,
            extended: true,
            filter: crate::filter::TrackFilter::default(),
            device: DEVICE,
        }
    }

    /// The session answering as `device` rather than 17.
    #[must_use]
    pub fn as_device(mut self, device: u8) -> Self {
        self.device = device;
        self
    }

    fn number(message: &Message, index: usize) -> u32 {
        match message.arguments.get(index) {
            Some(Argument::Number(n)) => *n,
            _ => 0,
        }
    }

    fn text(message: &Message, index: usize) -> String {
        match message.arguments.get(index) {
            Some(Argument::String(s)) => s.clone(),
            _ => String::new(),
        }
    }

    /// Answers a menu request: remember the menu, reply with its size.
    fn menu(&mut self, message: &Message, menu: Menu) -> Vec<Message> {
        let count = menu.len();
        self.menus.insert(Self::menu_location(message), menu);
        vec![menu_header(
            message.transaction,
            u32::from(message.kind),
            count,
        )]
    }

    fn library(&mut self, message: &Message, query: Query) -> Vec<Message> {
        let mut rows = self.catalog.list(&query);
        self.catalog.filter_rows(&mut rows, &self.filter);
        self.menu(message, Menu::Library { query, rows })
    }

    fn tracks(&mut self, message: &Message, scope: TrackScope) -> Vec<Message> {
        let sort = Sort::from_id(Self::number(message, 1));
        self.library(message, Query::Tracks { scope, sort })
    }

    fn menu_location(message: &Message) -> u8 {
        Self::number(message, 0).to_be_bytes()[1]
    }

    /// The items for a window of the current menu.
    fn items(&self, location: u8, offset: u32, limit: u32) -> Vec<Item> {
        let offset = offset as usize;
        let limit = limit as usize;
        let window = |all: Vec<Item>| all.into_iter().skip(offset).take(limit).collect::<Vec<_>>();
        match self.menus.get(&location).unwrap_or(&Menu::Empty) {
            Menu::Root => window(root_menu()),
            Menu::SortOptions(sorts) => window(sort_menu(sorts)),
            Menu::Selectors(items) => window(items.clone()),
            Menu::Keys => window(
                self.catalog
                    .key_ids()
                    .into_iter()
                    .map(|id| Item::named(id, keys::name(id), item_type::KEY))
                    .collect(),
            ),
            // `[distance, key, text]`: the distance is what a later
            // key-tracks request carries.
            Menu::RelatedKeys(key) => window(
                (0..3)
                    .map(|distance| Item {
                        a: distance,
                        id: *key,
                        text: keys::related_text(*key, distance),
                        item_type: item_type::KEY,
                        ..Item::default()
                    })
                    .collect(),
            ),
            Menu::Library { query, rows } => rows
                .iter()
                .skip(offset)
                .take(limit)
                .filter_map(|row| self.item(query, row))
                .collect(),
            Menu::Metadata(details) => window(metadata_rows(
                details,
                self.catalog.played(details.row.id),
                self.catalog.tagged(details.row.id),
                !self.extended,
            )),
            Menu::TrackInfo(details) => window(track_info_rows(details)),
            Menu::DeliveryInfo(details) => window(delivery_rows(details)),
            Menu::Empty => Vec::new(),
        }
    }

    fn render(&self, message: &Message) -> Vec<Message> {
        let offset = Self::number(message, 1);
        let limit = Self::number(message, 2);
        let mut out = vec![Message::new(
            message.transaction,
            kind::RENDER_HEADER,
            vec![Argument::Number(1), Argument::Number(offset)],
        )];
        out.extend(
            self.items(Self::menu_location(message), offset, limit)
                .iter()
                .map(|item| {
                    let mut reply = item.message(message.transaction);
                    if !self.extended {
                        reply.arguments.truncate(12);
                    }
                    reply
                }),
        );
        out.push(if self.extended {
            menu_footer(message.transaction)
        } else {
            Message::new(message.transaction, kind::MENU_FOOTER, vec![])
        });
        out
    }

    /// One library row as the item its menu draws it as.
    fn item(&self, query: &Query, row: &Row) -> Option<Item> {
        Some(match (query, row) {
            (Query::BpmBuckets, Row::Date(value)) => Item::number(*value, item_type::TEMPO),
            (Query::Ratings, Row::Date(value)) => Item::number(*value, item_type::RATING),
            (Query::Bitrates, Row::Date(value)) => Item::number(*value, item_type::BIT_RATE),
            (Query::DurationMinutes, Row::Date(value)) => Item::number(*value, item_type::DURATION),
            (Query::ReleaseDecades | Query::ReleaseYears(_), Row::Date(0xffff_ffff)) => Item::all(),
            (Query::ReleaseDecades | Query::ReleaseYears(_), Row::Date(value)) => {
                Item::number(*value, item_type::YEAR)
            }
            (Query::Colors, Row::Named { id, name }) => {
                let palette_type = item_type::COLOR_PINK + id.saturating_sub(1).min(7);
                Item::named(*id, name, palette_type)
            }
            (
                _,
                Row::Named {
                    id: 0xffff_ffff, ..
                },
            ) => Item::all(),
            (
                Query::Artists(_) | Query::GenreArtists(_) | Query::LabelArtists(_),
                Row::Named { id, name },
            ) => Item::named(*id, name, item_type::ARTIST),
            (
                Query::Albums(_)
                | Query::ArtistAlbums(_)
                | Query::GenreArtistAlbums { .. }
                | Query::LabelArtistAlbums { .. },
                Row::Named { id, name },
            ) => {
                if *id == 0xffff_ffff {
                    Item::all()
                } else {
                    Item::named_twice(*id, name, item_type::ALBUM)
                }
            }
            (Query::Histories, Row::Named { id, name }) => {
                Item::named(*id, name, item_type::HISTORY)
            }
            (Query::Labels(_), Row::Named { id, name }) => Item::named(*id, name, item_type::LABEL),
            (_, Row::Named { id, name }) => Item::named(*id, name, item_type::GENRE),
            (
                _,
                Row::List {
                    id,
                    name,
                    folder,
                    position,
                },
            ) => Item::list(*id, name, *folder, *position),
            (_, Row::Date(value)) => {
                if *value == 0xffff_ffff {
                    Item::all()
                } else {
                    Item::date_part(*value)
                }
            }
            (Query::Tracks { scope, .. }, Row::Track { id, position }) => {
                let track = if matches!(scope, TrackScope::FileName) {
                    self.catalog.file_name_row(*id)?
                } else {
                    self.catalog.track_row(*id)?
                };
                let listed = match scope {
                    TrackScope::Genre { .. }
                    | TrackScope::Label { .. }
                    | TrackScope::Artist { .. }
                    | TrackScope::Album(_)
                    | TrackScope::Playlist(_)
                    | TrackScope::TagList => track_flags::LISTED,
                    _ => 0,
                };
                // A history's rows are all played; elsewhere only the tracks
                // a player has loaded this session are, or every row of a
                // playlist greys.
                let played = matches!(scope, TrackScope::History(_)) || self.catalog.played(*id);
                let flags = listed
                    | if played { track_flags::PLAYED } else { 0 }
                    | u32::from(self.catalog.tagged(*id));
                Item::track(&track, flags, *position)
            }
            (_, Row::Track { .. }) => return None,
        })
    }

    /// A blob reply: `[request kind, 0, len, blob]` and, for some, a
    /// trailing number; or `[kind, 0x32, 0]` when there is nothing.
    fn blob(
        message: &Message,
        reply: u16,
        blob: Option<Vec<u8>>,
        tail: Option<u32>,
    ) -> Vec<Message> {
        let mut arguments = vec![Argument::Number(u32::from(message.kind))];
        match blob {
            Some(bytes) if !bytes.is_empty() => {
                arguments.push(Argument::Number(0));
                arguments.push(Argument::Number(
                    u32::try_from(bytes.len()).unwrap_or(u32::MAX),
                ));
                arguments.push(Argument::Blob(bytes));
            }
            _ => {
                arguments.push(Argument::Number(0x32));
                arguments.push(Argument::Number(0));
                arguments.push(Argument::Blob(Vec::new()));
            }
        }
        if let Some(tail) = tail {
            arguments.push(Argument::Number(tail));
        }
        vec![Message::new(message.transaction, reply, arguments)]
    }

    fn analysis(
        &self,
        message: &Message,
        track: u32,
        what: &Analysis,
        reply: u16,
        tail: Option<u32>,
    ) -> Vec<Message> {
        Self::blob(message, reply, self.catalog.analysis(track, what), tail)
    }

    /// RX3's `DBSMain_RetCueToClient` failure envelope for a Hot Cue Bank
    /// request. It is deliberately not the usual unavailable-blob reply:
    /// `dbcl_WaitCue` requires the eleven-field `4702` layout even when a
    /// source has no selected bank or rejects a change.
    fn hot_cue_bank_unavailable(message: &Message) -> Vec<Message> {
        vec![Message::new(
            message.transaction,
            kind::HOT_CUE_BANK_REPLY,
            vec![
                Argument::Number(u32::from(message.kind)),
                Argument::Number(0x32),
                Argument::Number(0),
                Argument::Blob(Vec::new()),
                Argument::Number(0x24),
                Argument::Number(0),
                Argument::Number(0),
                Argument::Number(0),
                Argument::Blob(Vec::new()),
                Argument::Number(0),
                Argument::Blob(Vec::new()),
            ],
        )]
    }

    /// RX3's `DBSMain_RetCueToClient` success layout.  The firmware reads
    /// three 36-byte legacy cue records and their paired eight-byte
    /// millisecond sidecars.  It identifies the slots from bits 16..23 of
    /// the first word (4, 5, and 6), not from their order in the blob.
    fn hot_cue_bank_reply(message: &Message, cues: Vec<HotCueBankCue>) -> Vec<Message> {
        let mut records = Vec::with_capacity(3 * 36);
        let mut sidecars = Vec::with_capacity(3 * 8);
        let mut count = 0_u32;
        for cue in cues.into_iter().filter(|cue| (1..=3).contains(&cue.slot)).take(3) {
            // `CueFmt_FmtBnkCue4Player` always receives the paired timing
            // sidecar and marks that fact with bit 8.
            let flags = 0x100 | u32::from(cue.out_ms.is_some()) | (u32::from(cue.slot) + 3) << 16;
            let in_frame = cue.in_ms.saturating_mul(3) / 20;
            let out_frame = cue.out_ms.map_or(u32::MAX, |ms| ms.saturating_mul(3) / 20);
            for word in [
                flags,
                cue.content,
                0,
                in_frame,
                out_frame,
                cue.color,
                cue.color_table_index,
                u32::from(cue.active_loop),
                cue.beat_loop_size,
            ] {
                // Cue records are passed to `DBComm_SendDatStrmReq` as the
                // RX3's native ARM memory.  They are therefore little-endian
                // binary fields, unlike the enclosing Link numbers.
                records.extend_from_slice(&word.to_le_bytes());
            }
            sidecars.extend_from_slice(&cue.in_ms.to_le_bytes());
            sidecars.extend_from_slice(&cue.out_ms.unwrap_or(u32::MAX).to_le_bytes());
            count += 1;
        }
        vec![Message::new(
            message.transaction,
            kind::HOT_CUE_BANK_REPLY,
            vec![
                Argument::Number(u32::from(message.kind)),
                Argument::Number(0),
                Argument::Number(u32::try_from(records.len()).unwrap_or(u32::MAX)),
                Argument::Blob(records),
                Argument::Number(0x24),
                Argument::Number(count),
                Argument::Number(0),
                Argument::Number(u32::try_from(sidecars.len()).unwrap_or(u32::MAX)),
                Argument::Blob(sidecars),
                Argument::Number(0),
                Argument::Blob(Vec::new()),
            ],
        )]
    }

    /// RX3's ordinary USB-cue `4702` reply. Firmware uses this after a
    /// successful `0x2201`: it reloads the target track's cues through
    /// `DBSMain_GetUsbCue`, so the result is deliberately not a bank list.
    fn usb_cue_reply(message: &Message, cues: Vec<UsbCue>) -> Vec<Message> {
        let mut records = Vec::with_capacity(cues.len() * 36);
        let mut sidecars = Vec::with_capacity(cues.len() * 8);
        let mut hot = 0_u32;
        let mut memory = 0_u32;
        for cue in cues {
            let flags = 0x100 | u32::from(cue.out_ms.is_some()) | u32::from(cue.slot) << 16;
            let in_frame = cue.in_ms.saturating_mul(3) / 20;
            let out_ms = cue.out_ms.unwrap_or(u32::MAX);
            let out_frame = out_ms.saturating_mul(3) / 20;
            for word in [flags, 0, 0, in_frame, out_frame, 0, cue.color_table_index, 0, 0] {
                records.extend_from_slice(&word.to_le_bytes());
            }
            sidecars.extend_from_slice(&cue.in_ms.to_le_bytes());
            sidecars.extend_from_slice(&out_ms.to_le_bytes());
            if cue.slot == 0 { memory += 1; } else { hot += 1; }
        }
        vec![Message::new(
            message.transaction,
            kind::HOT_CUE_BANK_REPLY,
            vec![
                Argument::Number(u32::from(message.kind)),
                Argument::Number(0),
                Argument::Number(u32::try_from(records.len()).unwrap_or(u32::MAX)),
                Argument::Blob(records),
                Argument::Number(0x24),
                Argument::Number(hot),
                Argument::Number(memory),
                Argument::Number(u32::try_from(sidecars.len()).unwrap_or(u32::MAX)),
                Argument::Blob(sidecars),
                Argument::Number(0),
                Argument::Blob(Vec::new()),
            ],
        )]
    }

    /// Parses the `0x2201` legacy cue record and its optional timing sidecar.
    fn hot_cue_bank_edit(message: &Message) -> Option<(u32, HotCueBankCue)> {
        let bank = Self::number(message, 1);
        let bytes = match message.arguments.get(3) {
            Some(Argument::Blob(bytes)) if Self::number(message, 2) == 0x24 && bytes.len() == 0x24 => bytes,
            _ => return None,
        };
        let word = |at: usize| -> Option<u32> {
            Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
        };
        let flags = word(0)?;
        let slot = u8::try_from((flags >> 16) & 0xff).ok()?.checked_sub(3)?;
        if !(1..=3).contains(&slot) { return None; }
        // The firmware zeroes its eight-byte timecode buffer then copies the
        // declared sidecar length, capped at eight.  Preserve that behavior
        // for truncated-but-valid client sidecars.
        let mut timecode = [0_u8; 8];
        let sidecar_length = Self::number(message, 4) as usize;
        let sidecar = if sidecar_length == 0 {
            Some(timecode)
        } else {
            match message.arguments.get(5) {
                Some(Argument::Blob(bytes)) if sidecar_length <= 8 && bytes.len() >= sidecar_length => {
                    timecode[..sidecar_length].copy_from_slice(&bytes[..sidecar_length]);
                    Some(timecode)
                }
                _ => None,
            }
        };
        let frame_to_ms = |frame: u32| frame.saturating_mul(20) / 3;
        let in_ms = sidecar.map_or_else(|| frame_to_ms(word(12).unwrap_or(0)), |bytes| {
            u32::from_le_bytes(bytes[0..4].try_into().unwrap_or([0; 4]))
        });
        let out_ms = (flags & 1 != 0).then(|| sidecar.map_or_else(
            || frame_to_ms(word(16).unwrap_or(0)),
            |bytes| u32::from_le_bytes(bytes[4..8].try_into().unwrap_or([0; 4])),
        ));
        Some((bank, HotCueBankCue {
            slot,
            content: word(4)?,
            in_ms,
            out_ms,
            color: word(20)?,
            color_table_index: word(24)?,
            active_loop: word(28)? != 0,
            beat_loop_size: word(32)?,
            cue_microsec: 0,
        }))
    }

    fn hot_cue_bank_menu(&mut self, message: &Message) -> Vec<Message> {
        // `0x2001` carries a parent/bank id and a mode after the connection
        // context.  Firmware's `DBSMain_GetHCBnkList` uses mode 1 for the
        // hierarchy and mode 0 for the three tracks in the selected bank.
        if Self::number(message, 0) & 0xff != 1 {
            return Self::hot_cue_bank_unavailable(message);
        }
        let bank = Self::number(message, 1);
        let mode = Self::number(message, 2);
        let items = if mode == 0 {
            self.catalog.hot_cue_bank_tracks(bank).into_iter()
                .enumerate()
                .map(|(position, track)| Item::track(&track, 0, u32::try_from(position).unwrap_or(u32::MAX)))
                .collect()
        } else if mode == 1 {
            self.catalog.hot_cue_banks((bank != 0).then_some(bank)).into_iter().map(|bank| {
                if bank.folder {
                    Item::named(bank.id, &bank.name, item_type::FOLDER)
                } else {
                    Item::named(bank.id, &bank.name, item_type::HOT_CUE_BANK)
                }
            }).collect()
        } else {
            Vec::new()
        };
        self.menu(message, Menu::Selectors(items))
    }

    /// A menu of one track's fields: metadata, track info or delivery info,
    /// all `[ctx, track_id]` and all opened the same way.
    fn track_menu(
        &mut self,
        message: &Message,
        make: fn(Box<TrackDetails>) -> Menu,
    ) -> Vec<Message> {
        let track = Self::number(message, 1);
        match self.catalog.track(track) {
            Some(details) => self.menu(message, make(Box::new(details))),
            None => self.menu(message, Menu::Empty),
        }
    }

    fn genre_artists(&mut self, message: &Message) -> Vec<Message> {
        let genre = Self::number(message, 2);
        let query = Query::GenreArtists(genre);
        let mut rows = self.catalog.list(&query);
        prepend_all_if_multiple(&mut rows);
        self.menu(message, Menu::Library { query, rows })
    }

    fn genre_albums(&mut self, message: &Message) -> Vec<Message> {
        let genre = Self::number(message, 2);
        let artist = Self::number(message, 3);
        let query = Query::GenreArtistAlbums {
            genre,
            artist: (artist != 0xffff_ffff).then_some(artist),
        };
        let mut rows = self.catalog.list(&query);
        prepend_all_if_multiple(&mut rows);
        self.menu(message, Menu::Library { query, rows })
    }

    fn genre_tracks(&mut self, message: &Message) -> Vec<Message> {
        let genre = Self::number(message, 2);
        let artist = Self::number(message, 3);
        let album = Self::number(message, 4);
        self.tracks(
            message,
            TrackScope::Genre {
                genre,
                artist: (artist != 0xffff_ffff).then_some(artist),
                album: (album != 0xffff_ffff).then_some(album),
            },
        )
    }

    fn label_artists(&mut self, message: &Message) -> Vec<Message> {
        let label = Self::number(message, 2);
        let query = Query::LabelArtists(label);
        let mut rows = self.catalog.list(&query);
        prepend_all_if_multiple(&mut rows);
        self.menu(message, Menu::Library { query, rows })
    }

    fn label_albums(&mut self, message: &Message) -> Vec<Message> {
        let label = Self::number(message, 2);
        let artist = Self::number(message, 3);
        let query = Query::LabelArtistAlbums {
            label,
            artist: (artist != u32::MAX).then_some(artist),
        };
        let mut rows = self.catalog.list(&query);
        prepend_all_if_multiple(&mut rows);
        self.menu(message, Menu::Library { query, rows })
    }

    fn label_tracks(&mut self, message: &Message) -> Vec<Message> {
        let artist = Self::number(message, 3);
        let album = Self::number(message, 4);
        self.tracks(
            message,
            TrackScope::Label {
                label: Self::number(message, 2),
                artist: (artist != u32::MAX).then_some(artist),
                album: (album != u32::MAX).then_some(album),
            },
        )
    }

    fn artist_role_albums(&mut self, message: &Message, role: ArtistRole) -> Vec<Message> {
        let artist = Self::number(message, 2);
        let query = Query::ArtistRoleAlbums { role, artist };
        let mut rows = self.catalog.list(&query);
        prepend_all_if_multiple(&mut rows);
        self.menu(message, Menu::Library { query, rows })
    }

    fn artist_role_tracks(&mut self, message: &Message, role: ArtistRole) -> Vec<Message> {
        let album = Self::number(message, 3);
        self.tracks(
            message,
            TrackScope::ArtistRole {
                role,
                artist: Self::number(message, 2),
                album: (album != u32::MAX).then_some(album),
            },
        )
    }

    /// The requests that open a menu: a count now, rows on render.
    #[allow(clippy::too_many_lines, reason = "one protocol dispatch table; splitting it would obscure its message coverage")]
    fn handle_menu(&mut self, message: &Message) -> Vec<Message> {
        match message.kind {
            kind::ROOT_MENU => self.menu(message, Menu::Root),
            kind::GENRE_MENU => {
                let sort = Sort::from_id(Self::number(message, 1));
                self.library(message, Query::Genres(sort))
            }
            kind::GENRE_ARTISTS => self.genre_artists(message),
            kind::GENRE_ARTIST_ALBUMS => self.genre_albums(message),
            kind::GENRE_ARTIST_ALBUM_TRACKS => self.genre_tracks(message),
            kind::LABEL_MENU => {
                let sort = Sort::from_id(Self::number(message, 1));
                self.library(message, Query::Labels(sort))
            }
            kind::LABEL_ARTISTS => self.label_artists(message),
            kind::LABEL_ARTIST_ALBUMS => self.label_albums(message),
            kind::LABEL_ARTIST_ALBUM_TRACKS => self.label_tracks(message),
            kind::ORIGINAL_ARTIST_MENU => {
                self.library(message, Query::ArtistRoleArtists(ArtistRole::Original))
            }
            kind::ORIGINAL_ARTIST_ALBUMS => self.artist_role_albums(message, ArtistRole::Original),
            kind::ORIGINAL_ARTIST_ALBUM_TRACKS => {
                self.artist_role_tracks(message, ArtistRole::Original)
            }
            kind::REMIXER_MENU => {
                self.library(message, Query::ArtistRoleArtists(ArtistRole::Remixer))
            }
            kind::REMIXER_ALBUMS => self.artist_role_albums(message, ArtistRole::Remixer),
            kind::REMIXER_ALBUM_TRACKS => self.artist_role_tracks(message, ArtistRole::Remixer),
            kind::SORT_MENU => self.menu(message, Menu::SortOptions(self.catalog.sorts())),
            kind::KEY_MENU | kind::LEGACY_KEY_MENU => self.menu(message, Menu::Keys),
            kind::RELATED_KEYS => {
                let key = Self::number(message, 2);
                self.menu(message, Menu::RelatedKeys(key))
            }
            kind::ARTIST_MENU => {
                let sort = Sort::from_id(Self::number(message, 1));
                self.library(message, Query::Artists(sort))
            }
            kind::ALBUM_MENU => {
                let sort = Sort::from_id(Self::number(message, 1));
                self.library(message, Query::Albums(sort))
            }
            kind::ARTIST_ALBUMS => {
                let artist = Self::number(message, 2);
                let mut rows = self.catalog.list(&Query::ArtistAlbums(artist));
                prepend_all_if_multiple(&mut rows);
                self.menu(
                    message,
                    Menu::Library {
                        query: Query::ArtistAlbums(artist),
                        rows,
                    },
                )
            }
            kind::ARTIST_ALBUM_TRACKS => {
                let artist = Self::number(message, 2);
                let album = Self::number(message, 3);
                let album = (album != 0xffff_ffff).then_some(album);
                self.tracks(message, TrackScope::Artist { artist, album })
            }
            kind::ALBUM_TRACKS => {
                let album = Self::number(message, 2);
                self.tracks(message, TrackScope::Album(album))
            }
            // RX3 uses `0x1200` for the same all-track list while changing
            // its load/search depth (`djdsqlGetTrack_Content`).
            kind::TRACK_MENU | kind::CONTENT_TRACKS => self.tracks(message, TrackScope::All),
            kind::FILE_NAME_MENU => self.tracks(message, TrackScope::FileName),
            kind::MATCHING_TRACKS => {
                self.tracks(message, TrackScope::Matching(Self::number(message, 2)))
            }
            kind::BPM_MENU => self.library(message, Query::BpmBuckets),
            kind::BPM_RANGES => self.menu(
                message,
                Menu::Selectors(
                    (0..=6)
                        .map(|distance| Item::number(distance, item_type::TEMPO))
                        .collect(),
                ),
            ),
            kind::BPM_TRACKS => self.tracks(
                message,
                TrackScope::Bpm {
                    bpm_x100: Self::number(message, 2),
                    tolerance_pct: Self::number(message, 3).min(6),
                },
            ),
            kind::RATING_MENU => self.library(message, Query::Ratings),
            kind::RATING_TRACKS => {
                self.tracks(message, TrackScope::Rating(Self::number(message, 2)))
            }
            kind::BITRATE_MENU => self.library(message, Query::Bitrates),
            kind::BITRATE_TRACKS => {
                self.tracks(message, TrackScope::Bitrate(Self::number(message, 2)))
            }
            kind::COLOR_MENU => self.library(message, Query::Colors),
            kind::COLOR_TRACKS => self.tracks(message, TrackScope::Color(Self::number(message, 2))),
            kind::TIME_MENU => self.library(message, Query::DurationMinutes),
            kind::TIME_TRACKS => self.tracks(
                message,
                TrackScope::DurationMinute(Self::number(message, 2)),
            ),
            kind::RELEASE_DECADES => self.library(message, Query::ReleaseDecades),
            kind::RELEASE_YEARS => {
                let decade = Self::number(message, 2);
                let query = Query::ReleaseYears(decade);
                let mut rows = self.catalog.list(&query);
                if rows.len() > 1 {
                    rows.insert(0, Row::Date(u32::MAX));
                }
                self.menu(message, Menu::Library { query, rows })
            }
            kind::RELEASE_YEAR_TRACKS => {
                let year = Self::number(message, 3);
                self.tracks(
                    message,
                    TrackScope::ReleaseYear {
                        decade: Self::number(message, 2),
                        year: (year != u32::MAX).then_some(year),
                    },
                )
            }
            kind::TAG_LIST => self.tracks(message, TrackScope::TagList),
            kind::KEY_TRACKS => {
                let key = Self::number(message, 2);
                let distance = Self::number(message, 3).min(2);
                self.tracks(message, TrackScope::Key { key, distance })
            }
            // The RX3 retains the pre-related-key route: `[ctx, key]`.
            // It has the same rows as a zero-distance new-key request.
            kind::LEGACY_KEY_TRACKS => self.tracks(
                message,
                TrackScope::Key {
                    key: Self::number(message, 2),
                    distance: 0,
                },
            ),
            kind::PLAYLIST_MENU => {
                let id = Self::number(message, 2);
                if Self::number(message, 3) == 1 {
                    self.library(message, Query::Folder(id))
                } else {
                    self.tracks(message, TrackScope::Playlist(id))
                }
            }
            kind::HISTORY_MENU => self.library(message, Query::Histories),
            kind::HISTORY_TRACKS => {
                let session = Self::number(message, 2);
                self.tracks(message, TrackScope::History(session))
            }
            kind::YEARS => self.library(message, Query::Years),
            kind::MONTHS => {
                let year = Self::number(message, 2);
                let mut rows = vec![Row::Date(0xffff_ffff)];
                rows.extend(self.catalog.list(&Query::Months(year)));
                self.menu(
                    message,
                    Menu::Library {
                        query: Query::Months(year),
                        rows,
                    },
                )
            }
            kind::DAYS => {
                let year = Self::number(message, 2);
                let month = Self::number(message, 3);
                if month == 0xffff_ffff {
                    // ⟨ALL⟩ months: the year's tracks come next, so there
                    // is only the ⟨ALL⟩ row to offer.
                    let query = Query::Days { year, month };
                    return self.menu(
                        message,
                        Menu::Library {
                            query,
                            rows: vec![Row::Date(0xffff_ffff)],
                        },
                    );
                }
                let mut rows = vec![Row::Date(0xffff_ffff)];
                rows.extend(self.catalog.list(&Query::Days { year, month }));
                self.menu(
                    message,
                    Menu::Library {
                        query: Query::Days { year, month },
                        rows,
                    },
                )
            }
            kind::DATE_TRACKS => {
                let year = Self::number(message, 2);
                let month = Self::number(message, 3);
                let day = Self::number(message, 4);
                let month = (month != 0xffff_ffff).then_some(month);
                let day = (day != 0xffff_ffff).then_some(day);
                self.tracks(message, TrackScope::DateAdded { year, month, day })
            }
            kind::SEARCH | kind::SEARCH_TRACK => {
                let text = Self::text(message, 3);
                self.tracks(message, TrackScope::Search(text))
            }
            kind::METADATA => self.track_menu(message, Menu::Metadata),
            kind::TRACK_INFO => self.track_menu(message, Menu::TrackInfo),
            kind::DELIVERY_INFO => self.track_menu(message, Menu::DeliveryInfo),
            // `3007` after setup, MATCHING, and anything not built: a menu
            // with nothing in it, which a player takes in its stride.
            _ => self.menu(message, Menu::Empty),
        }
    }

    /// The requests answered with a blob.
    fn handle_blob(&mut self, message: &Message) -> Vec<Message> {
        match message.kind {
            // rekordbox's blob carries its own account's KUVO details; ours
            // is zero, a user with nothing to say. Only its presence and
            // length were seen to matter: the player's KUVO ticket waits
            // for this reply, copies the first 32 bytes, and moves on to
            // the delivery info. `[UNKNOWN]` what a KUVO user would put
            // here; the capture is verification/link/kuvo-delivery-20260919.txt.
            kind::USER_INFO => Self::blob(
                message,
                kind::USER_INFO_REPLY,
                Some(vec![0; USER_INFO_LEN]),
                None,
            ),
            kind::ARTWORK | kind::CONTENT_ARTWORK => {
                let id = Self::number(message, 1);
                // `0x2003` with the size argument names a menu item's own id,
                // rather than the artwork field in it. `0x2103` uses that same
                // content-id lookup directly (RX3 `dbcl_GetImage2`).
                let art = if message.kind == kind::CONTENT_ARTWORK || message.arguments.len() > 2 {
                    self.catalog.item_artwork(id)
                } else {
                    self.catalog.artwork(id)
                };
                Self::blob(message, kind::ARTWORK_REPLY, art, None)
            }
            kind::WAVEFORM_PREVIEW => {
                let track = Self::number(message, 2);
                self.analysis(
                    message,
                    track,
                    &Analysis::WaveformPreview,
                    kind::WAVEFORM_PREVIEW_REPLY,
                    None,
                )
            }
            kind::BEAT_GRID => {
                let track = Self::number(message, 1);
                self.analysis(
                    message,
                    track,
                    &Analysis::BeatGrid,
                    kind::BEAT_GRID_REPLY,
                    Some(u32::from(u16::from_ne_bytes(
                        self.catalog.grid_offset(track).to_ne_bytes(),
                    ))),
                )
            }
            kind::CUES => {
                let track = Self::number(message, 1);
                self.analysis(message, track, &Analysis::CueList, kind::CUES_REPLY, None)
            }
            kind::WAVEFORM_DETAIL => {
                let track = Self::number(message, 1);
                self.analysis(
                    message,
                    track,
                    &Analysis::WaveformDetail,
                    kind::WAVEFORM_DETAIL_REPLY,
                    None,
                )
            }
            kind::EXTENDED_CUES => {
                let track = Self::number(message, 1);
                // The trailing number is the cue count; the catalog's blob
                // carries it in its header, which the reply repeats.
                let blob = self.catalog.analysis(track, &Analysis::ExtendedCueList);
                let count = blob.as_ref().map_or(0, |b| extended_cue_count(b));
                Self::blob(message, kind::EXTENDED_CUES_REPLY, blob, Some(count))
            }
            kind::ANLZ_TAG | kind::ANLZ_TAG_2EX => {
                let track = Self::number(message, 1);
                let fourcc = Self::number(message, 2).to_le_bytes();
                let ext = Self::number(message, 3).to_le_bytes();
                let what = Analysis::Tag {
                    fourcc,
                    extension: [ext[0], ext[1], ext[2]],
                };
                self.analysis(message, track, &what, kind::ANLZ_TAG_REPLY, Some(1))
            }
            _ => Vec::new(),
        }
    }
}

fn prepend_all_if_multiple(rows: &mut Vec<Row>) {
    if rows.len() > 1 {
        rows.insert(
            0,
            Row::Named {
                id: u32::MAX,
                name: String::new(),
            },
        );
    }
}

impl Session for LinkSession {
    #[allow(clippy::too_many_lines, reason = "one protocol dispatch table; splitting it would obscure its message coverage")]
    fn handle(&mut self, message: &Message) -> Vec<Message> {
        let tx = message.transaction;
        match message.kind {
            kind::SETUP => {
                self.player = u8::try_from(Self::number(message, 0)).unwrap_or(0);
                self.extended = message.arguments.len() > 1;
                if self.extended {
                    vec![setup_reply(tx, self.device)]
                } else {
                    vec![menu_header(tx, 0, u32::from(self.device))]
                }
            }
            // Neither request expects a reply. In particular, RX3's
            // `dbcl_SetOnAir` must not replace an unrelated pending menu with
            // a spurious generic `0x4000` response.
            kind::TEARDOWN | kind::SET_ON_AIR => Vec::new(),
            // RX3 `dbcl_GetBrowseType` falls back to this request when the
            // device-property response has no browse kind. `1` is the
            // database-backed/export-media kind the firmware uses for its
            // ordinary browse flow.
            kind::BROWSE_TYPE => vec![menu_header(tx, u32::from(message.kind), 1)],
            // RX3's `DBSMain_OnOtherClientCmd` answers these two scalar
            // queries through the ordinary `0x4000` envelope. The track id
            // follows the connection context in each request.
            kind::TRACK_BPM => vec![menu_header(
                tx,
                u32::from(message.kind),
                self.catalog
                    .track_row(Self::number(message, 1))
                    .map_or(0, |track| track.bpm_x100),
            )],
            kind::TRACK_PLAY_STATE => vec![menu_header(
                tx,
                u32::from(message.kind),
                u32::from(self.catalog.played(Self::number(message, 1))),
            )],
            // RX3 converts the `djmdKey` ID returned by its legacy key menu
            // before opening a related-key menu. The virtual legacy menu
            // already advertises the dense 1..=24 IDs, which are exactly the
            // values its newer menu family consumes.
            kind::LEGACY_KEY_TO_NEW_KEY => {
                let key = Self::number(message, 1);
                vec![menu_header(
                    tx,
                    u32::from(message.kind),
                    if (1..=24).contains(&key) { key } else { 0 },
                )]
            }
            // `Dsql_getContentNewKeyID` looks up a content record's raw key
            // and converts it to the same dense ID. `TrackRow::key` retains
            // that canonical value independently of the display key text.
            kind::CONTENT_NEW_KEY => vec![menu_header(
                tx,
                u32::from(message.kind),
                self.catalog
                    .track_row(Self::number(message, 1))
                    .map_or(0, |track| track.key),
            )],
            // `dbcl_GetIsRekordboxMobile` waits for a `0x4b02` reply, not a
            // menu header. rbxport is a desktop rekordbox-export source, so
            // report false and the empty mobile mount name, just as RX3 does
            // for a non-mobile source.
            kind::REKORDBOX_MOBILE => vec![Message::new(
                tx,
                kind::REKORDBOX_MOBILE_REPLY,
                vec![
                    Argument::Number(0),
                    Argument::Number(2),
                    Argument::String(String::new()),
                ],
            )],
            // RX3 Hot Cue Banks are their own browse/cue protocol, not an
            // ordinary track menu.  The device's database service returns
            // 4000/4101 for `2001`, then 4702 cue envelopes for reads/edits.
            kind::HOT_CUE_BANK => self.hot_cue_bank_menu(message),
            kind::HOT_CUE_BANK_CUES => {
                if Self::number(message, 0) & 0xff != 1 {
                    return Self::hot_cue_bank_unavailable(message);
                }
                Self::hot_cue_bank_reply(message, self.catalog.hot_cue_bank_cues(Self::number(message, 1)))
            }
            kind::CHANGE_HOT_CUE_BANK => {
                if Self::number(message, 0) & 0xff != 1 {
                    return Self::hot_cue_bank_unavailable(message);
                }
                let Some((bank, cue)) = Self::hot_cue_bank_edit(message) else {
                    return Self::hot_cue_bank_unavailable(message);
                };
                let track = cue.content;
                if !self.catalog.edit(&Edit::HotCueBankCue { bank, cue }) {
                    return Self::hot_cue_bank_unavailable(message);
                }
                Self::usb_cue_reply(message, self.catalog.usb_cues(track))
            }
            kind::GRID_OFFSET => vec![menu_header(
                tx,
                u32::from(message.kind),
                u32::from(u16::from_ne_bytes(
                    self.catalog
                        .grid_offset(Self::number(message, 1))
                        .to_ne_bytes(),
                )),
            )],
            kind::SAVE_GRID_OFFSET => {
                let raw = Self::number(message, 2).to_be_bytes();
                let success = self.catalog.edit(&Edit::GridOffset {
                    track: Self::number(message, 1),
                    offset_ms: i16::from_be_bytes([raw[2], raw[3]]),
                });
                vec![menu_header(
                    tx,
                    u32::from(message.kind),
                    u32::from(!success),
                )]
            }
            kind::FILTER_SWITCH => {
                self.filter.enabled = Self::number(message, 1) != 0;
                vec![menu_header(tx, u32::from(message.kind), 0)]
            }
            kind::FILTER_GET => Self::blob(
                message,
                kind::FILTER_REPLY,
                Some(self.filter.encode()),
                Some(4),
            ),
            kind::FILTER_SET => {
                let valid = match message.arguments.get(4) {
                    Some(Argument::Blob(bytes))
                        if bytes.len() == Self::number(message, 3) as usize =>
                    {
                        self.filter.update(Self::number(message, 1), bytes)
                    }
                    _ => false,
                };
                vec![menu_header(tx, u32::from(message.kind), u32::from(!valid))]
            }
            kind::CHANGE_TAG | kind::CLEAR_TAGS | kind::CHANGE_RATING => {
                let track = Self::number(message, 1);
                let value = Self::number(message, 2);
                let edit = match message.kind {
                    kind::CHANGE_TAG if value <= 1 => Some(Edit::Tag {
                        track,
                        add: value == 1,
                    }),
                    kind::CLEAR_TAGS => Some(Edit::ClearTags),
                    kind::CHANGE_RATING if value <= 5 => Some(Edit::Rating {
                        track,
                        stars: u8::try_from(value).unwrap_or(0),
                    }),
                    _ => None,
                };
                let success = edit.is_some_and(|edit| self.catalog.edit(&edit));
                vec![menu_header(
                    tx,
                    u32::from(message.kind),
                    u32::from(!success),
                )]
            }
            // rekordbox acts on these only for its own tracks (the track
            // type, the last byte of the first argument, is 1) and answers
            // only the track removal (`PSvDBMain::OnHistoryCmd`). The other
            // two a player sends and forgets: a reply would reach it as the
            // answer to nothing.
            kind::INSERT_HISTORY | kind::DELETE_HISTORY | kind::DELETE_HISTORY_TRACK => {
                let rekordbox_track = Self::number(message, 0) & 0xff == 1;
                let target = Self::number(message, 1);
                match message.kind {
                    kind::INSERT_HISTORY => {
                        if rekordbox_track {
                            self.catalog.edit(&Edit::HistoryAdd { track: target });
                        }
                        Vec::new()
                    }
                    kind::DELETE_HISTORY => {
                        if rekordbox_track {
                            self.catalog.edit(&Edit::HistoryDelete { history: target });
                        }
                        Vec::new()
                    }
                    // A player waits for this one, so it is answered either
                    // way, where rekordbox would leave a stranger unanswered.
                    _ => {
                        let success = rekordbox_track
                            && self.catalog.edit(&Edit::HistoryRemove { track: target });
                        vec![menu_header(
                            tx,
                            u32::from(message.kind),
                            if success { 0 } else { u32::MAX },
                        )]
                    }
                }
            }

            kind::RENDER => self.render(message),
            kind::ITEM_POSITION => {
                let id = Self::number(message, 1);
                let location = Self::menu_location(message);
                let position = match self.menus.get(&location) {
                    Some(Menu::Library { rows, .. }) => rows.iter().position(|row| match row {
                        Row::Named { id: item, .. }
                        | Row::List { id: item, .. }
                        | Row::Track { id: item, .. }
                        | Row::Date(item) => *item == id,
                    }),
                    _ => self
                        .items(location, 0, u32::MAX)
                        .iter()
                        .position(|item| item.id == id),
                }
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(u32::MAX);
                vec![menu_header(tx, u32::from(kind::ITEM_POSITION), position)]
            }
            kind::ARTWORK
            | kind::CONTENT_ARTWORK
            | kind::WAVEFORM_PREVIEW
            | kind::BEAT_GRID
            | kind::CUES
            | kind::WAVEFORM_DETAIL
            | kind::EXTENDED_CUES
            | kind::ANLZ_TAG
            | kind::ANLZ_TAG_2EX
            | kind::USER_INFO => self.handle_blob(message),
            _ => self.handle_menu(message),
        }
    }
}

/// The number of cues an extended cue list holds, which the reply repeats as
/// its trailing argument. The blob is the entries concatenated with no count
/// header, each led by its own byte length as a little-endian u32, so the
/// count is recovered by walking them — the way rekordbox's own count matches
/// its blob. Reading a fixed offset instead gave a CDJ a nonsense count (the
/// first entry's own fields) and it faulted allocating for that many cues.
fn extended_cue_count(blob: &[u8]) -> u32 {
    let mut offset = 0;
    let mut count = 0;
    while let [a, b, c, d] = blob.get(offset..offset + 4).unwrap_or(&[]) {
        let entry_len = u32::from_le_bytes([*a, *b, *c, *d]) as usize;
        if entry_len < 4 {
            break;
        }
        offset += entry_len;
        count += 1;
    }
    count
}

/// The sixteen metadata rows in the order negotiated for the player family.
fn metadata_rows(t: &TrackDetails, played: bool, tagged: bool, legacy: bool) -> Vec<Item> {
    let title = Item::track(
        &t.row,
        (if played { track_flags::PLAYED } else { 0 }) | u32::from(tagged),
        0,
    );
    let artist = Item::line(1, t.artist_id, &t.artist, item_type::ARTIST);
    let album = Item::line(1, t.album_id, &t.album, item_type::ALBUM);
    let duration = Item::line(0, t.duration_s, "", item_type::DURATION);
    let tempo = Item::line(0, t.row.bpm_x100, "", item_type::TEMPO);
    let key = Item::line(1, t.key_id, &t.key_name, item_type::KEY);
    let rating = Item::line(0, t.rating, "", item_type::RATING);
    let colour = Item::line(0, t.colour, "", item_type::COLOUR);
    let genre = Item::line(0, t.genre_id, &t.genre, item_type::GENRE);
    let date_added = Item::line(1, t.row.id, &t.date_added, item_type::DATE_ADDED);
    let comment = Item::line(0, t.row.id, &t.comment, item_type::COMMENT);
    let year = Item::line(0, t.year, "", item_type::YEAR);
    let bitrate = Item::line(0, t.bit_rate_kbps, "", item_type::BIT_RATE);
    let label = Item::line(0, t.label_id, &t.label, item_type::LABEL);
    let original_artist = Item::line(0, 0, &t.original_artist, item_type::ORIGINAL_ARTIST);
    let remixer = Item::line(0, 0, &t.remixer, item_type::REMIXER);

    if legacy {
        return vec![
            title,
            artist,
            album,
            duration,
            tempo,
            comment,
            key,
            rating,
            colour,
            genre,
            date_added,
            bitrate,
            year,
            label,
            original_artist,
            remixer,
        ];
    }

    vec![
        title,
        artist,
        album,
        duration,
        tempo,
        key,
        rating,
        colour,
        genre,
        date_added,
        comment,
        year,
        bitrate,
        label,
        original_artist,
        remixer,
    ]
}

/// The thirteen rows of a delivery-info reply, in rekordbox's order (captured
/// from 7.2.11 answering a CDJ-3000 that had just loaded a track from it).
/// The firmware's KUVO ticket reads them by type: the texts of ARTIST, ALBUM,
/// GENRE, LABEL, COMMENT, 0x36 and 0x37; the ids of DURATION, TEMPO, KEY and
/// `FILE_TYPE`; from the `TITLE` row its text, first slot and ninth slot; and
/// from the closing 0x4f row its first slot, ninth slot and second text.
fn delivery_rows(t: &TrackDetails) -> Vec<Item> {
    vec![
        Item::line(0, 0, "", item_type::DELIVERY_TEXT_36),
        Item::line(0, t.artist_id, &t.artist, item_type::ARTIST),
        Item::line(0, t.key_id, "", item_type::KEY),
        Item::line(0, t.duration_s, "", item_type::DURATION),
        Item {
            a: t.row.id,
            id: t.row.id,
            text: t.row.title.clone(),
            item_type: item_type::TITLE,
            flags: track_flags::LISTED,
            c: t.row.id,
            e: 0x100,
            f: t.row.bpm_x100,
            ..Item::default()
        },
        Item::line(0, t.row.id, &t.comment, item_type::COMMENT),
        Item::line(0, t.album_id, &t.album, item_type::ALBUM),
        Item::line(0, t.row.bpm_x100, "", item_type::TEMPO),
        Item::line(0, t.row.id, "", item_type::DELIVERY_TEXT_37),
        Item::line(0, t.label_id, &t.label, item_type::LABEL),
        Item::line(0, t.file_type, "", item_type::FILE_TYPE),
        Item::line(0, t.genre_id, &t.genre, item_type::GENRE),
        Item {
            a: t.row.id,
            item_type: item_type::DELIVERY_ID,
            c: 1,
            ..Item::default()
        },
    ]
}

/// The seven rows of a track-info reply.
fn track_info_rows(t: &TrackDetails) -> Vec<Item> {
    vec![
        // rekordbox puts the file's copyright comment here; we have no such
        // field, so the row is blank but shaped the same.
        Item {
            a: 1,
            id: 1,
            item_type: item_type::TRACK,
            e: 0x100,
            key: t.row.key,
            art: if t.row.artwork == 0 { 1 } else { t.row.artwork },
            text3: t.row.key_name.clone(),
            f: t.row.bpm_x100,
            ..Item::default()
        },
        Item::line(0, t.duration_s, "", item_type::DURATION),
        Item::line(0, t.row.bpm_x100, "", item_type::TEMPO),
        Item::line(0, t.row.id, &t.comment, item_type::COMMENT),
        Item::line(t.file_size, t.row.id, &t.path, item_type::PATH),
        Item::line(0, 1, "", item_type::INFO_UNKNOWN),
        Item::line(0, t.key_id, &t.key_name, item_type::KEY),
    ]
}
