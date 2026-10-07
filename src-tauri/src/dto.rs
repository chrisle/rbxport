//! Wire types.
//!
//! Field names are camelCase to match `src/ipc/types.ts`. Rows are kept flat
//! and small — a page of 64 is roughly 15 KB of JSON, well inside the 64 KB
//! response cap.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RowDto {
    pub id: String,
    pub track_no: u32,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub genre: String,
    pub label: String,
    pub comment: String,
    pub bpm_x100: u32,
    pub key: String,
    pub duration_sec: u32,
    pub rating: u8,
    pub analysed: u8,
    pub date_added: String,
    pub release_date: String,
    /// The track's hot cues, for the badges on the row's preview waveform.
    ///
    /// A tuple per cue rather than an object: `["A",46,"#3CEB50"]` is 18
    /// bytes against 45 with field names, and rekordbox 7 allows sixteen a
    /// track, so a page of 64 rows stays inside the 64 KB response cap even
    /// when every row is full.
    pub hot_cues: Vec<RowCueDto>,
    /// Saved memory-cue positions in milliseconds, including memory-loop starts.
    pub memory_cues: Vec<u32>,
    /// Deterministic tint, drawn when a track has no artwork — a little under
    /// half the reference library.
    pub artwork_hue: u16,
    /// Whether `rbl://localhost/artwork/<id>` will serve anything for this track.
    pub has_artwork: bool,
    /// The file's own name, for the Explorer's File Name column.
    pub file_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeNodeDto {
    pub id: String,
    pub name: String,
    pub kind: &'static str,
    pub depth: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expanded: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub child_count: Option<u32>,
}

/// Library edit history after a reversible edit, undo, or redo.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditHistoryDto {
    pub generation: u32,
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_label: Option<String>,
    pub redo_label: Option<String>,
}

/// One output the audio could go to.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceDto {
    /// What to store and what to open by: stable across runs and reboots.
    pub id: String,
    /// What to show.
    pub name: String,
}

/// The outputs, and which of them is in use.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevicesDto {
    pub devices: Vec<AudioDeviceDto>,
    /// The system's own choice, so the interface can say which one that is.
    pub default: Option<String>,
    /// What this app has been told to use, or `None` for the system's.
    pub chosen: Option<String>,
}

/// The master limiter, as the interface sets it and reads it back.
///
/// Both directions: what a command is given and what `master_limiter`
/// returns, so a value the engine clamped comes home clamped.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LimiterDto {
    /// Input gain in dB, −24 to +24. Missing in older clients means unity.
    #[serde(default)]
    pub input_gain_db: f32,
    pub enabled: bool,
    /// dBFS, −12 to 0.
    pub ceiling_db: f32,
    /// Milliseconds, 10 to 1000.
    pub release_ms: f32,
}

