use super::*;
use tauri::Manager;

#[test]
fn copied_import_controls_matched_and_unusable_sources() {
    let rig = rig();
    let app = tauri::test::mock_app();
    app.manage(Arc::new(state_over(&rig)));
    let output = tempfile::tempdir().unwrap();
    let one = device::read(rig.stick.path(), Format::OneLibrary).unwrap();
    let id = one
        .nodes
        .iter()
        .find(|n| n.name == "Friday")
        .unwrap()
        .id
        .to_string();
    let before = contents_under(rig.stick.path());
    let opens = app.state::<Arc<AppState>>().writer_opens();
    let zero = tauri::async_runtime::block_on(device_playlist_import(
        app.handle().clone(),
        app.state(),
        rig.stick.path().to_string_lossy().into_owned(),
        "oneLibrary".into(),
        id.clone(),
        MissingTracks::Copy,
        Some(output.path().to_string_lossy().into_owned()),
    ))
    .unwrap();
    assert_eq!((zero.tracks, zero.copied, zero.analysed), (2, 0, 0));
    assert_eq!(app.state::<Arc<AppState>>().writer_opens() - opens, 1);
    assert!(files_under(output.path()).is_empty());
    assert_eq!(contents_under(rig.stick.path()), before);

    // Separate generated state: a device names a source it cannot supply.
    rig.came_from_elsewhere(0);
    rig.came_from_elsewhere(1);
    let source = rig.sticks_file(0);
    let held = rig._src.path().join("held-export.wav");
    std::fs::rename(&source, &held).unwrap();
    let before = contents_under(rig.stick.path());
    let collection = contents_under(rig.dir.path());
    let opens = app.state::<Arc<AppState>>().writer_opens();
    let refused = tauri::async_runtime::block_on(device_playlist_import(
        app.handle().clone(),
        app.state(),
        rig.stick.path().to_string_lossy().into_owned(),
        "oneLibrary".into(),
        id,
        MissingTracks::Copy,
        Some(output.path().to_string_lossy().into_owned()),
    ));
    assert!(refused.is_err());
    assert_eq!(app.state::<Arc<AppState>>().writer_opens(), opens);
    assert!(files_under(output.path()).is_empty());
    assert_eq!(contents_under(rig.stick.path()), before);
    assert_eq!(contents_under(rig.dir.path()), collection);
}

