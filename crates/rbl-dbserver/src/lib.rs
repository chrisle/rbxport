//! The remote-database protocol a CDJ uses to browse another device's library.
//!
//! A client opens a TCP connection, asks port 12523 which port the database
//! server is on, then exchanges framed messages: a magic, a transaction id, a
//! type, and up to twelve typed arguments.
//!
//! # Verified and unverified
//!
//! The framing and field encodings here follow the community protocol analysis
//! and are tested by round-tripping. What is **not** yet verified is the part
//! that only a capture can settle: which menus rekordbox actually exposes as a
//! source, and the exact item-type codes a player accepts. Those are marked
//! [`Unverified`] and must be confirmed before a player is expected to browse us.
//!
//! Nothing here opens a socket; it is a codec, so it is testable exhaustively.

pub mod catalog;
pub mod filter;
pub mod item;
pub mod keys;
pub mod net;
pub mod session;

/// Every message starts with this.
pub const MAGIC: u32 = 0x8723_49ae;

/// The port a client asks for the database server's real port.
pub const PORT_QUERY: u16 = 12_523;

/// What a player sends to the port-query service: a four-byte big-endian
/// length, then the name with its NUL (measured: `00 00 00 0f RemoteDBServer 00`).
pub const PORT_QUERY_REQUEST: &[u8] = b"\x00\x00\x00\x0fRemoteDBServer\0";

/// The five bytes each side sends first on the database connection
/// (measured; a number field holding 1).
pub const GREETING: &[u8] = &[0x11, 0x00, 0x00, 0x00, 0x01];

/// Transaction id used for setup and teardown.
pub const SETUP_TXID: u32 = 0xffff_fffe;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DbError {
    #[error("message is truncated: wanted {wanted} bytes, had {had}")]
    Truncated { wanted: usize, had: usize },
    #[error("not a database message: magic was {0:#010x}")]
    BadMagic(u32),
    #[error("unknown argument tag {0:#04x}")]
    UnknownTag(u8),
    #[error("message declares {0} arguments, which is more than twelve")]
    TooManyArguments(u8),
}

pub type Result<T> = std::result::Result<T, DbError>;

/// A message argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Argument {
    /// A four-byte big-endian integer.
    Number(u32),
    /// UTF-16BE text.
    String(String),
    Blob(Vec<u8>),
}

impl Argument {
    /// The argument for a log line: a number as is, text quoted, a blob by
    /// its length.
    pub fn describe(&self) -> String {
        match self {
            Self::Number(n) => n.to_string(),
            Self::String(s) => format!("{s:?}"),
            Self::Blob(b) => format!("<{} bytes>", b.len()),
        }
    }

    /// The tag that appears in the header's type list.
    fn arg_tag(&self) -> u8 {
        match self {
            Self::String(_) => 0x02,
            Self::Blob(_) => 0x03,
            Self::Number(_) => 0x06,
        }
    }

    /// The tag that prefixes the value itself. Note it differs from the
    /// header tag — the protocol carries the type twice, in two encodings.
    fn field_tag(&self) -> u8 {
        match self {
            Self::Number(_) => 0x11,
            Self::Blob(_) => 0x14,
            Self::String(_) => 0x26,
        }
    }

    fn encode(&self, out: &mut Vec<u8>) {
        out.push(self.field_tag());
        match self {
            Self::Number(v) => out.extend_from_slice(&v.to_be_bytes()),
            Self::Blob(bytes) => {
                out.extend_from_slice(&u32::try_from(bytes.len()).unwrap_or(0).to_be_bytes());
                out.extend_from_slice(bytes);
            }
            Self::String(text) => {
                // The length counts UTF-16 code units including the trailing NUL.
                let units: Vec<u16> = text.encode_utf16().collect();
                let count = u32::try_from(units.len() + 1).unwrap_or(0);
                out.extend_from_slice(&count.to_be_bytes());
                for unit in units {
                    out.extend_from_slice(&unit.to_be_bytes());
                }
                out.extend_from_slice(&[0, 0]);
            }
        }
    }
}

/// One framed message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub transaction: u32,
    pub kind: u16,
    pub arguments: Vec<Argument>,
}

