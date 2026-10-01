//! Builds the library the compiled app serves to the CDJ-3000 emulator in
//! `scripts/e2e-link/run.sh`.
//!
//! `cargo run -q -p rbl-link --example emu_fixture -- <out-dir>`
//!
//! Writes `master.db` (the real schema, encrypted), three WAVs under
//! `audio/`, the analysis files for each under `share/PIONEER/USBANLZ/` —
//! authored by `rbl-analysis` and `rbl-anlz`, as the app's own analyse
//! command authors them — and an `options.json` for `RBXPORT_OPTIONS` to
//! name. Every track in the library is playable: a deck browsing it finds
//! three rows, each with a file, a beat grid and waveforms behind it.
//!
//! The WAVs are tones with a click on every beat, each a different pitch
//! and tempo, so the deck's BPM readout says which one it loaded and the
//! pytest can tell the first load from the second. Thirty seconds each:
//! long enough to play for five and still be playing.
#![allow(
    clippy::pedantic,
    clippy::print_stdout,
    clippy::unwrap_used,
    clippy::expect_used
)]

use std::f32::consts::TAU;
use std::path::Path;

use rbl_db::fixture::{self, Shape, FIXTURE_PASSPHRASE};

/// One track of the fixture: its title (the WAV's stem), tone and tempo.
struct Tone {
    title: &'static str,
    hz: f32,
    bpm: f32,
    key: &'static str,
}

/// In title order, which is the order the deck's TRACK list shows them.
const TONES: [Tone; 3] = [
    Tone {
        title: "01 Link Tone 220Hz",
        hz: 220.0,
        bpm: 124.0,
        key: "Am",
    },
    Tone {
        title: "02 Link Tone 440Hz",
        hz: 440.0,
        bpm: 128.0,
        key: "Abm",
    },
    Tone {
        title: "03 Link Tone 880Hz",
        hz: 880.0,
        bpm: 132.0,
        key: "B",
    },
];
const SECONDS: u32 = 30;
const RATE: u32 = 44_100;