/// Why the library did not load at startup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum LibraryProblemDto {
    /// No library here at all, and one can be made at `master_db`.
    Missing { master_db: String },
    /// There is a library, or something in its place, and it would not open.
    Failed { message: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySummaryDto {
    pub track_count: u32,
    pub playlist_count: u32,
    pub read_only: bool,
    pub db_version: Option<i64>,
    /// Milliseconds the library took to open and index; shown in diagnostics.
    pub load_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewHandleDto {
    pub view_id: u32,
    pub len: u32,
    pub gen: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum TrackSourceDto {
    #[serde(rename = "collection")]
    Collection,
    #[serde(rename = "playlist")]
    Playlist { id: String },
    #[serde(rename = "playlistFolder")]
    PlaylistFolder { id: String },
    #[serde(rename = "history")]
    History { id: String },
    /// A folder on disk, for the Explorer. Empty for the section heading,
    /// which lists nothing.
    #[serde(rename = "folder")]
    Folder { path: String },
    /// Related Tracks: the tracks that go with `track` under a criterion —
    /// `bpmKey`, `genreRecent` or `artist`. An empty track opens empty.
    #[serde(rename = "related")]
    Related { track: String, criterion: String },
    /// The Tag List.
    #[serde(rename = "tagList")]
    TagList,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewSpecDto {
    pub source: TrackSourceDto,
    pub sort: String,
    pub descending: bool,
    pub query: String,
    #[serde(default)]
    pub search_field: rbl_index::SearchField,
    /// The track filter bar's picks. Absent on the wire means no filter, so a
    /// caller that predates the bar keeps working.
    #[serde(default)]
    pub filter: TrackFilterDto,
}

/// A track whose audio file is no longer where the library says.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingTrackDto {
    pub id: String,
    pub title: String,
    pub artist: String,
    /// Where the library still expects it.
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingTracksDto {
    /// Every missing track, not just the ones listed.
    pub total: u32,
    pub tracks: Vec<MissingTrackDto>,
}

/// One copy in a group of duplicates.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateTrackDto {
    pub id: String,
    pub path: String,
    pub duration_sec: u32,
    /// Whether the file is where the library says.
    pub present: bool,
}

/// Tracks that share a title and artist.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroupDto {
    pub title: String,
    pub artist: String,
    pub tracks: Vec<DuplicateTrackDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicatesDto {
    /// Every group, not just the ones listed.
    pub groups: u32,
    /// Copies beyond the first, over every group.
    pub extra: u32,
    pub shown: Vec<DuplicateGroupDto>,
}

/// One cue point, as the interface needs it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CueDto {
    pub comment: String,
    /// `djmdCue.ID`, which `move_cue` and `delete_cue` take. Empty for a cue
    /// whose id is not a number under 2^32 — none in the reference library —
    /// which the interface shows but cannot edit.
    pub id: String,
    pub position_ms: u32,
    /// Where a loop ends, or 0 for a plain cue.
    pub out_ms: u32,
    /// `A` to `P` for a hot cue, empty for a memory cue.
    pub letter: String,
    pub memory: bool,
    /// What rekordbox paints for the cue's `ColorTableIndex`, as `#RRGGBB`,
    /// or `None` when no colour is selected (or an index is invalid).
    pub colour: Option<String>,
}

/// A hot cue on a track-list row: letter, position in ms, drawn colour.
///
/// Serialised as a JSON array, not an object — see [`RowDto::hot_cues`].
#[derive(Debug, Clone, Serialize)]
pub struct RowCueDto(pub char, pub u32, pub Option<String>);

/// `#RRGGBB` for a cue's `ColorTableIndex`, where it has been measured.
///
/// Index 0 is a cue with no colour (a NULL column reads as 0), not the black
/// sentinel at 0 in rekordbox's palette table: it gets `None`, so the pad and
/// badge fall back to the default hot-cue green.
pub fn cue_colour_css(index: u8) -> Option<String> {
    if index == 0 {
        return None;
    }
    rbl_anlz::cue_colour_drawn(index)
        .map(|[r, g, b]| format!("#{r:02X}{g:02X}{b:02X}"))
}

/// What an import batch did.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReportDto {
    pub imported: u32,
    /// One line per file that was not imported, saying why.
    pub skipped: Vec<String>,
    /// The tracks that landed, so they can be queued for analysis.
    pub tracks: Vec<ImportedTrackDto>,
    /// Files that were already in the library, with their existing track ids.
    /// Not counted as imported or skipped.
    pub existing: Vec<ImportedTrackDto>,
}

/// One track an import added.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedTrackDto {
    pub id: String,
    /// The file's name, for the analysis queue's readout.
    pub title: String,
}

/// A volume an export could be written to.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceDto {
    pub name: String,
    /// Where it is mounted; this is what an export is written to.
    pub path: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub file_system: String,
    pub removable: bool,
    /// Names the medium across a rename; `rbl_devices::volume_id`.
    pub volume_id: String,
    /// What is already on it, absent when it holds no export.
    pub export: Option<DeviceExportDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceExportDto {
    pub tracks: u32,
    pub playlists: u32,
    /// True when we wrote it, which is what makes the next export a sync.
    pub ours: bool,
    /// When our own export last ran; empty when this is not one of ours.
    pub written: String,
}

/// What an export wrote.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportReportDto {
    pub tracks: u32,
    pub playlists: u32,
    pub bytes_copied: u64,
    pub analysis_files: u32,
    /// Tracks already on the stick, unchanged, that did not need copying again.
    pub reused: u32,
    /// Tracks taken off the stick because the playlist no longer holds them.
    pub removed: u32,
    pub playlists_added: u32,
    pub playlists_removed: u32,
    /// Tracks left out because their audio was missing or unreadable.
    pub skipped: Vec<String>,
    /// Whether the export read back correctly with the independent parser.
    pub verified: bool,
}

/// What one destination got out of a sync: its report, or why it got none.
///
/// A stick that fails must not stop the others, so the outcome is per stick
/// rather than one error for the run.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncDeviceReportDto {
    /// The mount point it was written to.
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<ExportReportDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub ejected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eject_error: Option<String>,
}

/// A selected track whose source audio cannot be read before USB export.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingExportFileDto {
    pub title: String,
    pub path: String,
}

/// One playlist a stick was last synced with.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPlaylistDto {
    /// `djmdPlaylist.ID` in decimal: the tree's node id, so the window can
    /// tick the same rows again.
    pub library_id: String,
    pub name: String,
}

/// What a stick was last synced with, and what it holds now.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSyncStateDto {
    /// The playlists our last export was asked for; empty when the stick
    /// is not ours.
    pub selected: Vec<SyncPlaylistDto>,
    /// The playlist names in its `export.pdb`, folders left out; empty
    /// when it holds no export.
    pub on_device: Vec<String>,
    pub libraries: Vec<DeviceLibraryTreeDto>,
    /// The stick's sync record asks to be synced again when it is plugged
    /// in, and the record is this library's.
    pub automatic: bool,
}

/// One step of a sync, as the `sync:progress` event carries it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProgressDto {
    pub path: String,
    /// `writing`, then `done` or `failed`.
    pub state: &'static str,
}