/// Bytes of header before the tag list: each field carries its own type tag
/// — `11` magic, `11` transaction, `10` kind, `0f` argument count — and then
/// the tag list opens as a blob (`14`, four-byte length). Measured from
/// rekordbox 7.2.11 serving a CDJ-3000 (`verification/link`, 2026-09-12): the
/// earlier layout here had no field tags and a fixed twelve-byte tag list,
/// and matched nothing on the wire.
const HEADER_LEN: usize = 1 + 4 + 1 + 4 + 1 + 2 + 1 + 1 + 1 + 4;
/// The most arguments a message may carry; the tag list holds one byte each.
/// A menu item carries sixteen.
const TAG_SLOTS: usize = 32;

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([
        b.get(at).copied().unwrap_or(0),
        b.get(at + 1).copied().unwrap_or(0),
        b.get(at + 2).copied().unwrap_or(0),
        b.get(at + 3).copied().unwrap_or(0),
    ])
}

impl Message {
    pub fn new(transaction: u32, kind: u16, arguments: Vec<Argument>) -> Self {
        Self {
            transaction,
            kind,
            arguments,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let count = self.arguments.len().min(TAG_SLOTS);
        let mut out = Vec::with_capacity(HEADER_LEN + count * 8);
        out.push(0x11);
        out.extend_from_slice(&MAGIC.to_be_bytes());
        out.push(0x11);
        out.extend_from_slice(&self.transaction.to_be_bytes());
        out.push(0x10);
        out.extend_from_slice(&self.kind.to_be_bytes());
        out.push(0x0f);
        out.push(u8::try_from(count).unwrap_or(0));
        // The tag list is itself a blob field, one byte an argument.
        out.push(0x14);
        out.extend_from_slice(&u32::try_from(count).unwrap_or(0).to_be_bytes());
        for argument in self.arguments.iter().take(count) {
            out.push(argument.arg_tag());
        }
        for argument in self.arguments.iter().take(count) {
            // An empty blob is declared in the tag list and not written; the
            // number before it, its length, says it is absent.
            if matches!(argument, Argument::Blob(b) if b.is_empty()) {
                continue;
            }
            argument.encode(&mut out);
        }
        out
    }

    /// Decodes one message, returning it and how many bytes it consumed.
    ///
    /// Returning the length is what lets a reader handle several messages
    /// arriving in one TCP segment, and detect one split across segments.
    pub fn decode(bytes: &[u8]) -> Result<(Self, usize)> {
        if bytes.len() < HEADER_LEN {
            return Err(DbError::Truncated {
                wanted: HEADER_LEN,
                had: bytes.len(),
            });
        }
        // The field tags are checked as part of the magic: a stream that is
        // not at a message boundary fails here rather than misreading a body.
        let magic = be32(bytes, 1);
        if bytes[0] != 0x11
            || magic != MAGIC
            || bytes[5] != 0x11
            || bytes[10] != 0x10
            || bytes[13] != 0x0f
            || bytes[15] != 0x14
        {
            return Err(DbError::BadMagic(magic));
        }
        let transaction = be32(bytes, 6);
        let kind = u16::from_be_bytes([
            bytes.get(11).copied().unwrap_or(0),
            bytes.get(12).copied().unwrap_or(0),
        ]);
        let count = bytes.get(14).copied().unwrap_or(0);
        if count as usize > TAG_SLOTS {
            return Err(DbError::TooManyArguments(count));
        }
        // rekordbox and the CDJ-3000 send a tag list exactly `count` long;
        // alphatheta-connect (Now Playing) and its kin send a fixed twelve
        // bytes zero-padded, and players answer them. Both are accepted: the
        // first `count` tags are read and the rest of the list skipped.
        let tags = be32(bytes, 16) as usize;
        if tags < count as usize || tags > TAG_SLOTS {
            return Err(DbError::BadMagic(magic));
        }

        let mut at = HEADER_LEN + tags;
        if bytes.len() < at {
            return Err(DbError::Truncated {
                wanted: at,
                had: bytes.len(),
            });
        }
        let declared: Vec<u8> = bytes
            .get(HEADER_LEN..HEADER_LEN + count as usize)
            .unwrap_or(&[])
            .to_vec();
        let mut arguments = Vec::with_capacity(count as usize);
        for declared_tag in declared {
            // An empty blob is not sent at all. The number before a blob is
            // always its length, and when that number is 0 the blob field is
            // absent — the "no artwork" reply declares four arguments and
            // carries three, and a player's metadata request declares five
            // and carries four. [DOC] Deep Symmetry, track_metadata; measured
            // both ways in the capture.
            if declared_tag == 0x03 && matches!(arguments.last(), Some(Argument::Number(0)) | None)
            {
                arguments.push(Argument::Blob(Vec::new()));
                continue;
            }
            let tag = bytes.get(at).copied().ok_or(DbError::Truncated {
                wanted: at + 1,
                had: bytes.len(),
            })?;
            at += 1;
            match tag {
                0x0f..=0x11 => {
                    // Numbers are one, two or four bytes depending on the tag.
                    let width = match tag {
                        0x0f => 1,
                        0x10 => 2,
                        _ => 4,
                    };
                    if at + width > bytes.len() {
                        return Err(DbError::Truncated {
                            wanted: at + width,
                            had: bytes.len(),
                        });
                    }
                    let value = match width {
                        1 => u32::from(bytes.get(at).copied().unwrap_or(0)),
                        2 => u32::from(u16::from_be_bytes([
                            bytes.get(at).copied().unwrap_or(0),
                            bytes.get(at + 1).copied().unwrap_or(0),
                        ])),
                        _ => be32(bytes, at),
                    };
                    arguments.push(Argument::Number(value));
                    at += width;
                }
                0x14 => {
                    let len = be32(bytes, at) as usize;
                    at += 4;
                    if at + len > bytes.len() {
                        return Err(DbError::Truncated {
                            wanted: at + len,
                            had: bytes.len(),
                        });
                    }
                    arguments.push(Argument::Blob(
                        bytes.get(at..at + len).unwrap_or(&[]).to_vec(),
                    ));
                    at += len;
                }
                0x26 => {
                    // The count is UTF-16 code units including the trailing NUL.
                    let units = be32(bytes, at) as usize;
                    at += 4;
                    let byte_len = units.saturating_mul(2);
                    if at + byte_len > bytes.len() {
                        return Err(DbError::Truncated {
                            wanted: at + byte_len,
                            had: bytes.len(),
                        });
                    }
                    let raw = bytes.get(at..at + byte_len).unwrap_or(&[]);
                    let text: Vec<u16> = raw
                        .chunks_exact(2)
                        .map(|c| u16::from_be_bytes([c[0], c[1]]))
                        .take_while(|&u| u != 0)
                        .collect();
                    arguments.push(Argument::String(String::from_utf16_lossy(&text)));
                    at += byte_len;
                }
                other => return Err(DbError::UnknownTag(other)),
            }
        }

        Ok((
            Self {
                transaction,
                kind,
                arguments,
            },
            at,
        ))
    }

    /// Decodes as many whole messages as the buffer holds, returning them and
    /// how many bytes were consumed. A partial trailing message is left behind.
    pub fn decode_all(bytes: &[u8]) -> (Vec<Self>, usize) {
        let mut out = Vec::new();
        let mut at = 0;
        while at < bytes.len() {
            match Self::decode(bytes.get(at..).unwrap_or(&[])) {
                Ok((message, used)) if used > 0 => {
                    out.push(message);
                    at += used;
                }
                // Truncated means the rest is a partial message: stop and keep it.
                _ => break,
            }
        }
        (out, at)
    }
}

/// Message types.
///
/// Request codes come from the protocol analysis. Which of these rekordbox
/// answers as a source, and with what item types, is [`Unverified`].
pub mod kind {
    /// Opens a session; the reply carries the server's own device number.
    pub const SETUP: u16 = 0x0000;
    /// Closes a session.
    pub const TEARDOWN: u16 = 0x0100;
    /// The top-level menu for a media slot.
    pub const ROOT_MENU: u16 = 0x1000;
    pub const GENRE_MENU: u16 = 0x1001;
    pub const ARTIST_MENU: u16 = 0x1002;
    pub const ALBUM_MENU: u16 = 0x1003;
    /// Every track.
    pub const TRACK_MENU: u16 = 0x1004;
    /// Distinct rounded BPM values.
    pub const BPM_MENU: u16 = 0x1006;
    /// Distinct track ratings.
    pub const RATING_MENU: u16 = 0x1007;
    /// Release-year decades.
    pub const RELEASE_DECADES: u16 = 0x1008;
    /// Labels referenced by tracks.
    pub const LABEL_MENU: u16 = 0x100a;
    /// The fixed eight-colour palette.
    pub const COLOR_MENU: u16 = 0x100d;
    /// Track-duration minute buckets.
    pub const TIME_MENU: u16 = 0x1010;
    /// Distinct bitrates.
    pub const BITRATE_MENU: u16 = 0x1011;
    pub const HISTORY_MENU: u16 = 0x1012;
    /// Every track, named by its file name rather than its title.
    pub const FILE_NAME_MENU: u16 = 0x1013;
    pub const KEY_MENU: u16 = 0x1014;
    /// Tracks paired with a seed track in rekordbox's matching table.
    pub const MATCHING_TRACKS: u16 = 0x1017;
    pub const ORIGINAL_ARTIST_MENU: u16 = 0x1302;
    pub const ORIGINAL_ARTIST_ALBUMS: u16 = 0x1402;
    pub const ORIGINAL_ARTIST_ALBUM_TRACKS: u16 = 0x1502;
    pub const REMIXER_MENU: u16 = 0x1602;
    pub const REMIXER_ALBUMS: u16 = 0x1702;
    pub const REMIXER_ALBUM_TRACKS: u16 = 0x1802;
    /// An artist's albums.
    pub const GENRE_ARTISTS: u16 = 0x1101;
    pub const ARTIST_ALBUMS: u16 = 0x1102;
    /// An album's tracks.
    pub const ALBUM_TRACKS: u16 = 0x1103;
    /// Playlists and folders (last argument 1), or a playlist's tracks (0).
    pub const PLAYLIST_MENU: u16 = 0x1105;
    /// BPM tolerances from zero through six percent.
    pub const BPM_RANGES: u16 = 0x1106;
    /// Tracks with a rating.
    pub const RATING_TRACKS: u16 = 0x1107;
    /// Release years in a decade.
    pub const RELEASE_YEARS: u16 = 0x1108;
    /// Artists referenced by tracks on a label.
    pub const LABEL_ARTISTS: u16 = 0x110a;
    /// Tracks assigned a colour.
    pub const COLOR_TRACKS: u16 = 0x110d;
    /// Tracks in a duration minute bucket.
    pub const TIME_TRACKS: u16 = 0x1110;
    /// Tracks with a bitrate.
    pub const BITRATE_TRACKS: u16 = 0x1111;
    /// A session's tracks.
    pub const HISTORY_TRACKS: u16 = 0x1112;
    /// The three related-key rows for a key.
    pub const RELATED_KEYS: u16 = 0x1114;
    /// An artist's tracks, on one album or all.
    pub const GENRE_ARTIST_ALBUMS: u16 = 0x1201;
    pub const ARTIST_ALBUM_TRACKS: u16 = 0x1202;
    /// Tracks within a BPM tolerance.
    pub const BPM_TRACKS: u16 = 0x1206;
    /// Tracks released in a year.
    pub const RELEASE_YEAR_TRACKS: u16 = 0x1208;
    /// Albums referenced by tracks on a label and, optionally, an artist.
    pub const LABEL_ARTIST_ALBUMS: u16 = 0x120a;
    /// A genre's tracks, optionally narrowed to an artist and album.
    pub const GENRE_ARTIST_ALBUM_TRACKS: u16 = 0x1301;
    /// Tracks on a label, optionally narrowed to an artist and album.
    pub const LABEL_ARTIST_ALBUM_TRACKS: u16 = 0x130a;
    /// Tracks in a key, widened by a distance.
    pub const KEY_TRACKS: u16 = 0x1214;
    /// Search by text.
    pub const SEARCH: u16 = 0x1300;
    /// CDJ-3000 live keyboard search, with the same arguments as SEARCH.
    pub const SEARCH_TRACK: u16 = 0x1500;
    /// The sort options a track list offers.
    pub const SORT_MENU: u16 = 0x1400;
    /// DATE ADDED: the years.
    pub const YEARS: u16 = 0x1708;
    /// A year's months.
    pub const MONTHS: u16 = 0x1808;
    /// A month's days.
    pub const DAYS: u16 = 0x1908;
    /// The tracks added on a day, in a month, or in a year. `[ASSUME]` by
    /// the pattern of the three above; not captured.
    pub const DATE_TRACKS: u16 = 0x1a08;
    /// Metadata for one track.
    pub const METADATA: u16 = 0x2002;
    /// Album art.
    pub const ARTWORK: u16 = 0x2003;
    /// The small waveform preview.
    pub const WAVEFORM_PREVIEW: u16 = 0x2004;
    /// Track information: the path and the copyright text (7 rows).
    pub const TRACK_INFO: u16 = 0x2102;
    /// The beat grid.
    pub const BEAT_GRID: u16 = 0x2204;
    pub const SAVE_GRID_OFFSET: u16 = 0x2605;
    pub const GRID_OFFSET: u16 = 0x2804;
    /// The track a player delivers to KUVO, as the firmware names it
    /// (`CMD_GET_DELIVERY_INFO`): a 13-row menu much like [`METADATA`],
    /// asked for after every load from a rekordbox source, once the user
    /// info below has been answered. Captured from rekordbox 7.2.11
    /// (`verification/link/kuvo-delivery-20260919.txt`).
    pub const DELIVERY_INFO: u16 = 0x2602;
    /// Cues and loops.
    pub const CUES: u16 = 0x2504;
    /// The waveform detail.
    pub const WAVEFORM_DETAIL: u16 = 0x2904;
    /// Cues and loops with names and colours.
    pub const EXTENDED_CUES: u16 = 0x2b04;
    /// A whole analysis tag from the `.EXT` file.
    pub const ANLZ_TAG: u16 = 0x2c04;
    /// A whole analysis tag from the `.2EX` file.
    pub const ANLZ_TAG_2EX: u16 = 0x2d04;
    /// Asks for the rows of the menu just requested.
    pub const RENDER: u16 = 0x3000;
    /// The KUVO user info (`CMD_GET_USER_INFO`), asked for after every load
    /// from a rekordbox source. The player waits on the reply and sends no
    /// other request until it has one: unanswered, its browser sits on
    /// "Waiting…" for eighteen seconds, retries twice, and gives the source
    /// up. Answered with [`USER_INFO_REPLY`].
    pub const USER_INFO: u16 = 0x3006;
    /// The zero-based offset of an item in the current menu.
    pub const ITEM_POSITION: u16 = 0x3100;
    /// A player adding a track to its history (`CMD_INSERT_HISTORY`):
    /// `[r:m:s:t, track]`, sent without waiting for a reply, and rekordbox
    /// 7.2.11 sends none (`PSvDBMain::OnHistoryCmd`).
    pub const INSERT_HISTORY: u16 = 0x3001;
    /// A player deleting its history (`CMD_DEL_HISTORY`): `[r:m:s:t,
    /// history]`, again without a reply.
    pub const DELETE_HISTORY: u16 = 0x3101;
    /// A player taking a track off its history (`CMD_DEL_HISTORY_TRACK`):
    /// `[r:m:s:t, track]`, answered `[0x3401, 0]`, or `-1` when it failed.
    pub const DELETE_HISTORY_TRACK: u16 = 0x3401;
    pub const TAG_LIST: u16 = 0x100f;
    pub const CHANGE_TAG: u16 = 0x3002;
    pub const CLEAR_TAGS: u16 = 0x3202;
    pub const CHANGE_RATING: u16 = 0x2107;
    pub const FILTER_SWITCH: u16 = 0x3007;
    pub const FILTER_GET: u16 = 0x3107;
    pub const FILTER_SET: u16 = 0x3207;
    pub const FILTER_REPLY: u16 = 0x4004;
    /// "Here is how many items your query matched."
    pub const MENU_HEADER: u16 = 0x4000;
    /// Opens a rendered menu: `[1, offset]`.
    pub const RENDER_HEADER: u16 = 0x4001;
    pub const ARTWORK_REPLY: u16 = 0x4002;
    /// The query failed.
    pub const ERROR: u16 = 0x4003;
    /// One row of a menu.
    pub const MENU_ITEM: u16 = 0x4101;
    /// End of a menu.
    pub const MENU_FOOTER: u16 = 0x4201;
    pub const WAVEFORM_PREVIEW_REPLY: u16 = 0x4402;
    pub const CUES_REPLY: u16 = 0x4502;
    pub const BEAT_GRID_REPLY: u16 = 0x4602;
    pub const WAVEFORM_DETAIL_REPLY: u16 = 0x4a02;
    /// `CMD_RET_USER_INFO`: `[0x3006, 0, 160, blob[160]]`, as rekordbox
    /// sends it. The player copies the blob's first 32 bytes into the
    /// delivery it makes to KUVO, and needs it at least that long.
    pub const USER_INFO_REPLY: u16 = 0x4d02;
    pub const EXTENDED_CUES_REPLY: u16 = 0x4e02;
    pub const ANLZ_TAG_REPLY: u16 = 0x4f02;