#[test]
fn copied_import_prefix_row_refusal_keeps_success_and_continues_no_dat() {
    for last_without_dat in [false, true] {
        let rig = rig_with(|_, track| analysed_on_the_stick(0, track));
        let one_path = rig.stick.path().join("PIONEER/rekordbox/exportLibrary.db");
        let one = device::read(rig.stick.path(), Format::OneLibrary).unwrap();
        let friday = one.nodes.iter().find(|n| n.name == "Friday").unwrap();
        let rows: Vec<_> = (0..3)
            .map(|i| {
                one.tracks
                    .iter()
                    .find(|t| {
                        t.master_content_id == rbl_db::fixture::track_id(i).parse::<u64>().unwrap()
                    })
                    .unwrap()
            })
            .collect();
        rbl_onelibrary::playlists::apply(
            &one_path,
            &rbl_onelibrary::playlists::Change {
                contents: vec![(
                    i64::from(friday.id),
                    rows.iter().map(|t| i64::from(t.id)).collect(),
                )],
                ..Default::default()
            },
        )
        .unwrap();
        for i in 0..3 {
            rig.came_from_elsewhere(i);
        }
        if last_without_dat {
            for extension in ["DAT", "EXT", "2EX"] {
                let path =
                    audio_path(rig.stick.path(), &rows[2].analysis_path).with_extension(extension);
                if path.exists() {
                    std::fs::remove_file(path).unwrap();
                }
            }
        }
        let writer = rig.writer();
        writer.library().connection().execute_batch("CREATE TRIGGER probe_794_middle BEFORE UPDATE OF AnalysisDataPath ON djmdContent WHEN NEW.FileNameL = 't1.wav' BEGIN SELECT RAISE(ABORT,'generated row refusal'); END;").unwrap();
        drop(writer);
        let app = tauri::test::mock_app();
        app.manage(Arc::new(state_over(&rig)));
        let output = tempfile::tempdir().unwrap();
        let before = contents_under(rig.stick.path());
        let imported = tauri::async_runtime::block_on(device_playlist_import(
            app.handle().clone(),
            app.state(),
            rig.stick.path().to_string_lossy().into_owned(),
            "oneLibrary".into(),
            friday.id.to_string(),
            MissingTracks::Copy,
            Some(output.path().to_string_lossy().into_owned()),
        ))
        .unwrap();
        assert_eq!(
            (imported.copied, imported.analysed, imported.analysis_failed),
            (3, 2, 1)
        );
        assert_eq!(rig.entries(imported.id.as_ref().unwrap()).len(), 3);
        let db = rbl_db::Library::open(rig.location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
        let mut stmt = db.connection().prepare("SELECT FileNameL,COALESCE(AnalysisDataPath,''),COALESCE(BPM,0),(SELECT COUNT(*) FROM djmdCue WHERE ContentID=djmdContent.ID AND rb_local_deleted=0) FROM djmdContent WHERE FileNameL IN ('t0.wav','t1.wav','t2.wav') ORDER BY FileNameL").unwrap();
        let rows: Vec<(String, String, u32, u32)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(rows.len(), 3);
        assert!(!rows[0].1.is_empty());
        assert_eq!((rows[0].2, rows[0].3), (12_800, 2));
        assert_eq!((rows[1].1.as_str(), rows[1].2, rows[1].3), ("", 0, 0));
        if last_without_dat {
            assert_eq!((rows[2].1.as_str(), rows[2].2, rows[2].3), ("", 12_800, 0));
        } else {
            assert!(!rows[2].1.is_empty());
            assert_eq!((rows[2].2, rows[2].3), (12_800, 2));
        }
        assert!(files_under(&rig.dir.path().join("backups/analysis-journal")).is_empty());
        assert_eq!(contents_under(rig.stick.path()), before);
    }
}

#[test]
fn copied_import_primary_association_and_source_revalidation() {
    for change_source in [false, true] {
        let rig = rig_with(analysed_on_the_stick);
        rig.came_from_elsewhere(0);
        let state = Arc::new(state_over(&rig));
        let plan = rig.plan(Format::OneLibrary, "Friday");
        let output = tempfile::tempdir().unwrap();
        let (entries, _copies, carried) =
            copy_missing(&plan, &plan.copies(), output.path()).unwrap();
        let made = state
            .write(|w| w.import_device_playlist_entries(&plan.name, &entries))
            .unwrap();
        let (content, copy) = &made.added[0];
        let relative = state
            .write(|w| w.analysis_data_path_for(content))
            .unwrap()
            .replace(".DAT", ".dAt");
        state
            .write(|w| {
                w.library().connection().execute(
                    "UPDATE djmdContent SET AnalysisDataPath=?1 WHERE ID=?2",
                    [&relative, content],
                )?;
                Ok(())
            })
            .unwrap();
        let target = rbl_anlz::resolve(&rig.location.share_root, &relative);
        let mut existing = rbl_anlz::write::AnlzBuilder::new();
        existing.path("?/old.wav");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, existing.finish()).unwrap();
        let distinct_case = !target.with_extension("DAT").exists();
        let stick = &carried.iter().find(|(path, _)| path == copy).unwrap().1;
        let item = prepare_stick_analysis(rig.stick.path(), content, copy, stick).unwrap();
        seed_redo(&state);
        let before = contents_under(rig.dir.path());
        if change_source {
            let dat = audio_path(rig.stick.path(), &stick.analysis);
            let mut changed = rbl_anlz::write::AnlzBuilder::new();
            changed.path("/generated/replaced.wav");
            std::fs::write(dat, changed.finish()).unwrap();
        }
        let mut counts = (0, 0);
        apply_stick_analysis_batch(&state, rig.stick.path(), vec![item], &mut counts);
        if change_source {
            assert_eq!(counts, (0, 1));
            assert!(crate::commands::current_history(&state).can_redo);
            assert_eq!(contents_under(rig.dir.path()), before);
        } else {
            assert_eq!(counts, (1, 0));
            assert!(!crate::commands::current_history(&state).can_redo);
            assert_eq!(
                rbl_anlz::Anlz::read(&target).unwrap().path().as_deref(),
                Some(rbl_anlz::share_tree_ppth(&copy.to_string_lossy()).as_str())
            );
            if distinct_case {
                assert!(!target.with_extension("DAT").exists());
            }
            let path: String = state
                .read_db(|db| {
                    Ok(db.connection().query_row(
                        "SELECT AnalysisDataPath FROM djmdContent WHERE ID=?1",
                        [content],
                        |r| r.get(0),
                    )?)
                })
                .unwrap();
            assert_eq!(path, relative);
        }
    }
}