/// What importing a rekordbox XML document did.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XmlImportReportDto {
    pub imported: u32,
    pub existing: u32,
    pub skipped: Vec<String>,
    pub playlists: u32,
    pub cues: u32,
    /// The tracks that landed, so they can be queued for analysis.
    pub tracks: Vec<ImportedTrackDto>,
}

/// An iTunes / Music library read for the Sync Manager's iTunes column: where
/// its XML is, and its playlist tree to tick from. Nothing is imported yet.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItunesLibraryDto {
    /// The file the tree was read from, to pass back with the chosen playlists.
    pub path: String,
    /// Folders and playlists only, flattened with a 1-based depth, ids
    /// `itunes:<index>` so the chosen ones can be named for a selective import.
    pub tree: Vec<TreeNodeDto>,
}

/// One explicit library backup, for Preferences › Backups.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupDto {
    pub created_at: u64,
    pub includes_analysis: bool,
    pub includes_artwork: bool,
    pub path: String,
    /// The file's name, which carries when it was taken.
    pub name: String,
    pub bytes: u64,
}

/// Per-device progress, including verified completion or failure.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportProgressDto {
    pub path: String,
    pub state: &'static str,
    pub done: u32,
    pub total: u32,
    pub title: String,
}

/// One phrase of the song structure, as the phrase strip needs it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhraseDto {
    /// The beat the phrase starts on, 1-based.
    pub beat: u32,
    /// What rekordbox draws: `INTRO 2`, `UP 3`, `VERSE 1`, and so on. Empty
    /// for a phrase kind no mood defines.
    pub label: String,
    /// The raw kind byte, which only means anything alongside the mood.
    pub kind: u16,
    /// Where that beat falls, from the `PQTZ` grid. `None` when the grid does
    /// not reach the phrase — a phrase strip can still be drawn by beat.
    pub time_ms: Option<u32>,
}

/// The track filter bar's BPM column: picked whole BPMs (empty is `All`),
/// the `MASTER PLAYER ± n%` pick, and the master player's BPM if a deck is
/// loaded.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BpmFilterDto {
    #[serde(default)]
    pub values: Vec<u32>,
    #[serde(default)]
    pub tolerance_pct: u8,
    #[serde(default)]
    pub master_bpm_x100: Option<u32>,
}

/// One entry per column of the track filter bar; `None` is an unticked column.
///
/// Keys and colours travel as names — what the bar shows — and are resolved
/// to ids against the library on the way in.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackFilterDto {
    #[serde(default)]
    pub bpm: Option<BpmFilterDto>,
    #[serde(default)]
    pub keys: Option<Vec<String>>,
    #[serde(default)]
    pub ratings: Option<Vec<u8>>,
    #[serde(default)]
    pub colors: Option<Vec<String>>,
}

/// A value the filter bar can offer, and how many tracks of the list carry it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CountedDto<T> {
    pub value: T,
    pub count: u32,
}

/// A My Tag category and the tags under it, by name.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagCategoryDto {
    pub name: String,
    pub tags: Vec<String>,
}

/// What the filter bar's lists hold for a source and query.
///
/// A few kilobytes: the reference library has 133 whole BPMs, 28 keys and 99
/// tags. The BPM list is capped in `rbl-index` so it cannot reach the cap.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterValuesDto {
    pub bpms: Vec<CountedDto<u32>>,
    pub keys: Vec<CountedDto<String>>,
    pub tags: Vec<TagCategoryDto>,
}

/// One of the folders the Explorer starts from.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplorerRootDto {
    /// What to show: `Music`, the user's name, `Macintosh HD`, a stick's name.
    pub name: String,
    pub path: String,
}

/// The folders directly under one folder.
///
/// Names only: the caller has the parent's path, and a thousand paths of a
/// hundred bytes each would be past the response cap where a thousand names
/// are not.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplorerChildrenDto {
    /// The first of them by name, up to the cap.
    pub names: Vec<String>,
    /// How many there were: more than `names` holds when the cap cut it.
    pub total: u32,
}

/// An intelligent playlist's rule as the editor shows it: one group of
/// conditions, all of them or any of them. rekordbox's own editor is flat
/// too; a rule with groups inside it (which the reader keeps) is not
/// offered for editing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartRuleDto {
    /// `all` or `any`.
    pub logic: String,
    pub conditions: Vec<SmartConditionDto>,
}

/// One line of a rule, in rekordbox's own vocabulary: the property's
/// internal name (`artist`, `name` for the title, `stockDate` for the date
/// added…) and the operator's number (1 equal … 11 ends with).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartConditionDto {
    pub property: String,
    pub operator: String,
    pub left: String,
    pub right: String,
    /// `day`, `week`, `month` or `year` for "in the last"; empty otherwise.
    pub unit: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceLibraryTreeDto {
    pub name: String,
    pub nodes: Vec<DevicePlaylistNodeDto>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicePlaylistNodeDto {
    pub id: String,
    pub parent_id: String,
    pub name: String,
    pub folder: bool,
}
