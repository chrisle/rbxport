//! Generated disposable fixtures, not stock rekordbox captures or hardware tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use rbl_export::{cbm, ExportOptions, SourcePlaylist, SourceTrack};
use std::sync::atomic::{AtomicBool, Ordering};
fn wav(path: &std::path::Path) {
    let len = 44100_u32 * 4;
    let mut b = Vec::new();
    b.extend(b"RIFF");
    b.extend((len + 36).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16_u32.to_le_bytes());
    b.extend(1_u16.to_le_bytes());
    b.extend(2_u16.to_le_bytes());
    b.extend(44100_u32.to_le_bytes());
    b.extend(176_400_u32.to_le_bytes());
    b.extend(4_u16.to_le_bytes());
    b.extend(16_u16.to_le_bytes());
    b.extend(b"data");
    b.extend(len.to_le_bytes());
    b.resize(b.len() + len as usize, 0);
    std::fs::write(path, b).unwrap();
}
fn source(dir: &std::path::Path, id: u64) -> SourceTrack {
    let path = dir.join(format!("track{id}.wav"));
    wav(&path);
    SourceTrack {
        id,
        source_path: path,
        title: format!("Track {id}"),
        artist: "Artist".into(),
        album: "Album".into(),
        sample_rate: 44100,
        duration_sec: 1,
        metadata: rbl_core::ExportMetadata {
            bit_depth: 16,
            hot_cue_auto_load: true,
            ..Default::default()
        },
        ..Default::default()
    }
}
fn options(enabled: bool) -> ExportOptions<'static> {
    ExportOptions {
        xdj_r1_support: enabled,
        ..Default::default()
    }
}
fn run(stick: &std::path::Path, tracks: &[SourceTrack], enabled: bool) -> rbl_export::ExportReport {
    rbl_export::export_with_options(
        stick,
        tracks,
        &[SourcePlaylist {
            id: 1,
            name: "Set".into(),
            track_indices: (0..tracks.len()).collect(),
            ..Default::default()
        }],
        &[],
        &options(enabled),
        &mut |_| {},
    )
    .unwrap()
}
fn set(stick: &std::path::Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    cbm::FILES
        .into_iter()
        .map(|n| {
            (
                n.to_owned(),
                std::fs::read(stick.join("PIONEER/DBH").join(n)).unwrap(),
            )
        })
        .collect()
}
#[test]
fn off_then_on_unchanged_input_coexists_noop_and_off_retires_owned_r1() {
    let dir = tempfile::tempdir().unwrap();
    let stick = tempfile::tempdir().unwrap();
    let tracks = vec![source(dir.path(), 1)];
    assert!(run(stick.path(), &tracks, false).verified);
    assert!(!stick.path().join("PIONEER/DBH/CbmDB.DBH").exists());
    // A pre-R1 manifest omits the new profile field and keeps its old digest.
    let manifest_path = rbl_export::Manifest::path(stick.path());
    let mut old: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    old["generation"]
        .as_object_mut()
        .unwrap()
        .remove("r1_files");
    std::fs::write(&manifest_path, serde_json::to_vec(&old).unwrap()).unwrap();
    assert!(run(stick.path(), &tracks, false).up_to_date);

    let report = run(stick.path(), &tracks, true);
    assert!(report.verified);
    assert!(!report.up_to_date);
    assert_eq!(report.bytes_copied, 0);
    assert!(stick.path().join("PIONEER/rekordbox/export.pdb").is_file());
    assert!(stick
        .path()
        .join("PIONEER/rekordbox/exportLibrary.db")
        .is_file());
    cbm::validate(&set(stick.path())).unwrap();
    assert!(run(stick.path(), &tracks, true).up_to_date);
    let off = run(stick.path(), &tracks, false);
    assert!(off.verified);
    assert!(!off.up_to_date);
    for f in cbm::FILES {
        assert!(!stick.path().join("PIONEER/DBH").join(f).exists());
    }
    assert!(rbl_export::verify(stick.path()).unwrap().is_ok());
}
#[test]
fn add_update_remove_and_empty_generation_are_structurally_coherent() {
    let dir = tempfile::tempdir().unwrap();
    let stick = tempfile::tempdir().unwrap();
    let mut tracks = vec![source(dir.path(), 1)];
    run(stick.path(), &tracks, true);
    tracks.push(source(dir.path(), 2));
    run(stick.path(), &tracks, true);
    tracks[0].title = "Renamed".into();
    run(stick.path(), &tracks, true);
    tracks.remove(1);
    run(stick.path(), &tracks, true);
    assert_eq!(
        cbm::parse_contents(&set(stick.path())[cbm::FILES[0]])
            .unwrap()
            .records
            .len(),
        1
    );
    let report = run(stick.path(), &[], true);
    assert!(report.verified);
    let set = set(stick.path());
    cbm::validate(&set).unwrap();
    assert_eq!(
        cbm::parse_contents(&set[cbm::FILES[0]])
            .unwrap()
            .records
            .len(),
        0
    );
}
#[test]
fn pending_edits_and_foreign_libraries_block_on_and_off_without_deleting_audio() {
    let dir = tempfile::tempdir().unwrap();
    let stick = tempfile::tempdir().unwrap();
    let tracks = vec![source(dir.path(), 1)];
    run(stick.path(), &tracks, true);
    let library = set(stick.path());
    let mut pl = library[cbm::FILES[5]].clone();
    let offset = u32::from_le_bytes(pl[20..24].try_into().unwrap()) as usize;
    pl[offset + 0x18c] = 4;
    std::fs::write(stick.path().join("PIONEER/DBH/CbmPL.DBH"), &pl).unwrap();
    for enabled in [true, false] {
        let e = rbl_export::export_with_options(
            stick.path(),
            &[],
            &[],
            &[],
            &options(enabled),
            &mut |_| {},
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("pending device playlist edits"), "{e}");
    }
    assert_eq!(
        std::fs::read(stick.path().join("PIONEER/DBH/CbmDB.DBH")).unwrap(),
        library[cbm::FILES[0]]
    );
    // Removing ownership simulates another writer's library. Keep its files intact.
    std::fs::write(
        stick.path().join("PIONEER/DBH/CbmPL.DBH"),
        &library[cbm::FILES[5]],
    )
    .unwrap();
    std::fs::remove_file(rbl_export::Manifest::path(stick.path())).unwrap();
    let e =
        rbl_export::export_with_options(stick.path(), &[], &[], &[], &options(false), &mut |_| {})
            .unwrap_err()
            .to_string();
    assert!(e.contains("not owned"), "{e}");
}
#[test]
fn cancellation_and_invalid_audio_leave_the_prior_library_set_intact() {
    let dir = tempfile::tempdir().unwrap();
    let stick = tempfile::tempdir().unwrap();
    let mut tracks = vec![source(dir.path(), 1)];
    run(stick.path(), &tracks, true);
    let prior = set(stick.path());
    tracks[0].title = "Changed".into();
    let cancel = AtomicBool::new(false);
    let e = rbl_export::export_cancellable(
        stick.path(),
        &tracks,
        &[],
        &[],
        &options(true),
        &mut |p| {
            if p.stage == "database" {
                cancel.store(true, Ordering::Relaxed);
            }
        },
        &|| cancel.load(Ordering::Relaxed),
    )
    .unwrap_err();
    assert!(matches!(e, rbl_export::ExportError::Cancelled));
    assert_eq!(set(stick.path()), prior);
    std::fs::write(&tracks[0].source_path, b"invalid WAV").unwrap();
    assert!(rbl_export::export_with_options(
        stick.path(),
        &tracks,
        &[],
        &[],
        &options(true),
        &mut |_| {}
    )
    .is_err());
    assert_eq!(set(stick.path()), prior);
    rbl_export::recover(stick.path()).unwrap();
    assert!(rbl_export::verify(stick.path()).unwrap().is_ok());
}