fn seed_redo(state: &Arc<AppState>) {
    let app = tauri::test::mock_app();
    app.manage(Arc::clone(state));
    tauri::async_runtime::block_on(crate::commands::rename_playlist(
        app.handle().clone(),
        app.state(),
        rbl_db::fixture::playlist_id(0),
        "Generated undo".into(),
    ))
    .unwrap();
    let undone = tauri::async_runtime::block_on(crate::commands::undo_edit(
        app.handle().clone(),
        app.state(),
    ))
    .unwrap();
    assert!(undone.can_redo);
}

#[test]
fn copied_import_crosses_batch_boundary_and_ends_redo_only_after_writes() {
    let rig = rig_with(analysed_on_the_stick);
    let state = Arc::new(state_over(&rig));
    let plan = rig.plan(Format::OneLibrary, "Friday");
    let stick = match &plan.entries[0] {
        PlanEntry::Matched(_) => {
            let one = device::read(rig.stick.path(), Format::OneLibrary).unwrap();
            let t = one
                .tracks
                .iter()
                .find(|t| {
                    t.master_content_id == rbl_db::fixture::track_id(0).parse::<u64>().unwrap()
                })
                .unwrap();
            MissingFile {
                source: rig.sticks_file(0),
                name: "t0.wav".into(),
                artist: "TRIODE".into(),
                album: String::new(),
                bytes: 0,
                bpm_x100: t.bpm_x100,
                key: t.key.clone(),
                analysis: t.analysis_path.clone(),
            }
        }
        PlanEntry::Missing(_) => panic!("generated source is matched"),
    };
    let copies = tempfile::tempdir().unwrap();
    let entries: Vec<_> = (0..33)
        .map(|i| {
            let file = copies.path().join(format!("copy-{i}.wav"));
            std::fs::write(&file, wav(4_410 + i)).unwrap();
            rbl_db::write::DeviceEntry::File(file)
        })
        .collect();
    let made = state
        .write(|w| w.import_device_playlist_entries("Boundary33", &entries))
        .unwrap();
    let added = made.added;
    let carried: Vec<_> = added
        .iter()
        .map(|(_, path)| (path.clone(), stick.clone()))
        .collect();
    seed_redo(&state);
    let before = contents_under(rig.stick.path());
    let opens = state.writer_opens();
    assert_eq!(
        carry_all(&state, rig.stick.path(), &added, &carried),
        (33, 0)
    );
    assert_eq!(state.writer_opens() - opens, 2);
    assert!(!crate::commands::current_history(&state).can_redo);
    assert_eq!(contents_under(rig.stick.path()), before);
    let db = rbl_db::Library::open(rig.location.clone(), rbl_db::OpenMode::ReadOnly).unwrap();
    for (id, _) in &added {
        let cues: u32 = db
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM djmdCue WHERE ContentID=?1 AND rb_local_deleted=0",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cues, 2);
    }
    assert!(files_under(&rig.dir.path().join("backups/analysis-journal")).is_empty());
}

