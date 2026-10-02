//! What a script reads, taken from the library and the preferences.
//!
//! No Objective-C here: `cocoa.rs` turns these values into the objects Cocoa
//! Scripting hands back, and everything in this file can be tested on any
//! platform against a library built in memory.

use std::path::PathBuf;

use rbl_index::{Library, Playlists, Row, NO_ID};
use serde_json::Value as Json;

use super::ScriptError;

/// A value on its way to a script.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptValue {
    /// `missing value`.
    Missing,
    Bool(bool),
    Int(i64),
    Real(f64),
    Text(String),
    List(Vec<ScriptValue>),
    /// One of the dictionary's enumerators, by its four-character code.
    Enum(u32),
    File(PathBuf),
    /// A track; inside a playlist when it was reached through one, so that
    /// deleting it takes it out of that playlist.
    Track { id: u64, playlist: Option<u64> },
    Playlist(u64),
}

/// A four-character code as the number Cocoa carries it in.
pub const fn four_cc(code: [u8; 4]) -> u32 {
    u32::from_be_bytes(code)
}

/// `track color`'s enumerators in `ColorID` order: none, then Pink to Purple.
///
/// `[REF]` the same order as `COLOR_NAMES` in `src/lib/trackFilter.ts`.
pub const COLOR_CODES: [u32; 9] = [
    four_cc(*b"RCno"),
    four_cc(*b"RCpk"),
    four_cc(*b"RCrd"),
    four_cc(*b"RCor"),
    four_cc(*b"RCyl"),
    four_cc(*b"RCgn"),
    four_cc(*b"RCaq"),
    four_cc(*b"RCbl"),
    four_cc(*b"RCpu"),
];

/// The `ColorID` a `track color` enumerator stands for: `0` for none.
pub fn color_id(code: u32) -> Option<u8> {
    COLOR_CODES.iter().position(|&c| c == code).and_then(|at| u8::try_from(at).ok())
}

/// `playlist kind`'s enumerators.
pub const KIND_PLAYLIST: u32 = four_cc(*b"RKpl");
pub const KIND_FOLDER: u32 = four_cc(*b"RKfd");
pub const KIND_SMART: u32 = four_cc(*b"RKsm");

/// A track property, by the Cocoa key the dictionary gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKey {
    Id,
    Name,
    Artist,
    Album,
    Genre,
    Label,
    Key,
    Bpm,
    Duration,
    Year,
    Rating,
    Color,
    Comment,
    PlayCount,
    DateAdded,
    Location,
    Analysed,
    BitRate,
    SampleRate,
}

impl TrackKey {
    pub fn parse(key: &str) -> Option<Self> {
        Some(match key {
            "uniqueID" => Self::Id,
            "name" => Self::Name,
            "rbxArtist" => Self::Artist,
            "rbxAlbum" => Self::Album,
            "rbxGenre" => Self::Genre,
            "rbxLabel" => Self::Label,
            "rbxKey" => Self::Key,
            "rbxBpm" => Self::Bpm,
            "rbxDuration" => Self::Duration,
            "rbxYear" => Self::Year,
            "rbxRating" => Self::Rating,
            "rbxColor" => Self::Color,
            "rbxComment" => Self::Comment,
            "rbxPlayCount" => Self::PlayCount,
            "rbxDateAdded" => Self::DateAdded,
            "rbxLocation" => Self::Location,
            "rbxAnalysed" => Self::Analysed,
            "rbxBitRate" => Self::BitRate,
            "rbxSampleRate" => Self::SampleRate,
            _ => return None,
        })
    }

    /// The information panel's field a `set` of this property writes, when
    /// it is one of those (`rbl_db::write::TrackField` names).
    pub fn field(self) -> Option<&'static str> {
        Some(match self {
            Self::Name => "title",
            Self::Artist => "artist",
            Self::Album => "album",
            Self::Genre => "genre",
            Self::Label => "label",
            Self::Key => "key",
            Self::Bpm => "bpm",
            Self::Year => "year",
            Self::PlayCount => "playCount",
            _ => return None,
        })
    }
}