fn native_dat(audio: &str) -> String {
    let hash = audio.encode_utf16().fold(0_u32, |h, u| {
        (h.wrapping_mul(0x5bc9).wrapping_add(u32::from(u)))
            .wrapping_mul(0x93b5)
            .wrapping_add(u32::from(u))
    }) % 200_003;
    let shard = [0, 2, 6, 7, 9, 13, 16]
        .iter()
        .enumerate()
        .fold(0_u32, |s, (i, b)| s | ((hash >> b) & 1) << i);
    format!("PIONEER/USBANLZ/P{shard:03X}/{hash:08X}/ANLZ0000.DAT")
}
#[test]
fn actual_audio_properties_cues_and_artwork_survive_the_native_path_lookup() {
    let dir = tempfile::tempdir().unwrap();
    let stick = tempfile::tempdir().unwrap();
    let mut track = source(dir.path(), 1);
    track.sample_rate = 96000;
    track.metadata.bit_depth = 24;
    track.duration_sec = 99; // deliberately stale Collection values
    track.cues = Some(vec![
        rbl_anlz::cues::ExportCue {
            kind: 1,
            time_ms: 200,
            ..Default::default()
        },
        rbl_anlz::cues::ExportCue {
            kind: 0,
            time_ms: 300,
            loop_time_ms: Some(600),
            ..Default::default()
        },
    ]);
    let image = dir.path().join("cover.jpg");
    image::RgbImage::from_pixel(100, 50, image::Rgb([255, 0, 0]))
        .save(&image)
        .unwrap();
    track.artwork = Some(image);
    run(stick.path(), &[track], true);
    let library = set(stick.path());
    let parsed = cbm::parse_contents(&library[cbm::FILES[0]]).unwrap();
    let r = &parsed.records[0];
    assert_eq!(r[0x23], 0x20);
    assert_eq!(u32::from_le_bytes(r[0x9e8..0x9ec].try_into().unwrap()), 75);
    let audio = cbm::audio_path(r).unwrap();
    let dat = std::fs::read(stick.path().join(native_dat(&audio))).unwrap();
    let dat = rbl_anlz::parse(&dat).unwrap();
    assert_eq!(dat.path().unwrap(), audio);
    let lists: Vec<_> = dat
        .sections
        .iter()
        .filter(|s| &s.tag.0 == b"PCOB")
        .collect();
    assert_eq!(lists.len(), 2);
    for list in &lists {
        assert_eq!(u16::from_be_bytes(list.header[6..8].try_into().unwrap()), 1);
        assert_eq!(&list.payload[..4], b"PCPT");
        assert_eq!(
            u16::from_be_bytes(list.payload[30..32].try_into().unwrap()),
            1000
        );
        assert_eq!(
            u32::from_be_bytes(list.payload[8..12].try_into().unwrap()),
            56
        );
    }
    let large = stick.path().join("PIONEER/Artwork/00001/a1_l.jpg");
    assert_eq!(image::image_dimensions(large).unwrap(), (320, 320));
}
#[test]
fn a_foreign_hash_candidate_is_preserved_and_ppth_remains_unambiguous() {
    let dir = tempfile::tempdir().unwrap();
    let stick = tempfile::tempdir().unwrap();
    let t = source(dir.path(), 1);
    let canonical = native_dat("/Contents/Artist/Album/track1.wav");
    let foreign = rbl_anlz::AnlzBuilder::new()
        .path("/Contents/foreign.wav")
        .finish();
    let path = stick.path().join(&canonical);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, &foreign).unwrap();
    run(stick.path(), &[t], true);
    assert_eq!(std::fs::read(&path).unwrap(), foreign);
    let library = set(stick.path());
    let parsed = cbm::parse_contents(&library[cbm::FILES[0]]).unwrap();
    let audio = cbm::audio_path(&parsed.records[0]).unwrap();
    assert_ne!(native_dat(&audio), canonical);
    assert_eq!(
        rbl_anlz::Anlz::read(&stick.path().join(native_dat(&audio)))
            .unwrap()
            .path()
            .unwrap(),
        audio
    );
}
#[test]
fn a_partial_r1_publication_rolls_back_when_its_new_generation_is_invalid() {
    let dir = tempfile::tempdir().unwrap();
    let stick = tempfile::tempdir().unwrap();
    run(stick.path(), &[source(dir.path(), 1)], true);
    let prior = set(stick.path());
    let publication =
        rbl_core::durable::Publication::new(stick.path(), ".rbxport-publication").unwrap();
    let mut files = Vec::new();
    for file in cbm::FILES {
        let path = std::path::PathBuf::from(format!("PIONEER/DBH/{file}"));
        let staged = publication.stage().join(&path);
        std::fs::create_dir_all(staged.parent().unwrap()).unwrap();
        std::fs::write(staged, b"invalid new DBH").unwrap();
        files.push(path);
    }
    publication.commit_retained(&files).unwrap();
    drop(publication);
    assert!(rbl_export::recover(stick.path()).is_err());
    assert_eq!(set(stick.path()), prior);
    assert!(rbl_export::verify(stick.path()).unwrap().is_ok());
}

