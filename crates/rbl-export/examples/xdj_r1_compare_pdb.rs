//! Codec-only differential input from a copied two-WAV stock Device Library.
//! Reads classic PDB/audio, never Cbm fields or an installed Collection.
use std::{collections::BTreeMap, io::Write, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("RB_LITE_TEST").as_deref() != Ok("1") {
        return Err("Set RB_LITE_TEST=1 for disposable comparison output".into());
    }
    let mut args = std::env::args_os().skip(1);
    let root =
        PathBuf::from(args.next().ok_or("Missing copied stock export root")?).canonicalize()?;
    let output = PathBuf::from(args.next().ok_or("Missing fresh output directory")?);
    if args.next().is_some() || output.exists() {
        return Err("Provide exactly two paths; output must not exist".into());
    }
    let pdb_path = root.join("PIONEER/rekordbox/export.pdb");
    if pdb_path.metadata()?.len() > 64 * 1024 * 1024 {
        return Err("Comparison PDB exceeds 64 MiB".into());
    }
    let bytes = std::fs::read(pdb_path)?;
    let pdb = rbl_pdb::Pdb::parse(&bytes)?;
    let rows = pdb.track_rows(
        pdb.table(rbl_pdb::PageType::Tracks)
            .ok_or("Missing tracks")?,
    );
    if rows.len() != 2 {
        return Err("This comparison expects only two generated WAV tracks".into());
    }
    let mut tracks = Vec::new();
    for row in rows {
        if !matches!(row.filename.as_str(), "alpha.wav" | "beta.wav")
            || row.artist_id != 0
            || row.album_id != 0
            || row.genre_id != 0
            || row.key_id != 0
            || row.artwork_id != 0
        {
            return Err("Expected generated alpha/beta WAVs without categories or artwork".into());
        }
        let audio = format!("/{}", row.file_path.trim_start_matches('/'));
        let path = root.join(audio.trim_start_matches('/')).canonicalize()?;
        if !path.starts_with(&root) {
            return Err("Audio path escapes copied export".into());
        }
        let info =
            rbl_audio::legacy::inspect(&path, &|| false)?.ok_or("Unsupported fixture audio")?;
        tracks.push(rbl_export::cbm::Track {
            id: row.id,
            audio,
            filename: row.filename,
            title: row.title,
            artist: String::new(),
            artist_id: 0,
            album: String::new(),
            album_id: 0,
            genre: String::new(),
            key: String::new(),
            comment: row.comment,
            artwork: String::new(),
            track_number: row.track_number,
            bpm: row.tempo_x100,
            hot_cue_auto_load: row.hot_cue_auto_load,
            bytes: info.bytes,
            data_offset: info.offset,
            frames75: info.frames75,
            rate: info.rate,
            format: info.format,
            emphasis: info.emphasis,
        });
    }
    let nodes = pdb.playlist_nodes(
        pdb.table(rbl_pdb::PageType::PlaylistTree)
            .ok_or("Missing playlists")?,
    );
    if nodes.len() != 1 || nodes.iter().any(|n| n.is_folder || n.parent_id != 0) {
        return Err("Expected one flat fixture playlist".into());
    }
    let entries = pdb.playlist_entries(
        pdb.table(rbl_pdb::PageType::PlaylistEntries)
            .ok_or("Missing entries")?,
    );
    let mut playlists = Vec::new();
    for node in &nodes {
        let mut rows: Vec<_> = entries
            .iter()
            .filter(|e| e.playlist_id == node.id)
            .collect();
        rows.sort_by_key(|e| e.entry_index);
        playlists.push(rbl_export::cbm::Playlist {
            id: node.id,
            name: node.name.clone(),
            tracks: rows.iter().map(|e| e.track_id).collect(),
        });
    }
    let files: BTreeMap<_, _> = rbl_export::cbm::build(&tracks, &playlists, nodes.len(), 512, 0)?;
    rbl_export::cbm::validate(&files)?;
    std::fs::create_dir(&output)?;
    let directory = output.join("PIONEER/DBH");
    std::fs::create_dir_all(&directory)?;
    for (name, data) in files {
        std::fs::write(directory.join(name), data)?;
    }
    writeln!(
        std::io::stdout().lock(),
        "Generated Cbm from classic PDB and actual WAVs; no stock Cbm input read"
    )?;
    Ok(())
}