#[cfg(unix)]
#[test]
fn copied_import_allowed_mount_alias_remains_readable() {
    let rig = rig_with(analysed_on_the_stick);
    rig.came_from_elsewhere(0);
    let state = state_over(&rig);
    let plan = rig.plan(Format::OneLibrary, "Friday");
    let output = tempfile::tempdir().unwrap();
    let (entries, _copies, carried) = copy_missing(&plan, &plan.copies(), output.path()).unwrap();
    let made = state
        .write(|w| w.import_device_playlist_entries(&plan.name, &entries))
        .unwrap();
    let source_dat = audio_path(rig.stick.path(), &carried[0].1.analysis);
    let old_time = std::time::UNIX_EPOCH - std::time::Duration::from_secs(1);
    std::fs::File::options()
        .write(true)
        .open(source_dat)
        .unwrap()
        .set_modified(old_time)
        .unwrap();
    let alias_root = tempfile::tempdir().unwrap();
    let alias = alias_root.path().join("mount-alias");
    std::os::unix::fs::symlink(rig.stick.path(), &alias).unwrap();
    let before = contents_under(rig.stick.path());
    assert_eq!(carry_all(&state, &alias, &made.added, &carried), (1, 0));
    assert_eq!(contents_under(rig.stick.path()), before);
}

#[test]
fn copied_import_unavailable_optional_file_properties_are_not_a_refusal() {
    let weak = ImportFileRevision {
        size: 4,
        modified: None,
        identity: None,
        changed: None,
    };
    let known = ImportFileRevision {
        size: 4,
        modified: Some(std::time::UNIX_EPOCH - std::time::Duration::from_secs(1)),
        identity: None,
        changed: Some((1, 2)),
    };
    assert!(weak.matches(&known));
    assert!(known.matches(&weak));
    assert!(!ImportFileRevision { size: 5, ..weak }.matches(&known));
}

#[test]
fn copied_import_flushes_source_images_at_the_byte_boundary() {
    let rig = rig_with(|_, track| analysed_on_the_stick(0, track));
    for i in 0..2 {
        rig.came_from_elsewhere(i);
    }
    let one = device::read(rig.stick.path(), Format::OneLibrary).unwrap();
    for row in one.tracks.iter().filter(|t| {
        t.master_content_id == rbl_db::fixture::track_id(0).parse::<u64>().unwrap()
            || t.master_content_id == rbl_db::fixture::track_id(1).parse::<u64>().unwrap()
    }) {
        let path = audio_path(rig.stick.path(), &row.analysis_path);
        let mut dat = rbl_anlz::write::AnlzBuilder::new();
        dat.path(&row.path)
            .waveform_preview(b"PWAV", &vec![7; DEVICE_IMPORT_BATCH_BYTES / 2]);
        std::fs::write(path, dat.finish()).unwrap();
    }
    let state = state_over(&rig);
    let plan = rig.plan(Format::OneLibrary, "Friday");
    let output = tempfile::tempdir().unwrap();
    let (entries, _copies, carried) = copy_missing(&plan, &plan.copies(), output.path()).unwrap();
    let made = state
        .write(|w| w.import_device_playlist_entries(&plan.name, &entries))
        .unwrap();
    let before = contents_under(rig.stick.path());
    let opens = state.writer_opens();
    assert_eq!(
        carry_all(&state, rig.stick.path(), &made.added, &carried),
        (2, 0)
    );
    assert_eq!(
        state.writer_opens() - opens,
        2,
        "two source images exceed one byte-bounded batch"
    );
    assert_eq!(contents_under(rig.stick.path()), before);
    assert!(files_under(&rig.dir.path().join("backups/analysis-journal")).is_empty());
}