#[test]
fn all_excluded_is_explicit_and_keeps_the_normal_libraries() {
    let dir = tempfile::tempdir().unwrap();
    let stick = tempfile::tempdir().unwrap();
    let t = source(dir.path(), 1);
    let mut bytes = std::fs::read(&t.source_path).unwrap();
    bytes[24..28].copy_from_slice(&22_050_u32.to_le_bytes());
    bytes[28..32].copy_from_slice(&88_200_u32.to_le_bytes());
    std::fs::write(&t.source_path, bytes).unwrap();
    let report = run(stick.path(), &[t], true);
    assert_eq!(report.tracks, 1);
    assert!(report
        .warnings
        .iter()
        .any(|w| w.reason == "r1-format-excluded"));
    let set = set(stick.path());
    cbm::validate(&set).unwrap();
    assert_eq!(
        cbm::parse_contents(&set[cbm::FILES[0]])
            .unwrap()
            .records
            .len(),
        0
    );
    assert!(rbl_export::verify(stick.path()).unwrap().is_ok());
}

#[test]
fn r1_keeps_native_paths_while_normal_libraries_keep_the_hidden_root() {
    let dir = tempfile::tempdir().unwrap();
    let stick = tempfile::tempdir().unwrap();
    let mut t = source(dir.path(), 1);
    let image = dir.path().join("hidden-cover.jpg");
    image::RgbImage::from_pixel(30, 50, image::Rgb([0, 255, 0]))
        .save(&image)
        .unwrap();
    t.artwork = Some(image);
    let options = ExportOptions {
        root: Some(rbl_export::ExportRoot::Hidden),
        ..options(true)
    };
    let report =
        rbl_export::export_with_options(stick.path(), &[t], &[], &[], &options, &mut |_| {})
            .unwrap();
    assert!(report.verified);
    assert!(stick.path().join(".PIONEER/rekordbox/export.pdb").is_file());
    assert!(stick
        .path()
        .join(".PIONEER/rekordbox/exportLibrary.db")
        .is_file());
    assert!(stick.path().join("PIONEER/Artwork/00001/a1.jpg").is_file());
    cbm::validate(&set(stick.path())).unwrap();
    let parsed = cbm::parse_contents(&set(stick.path())[cbm::FILES[0]]).unwrap();
    let audio = cbm::audio_path(&parsed.records[0]).unwrap();
    assert!(stick.path().join(native_dat(&audio)).is_file());
    assert!(rbl_export::verify(stick.path()).unwrap().is_ok());
}

