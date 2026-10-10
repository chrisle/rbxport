//! What an export needs from the library beyond the index's columns.
//!
//! Read straight from the database for the tracks being exported rather than
//! kept in memory for every track: My Tag memberships and the alternative
//! paths of a cloud-synced file are looked at once per export, and the index
//! would carry them for 38,681 rows to answer for 61.

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension};

use crate::Result;

/// One row of `djmdMyTag`: a category (`attribute` 1, parent `root`) or a
/// tag under one (`attribute` 0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MyTagRow {
    pub id: String,
    pub seq: i64,
    pub name: String,
    pub attribute: i64,
    /// The category's id, or `root` for a category.
    pub parent: String,
}

/// Whether a table exists, so a library without My Tags still exports.
fn has_table(conn: &Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table],
        |r| r.get::<_, i64>(0),
    )
    .is_ok_and(|n| n > 0)
}

/// Every live My Tag, categories first in their order, then the tags in
/// theirs.
pub fn my_tags(conn: &Connection) -> Result<Vec<MyTagRow>> {
    if !has_table(conn, "djmdMyTag") {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT ID, Seq, Name, Attribute, ParentID FROM djmdMyTag
         WHERE rb_local_deleted = 0 ORDER BY Attribute DESC, Seq, ID",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(MyTagRow {
            id: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
            seq: r.get::<_, Option<i64>>(1)?.unwrap_or(0),
            name: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
            attribute: r.get::<_, Option<i64>>(3)?.unwrap_or(0),
            parent: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
        })
    })?;
    Ok(rows
        .filter_map(std::result::Result::ok)
        .filter(|t| !t.id.is_empty())
        .collect())
}

/// `djmdProperty.DBID`, the library's own id, which a stick's sync record
/// names so rekordbox knows which library synced it. 0 when the row is
/// missing or not a number.
pub fn db_id(conn: &Connection) -> Result<u64> {
    if !has_table(conn, "djmdProperty") {
        return Ok(0);
    }
    let id: Option<String> = conn
        .query_row("SELECT DBID FROM djmdProperty LIMIT 1", [], |r| r.get(0))
        .optional()?
        .flatten();
    Ok(id.and_then(|id| id.parse().ok()).unwrap_or(0))
}

/// What one exported track needs that the index does not hold.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackExtras {
    pub metadata: rbl_core::ExportMetadata,
    pub cues: Vec<rbl_anlz::cues::ExportCue>,
    /// Other places the file may be, after `FolderPath`: the local copy of a
    /// cloud-synced track (`rb_LocalFolderPath`) and where it was imported
    /// from (`OrgFolderPath`). Empty entries are left out.
    pub alternate_paths: Vec<String>,
    /// The ids of the My Tags on the track.
    pub my_tags: Vec<String>,
    /// What analysis last wrote to the track's row, read from the database
    /// as it is now; `None` when the row was not found.
    pub analysis: Option<AnalysisFacts>,
}

/// The columns an analysis writes to a track's row (`AnalysisDataPath`,
/// `BPM`, `Length`, the key, and the artwork it imports).
///
/// The export reads them from the database rather than from the index: an
/// analysis run writes them one track at a time and the index is re-read
/// only when the whole run is over, so a sync made while a run was going
/// took the tracks it had already analysed as unanalysed, with no BPM, key,
/// waveform or grid on the stick (#309).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnalysisFacts {
    /// Share-relative, `/PIONEER/USBANLZ/…/ANLZ0000.DAT`; empty when none.
    pub analysis_path: String,
    pub bpm_x100: u32,
    pub length_sec: u32,
    /// The key's `ScaleName`; empty when the track has none.
    pub key: String,
    /// Share-relative artwork; empty when none.
    pub artwork_path: String,
}

/// A whole number from a column that may hold an integer, a real or text,
/// as rekordbox's columns sometimes do; 0 when it holds none.
fn whole(value: rusqlite::types::ValueRef<'_>) -> i64 {
    use rusqlite::types::ValueRef;
    match value {
        ValueRef::Integer(v) => v,
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a column value, clamped by the caller"
        )]
        ValueRef::Real(v) => v as i64,
        ValueRef::Text(t) => std::str::from_utf8(t)
            .ok()
            .and_then(|t| t.trim().parse().ok())
            .unwrap_or(0),
        ValueRef::Null | ValueRef::Blob(_) => 0,
    }
}

