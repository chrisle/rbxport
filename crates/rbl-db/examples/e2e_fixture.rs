//! Builds the library the compiled app is driven against on another machine.
//!
//! `cargo run -q -p rbl-db --example e2e_fixture -- <out-dir> <dir-as-seen-there>`
//!
//! Writes `master.db` (the real schema, encrypted), `share/`, three short WAVs
//! under `audio/`, and an `options.json` whose `db-path` is where `master.db`
//! will be on the other machine, so `RBXPORT_OPTIONS` can point the
//! app at it. The first three tracks play those WAVs; the rest point nowhere,
//! as the fixture's always have. The first [`ARTWORK_TRACKS`] also have a
//! sleeve under `share/PIONEER/Artwork/`, where rekordbox keeps its own.
#![allow(clippy::pedantic, clippy::print_stdout, clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use rbl_db::fixture::{self, Shape, FIXTURE_PASSPHRASE};

/// Seconds of each audio file, in track order. Different so a test can tell
/// them apart by what the deck reports.
const AUDIO_SECONDS: [u32; 3] = [4, 2, 6];

/// How many tracks, from the first, have artwork. The third playable track has
/// none, so a test can see the empty sleeve beside two drawn ones.
const ARTWORK_TRACKS: usize = 2;

/// An 80x80 JPEG, the kind of file rekordbox caches a sleeve as.
const ARTWORK: &[u8] = include_bytes!("e2e_fixture_artwork.jpg");

fn main() {
    let mut args = std::env::args().skip(1);
    if let Some(root) = std::env::var_os("RBXPORT_DESKTOP_SNAPSHOT") {
        let root = Path::new(&root);
        let db = rbl_db::Library::open(rbl_db::LibraryLocation {
            master_db: root.join("master.db"), share_root: root.join("share"),
            passphrase: FIXTURE_PASSPHRASE.into(), is_real_install: false,
        }, rbl_db::OpenMode::ReadOnly).expect("read desktop fixture");
        let mut query = db.connection().prepare(
            "SELECT ContentID, Kind, InMsec FROM djmdCue WHERE rb_local_deleted = 0 ORDER BY ContentID, Kind, InMsec"
        ).expect("cue query");
        let cues: Vec<serde_json::Value> = query.query_map([], |row| Ok(serde_json::json!({
            "track": row.get::<_, String>(0)?, "kind": row.get::<_, i64>(1)?, "positionMs": row.get::<_, i64>(2)?,
        }))).expect("read cues").collect::<Result<_, _>>().expect("decode cues");
        let mut query = db.connection().prepare(
            "SELECT ID, Title, FolderPath, Rating, BPM, AnalysisDataPath FROM djmdContent WHERE rb_local_deleted = 0 ORDER BY ID"
        ).expect("track query");
        let tracks: Vec<serde_json::Value> = query.query_map([], |row| Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?, "title": row.get::<_, Option<String>>(1)?,
            "path": row.get::<_, Option<String>>(2)?, "rating": row.get::<_, Option<i64>>(3)?,
            "bpm": row.get::<_, Option<i64>>(4)?, "analysis": row.get::<_, Option<String>>(5)?,
        }))).expect("read tracks").collect::<Result<_, _>>().expect("decode tracks");
        let mut query = db.connection().prepare(
            "SELECT ID, Name FROM djmdPlaylist WHERE rb_local_deleted = 0 ORDER BY ID"
        ).expect("playlist query");
        let playlists: Vec<serde_json::Value> = query.query_map([], |row| Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?, "name": row.get::<_, String>(1)?,
        }))).expect("read playlists").collect::<Result<_, _>>().expect("decode playlists");
        println!("{}", serde_json::json!({ "cues": cues, "tracks": tracks, "playlists": playlists }));
        return;
    }
    let (Some(out), Some(seen_as)) = (args.next(), args.next()) else {
        eprintln!("usage: e2e_fixture <out-dir> <dir-as-seen-on-the-target>");
        std::process::exit(2);
    };
    let out = Path::new(&out);
    if out.exists() {
        std::fs::remove_dir_all(out).expect("clear the output directory");
    }
    std::fs::create_dir_all(out.join("audio")).expect("create the output directory");

    let location = fixture::build(out, Shape::default()).expect("build the fixture");

    // A Windows path when the target is Windows, whatever this machine is.
    let sep = if seen_as.contains('\\') { "\\" } else { "/" };
    for (i, seconds) in AUDIO_SECONDS.iter().enumerate() {
        let name = format!("Fixture Track {i}.wav");
        write_wav(&out.join("audio").join(&name), *seconds);
        let there = format!("{seen_as}{sep}audio{sep}{name}");
        fixture::point_at_audio(&location, i, &there, *seconds).expect("point the track at its file");
    }

    // Relative to the share root, as `ImagePath` holds it.
    for i in 0..ARTWORK_TRACKS {
        let relative = format!("/PIONEER/Artwork/00{i}/a{i}.jpg");
        let file = location.share_root.join(relative.trim_start_matches('/'));
        std::fs::create_dir_all(file.parent().expect("an artwork directory")).expect("create the artwork directory");
        std::fs::write(&file, ARTWORK).expect("write the artwork");
        fixture::set_image_path(&location, i, &relative).expect("point the track at its artwork");
    }

    let master_db_as = format!("{seen_as}{sep}master.db");
    fixture::write_options_json(&out.join("options.json"), &master_db_as, FIXTURE_PASSPHRASE)
        .expect("write options.json");

    println!(
        "{}",
        serde_json::json!({
            "out": out.display().to_string(),
            "masterDbAs": master_db_as,
            "optionsJson": out.join("options.json").display().to_string(),
            "tracks": Shape::default().tracks,
            "playlists": Shape::default().playlists,
            "audio": AUDIO_SECONDS.iter().enumerate().map(|(i, s)| serde_json::json!({
                "title": format!("Fixture Track {i}"), "seconds": s, "artwork": i < ARTWORK_TRACKS
            })).collect::<Vec<_>>(),
        })
    );
}

/// A stereo 44.1 kHz WAV of a quiet tone, so a meter moves when it plays.
fn write_wav(path: &Path, seconds: u32) {
    let rate = 44_100_u32;
    let frames = rate * seconds;
    let data_len = frames * 4;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16_u32.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes());
    out.extend_from_slice(&2_u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 4).to_le_bytes());
    out.extend_from_slice(&4_u16.to_le_bytes());
    out.extend_from_slice(&16_u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..frames {
        let t = f64::from(i) / f64::from(rate);
        let sample = ((t * 220.0 * std::f64::consts::TAU).sin() * 0.25 * 32767.0) as i16;
        out.extend_from_slice(&sample.to_le_bytes());
        out.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(path, out).expect("write the wav");
}
