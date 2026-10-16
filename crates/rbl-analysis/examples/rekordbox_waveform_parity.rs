//! How close `wave_creator` comes to rekordbox's own waveforms for tracks
//! rekordbox analysed, read only:
//!
//! ```sh
//! cargo run --release -p rbl-analysis --example rekordbox_waveform_parity -- AUDIO DAT
//! cargo run --release -p rbl-analysis --example rekordbox_waveform_parity -- --library 8
//! ```
//!
//! The second form takes the first tracks of the installed library that have
//! a `PWVC`. Each line is a section field: how many values equal rekordbox's,
//! and how many are within one.
#![allow(clippy::print_stdout, clippy::cast_precision_loss)]

use std::path::{Path, PathBuf};

use rbl_analysis::wave_creator::{WaveCreator, Waves};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let tracks = match args.first().map(String::as_str) {
        Some("--library") => installed(args.get(1).map_or(Ok(8), |n| n.parse())?)?,
        Some(audio) => vec![(PathBuf::from(audio), PathBuf::from(args.get(1).ok_or("DAT path required")?))],
        None => return Err("AUDIO DAT, or --library COUNT".into()),
    };
    for (audio, dat) in tracks {
        println!("{}", audio.display());
        compare(&waves_of(&audio)?, &dat)?;
    }
    Ok(())
}

/// Tracks of the installed library rekordbox analysed with a `PWVC`.
fn installed(count: usize) -> Result<Vec<(PathBuf, PathBuf)>, Box<dyn std::error::Error>> {
    let library = rbl_db::Library::open_installed_read_only()?;
    let share = library.location().share_root.clone();
    let mut query = library.connection().prepare(
        "SELECT c.FolderPath, c.AnalysisDataPath FROM djmdContent c
         JOIN contentFile f ON f.ContentID = c.ID AND f.Path LIKE '%.2EX'
         WHERE c.rb_local_deleted = 0 ORDER BY c.ID",
    )?;
    let rows: Vec<(String, String)> =
        query.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?.flatten().collect();
    Ok(rows
        .into_iter()
        .map(|(audio, relative)| (PathBuf::from(audio), rbl_anlz::resolve(&share, &relative)))
        .filter(|(_, dat)| {
            rbl_anlz::Anlz::read(&rbl_anlz::sibling(dat, "2EX")).is_ok_and(|file| file.section(b"PWVC").is_some())
        })
        .take(count)
        .collect())
}

fn waves_of(audio: &Path) -> Result<Waves, Box<dyn std::error::Error>> {
    let mut stream = rbl_audio::MonoStream::open(audio)?;
    let mut creator = WaveCreator::new(stream.sample_rate());
    while let Some((_, frames)) = stream.next_frames() {
        creator.push(frames);
    }
    Ok(creator.finish())
}

fn compare(ours: &Waves, dat: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let dat_file = rbl_anlz::Anlz::read(dat)?;
    let ext = rbl_anlz::Anlz::read(&rbl_anlz::sibling(dat, "EXT"))?;
    let two_ex = rbl_anlz::Anlz::read(&rbl_anlz::sibling(dat, "2EX"))?;
    let pwvc: Vec<u8> = ours.pwvc.iter().flat_map(|gain| gain.to_be_bytes()).collect();
    for (tag, mine, file) in [
        (b"PWAV", &ours.pwav, &dat_file),
        (b"PWV2", &ours.pwv2, &dat_file),
        (b"PWVC", &pwvc, &two_ex),
        (b"PWV6", &ours.pwv6, &two_ex),
        (b"PWV7", &ours.pwv7, &two_ex),
        (b"PWV4", &ours.pwv4, &ext),
        (b"PWV5", &ours.pwv5, &ext),
        (b"PWV3", &ours.pwv3, &ext),
    ] {
        let name = String::from_utf8_lossy(tag);
        let Some(section) = file.section(tag) else {
            println!("  {name}: not in rekordbox's file");
            continue;
        };
        if mine.len() != section.payload.len() {
            println!("  {name}: {} bytes against rekordbox's {}", mine.len(), section.payload.len());
            continue;
        }
        for (field, (a, b)) in fields(*tag, mine).iter().zip(fields(*tag, &section.payload)).enumerate() {
            let exact = a.iter().zip(&b).filter(|(x, y)| x == y).count();
            let close = a.iter().zip(&b).filter(|(x, y)| x.abs_diff(**y) <= 1).count();
            let share = |n: usize| 100.0 * n as f64 / b.len().max(1) as f64;
            println!("  {name} field {field}: {:6.2}% exact, {:6.2}% within one, of {}", share(exact), share(close), b.len());
        }
    }
    Ok(())
}

/// A section's values as rekordbox packs them: `PWVC`'s big-endian gains,
/// `PWV3`'s whiteness and height, `PWV5`'s three colours and height, or
/// whole bytes.
fn fields(tag: [u8; 4], bytes: &[u8]) -> Vec<Vec<u16>> {
    match &tag {
        b"PWVC" => vec![bytes.chunks(2).map(|w| u16::from_be_bytes([w[0], w[1]])).collect()],
        b"PWAV" | b"PWV3" => vec![bytes.iter().map(|b| u16::from(b >> 5)).collect(), bytes.iter().map(|b| u16::from(b & 31)).collect()],
        b"PWV5" => {
            let words: Vec<u16> = bytes.chunks(2).map(|w| u16::from_be_bytes([w[0], w[1]])).collect();
            [(13, 7), (10, 7), (7, 7), (2, 31)].iter().map(|&(shift, mask)| words.iter().map(|w| (w >> shift) & mask).collect()).collect()
        }
        _ => vec![bytes.iter().map(|&b| u16::from(b)).collect()],
    }
}