/// The extras for a set of tracks, keyed by `djmdContent.ID`.
///
/// One query per thousand ids, so a whole-library export stays a handful
/// of statements rather than one per track.
pub fn track_extras(conn: &Connection, ids: &[String]) -> Result<HashMap<String, TrackExtras>> {
    let mut out: HashMap<String, TrackExtras> = HashMap::with_capacity(ids.len());
    let tagged = has_table(conn, "djmdSongMyTag");
    for chunk in ids.chunks(900) {
        let marks = vec!["?"; chunk.len()].join(",");
        let params = rusqlite::params_from_iter(chunk.iter());
        let mut stmt = conn.prepare(&format!(
            "SELECT ID, rb_LocalFolderPath, OrgFolderPath, TrackNo, DiscNo, BitDepth, DJPlayCount, Analysed, HotCueAutoLoad, DateCreated, ISRC FROM djmdContent WHERE ID IN ({marks})"
        ))?;
        let rows = stmt.query_map(params, |r| {
            Ok((
                r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                rbl_core::ExportMetadata {
                    track_number: r.get::<_, Option<u32>>(3)?.unwrap_or(0),
                    disc_number: r.get::<_, Option<u16>>(4)?.unwrap_or(0),
                    bit_depth: r.get::<_, Option<u16>>(5)?.unwrap_or(0),
                    play_count: r.get::<_, Option<u32>>(6)?.unwrap_or(0),
                    analysed: r.get::<_, Option<u32>>(7)?.unwrap_or(0),
                    hot_cue_auto_load: r.get::<_, Option<String>>(8)?.is_some_and(|v| v == "on"),
                    date_created: r.get::<_, Option<String>>(9)?.unwrap_or_default(),
                    isrc: r.get::<_, Option<String>>(10)?.unwrap_or_default(),
                },
            ))
        })?;
        for row in rows {
            let (id, local, org, metadata) = row?;
            let extras = out.entry(id).or_default();
            extras.metadata = metadata;
            for path in [local, org] {
                if !path.is_empty() && !extras.alternate_paths.contains(&path) {
                    extras.alternate_paths.push(path);
                }
            }
        }
        let keyed = has_table(conn, "djmdKey");
        let mut stmt = conn.prepare(&format!(
            "SELECT c.ID, c.AnalysisDataPath, c.BPM, c.Length, {key}, c.ImagePath
             FROM djmdContent c {join} WHERE c.ID IN ({marks})",
            key = if keyed { "k.ScaleName" } else { "NULL" },
            join = if keyed {
                "LEFT JOIN djmdKey k ON k.ID = c.KeyID"
            } else {
                ""
            },
        ))?;
        let rows = stmt.query_map(rusqlite::params_from_iter(chunk.iter()), |r| {
            let count = |idx| {
                Ok::<_, rusqlite::Error>(
                    u32::try_from(whole(r.get_ref(idx)?).clamp(0, i64::from(u32::MAX)))
                        .unwrap_or(0),
                )
            };
            Ok((
                r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                AnalysisFacts {
                    analysis_path: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    bpm_x100: count(2)?,
                    length_sec: count(3)?,
                    key: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    artwork_path: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
                },
            ))
        })?;
        for row in rows {
            let (id, facts) = row?;
            out.entry(id).or_default().analysis = Some(facts);
        }
        if has_table(conn, "djmdCue") {
            let mut stmt = conn.prepare(&format!(
                "SELECT q.ContentID, q.Kind, q.InMsec, q.OutMsec, q.Color, q.ColorTableIndex,
                        q.Comment, q.ActiveLoop, q.BeatLoopSize, c.BPM
                 FROM djmdCue q JOIN djmdContent c ON c.ID = q.ContentID
                 WHERE q.rb_local_deleted=0 AND q.Kind IN (0,1,2,3,5,6,7,8,9,10,11,12,13,14,15,16,17)
                 AND q.ContentID IN ({marks}) ORDER BY q.rowid DESC"
            ))?;
            let rows = stmt.query_map(rusqlite::params_from_iter(chunk.iter()), |r| {
                let kind = r.get::<_, Option<u8>>(1)?.unwrap_or(0);
                let color = r.get::<_, Option<i64>>(4)?.unwrap_or(255);
                let end = r.get::<_, Option<i64>>(3)?.unwrap_or(-1);
                let beats = r.get::<_, Option<u32>>(8)?.unwrap_or(0);
                let time_ms = r.get::<_, Option<u32>>(2)?.unwrap_or(0);
                let bpm_x100 = r.get::<_, Option<u32>>(9)?.unwrap_or(0);
                Ok((
                    r.get::<_, String>(0)?,
                    rbl_anlz::cues::ExportCue {
                        kind,
                        time_ms,
                        loop_time_ms: loop_end(time_ms, end, beats, bpm_x100),
                        color_id: if kind == 0 {
                            u8::try_from(color).ok().filter(|c| *c != 255).unwrap_or(0)
                        } else {
                            0
                        },
                        color_code: r.get::<_, Option<u8>>(5)?.unwrap_or(0),
                        comment: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
                        active_loop: r.get::<_, Option<i64>>(7)?.unwrap_or(0) != 0,
                        loop_numerator: u16::try_from(beats >> 16).unwrap_or(0),
                        loop_denominator: u16::try_from(beats & 0xffff).unwrap_or(0),
                    },
                ))
            })?;
            for row in rows {
                let (id, cue) = row?;
                out.entry(id).or_default().cues.push(cue);
            }
        }
        if tagged {
            let params = rusqlite::params_from_iter(chunk.iter());
            let mut stmt = conn.prepare(&format!(
                "SELECT ContentID, MyTagID FROM djmdSongMyTag
                 WHERE rb_local_deleted = 0 AND ContentID IN ({marks}) ORDER BY ContentID, MyTagID"
            ))?;
            let rows = stmt.query_map(params, |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                ))
            })?;
            for (content, tag) in rows.filter_map(std::result::Result::ok) {
                if !tag.is_empty() {
                    out.entry(content).or_default().my_tags.push(tag);
                }
            }
        }
    }
    Ok(out)
}

