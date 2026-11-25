//! Aggregate save timing is diagnostic coverage, not a speed or device test.
#![allow(clippy::unwrap_used, clippy::panic)]
use rbl_export::{ExportOptions, SourcePlaylist, SourceTrack, SyncSource};
use std::collections::BTreeMap;

type Fields = BTreeMap<String, String>;
// Scoped tracing subscribers share process-wide callsite interest. Serialize
// entire fixtures, including their uncaptured initial exports, so another test
// cannot first register a save callsite without this test's subscriber.
static FIXTURE: std::sync::Mutex<()> = std::sync::Mutex::new(());
// Same fmt capture as the snapshot-read diagnostics (#950).
fn capture<T>(action: impl FnOnce() -> T) -> (T, String) {
    let log = tempfile::NamedTempFile::new().unwrap();
    let writer = log.reopen().unwrap();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_target(false)
        .with_level(false)
        .with_max_level(tracing::Level::INFO)
        .with_writer(move || writer.try_clone().unwrap())
        .finish();
    let result = tracing::subscriber::with_default(subscriber, action);
    (result, std::fs::read_to_string(log.path()).unwrap())
}
fn phases(log: &str) -> Vec<Fields> {
    log.lines()
        .filter_map(|line| {
            let line = line.trim();
            let message = ["USB save phase started", "USB save phase finished"]
                .into_iter()
                .find(|m| line.starts_with(m))?;
            let mut fields = Fields::from([("message".into(), message.into())]);
            for field in line[message.len()..].split_whitespace() {
                let (key, value) = field.split_once('=').unwrap();
                fields.insert(key.into(), value.trim_matches('"').into());
            }
            Some(fields)
        })
        .collect()
}
fn fixture(root: &std::path::Path) -> SourceTrack {
    let source = root.join("PRIVATE_SAVE_PHASE_PATH.wav");
    let samples = [0_i16, 1000, -1000, 0];
    let size = u32::try_from(samples.len() * 2).unwrap();
    let mut wav = b"RIFF".to_vec();
    wav.extend((36 + size).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16_u32.to_le_bytes());
    wav.extend(1_u16.to_le_bytes());
    wav.extend(1_u16.to_le_bytes());
    wav.extend(44100_u32.to_le_bytes());
    wav.extend(88200_u32.to_le_bytes());
    wav.extend(2_u16.to_le_bytes());
    wav.extend(16_u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(size.to_le_bytes());
    for sample in samples {
        wav.extend(sample.to_le_bytes());
    }
    std::fs::write(&source, wav).unwrap();
    SourceTrack {
        id: 1,
        source_path: source,
        title: "PRIVATE_SAVE_PHASE_PATH".into(),
        sample_rate: 44100,
        analysis: vec![(
            "DAT".into(),
            rbl_anlz::AnlzBuilder::new()
                .path("/PRIVATE_SAVE_PHASE_PATH.wav")
                .finish(),
        )],
        ..Default::default()
    }
}
fn export(
    root: &std::path::Path,
    track: &SourceTrack,
    progress: &mut dyn FnMut(&rbl_export::ExportProgress),
) -> rbl_export::Result<rbl_export::ExportReport> {
    rbl_export::export_with_options(
        root,
        std::slice::from_ref(track),
        &[SourcePlaylist {
            id: 1,
            name: "PRIVATE_SAVE_PHASE_PATH playlist".into(),
            track_indices: vec![0],
            ..Default::default()
        }],
        &[],
        &ExportOptions {
            sync: Some(&SyncSource {
                db_id: 123,
                ..Default::default()
            }),
            ..Default::default()
        },
        progress,
    )
}
fn assert_fixed_fields(records: &[Fields]) {
    for fields in records {
        let keys: Vec<_> = fields.keys().map(String::as_str).collect();
        if fields["message"] == "USB save phase started" {
            assert_eq!(keys, ["message", "phase"]);
        } else {
            assert_eq!(keys, ["elapsed_ms", "message", "ok", "phase"]);
            fields["elapsed_ms"].parse::<u64>().unwrap();
            assert!(["true", "false"].contains(&fields["ok"].as_str()));
        }
        assert!(!format!("{fields:?}").contains("PRIVATE_SAVE_PHASE_PATH"));
        assert!(!fields
            .values()
            .any(|value| value.contains('/') || value.contains('\\')));
    }
}
#[test]
fn successful_export_records_fixed_save_subphases_without_paths_and_repeat_does_not() {
    let _fixture = FIXTURE.lock().unwrap();
    let source = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    let track = fixture(source.path());
    let original = std::fs::read(&track.source_path).unwrap();
    let (report, log) = capture(|| export(usb.path(), &track, &mut |_| {}));
    let report = report.unwrap();
    assert!(report.verified);
    assert!(
        log.contains("USB sync completed"),
        "subscriber control sees existing completion record"
    );
    let records = phases(&log);
    assert_eq!(
        records.len(),
        10,
        "one begin/end pair for each save subphase: {records:?}"
    );
    let expected_phases = [
        "staged_verify",
        "publication",
        "published_verify",
        "receipt_seal",
        "finalization",
    ];
    for (pair, phase) in records.chunks_exact(2).zip(expected_phases) {
        assert_eq!(pair[0]["phase"], phase);
        assert_eq!(pair[1]["phase"], phase);
        assert_eq!(pair[0]["message"], "USB save phase started");
        assert_eq!(pair[1]["message"], "USB save phase finished");
        assert_eq!(pair[1]["ok"], "true");
    }
    assert_fixed_fields(&records);
    let (repeat, repeated_log) = capture(|| export(usb.path(), &track, &mut |_| {}));
    assert!(repeat.unwrap().up_to_date);
    assert_eq!(phases(&repeated_log), []);
    assert_eq!(std::fs::read(&track.source_path).unwrap(), original);
    assert!(rbl_export::verify(usb.path()).unwrap().is_ok());
}
#[test]
fn publication_failure_records_only_fixed_fields_and_preserves_the_old_generation() {
    let _fixture = FIXTURE.lock().unwrap();
    let source = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    let mut track = fixture(source.path());
    export(usb.path(), &track, &mut |_| {}).unwrap();
    let paths = [
        "PIONEER/rekordbox/export.pdb",
        "PIONEER/rekordbox/exportLibrary.db",
        "PIONEER/rbxport/manifest.json",
    ];
    let before: Vec<_> = paths
        .iter()
        .map(|path| std::fs::read(usb.path().join(path)).unwrap())
        .collect();
    track.rating = 3;
    let injected = std::cell::Cell::new(false);
    let (result, log) = capture(|| {
        export(usb.path(), &track, &mut |progress| {
            if progress.stage == "publishing" {
                let staging = std::fs::read_dir(usb.path())
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .find(|path| {
                        path.file_name()
                            .unwrap()
                            .to_string_lossy()
                            .starts_with(".rbxport-staging-")
                    })
                    .unwrap();
                // Exact existing durable marker write now refuses before commit intent.
                std::fs::create_dir(staging.join(".verification-required")).unwrap();
                injected.set(true);
            }
        })
    });
    assert!(injected.get());
    assert!(result.is_err());
    for (path, bytes) in paths.iter().zip(before) {
        assert_eq!(std::fs::read(usb.path().join(path)).unwrap(), bytes);
    }
    assert!(rbl_export::verify(usb.path()).unwrap().is_ok());
    let records = phases(&log);
    assert_eq!(records.len(), 4, "{records:?}");
    assert_eq!(records[3]["phase"], "publication");
    assert_eq!(records[3]["ok"], "false");
    assert_fixed_fields(&records);
}
#[test]
fn published_verification_failure_is_logged_before_unchanged_rollback() {
    let _fixture = FIXTURE.lock().unwrap();
    let source = tempfile::tempdir().unwrap();
    let usb = tempfile::tempdir().unwrap();
    let mut track = fixture(source.path());
    export(usb.path(), &track, &mut |_| {}).unwrap();
    let paths = [
        "PIONEER/rekordbox/export.pdb",
        "PIONEER/rekordbox/exportLibrary.db",
        "PIONEER/rbxport/manifest.json",
    ];
    let before: Vec<_> = paths
        .iter()
        .map(|path| std::fs::read(usb.path().join(path)).unwrap())
        .collect();
    track.rating = 3;
    let injected = std::cell::Cell::new(false);
    let (result, log) = capture(|| {
        export(usb.path(), &track, &mut |progress| {
            if progress.stage == "publishing" {
                let staging = std::fs::read_dir(usb.path())
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .find(|path| {
                        path.file_name()
                            .unwrap()
                            .to_string_lossy()
                            .starts_with(".rbxport-staging-")
                    })
                    .unwrap();
                // The staged check already passed. A valid SQL edit makes only one
                // database disagree, so the published check must restore both.
                let db = rbl_onelibrary::playlists::open_read_write(
                    &staging.join("PIONEER/rekordbox/exportLibrary.db"),
                )
                .unwrap();
                db.execute(
                    "UPDATE content SET title='PRIVATE_SAVE_PHASE_PATH changed'",
                    [],
                )
                .unwrap();
                injected.set(true);
            }
        })
    });
    assert!(injected.get());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("the previous library was restored"));
    for (path, bytes) in paths.iter().zip(before) {
        assert_eq!(std::fs::read(usb.path().join(path)).unwrap(), bytes);
    }
    assert!(rbl_export::verify(usb.path()).unwrap().is_ok());
    let records = phases(&log);
    assert_eq!(records.len(), 6, "{records:?}");
    assert_eq!(records[3]["ok"], "true");
    assert_eq!(records[5]["phase"], "published_verify");
    assert_eq!(records[5]["ok"], "false");
    assert_fixed_fields(&records);
}