/// One property of one track, or `None` when the track is not in the library.
pub fn track_value(library: &Library, id: u64, key: TrackKey) -> Option<ScriptValue> {
    let row = library.row_of_id(id)?;
    let index = row as usize;
    let text = |s: &str| ScriptValue::Text(s.to_owned());
    Some(match key {
        TrackKey::Id => ScriptValue::Text(id.to_string()),
        TrackKey::Name => text(library.title.get(index)),
        TrackKey::Artist => text(library.artist_name(row)),
        TrackKey::Album => text(library.album_name(row)),
        TrackKey::Genre => text(library.genre_name(row)),
        TrackKey::Label => text(library.label_name(row)),
        TrackKey::Key => text(library.key_name(row)),
        TrackKey::Bpm => ScriptValue::Real(f64::from(library.bpm_x100.get(index).copied().unwrap_or(0)) / 100.0),
        TrackKey::Duration => ScriptValue::Real(f64::from(library.length_sec.get(index).copied().unwrap_or(0))),
        TrackKey::Year => ScriptValue::Int(i64::from(library.year.get(index).copied().unwrap_or(0))),
        TrackKey::Rating => ScriptValue::Int(i64::from(library.rating.get(index).copied().unwrap_or(0))),
        TrackKey::Color => {
            let id = usize::from(library.color.get(index).copied().unwrap_or(0));
            ScriptValue::Enum(COLOR_CODES.get(id).copied().unwrap_or(COLOR_CODES[0]))
        }
        TrackKey::Comment => text(library.comment.get(index)),
        TrackKey::PlayCount => ScriptValue::Int(i64::from(library.play_count.get(index).copied().unwrap_or(0))),
        TrackKey::DateAdded => text(library.date_added.get(index)),
        TrackKey::Location => {
            let display = id.to_string();
            library.audio_path_of(&display).map_or(ScriptValue::Missing, |path| ScriptValue::File(PathBuf::from(path)))
        }
        TrackKey::Analysed => ScriptValue::Bool(library.analysed.get(index).copied().unwrap_or(0) != 0),
        TrackKey::BitRate => ScriptValue::Int(i64::from(library.bitrate.get(index).copied().unwrap_or(0))),
        TrackKey::SampleRate => ScriptValue::Int(i64::from(library.sample_rate.get(index).copied().unwrap_or(0))),
    })
}

/// What a `set` of a track property sends to the backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackEdit {
    Field(&'static str, String),
    Rating(u8),
    Comment(String),
    /// `ColorID` as text: `"0"` for none, which is what 38,671 of the
    /// reference library's tracks carry, not NULL.
    Color(String),
}

/// Checks a value a script is setting and says what to write.
pub fn track_edit(key: TrackKey, value: &ScriptValue) -> Result<TrackEdit, ScriptError> {
    let not_settable = || ScriptError::not_modifiable(format!("The {} of a track cannot be set.", key_name(key)));
    match (key, value) {
        (TrackKey::Rating, ScriptValue::Int(stars)) => u8::try_from(*stars)
            .ok()
            .filter(|s| *s <= 5)
            .map(TrackEdit::Rating)
            .ok_or_else(|| ScriptError::wrong_type("A rating is 0 to 5 stars.")),
        (TrackKey::Rating, _) => Err(ScriptError::wrong_type("A rating is 0 to 5 stars.")),
        (TrackKey::Comment, ScriptValue::Text(text)) => Ok(TrackEdit::Comment(text.clone())),
        (TrackKey::Color, ScriptValue::Enum(code)) => color_id(*code)
            .map(|id| TrackEdit::Color(id.to_string()))
            .ok_or_else(|| ScriptError::wrong_type("That is not a track color.")),
        (TrackKey::Color, _) => Err(ScriptError::wrong_type("That is not a track color.")),
        (TrackKey::Bpm, ScriptValue::Real(bpm)) if bpm.is_finite() && *bpm > 0.0 => {
            Ok(TrackEdit::Field("bpm", format!("{bpm:.2}")))
        }
        (TrackKey::Bpm, ScriptValue::Int(bpm)) if *bpm > 0 => Ok(TrackEdit::Field("bpm", format!("{bpm}.00"))),
        (TrackKey::Bpm, _) => Err(ScriptError::wrong_type("A BPM is a number above 0.")),
        (TrackKey::Year | TrackKey::PlayCount, ScriptValue::Int(n)) if *n >= 0 => {
            Ok(TrackEdit::Field(key.field().ok_or_else(not_settable)?, n.to_string()))
        }
        (TrackKey::Year | TrackKey::PlayCount, _) => Err(ScriptError::wrong_type("That has to be a whole number, 0 or more.")),
        (_, ScriptValue::Text(text)) => Ok(TrackEdit::Field(key.field().ok_or_else(not_settable)?, text.clone())),
        _ => Err(if key.field().is_some() { ScriptError::wrong_type("That has to be text.") } else { not_settable() }),
    }
}