/// Returns a cue's explicit end, or derives one from rekordbox's beat-loop
/// fraction and track tempo when the end is absent.
///
/// `[OBS]` Issue #45 shows active loops becoming plain exported cues with no
/// end. `BeatLoopSize` is `(numerator << 16) | denominator`; measured loops'
/// `OutMsec - InMsec` spans that fraction of a beat at `djmdContent.BPM`, so
/// those stored fields supply the missing end without inventing a loop length.
fn loop_end(start_ms: u32, stored_end: i64, beat_loop_size: u32, bpm_x100: u32) -> Option<u32> {
    if let Ok(end) = u32::try_from(stored_end) {
        if end > start_ms {
            return Some(end);
        }
    }
    let numerator = u64::from(beat_loop_size >> 16);
    let denominator = u64::from(beat_loop_size & 0xffff);
    if numerator == 0 || denominator == 0 || bpm_x100 == 0 {
        return None;
    }
    // BPM is stored times 100, hence 6,000,000 milliseconds per beat.
    let divisor = denominator.saturating_mul(u64::from(bpm_x100));
    let duration_ms = numerator
        .saturating_mul(6_000_000)
        .saturating_add(divisor / 2)
        / divisor;
    u32::try_from(u64::from(start_ms).saturating_add(duration_ms)).ok()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use rusqlite::Connection;

    use super::{track_extras, AnalysisFacts};

    /// The export takes what analysis wrote from the row as it is now, not
    /// from an index read before the analysis (#309).
    #[test]
    fn the_extras_carry_what_analysis_wrote_to_the_row() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE djmdContent (
                ID TEXT PRIMARY KEY, rb_LocalFolderPath TEXT, OrgFolderPath TEXT,
                TrackNo INTEGER, DiscNo INTEGER, BitDepth INTEGER, DJPlayCount INTEGER,
                Analysed INTEGER, HotCueAutoLoad TEXT, DateCreated TEXT, ISRC TEXT, BPM,
                AnalysisDataPath TEXT, Length INTEGER, KeyID TEXT, ImagePath TEXT
             );
             CREATE TABLE djmdKey (ID TEXT PRIMARY KEY, ScaleName TEXT);
             INSERT INTO djmdKey VALUES ('7', 'F#m');
             INSERT INTO djmdContent (ID, BPM, AnalysisDataPath, Length, KeyID, ImagePath, Analysed)
                VALUES ('1', 12800, '/PIONEER/USBANLZ/P001/0001/ANLZ0000.DAT', 301, '7', '/PIONEER/Artwork/a.jpg', 105),
                       ('2', NULL, NULL, NULL, NULL, NULL, 0),
                       ('3', '12450', '', 200, 'gone', '', 0);",
        )
        .unwrap();

        let extras = track_extras(
            &conn,
            &[
                "1".to_owned(),
                "2".to_owned(),
                "3".to_owned(),
                "4".to_owned(),
            ],
        )
        .unwrap();
        assert_eq!(
            extras["1"].analysis,
            Some(AnalysisFacts {
                analysis_path: "/PIONEER/USBANLZ/P001/0001/ANLZ0000.DAT".into(),
                bpm_x100: 12_800,
                length_sec: 301,
                key: "F#m".into(),
                artwork_path: "/PIONEER/Artwork/a.jpg".into(),
            })
        );
        assert_eq!(
            extras["2"].analysis,
            Some(AnalysisFacts::default()),
            "a row never analysed reads as such"
        );
        let text_bpm = extras["3"].analysis.as_ref().unwrap();
        assert_eq!(
            (text_bpm.bpm_x100, text_bpm.key.as_str()),
            (12_450, ""),
            "a BPM stored as text, a key row that is gone"
        );
        assert!(!extras.contains_key("4"), "no row, no facts");
    }

    #[test]
    fn an_active_beat_loop_without_an_out_point_is_exported_as_a_loop() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE djmdContent (
                ID TEXT PRIMARY KEY, rb_LocalFolderPath TEXT, OrgFolderPath TEXT,
                TrackNo INTEGER, DiscNo INTEGER, BitDepth INTEGER, DJPlayCount INTEGER,
                Analysed INTEGER, HotCueAutoLoad TEXT, DateCreated TEXT, ISRC TEXT, BPM INTEGER,
                AnalysisDataPath TEXT, Length INTEGER, KeyID TEXT, ImagePath TEXT
             );
             CREATE TABLE djmdCue (
                ContentID TEXT, Kind INTEGER, InMsec INTEGER, OutMsec INTEGER,
                Color INTEGER, ColorTableIndex INTEGER, Comment TEXT, ActiveLoop INTEGER,
                BeatLoopSize INTEGER, rb_local_deleted INTEGER
             );
             INSERT INTO djmdContent (ID, BPM) VALUES ('42', 12800);
             INSERT INTO djmdCue VALUES
                ('42', 0, 27851, -1, 255, 0, 'CUE(Auto)', 1, 262145, 0),
                ('42', 1, 40000, 42000, -1, 21, '', 0, 0, 0),
                ('42', 2, 50000, -1, -1, 21, '', 0, 0, 0);",
        )
        .unwrap();

        let extras = track_extras(&conn, &["42".to_owned()]).unwrap();
        let cues = &extras.get("42").unwrap().cues;
        assert_eq!(cues.len(), 3);
        let active = cues.iter().find(|cue| cue.active_loop).unwrap();
        assert_eq!(active.time_ms, 27_851);
        assert_eq!(active.loop_time_ms, Some(29_726));
        assert_eq!((active.loop_numerator, active.loop_denominator), (4, 1));
        let sections = rbl_anlz::cues::sections(std::slice::from_ref(active), true);
        let exported = sections[3].as_cue_entries().unwrap();
        assert_eq!(exported.len(), 1);
        assert_eq!(exported[0].kind, 2, "PCP2 loop, not a plain CUE(Auto)");
        assert_eq!(exported[0].loop_time_ms, 29_726);
        assert_eq!(
            cues.iter().find(|cue| cue.kind == 1).unwrap().loop_time_ms,
            Some(42_000)
        );
        assert_eq!(
            cues.iter().find(|cue| cue.kind == 2).unwrap().loop_time_ms,
            None
        );
    }
}