fn main() {
    let Some(out) = std::env::args().nth(1) else {
        eprintln!("usage: emu_fixture <out-dir>");
        std::process::exit(2);
    };
    let out = Path::new(&out);
    if out.exists() {
        std::fs::remove_dir_all(out).expect("clear the output directory");
    }
    let audio_dir = out.join("audio");
    std::fs::create_dir_all(&audio_dir).expect("create the output directory");

    let shape = Shape {
        tracks: TONES.len(),
        playlists: 1,
        tracks_per_playlist: TONES.len(),
        ..Shape::default()
    };
    let location = fixture::build(out, shape).expect("build the fixture");

    // Nonalphabetical keys and distinct artists exercise the actual sort and
    // nested BACK controls, not just an anonymous three-row flat list.
    let db = rbl_db::Library::open(location.clone(), rbl_db::OpenMode::ReadWrite)
        .expect("open fixture metadata");
    for (i, tone) in TONES.iter().enumerate() {
        let id = (i + 1).to_string();
        let artist = format!("Link Artist {}", i + 1);
        let album = format!("Link Album {}", i + 1);
        db.connection().execute("INSERT INTO djmdKey (ID, ScaleName, created_at, updated_at) VALUES (?1, ?2, '2026-09-20', '2026-09-20')", (&id, tone.key)).unwrap();
        db.connection().execute("INSERT INTO djmdArtist (ID, Name, created_at, updated_at) VALUES (?1, ?2, '2026-09-20', '2026-09-20')", (&id, &artist)).unwrap();
        db.connection().execute("INSERT INTO djmdAlbum (ID, Name, AlbumArtistID, created_at, updated_at) VALUES (?1, ?2, ?1, '2026-09-20', '2026-09-20')", (&id, &album)).unwrap();
        db.connection()
            .execute(
                "UPDATE djmdContent SET KeyID = ?1, ArtistID = ?1, AlbumID = ?1 WHERE ID = ?2",
                (&id, fixture::track_id(i)),
            )
            .unwrap();
    }
    drop(db);
    let mut tracks = Vec::new();
    for (i, tone) in TONES.iter().enumerate() {
        let wav = audio_dir.join(format!("{}.wav", tone.title));
        write_tone_wav(&wav, tone);
        let path = wav.to_str().expect("a UTF-8 path");
        fixture::point_at_audio(&location, i, path, SECONDS).expect("point the track at its file");

        // The same recipe as the app's analyse command: decode, analyse,
        // author the three files, register them on the row.
        let audio = rbl_audio::decode_mono(&wav, None).expect("decode the WAV");
        let analysis = rbl_analysis::analyse(&audio.samples, audio.sample_rate);
        let beats: Vec<rbl_anlz::Beat> = analysis
            .tempo
            .beats
            .iter()
            .map(|b| rbl_anlz::Beat {
                beat_number: b.beat_number,
                tempo_x100: b.tempo_x100,
                time_ms: b.time_ms,
            })
            .collect();
        let columns: Vec<rbl_anlz::BandColumn> = analysis
            .waveform
            .columns
            .iter()
            .map(|c| rbl_anlz::BandColumn {
                low: c.low,
                mid: c.mid,
                high: c.high,
                peak: c.peak,
            })
            .collect();
        let files = rbl_anlz::author_with_overview(
            path,
            &beats,
            &columns,
            analysis.waveform.overview.as_slice().try_into().ok(),
            rbl_anlz::Existing {
                dat: None,
                ext: None,
                two_ex: None,
            },
        );

        let relative = format!("/PIONEER/USBANLZ/P{:03}/{:08X}/ANLZ0000.DAT", i, 10_000 + i);
        let dat = rbl_anlz::resolve(&location.share_root, &relative);
        std::fs::create_dir_all(dat.parent().unwrap()).expect("create the analysis folder");
        std::fs::write(&dat, &files.dat).expect("write the DAT");
        std::fs::write(rbl_anlz::sibling(&dat, "EXT"), &files.ext).expect("write the EXT");
        std::fs::write(rbl_anlz::sibling(&dat, "2EX"), &files.two_ex).expect("write the 2EX");
        fixture::set_analysis_path(&location, i, &relative).expect("register the analysis path");
        let bpm_x100 = (analysis.tempo.bpm * 100.0).round() as u32;
        fixture::set_tempo(&location, i, bpm_x100).expect("register the tempo");

        tracks.push(serde_json::json!({
            "id": fixture::track_id(i),
            "title": tone.title,
            "key": tone.key,
            "artist": format!("Link Artist {}", i + 1),
            "hz": tone.hz,
            "seconds": SECONDS,
            "bpmX100": bpm_x100,
            "beats": beats.len(),
            "path": path,
        }));
    }

    let master_db = out.join("master.db");
    fixture::write_options_json(
        &out.join("options.json"),
        master_db.to_str().unwrap(),
        FIXTURE_PASSPHRASE,
    )
    .expect("write options.json");

    println!(
        "{}",
        serde_json::json!({
            "out": out.display().to_string(),
            "optionsJson": out.join("options.json").display().to_string(),
            "tracks": tracks,
        })
    );
}

/// A mono 16-bit WAV: a steady tone with a short click on every beat, loud
/// enough for the tempo detector to grid it.
fn write_tone_wav(path: &Path, tone: &Tone) {
    let frames = RATE * SECONDS;
    let period = (60.0 / tone.bpm * RATE as f32) as u32;
    let mut data = Vec::with_capacity(frames as usize * 2);
    for i in 0..frames {
        let t = i as f32 / RATE as f32;
        let since = i % period.max(1);
        let click = if since < 600 {
            0.7 * (1.0 - since as f32 / 600.0)
        } else {
            0.0
        };
        let sample = 0.25 * (TAU * tone.hz * t).sin() + click;
        data.extend_from_slice(&((sample.clamp(-1.0, 1.0) * 32_000.0) as i16).to_le_bytes());
    }
    let data_len = u32::try_from(data.len()).unwrap();
    let mut out = Vec::with_capacity(44 + data.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16_u32.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes());
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes());
    out.extend_from_slice(&2_u16.to_le_bytes());
    out.extend_from_slice(&16_u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(&data);
    std::fs::write(path, out).expect("write the WAV");
}
