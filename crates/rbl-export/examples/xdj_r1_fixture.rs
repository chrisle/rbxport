//! Generated disposable input for differential tests against stock rekordbox 6.
//! This creates an export only; it never opens an installed Collection library.
use std::io::Write;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("RB_LITE_TEST").as_deref() != Ok("1") {
        return Err("Set RB_LITE_TEST=1 for disposable fixture exports".into());
    }
    let mut args = std::env::args_os().skip(1);
    let source = PathBuf::from(args.next().ok_or("Missing generated WAV directory")?);
    let destination = PathBuf::from(args.next().ok_or("Missing fresh destination directory")?);
    let enabled = match args.next() {
        None => false,
        Some(arg) if arg == "--r1" => true,
        Some(_) => return Err("Only --r1 is accepted after the destination".into()),
    };
    if args.next().is_some() {
        return Err("Unexpected extra fixture argument".into());
    }
    if destination.exists() {
        return Err("Fixture destination must be fresh; existing data is preserved".into());
    }
    let mut tracks = Vec::new();
    for (id, file, title, seconds) in [(1, "alpha.wav", "Alpha", 3), (2, "beta.wav", "Beta", 4)] {
        let path = source.join(file).canonicalize()?;
        tracks.push(rbl_export::SourceTrack {
            id,
            source_path: path,
            title: title.into(),
            artist: "R1 Fixture".into(),
            album: "Comparison".into(),
            genre: "Test".into(),
            comment: "Generated disposable fixture".into(),
            bpm_x100: 12_800,
            duration_sec: seconds,
            sample_rate: 44_100,
            bitrate: 1411,
            metadata: rbl_core::ExportMetadata {
                bit_depth: 16,
                hot_cue_auto_load: true,
                ..Default::default()
            },
            ..Default::default()
        });
    }
    let playlists = vec![rbl_export::SourcePlaylist {
        id: 1,
        name: "R1 Comparison".into(),
        track_indices: vec![1, 0],
        ..Default::default()
    }];
    let report = rbl_export::export_with_options(
        &destination,
        &tracks,
        &playlists,
        &[],
        &rbl_export::ExportOptions {
            xdj_r1_support: enabled,
            ..Default::default()
        },
        &mut |_| {},
    )?;
    if !report.verified {
        return Err("Generated export did not verify".into());
    }
    writeln!(
        std::io::stdout().lock(),
        "{}",
        serde_json::json!({"destination":destination,"r1":enabled,"tracks":report.tracks,"verified":report.verified})
    )?;
    Ok(())
}
