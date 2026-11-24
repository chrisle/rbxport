//! Existing revision-5 outputs must gain the missing default cue RGB.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use rbl_export::{export, incremental, Manifest, SourceTrack};

fn colours(file: &rbl_anlz::Anlz) -> Vec<[u8; 3]> {
    let hot=file.sections.iter().find(|section|section.tag==rbl_core::FourCc::new(b"PCO2") && section.header.get(..4)==Some(1_u32.to_be_bytes().as_slice())).unwrap();
    hot.payload.chunks_exact(88).map(|entry|entry[45..48].try_into().unwrap()).collect()
}

#[test]
fn revision_five_defaults_refresh_without_copying_audio_or_changing_the_source() {
    upgrade_old_receipt(false);
}

#[test]
fn historical_transform_five_layout_three_refreshes_metadata_and_default_rgb() {
    upgrade_old_receipt(true);
}

fn upgrade_old_receipt(old_layout: bool) {
    let source=tempfile::tempdir().unwrap();let dest=tempfile::tempdir().unwrap();
    let audio=source.path().join("track.mp3");std::fs::write(&audio,[1_u8;1024]).unwrap();
    let original=rbl_anlz::AnlzBuilder::new().path("/track.mp3").finish();
    let dat=source.path().join("library.DAT");let ext=source.path().join("library.EXT");
    std::fs::write(&dat,&original).unwrap();std::fs::write(&ext,&original).unwrap();
    let cues=[5_u8,6,7,8,13,14,15,16].into_iter().map(|slot|rbl_anlz::cues::ExportCue {
        kind:slot+1,time_ms:u32::from(slot)*1000,..Default::default()
    }).collect();
    let mut track=SourceTrack{id:1,source_path:audio.clone(),title:"Default colours".into(),analysis_source:Some(dat.clone()),cues:Some(cues),..Default::default()};
    track.metadata.analysed=0xe9;
    export(dest.path(),std::slice::from_ref(&track),&[]).unwrap();
    let mut older=Manifest::load(dest.path()).unwrap();
    let database=dest.path().join("PIONEER/rekordbox/exportLibrary.db");
    if old_layout {
        let db=rbl_onelibrary::playlists::open_read_write(&database).unwrap();
        db.execute_batch("UPDATE property SET dbVersion='1000'; UPDATE content SET analysedBits=169").unwrap();
    }
    let guarded=[audio.clone(),dat.clone(),ext.clone(),dest.path().join(older.tracks[0].audio.trim_start_matches('/'))];
    let before:Vec<_>=guarded.iter().map(|path|(std::fs::read(path).unwrap(),std::fs::metadata(path).unwrap().modified().unwrap())).collect();
    // Reconstruct the actual transform-5 recipe/request tuples instead of
    // relabelling a current recipe. Layout 4 isolates RGB; layout 3 combines
    // the historical property/analysis-flags metadata with the old RGB.
    let options=rbl_export::ExportOptions::default();
    let lists:&[rbl_export::SourcePlaylist]=&[];
    let tags:&[rbl_export::SourceMyTag]=&[];
    let request=incremental::digest(&(std::slice::from_ref(&track),lists,tags,
        options.compatibility,options.keep_unlisted,format!("{:?}",options.sync),
        options.db_id_override,options.my_tag_master_db_id_override,format!("{:?}",options.defaults),
        5_u32,if old_layout {3_u32} else {4_u32})).unwrap();
    let revisions=track.analysis_source.as_ref().map(|base|["DAT","EXT","2EX"].into_iter()
        .map(|extension|(extension,incremental::revision(&base.with_extension(extension)).unwrap())).collect::<Vec<_>>());
    let audio_revision=incremental::revision(&track.source_path).unwrap();
    let recipe_for=|version|incremental::digest(&(older.tracks[0].audio.as_str(),&track.cues,
        &track.analysis,&track.analysis_source,&revisions,&audio_revision,version)).unwrap();
    let current_recipe=recipe_for(incremental::TRANSFORM_VERSION);
    let generation=older.generation.as_ref().unwrap();
    assert_eq!(generation.request,incremental::request(std::slice::from_ref(&track),lists,tags,&options).unwrap());
    assert_eq!(generation.analysis_recipes.len(),1);
    assert_eq!(generation.analysis_recipes.values().next().unwrap(),&current_recipe);
    let historical_recipe=recipe_for(5_u32);

    let folder=dest.path().join(older.tracks[0].anlz_dir.trim_start_matches('/'));
    let saved=folder.join("ANLZ0000.EXT");let mut stale=rbl_anlz::Anlz::read(&saved).unwrap();
    for section in &mut stale.sections {
        if section.tag==rbl_core::FourCc::new(b"PCO2") {
            for entry in section.payload.chunks_exact_mut(88) {entry[45..48].fill(0);}
        }
    }
    std::fs::write(&saved,stale.to_bytes()).unwrap();
    let analysis:Vec<_>=["DAT","EXT"].into_iter().map(|extension|(extension.to_owned(),std::fs::read(folder.join(format!("ANLZ0000.{extension}"))).unwrap())).collect();
    older.tracks[0].analysis=rbl_export::snapshot::analysis_hash(&analysis);
    older.tracks[0].analysis_hashes=analysis.iter().map(|(extension,bytes)|(extension.clone(),rbl_export::manifest::hash(bytes))).collect();
    let outputs=incremental::output_revisions(dest.path(),&older.tracks).unwrap();
    let structure=incremental::manifest_structure(&older).unwrap();
    let generation=older.generation.as_mut().unwrap();
    generation.transform_version=5;generation.outputs=outputs;generation.structure=structure;
    generation.request=request;
    *generation.analysis_recipes.values_mut().next().unwrap()=historical_recipe;
    generation.databases=incremental::databases(dest.path()).unwrap();
    older.save(dest.path()).unwrap();
    let refreshed=export(dest.path(),std::slice::from_ref(&track),&[]).unwrap();
    assert_eq!(colours(&rbl_anlz::Anlz::read(&saved).unwrap()),[[0,255,48],[255,94,0],[0,0,255],[255,232,0],[0,255,48],[255,94,0],[0,0,255],[255,232,0]]);
    assert_eq!(refreshed.bytes_copied,0);
    assert!(refreshed.metrics.analysis_bytes_read>0);
    assert_eq!(std::fs::read(&dat).unwrap(),original);assert_eq!(std::fs::read(&ext).unwrap(),original);
    assert_eq!(std::fs::read(&audio).unwrap(),[1_u8;1024]);
    assert_eq!(Manifest::load(dest.path()).unwrap().generation.unwrap().transform_version,incremental::TRANSFORM_VERSION);
    {
        let db=rbl_onelibrary::ExportLibrary::open_read_only(&database).unwrap();
        let version:String=db.connection().query_row("SELECT dbVersion FROM property",[],|row|row.get(0)).unwrap();
        let flags:u32=db.connection().query_row("SELECT analysedBits FROM content",[],|row|row.get(0)).unwrap();
        assert_eq!((version.as_str(),flags),("10000",41));
    }
    let again=export(dest.path(),&[track],&[]).unwrap();
    assert!(again.up_to_date);
    for (path,(bytes,stamp)) in guarded.iter().zip(before) {
        assert_eq!(std::fs::read(path).unwrap(),bytes);
        assert_eq!(std::fs::metadata(path).unwrap().modified().unwrap(),stamp);
    }
    assert_eq!((again.bytes_copied,again.analysis_files),(0,0));
}
