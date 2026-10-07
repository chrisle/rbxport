//! Intelligent playlists on a My Tag, read out of a real-schema library.
//!
//! rekordbox writes a `myTag` condition with the tag's `djmdMyTag.ID` as its
//! value and answers it from the track's `djmdSongMyTag` rows (issue #84:
//! such playlists opened empty because the index read no memberships and
//! treated the property as unknown). Everything is written to a temporary
//! fixture library, never to an installed one.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use rbl_db::fixture::{self, track_id, Shape, MY_TAG_PEAK, MY_TAG_WARM_UP};
use rbl_db::write::Writer;
use rbl_db::{Library as Db, OpenMode};
use rbl_index::cache::{decode, encode, Fingerprint};
use rbl_index::TrackSource;

struct Fixture {
    _dir: tempfile::TempDir,
    library: rbl_index::Library,
    peak: String,
    not_warm_up: String,
    peak_equal: String,
}

fn rule(id: &str, operator: u8, tag: &str) -> String {
    format!(
        r#"<NODE Id="{id}" LogicalOperator="1" AutomaticUpdate="1"><CONDITION PropertyName="myTag" Operator="{operator}" ValueUnit="" ValueLeft="{tag}" ValueRight=""/></NODE>"#
    )
}

fn open() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let location = fixture::build(dir.path(), Shape::default()).expect("build the fixture");
    let mut writer = Writer::open(location.clone(), dir.path().join("backups")).expect("open for writing");
    writer.set_my_tags(&track_id(1), &[MY_TAG_PEAK.to_owned()]).unwrap();
    writer.set_my_tags(&track_id(2), &[MY_TAG_PEAK.to_owned(), MY_TAG_WARM_UP.to_owned()]).unwrap();
    writer.set_my_tags(&track_id(3), &[MY_TAG_WARM_UP.to_owned()]).unwrap();
    // A membership removed again must not count.
    writer.set_my_tags(&track_id(4), &[MY_TAG_PEAK.to_owned()]).unwrap();
    writer.set_my_tags(&track_id(4), &[]).unwrap();
    let peak = writer.create_smart_playlist("Peak", "root", |id| rule(id, 8, MY_TAG_PEAK)).unwrap();
    let not_warm_up = writer
        .create_smart_playlist("Not warm-up", "root", |id| rule(id, 9, MY_TAG_WARM_UP))
        .unwrap();
    let peak_equal = writer.create_smart_playlist("Peak =", "root", |id| rule(id, 1, MY_TAG_PEAK)).unwrap();
    drop(writer);

    let db = Db::open(location, OpenMode::ReadOnly).expect("open read-only");
    let (library, _stats) = rbl_index::load(&db).expect("index");
    Fixture { _dir: dir, library, peak, not_warm_up, peak_equal }
}

fn ids(library: &rbl_index::Library, playlist: &str) -> Vec<String> {
    let index = library.playlists().index_of(playlist.parse().unwrap()).unwrap();
    library
        .source_rows(&TrackSource::SmartPlaylist(index))
        .iter()
        .map(|&row| library.ids[row as usize].to_string())
        .collect()
}

#[test]
fn contains_admits_the_tracks_carrying_the_tag() {
    let f = open();
    assert_eq!(ids(&f.library, &f.peak), [track_id(1), track_id(2)]);
}

#[test]
fn does_not_contain_admits_untagged_tracks_too() {
    let f = open();
    let rows = ids(&f.library, &f.not_warm_up);
    assert!(!rows.contains(&track_id(2)));
    assert!(!rows.contains(&track_id(3)));
    assert!(rows.contains(&track_id(1)));
    assert!(rows.contains(&track_id(0)), "an untagged track does not contain the tag");
    assert_eq!(rows.len(), f.library.len() - 2);
}

#[test]
fn other_operators_admit_nothing_as_rekordbox_does() {
    let f = open();
    assert!(ids(&f.library, &f.peak_equal).is_empty());
}

#[test]
fn the_rule_reads_back_as_a_my_tag_condition() {
    let f = open();
    let index = f.library.playlists().index_of(f.peak.parse().unwrap()).unwrap();
    let rule = f.library.playlists().smart_rule(index).unwrap();
    assert_eq!(rule.unsupported(), 0);
    assert!(rule.to_xml(1).contains(r#"PropertyName="myTag" Operator="8""#));
}

#[test]
fn a_snapshot_keeps_each_tracks_tags() {
    let f = open();
    let fp = Fingerprint {
        format: rbl_index::cache::FORMAT,
        db_len: 1,
        db_modified_ns: 2,
        wal_len: 3,
        wal_modified_ns: 4,
        db_version: 6000,
        content: 5,
        database: 6,
    };
    let restored = decode(&encode(&f.library, fp), fp).expect("decodes");
    for row in 0..f.library.len() {
        assert_eq!(restored.my_tag_keys(row), f.library.my_tag_keys(row));
    }
    assert_eq!(ids(&restored, &f.peak), [track_id(1), track_id(2)]);
}