#[test]
fn generated_aiff_and_mp3_have_actual_format_indices_and_offsets() {
    let dir = tempfile::tempdir().unwrap();
    let original = source(dir.path(), 1);
    for (format, code, offset) in [
        (rbl_audio::compatibility::Format::Aiff, 0x30, 54),
        (rbl_audio::compatibility::Format::Mp3, 0x1e, 0),
    ] {
        let path = dir.path().join(format!("converted.{}", format.extension()));
        rbl_audio::compatibility::convert(&original.source_path, &path, format).unwrap();
        let info = rbl_audio::legacy::inspect(&path, &|| false)
            .unwrap()
            .unwrap();
        assert_eq!(info.format, code);
        assert_eq!(info.offset, offset);
        assert_eq!(
            u64::from(info.bytes),
            std::fs::metadata(path).unwrap().len()
        );
    }
}
#[test]
fn compatibility_conversion_is_shared_by_all_three_library_families() {
    let dir = tempfile::tempdir().unwrap();
    let stick = tempfile::tempdir().unwrap();
    let mut t = source(dir.path(), 1);
    let len = 44_100_u32 * 8;
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((len + 36).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16_u32.to_le_bytes());
    bytes.extend(3_u16.to_le_bytes());
    bytes.extend(2_u16.to_le_bytes());
    bytes.extend(44_100_u32.to_le_bytes());
    bytes.extend(352_800_u32.to_le_bytes());
    bytes.extend(8_u16.to_le_bytes());
    bytes.extend(32_u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(len.to_le_bytes());
    bytes.resize(bytes.len() + len as usize, 0);
    std::fs::write(&t.source_path, &bytes).unwrap();
    t.metadata.bit_depth = 32;
    let options = ExportOptions {
        compatibility: Some(rbl_export::CompatibilityFormat::Mp3),
        ..options(true)
    };
    let report = rbl_export::export_with_options(
        stick.path(),
        &[t.clone()],
        &[],
        &[],
        &options,
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(report.converted, 1);
    let parsed = cbm::parse_contents(&set(stick.path())[cbm::FILES[0]]).unwrap();
    let audio = cbm::audio_path(&parsed.records[0]).unwrap();
    assert_eq!(
        std::path::Path::new(&audio)
            .extension()
            .and_then(|s| s.to_str()),
        Some("mp3")
    );
    assert_eq!(parsed.records[0][0x23], 0x1e);
    let manifest = rbl_export::Manifest::load(stick.path()).unwrap();
    assert_eq!(manifest.tracks[0].audio, audio);
    assert_eq!(std::fs::read(t.source_path).unwrap(), bytes);
    assert!(rbl_export::verify(stick.path()).unwrap().is_ok());
}

#[test]
fn pcm_format_code_uses_sample_rate_independently_of_bit_depth() {
    let dir = tempfile::tempdir().unwrap();
    for rate in [44_100_u32, 48_000] {
        for depth in [16_u16, 24] {
            let len = rate * 2 * u32::from(depth / 8);
            for aiff in [false, true] {
                let mut data = Vec::new();
                if aiff {
                    data.extend(b"FORM");
                    data.extend((len + 46).to_be_bytes());
                    data.extend(b"AIFFCOMM");
                    data.extend(18_u32.to_be_bytes());
                    data.extend(2_u16.to_be_bytes());
                    data.extend(rate.to_be_bytes());
                    data.extend(depth.to_be_bytes());
                    data.extend([0x40, 0x0e]);
                    data.extend(u16::try_from(rate).unwrap().to_be_bytes());
                    data.extend([0; 6]);
                    data.extend(b"SSND");
                    data.extend((len + 8).to_be_bytes());
                    data.extend([0; 8]);
                } else {
                    data.extend(b"RIFF");
                    data.extend((len + 36).to_le_bytes());
                    data.extend(b"WAVEfmt ");
                    data.extend(16_u32.to_le_bytes());
                    data.extend(1_u16.to_le_bytes());
                    data.extend(2_u16.to_le_bytes());
                    data.extend(rate.to_le_bytes());
                    data.extend(len.to_le_bytes());
                    data.extend((2 * (depth / 8)).to_le_bytes());
                    data.extend(depth.to_le_bytes());
                    data.extend(b"data");
                    data.extend(len.to_le_bytes());
                }
                data.resize(data.len() + len as usize, 0);
                let path = dir.path().join(format!(
                    "{rate}-{depth}.{}",
                    if aiff { "aiff" } else { "wav" }
                ));
                std::fs::write(&path, data).unwrap();
                let info = rbl_audio::legacy::inspect(&path, &|| false)
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    info.format,
                    if aiff { 0x30 } else { 0x20 } + u8::from(rate == 48_000)
                );
                assert_eq!(u32::from(info.rate), rate * 2 * u32::from(depth) / 100);
                assert_eq!(info.frames75, 75);
            }
        }
    }
}

#[test]
fn exclusions_keep_the_original_device_library_category_and_content_ids() {
    let dir = tempfile::tempdir().unwrap();
    let stick = tempfile::tempdir().unwrap();
    let mut excluded = source(dir.path(), 1);
    excluded.artist = "A".into();
    excluded.album = "A".into();
    let mut data = std::fs::read(&excluded.source_path).unwrap();
    data[24..28].copy_from_slice(&22_050_u32.to_le_bytes());
    data[28..32].copy_from_slice(&88_200_u32.to_le_bytes());
    std::fs::write(&excluded.source_path, data).unwrap();
    let mut included = source(dir.path(), 2);
    included.artist = "B".into();
    included.album = "B".into();
    run(stick.path(), &[excluded, included], true);
    let files = set(stick.path());
    let parsed = cbm::parse_contents(&files[cbm::FILES[0]]).unwrap();
    assert_eq!(parsed.records.len(), 1);
    assert_eq!(
        u32::from_le_bytes(parsed.records[0][0x9e4..0x9e8].try_into().unwrap()),
        2
    );
    for file in [cbm::FILES[2], cbm::FILES[3]] {
        let groups = cbm::parse_category(&files[file], &parsed.records).unwrap();
        assert!(groups
            .directories
            .iter()
            .any(|d| u32::from_le_bytes(d[0x188..0x18c].try_into().unwrap()) == 2));
    }
}
