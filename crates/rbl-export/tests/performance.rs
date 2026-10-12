//! Deterministic operation budgets and local timing. Never writes real media.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use rbl_export::{ExportOptions, SourcePlaylist, SourceTrack, SyncSource};

#[test]
#[ignore = "opt-in 200/2000/10000 track benchmark on disposable directories"]
#[allow(clippy::print_stdout, reason = "opt-in benchmark emits machine-readable measurements")]
fn incremental_sync_operation_budgets() {
    for count in [200, 2_000, 10_000] {
        let source = tempfile::tempdir().unwrap(); let usb = tempfile::tempdir().unwrap();
        let mut tracks = Vec::with_capacity(count);
        for i in 0..count {
            let path = source.path().join(format!("{i}.mp3"));
            std::fs::write(&path, vec![i as u8; 8192]).unwrap();
            let analysis = source.path().join(format!("{i}/ANLZ0000.DAT"));
            std::fs::create_dir_all(analysis.parent().unwrap()).unwrap();
            let mut dat = rbl_anlz::AnlzBuilder::new();
            let beats: Vec<_> = (0..360).map(|n| rbl_anlz::Beat { beat_number: n as u16 % 4 + 1, tempo_x100: 12000, time_ms: n * 500 }).collect();
            dat.path("/source.mp3").beat_grid(&beats).waveform_preview(b"PWAV", &vec![12; 1200]);
            std::fs::write(&analysis, dat.finish()).unwrap();
            for ext in ["EXT", "2EX"] { std::fs::write(analysis.with_extension(ext), rbl_anlz::AnlzBuilder::new().path("/source.mp3").finish()).unwrap(); }
            tracks.push(SourceTrack { id: i as u64 + 1, source_path: path, analysis_source: Some(analysis),
                title: format!("Track {i}"), artist: "Fixture".into(), album: format!("Album {}", i / 20), duration_sec: 180, ..Default::default() });
        }
        let mut playlists = vec![SourcePlaylist { id: 1, name: "Set".into(), track_indices: (0..count).collect(), ..Default::default() },
            SourcePlaylist { id: 2, name: "Overlapping".into(), track_indices: (0..count).step_by(2).collect(), ..Default::default() }];
        let options = ExportOptions { sync: Some(&SyncSource { db_id: 123, ..Default::default() }), ..Default::default() };
        let run = |playlists: &[SourcePlaylist]| rbl_export::export_with_options(usb.path(), &tracks, playlists, &[], &options, &mut |_| {}).unwrap();
        let cold = run(&playlists);
        let mut warm = Vec::new();
        for _ in 0..20 {
            let clock = std::time::Instant::now(); let report = run(&playlists); warm.push(clock.elapsed().as_millis());
            assert!(report.up_to_date);
            assert_eq!((report.metrics.audio_bytes_read, report.metrics.analysis_bytes_read, report.bytes_copied, report.metrics.database_builds, report.metrics.probes, report.metrics.conversions), (0, 0, 0, 0, 0, 0));
        }
        playlists[0].track_indices.reverse();
        let clock = std::time::Instant::now(); let changed = run(&playlists); let playlist_ms = clock.elapsed().as_millis();
        assert_eq!((changed.metrics.audio_bytes_read, changed.metrics.analysis_bytes_read, changed.bytes_copied, changed.metrics.conversions), (0, 0, 0, 0));
        assert_eq!(changed.metrics.database_builds, 1);
        let clock = std::time::Instant::now(); let preview = rbl_export::plan::preview(usb.path(), &tracks, &playlists, &[], &options).unwrap(); let preview_ms = clock.elapsed().as_millis();
        assert_eq!((preview.peak_bytes, preview.net_additional_bytes), (0, 0));
        warm.sort_unstable();
        println!("{{\"tracks\":{count},\"cold_ms\":{},\"warm_p95_ms\":{},\"playlist_ms\":{playlist_ms},\"preview_ms\":{preview_ms},\"metrics\":{}}}", cold.elapsed_ms, warm[18], serde_json::to_string(&changed.metrics).unwrap());
    }
}