fn key_name(key: TrackKey) -> &'static str {
    match key {
        TrackKey::Id => "id",
        TrackKey::Duration => "duration",
        TrackKey::DateAdded => "date added",
        TrackKey::Location => "location",
        TrackKey::Analysed => "analysed flag",
        TrackKey::BitRate => "bit rate",
        TrackKey::SampleRate => "sample rate",
        _ => "value",
    }
}

/// Every track id, in the collection's own order.
pub fn all_tracks(library: &Library) -> Vec<u64> {
    library.ids.clone()
}

/// A playlist as a script sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistInfo {
    pub id: u64,
    pub name: String,
    pub kind: u32,
    /// The folder it is in, `None` at the top.
    pub parent: Option<u64>,
}

fn info(playlists: &Playlists, index: usize) -> PlaylistInfo {
    let parent = playlists.parent.get(index).copied().unwrap_or(NO_ID);
    PlaylistInfo {
        id: playlists.ids.get(index).copied().unwrap_or(0),
        name: playlists.name(index).to_owned(),
        kind: if playlists.is_folder(index) {
            KIND_FOLDER
        } else if playlists.is_smart(index) {
            KIND_SMART
        } else {
            KIND_PLAYLIST
        },
        parent: (parent != NO_ID).then(|| playlists.ids.get(parent as usize).copied()).flatten(),
    }
}

pub fn playlist(library: &Library, id: u64) -> Option<PlaylistInfo> {
    let playlists = library.playlists();
    playlists.index_of(id).map(|index| info(&playlists, index))
}

/// The indices under `parent` (`NO_ID` for the top), in the tree's order.
fn children_of(playlists: &Playlists, parent: u32) -> Vec<usize> {
    let mut children: Vec<usize> = (0..playlists.len())
        .filter(|&i| playlists.parent.get(i).copied() == Some(parent))
        .collect();
    children.sort_by_key(|&i| playlists.seq.get(i).copied().unwrap_or(u32::MAX));
    children
}

/// A folder's playlists one level down, or the top level's for `None`.
pub fn children(library: &Library, parent: Option<u64>) -> Vec<u64> {
    let playlists = library.playlists();
    let parent = match parent {
        None => NO_ID,
        Some(id) => match playlists.index_of(id).and_then(|i| u32::try_from(i).ok()) {
            Some(index) => index,
            None => return Vec::new(),
        },
    };
    children_of(&playlists, parent).into_iter().filter_map(|i| playlists.ids.get(i).copied()).collect()
}

/// Every playlist and folder at any depth, in the order the tree shows them:
/// each folder followed by what is in it.
pub fn all_playlists(library: &Library) -> Vec<u64> {
    let playlists = library.playlists();
    let mut out = Vec::with_capacity(playlists.len());
    let mut stack: Vec<usize> = children_of(&playlists, NO_ID).into_iter().rev().collect();
    while let Some(index) = stack.pop() {
        if let Some(&id) = playlists.ids.get(index) {
            out.push(id);
        }
        if let Ok(as_parent) = u32::try_from(index) {
            stack.extend(children_of(&playlists, as_parent).into_iter().rev());
        }
    }
    out
}

/// A playlist's tracks in its order; an intelligent playlist's are what its
/// rule admits now. A folder has none.
pub fn playlist_tracks(library: &Library, id: u64) -> Vec<u64> {
    let playlists = library.playlists();
    let Some(index) = playlists.index_of(id) else { return Vec::new() };
    let rows: Vec<Row> = if playlists.is_smart(index) {
        let rule = playlists.smart_rule(index);
        drop(playlists);
        rule.map(|rule| rule.evaluate(library)).unwrap_or_default()
    } else {
        playlists.members.get(index).cloned().unwrap_or_default()
    };
    rows.iter().filter_map(|&row| library.ids.get(row as usize).copied()).collect()
}

