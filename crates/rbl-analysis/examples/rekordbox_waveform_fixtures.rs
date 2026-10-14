//! Writes the generated signals behind `tests/rekordbox_waveform` as 16-bit
//! WAV files, for rekordbox to analyse when the fixtures are refreshed:
//! `cargo run -p rbl-analysis --example rekordbox_waveform_fixtures -- DIR`.
#![allow(clippy::print_stdout, clippy::cast_possible_truncation)]

use std::io::Write;
use std::path::Path;

#[path = "../tests/rekordbox_waveform/signals.rs"]
#[allow(dead_code, clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::cast_sign_loss)]
mod signals;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).ok_or("output directory required")?;
    std::fs::create_dir_all(&out)?;
    for signal in &signals::SIGNALS {
        let path = Path::new(&out).join(format!("{}.wav", signal.name));
        write_wav(&path, signal.rate, &(signal.frames)(signal.rate))?;
        println!("{}", path.display());
    }
    Ok(())
}

/// A canonical 44-byte header and interleaved 16-bit stereo PCM.
fn write_wav(path: &Path, rate: u32, frames: &[[i16; 2]]) -> std::io::Result<()> {
    let data = (frames.len() * 4) as u32;
    let mut file = std::io::BufWriter::new(std::fs::File::create(path)?);
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + data).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&2u16.to_le_bytes())?;
    file.write_all(&rate.to_le_bytes())?;
    file.write_all(&(rate * 4).to_le_bytes())?;
    file.write_all(&4u16.to_le_bytes())?;
    file.write_all(&16u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data.to_le_bytes())?;
    for frame in frames {
        file.write_all(&frame[0].to_le_bytes())?;
        file.write_all(&frame[1].to_le_bytes())?;
    }
    file.flush()
}