    /// What a message kind is called, for a log line; `kind 0x....` for one
    /// this server has no name for.
    pub fn name(kind: u16) -> String {
        match kind {
            SETUP => "setup".to_owned(),
            TEARDOWN => "teardown".to_owned(),
            ROOT_MENU => "root menu".to_owned(),
            GENRE_MENU => "genre menu".to_owned(),
            GENRE_ARTISTS => "genre's artists".to_owned(),
            GENRE_ARTIST_ALBUMS => "genre artist's albums".to_owned(),
            GENRE_ARTIST_ALBUM_TRACKS => "genre artist's album tracks".to_owned(),
            ARTIST_MENU => "artist menu".to_owned(),
            ALBUM_MENU => "album menu".to_owned(),
            TRACK_MENU => "track menu".to_owned(),
            BPM_MENU => "BPM menu".to_owned(),
            BPM_RANGES => "BPM ranges".to_owned(),
            BPM_TRACKS => "BPM tracks".to_owned(),
            RATING_MENU => "rating menu".to_owned(),
            RATING_TRACKS => "rating tracks".to_owned(),
            RELEASE_DECADES => "release decades".to_owned(),
            RELEASE_YEARS => "release years".to_owned(),
            RELEASE_YEAR_TRACKS => "release-year tracks".to_owned(),
            LABEL_MENU => "label menu".to_owned(),
            LABEL_ARTISTS => "label's artists".to_owned(),
            LABEL_ARTIST_ALBUMS => "label artist's albums".to_owned(),
            LABEL_ARTIST_ALBUM_TRACKS => "label artist's album tracks".to_owned(),
            COLOR_MENU => "color menu".to_owned(),
            COLOR_TRACKS => "color tracks".to_owned(),
            TIME_MENU => "time menu".to_owned(),
            TIME_TRACKS => "time tracks".to_owned(),
            BITRATE_MENU => "bitrate menu".to_owned(),
            BITRATE_TRACKS => "bitrate tracks".to_owned(),
            FILE_NAME_MENU => "file-name menu".to_owned(),
            MATCHING_TRACKS => "matching tracks".to_owned(),
            HISTORY_MENU => "history menu".to_owned(),
            KEY_MENU => "key menu".to_owned(),
            ARTIST_ALBUMS => "artist's albums".to_owned(),
            ALBUM_TRACKS => "album's tracks".to_owned(),
            PLAYLIST_MENU => "playlist menu".to_owned(),
            HISTORY_TRACKS => "history's tracks".to_owned(),
            INSERT_HISTORY => "insert history".to_owned(),
            DELETE_HISTORY => "delete history".to_owned(),
            DELETE_HISTORY_TRACK => "delete history track".to_owned(),
            RELATED_KEYS => "related keys".to_owned(),
            ARTIST_ALBUM_TRACKS => "artist's album tracks".to_owned(),
            KEY_TRACKS => "key's tracks".to_owned(),
            SEARCH => "search".to_owned(),
            SORT_MENU => "sort menu".to_owned(),
            YEARS => "years".to_owned(),
            MONTHS => "months".to_owned(),
            DAYS => "days".to_owned(),
            DATE_TRACKS => "date's tracks".to_owned(),
            METADATA => "metadata".to_owned(),
            DELIVERY_INFO => "delivery info".to_owned(),
            USER_INFO => "user info".to_owned(),
            USER_INFO_REPLY => "user info reply".to_owned(),
            ARTWORK => "artwork".to_owned(),
            WAVEFORM_PREVIEW => "waveform preview".to_owned(),
            TRACK_INFO => "track info".to_owned(),
            BEAT_GRID => "beat grid".to_owned(),
            SAVE_GRID_OFFSET => "save grid offset".to_owned(),
            GRID_OFFSET => "grid offset".to_owned(),
            CUES => "cues".to_owned(),
            WAVEFORM_DETAIL => "waveform detail".to_owned(),
            EXTENDED_CUES => "extended cues".to_owned(),
            ANLZ_TAG => "anlz tag (EXT)".to_owned(),
            ANLZ_TAG_2EX => "anlz tag (2EX)".to_owned(),
            RENDER => "render".to_owned(),
            ITEM_POSITION => "item position".to_owned(),
            TAG_LIST => "tag list".to_owned(),
            CHANGE_TAG => "change tag".to_owned(),
            CLEAR_TAGS => "clear tags".to_owned(),
            CHANGE_RATING => "change rating".to_owned(),
            FILTER_SWITCH => "filter switch".to_owned(),
            FILTER_GET => "filter properties".to_owned(),
            FILTER_SET => "set filter properties".to_owned(),
            SEARCH_TRACK => "search track".to_owned(),
            MENU_HEADER => "menu header".to_owned(),
            RENDER_HEADER => "render header".to_owned(),
            ARTWORK_REPLY => "artwork reply".to_owned(),
            ERROR => "error".to_owned(),
            MENU_ITEM => "menu item".to_owned(),
            MENU_FOOTER => "menu footer".to_owned(),
            WAVEFORM_PREVIEW_REPLY => "waveform preview reply".to_owned(),
            CUES_REPLY => "cues reply".to_owned(),
            BEAT_GRID_REPLY => "beat grid reply".to_owned(),
            WAVEFORM_DETAIL_REPLY => "waveform detail reply".to_owned(),
            EXTENDED_CUES_REPLY => "extended cues reply".to_owned(),
            ANLZ_TAG_REPLY => "anlz tag reply".to_owned(),
            other => format!("kind {other:#06x}"),
        }
    }
}

/// A requester device number a real database server will answer.
///
/// Servers answer 1 through 6 and silently ignore 7 and above — a client that
/// picks a high number sees the connection succeed and then nothing happen,
/// which is confusing enough to be worth rejecting explicitly.
pub fn is_answerable_device(device: u8) -> bool {
    (1..=6).contains(&device)
}

/// Builds the setup message a client sends first.
pub fn setup_request(device: u8) -> Message {
    Message::new(
        SETUP_TXID,
        kind::SETUP,
        vec![
            Argument::Number(u32::from(device)),
            Argument::Number(SETUP_MAGIC),
        ],
    )
}

/// The second setup argument, sent by both sides; meaning unknown, value
/// measured.
pub const SETUP_MAGIC: u32 = 0x14;

/// Builds the reply to a setup message, carrying our own device number.
///
/// The reply has the request's own kind, not a menu header (measured).
pub fn setup_reply(transaction: u32, our_device: u8) -> Message {
    Message::new(
        transaction,
        kind::SETUP,
        vec![
            Argument::Number(u32::from(our_device)),
            Argument::Number(SETUP_MAGIC),
        ],
    )
}

/// Builds the header that tells a client how many rows its query matched:
/// the request's own kind, then the count (measured).
pub fn menu_header(transaction: u32, request_kind: u32, item_count: u32) -> Message {
    Message::new(
        transaction,
        kind::MENU_HEADER,
        vec![Argument::Number(request_kind), Argument::Number(item_count)],
    )
}

/// Builds the footer that ends a menu.
pub fn menu_footer(transaction: u32) -> Message {
    Message::new(
        transaction,
        kind::MENU_FOOTER,
        vec![Argument::Number(0), Argument::Number(0)],
    )
}