/// The preferences a script can read and set, flattened to
/// `pane.field` names in the order the window stores them.
///
/// A choice made of records — the DJ System's category and sort rows, the
/// keyboard's changed keys — has an editor of its own and is left out: a
/// script setting one wholesale is more likely to break it than mean it.
pub fn settings(preferences: &Json) -> Vec<(String, Json)> {
    let mut out = Vec::new();
    let Some(panes) = preferences.as_object() else { return out };
    for (pane, fields) in panes {
        if pane == "keyboard" {
            continue;
        }
        let Some(fields) = fields.as_object() else { continue };
        for (field, value) in fields {
            // Null until they are edited, and records after: left out either
            // way, rather than listed only while they are empty.
            if pane == "djSystem" && (field == "categories" || field == "sorts") {
                continue;
            }
            if is_scalar(value) || value.as_array().is_some_and(|items| items.iter().all(is_scalar)) {
                out.push((format!("{pane}.{field}"), value.clone()));
            }
        }
    }
    out
}

fn is_scalar(value: &Json) -> bool {
    !(value.is_object() || value.is_array())
}

/// A preference's value as a script sees it.
pub fn setting_value(value: &Json) -> ScriptValue {
    match value {
        Json::Null | Json::Object(_) => ScriptValue::Missing,
        Json::Bool(b) => ScriptValue::Bool(*b),
        Json::Number(n) => n.as_i64().map_or_else(|| ScriptValue::Real(n.as_f64().unwrap_or(0.0)), ScriptValue::Int),
        Json::String(s) => ScriptValue::Text(s.clone()),
        Json::Array(items) => ScriptValue::List(items.iter().map(setting_value).collect()),
    }
}

/// What a script is setting a preference to, as the window stores it.
pub fn setting_json(value: &ScriptValue) -> Result<Json, ScriptError> {
    Ok(match value {
        ScriptValue::Missing => Json::Null,
        ScriptValue::Bool(b) => Json::Bool(*b),
        ScriptValue::Int(n) => Json::from(*n),
        ScriptValue::Real(n) => serde_json::Number::from_f64(*n)
            .map(Json::Number)
            .ok_or_else(|| ScriptError::wrong_type("That number cannot be stored."))?,
        ScriptValue::Text(s) => Json::String(s.clone()),
        ScriptValue::List(items) => Json::Array(items.iter().map(setting_json).collect::<Result<_, _>>()?),
        _ => return Err(ScriptError::wrong_type("A setting takes a boolean, a number, text or a list of text.")),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use rbl_index::testing::{library_from, TestTrack};
    use serde_json::json;

    #[test]
    fn a_track_reads_back_as_the_library_holds_it() {
        let library = library_from(&[TestTrack { id: 7, title: "Take Me Home", bpm_x100: 12_850, ..TestTrack::default() }]);
        assert_eq!(track_value(&library, 7, TrackKey::Name), Some(ScriptValue::Text("Take Me Home".into())));
        assert_eq!(track_value(&library, 7, TrackKey::Bpm), Some(ScriptValue::Real(128.5)));
        assert_eq!(track_value(&library, 7, TrackKey::Id), Some(ScriptValue::Text("7".into())));
        assert_eq!(track_value(&library, 7, TrackKey::Color), Some(ScriptValue::Enum(COLOR_CODES[0])));
        assert_eq!(track_value(&library, 8, TrackKey::Name), None);
    }

    #[test]
    fn every_track_property_key_in_the_dictionary_parses() {
        let sdef = include_str!("../../rbxport.sdef");
        let track = sdef.split("<class name=\"track\"").nth(1).unwrap().split("</class>").next().unwrap();
        for key in track.split("<cocoa key=\"").skip(1).map(|rest| rest.split('"').next().unwrap()) {
            assert!(TrackKey::parse(key).is_some(), "{key} is in the dictionary but not read");
        }
    }

    #[test]
    fn playlists_come_in_tree_order_with_their_folders() {
        use rbl_index::testing::{add_folder, add_playlist};
        let mut library = library_from(&[
            TestTrack { id: 7, title: "One", ..TestTrack::default() },
            TestTrack { id: 9, title: "Two", ..TestTrack::default() },
        ]);
        let folder = add_folder(&mut library, "Sets");
        let top = add_playlist(&mut library, "Friday", &[1, 0]);
        let inside = add_playlist(&mut library, "Warm-up", &[0]);
        // Put "Warm-up" inside "Sets", ahead of where it was added.
        let mut playlists = (*library.playlists()).clone();
        playlists.parent[inside] = u32::try_from(folder).unwrap();
        playlists.seq[inside] = 0;
        library.set_playlists(playlists);
        let (folder, top, inside) = (1000 + folder as u64, 1000 + top as u64, 1000 + inside as u64);

        assert_eq!(all_playlists(&library), vec![folder, inside, top]);
        assert_eq!(children(&library, None), vec![folder, top]);
        assert_eq!(children(&library, Some(folder)), vec![inside]);
        assert_eq!(playlist(&library, inside).unwrap().parent, Some(folder));
        assert_eq!(playlist(&library, folder).unwrap().kind, KIND_FOLDER);
        assert_eq!(playlist(&library, top).unwrap().kind, KIND_PLAYLIST);
        // In the playlist's own order, not the collection's.
        assert_eq!(playlist_tracks(&library, top), vec![9, 7]);
        assert_eq!(playlist_tracks(&library, folder), [] as [u64; 0]);
    }

    #[test]
    fn colours_map_both_ways() {
        assert_eq!(color_id(COLOR_CODES[0]), Some(0));
        assert_eq!(color_id(four_cc(*b"RCpu")), Some(8));
        assert_eq!(color_id(four_cc(*b"nope")), None);
    }

    #[test]
    fn a_set_is_checked_before_anything_is_written() {
        assert_eq!(track_edit(TrackKey::Rating, &ScriptValue::Int(4)).unwrap(), TrackEdit::Rating(4));
        assert!(track_edit(TrackKey::Rating, &ScriptValue::Int(6)).is_err());
        assert_eq!(
            track_edit(TrackKey::Color, &ScriptValue::Enum(four_cc(*b"RCrd"))).unwrap(),
            TrackEdit::Color("2".into())
        );
        assert_eq!(
            track_edit(TrackKey::Color, &ScriptValue::Enum(COLOR_CODES[0])).unwrap(),
            TrackEdit::Color("0".into())
        );
        assert_eq!(
            track_edit(TrackKey::Name, &ScriptValue::Text("New".into())).unwrap(),
            TrackEdit::Field("title", "New".into())
        );
        assert_eq!(track_edit(TrackKey::Bpm, &ScriptValue::Real(128.0)).unwrap(), TrackEdit::Field("bpm", "128.00".into()));
        assert_eq!(track_edit(TrackKey::Year, &ScriptValue::Int(1999)).unwrap(), TrackEdit::Field("year", "1999".into()));
        assert_eq!(track_edit(TrackKey::Duration, &ScriptValue::Real(1.0)).unwrap_err().code, ScriptError::NOT_MODIFIABLE);
        assert_eq!(track_edit(TrackKey::Location, &ScriptValue::Text("/x".into())).unwrap_err().code, ScriptError::NOT_MODIFIABLE);
    }

    #[test]
    fn settings_are_the_scalar_choices_by_pane_and_field() {
        let preferences = json!({
            "view": { "keyDisplay": "classic", "tooltips": false, "shortcuts": ["12"] },
            "audio": { "bufferSize": 512 },
            "djSystem": { "categories": [{ "id": 1 }], "sorts": null, "linkInterface": null },
            "keyboard": { "overrides": {} },
        });
        let names: Vec<String> = settings(&preferences).into_iter().map(|(name, _)| name).collect();
        assert!(names.contains(&"view.keyDisplay".to_owned()));
        assert!(names.contains(&"view.shortcuts".to_owned()));
        assert!(names.contains(&"audio.bufferSize".to_owned()));
        assert!(names.contains(&"djSystem.linkInterface".to_owned()));
        assert!(!names.iter().any(|n| n.starts_with("keyboard.") || n == "djSystem.categories" || n == "djSystem.sorts"));
    }

    #[test]
    fn a_setting_round_trips_through_a_script_value() {
        for value in [json!(true), json!(512), json!(0.5), json!("classic"), json!(["a", "b"]), json!(null)] {
            assert_eq!(setting_json(&setting_value(&value)).unwrap(), value);
        }
    }
}
