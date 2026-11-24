//! Existing revision-5 outputs must gain the missing default cue RGB.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use rbl_export::{export, incremental, Manifest, SourceTrack};

fn colours(file: &rbl_anlz::Anlz) -> Vec<[u8; 3]> {
    let hot=file.sections.iter().find(|section|section.tag==rbl_core::FourCc::new(b"PCO2") && section.header.get(..4)==Some(1_u32.to_be_bytes().as_slice())).unwrap();
    hot.payload.chunks_exact(88).map(|entry|entry[45..48].try_into().unwrap()).collect()
}

#[test]
fn revision_five_defaults_refresh_without_copying_audio_or_changing_the_source() {
    let source=tempfile::tempdir().unwrap();let dest=tempfile::tempdir().unwrap();
    let audio=source.path().join("track.mp3");std::fs::write(&audio,[1_u8;1024]).unwrap();
    let original=rbl_anlz::AnlzBuilder::new().path("/track.mp3").finish();
    let dat=source.path().join("library.DAT");let ext=source.path().join("library.EXT");
    std::fs::write(&dat,&original).unwrap();std::fs::write(&ext,&original).unwrap();
    let cues=[5_u8,6,7,8,13,14,15,16].into_iter().map(|slot|rbl_anlz::cues::ExportCue {
        kind:slot+1,time_ms:u32::from(slot)*1000,..Default::default()
    }).collect();
    let track=SourceTrack{id:1,source_path:audio.clone(),title:"Default colours".into(),analysis_source:Some(dat.clone()),cues:Some(cues),..Default::default()};
    export(dest.path(),std::slice::from_ref(&track),&[]).unwrap();
    let mut older=Manifest::load(dest.path()).unwrap();
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
    older.save(dest.path()).unwrap();
    let refreshed=export(dest.path(),std::slice::from_ref(&track),&[]).unwrap();
    assert_eq!(colours(&rbl_anlz::Anlz::read(&saved).unwrap()),[[0,255,48],[255,94,0],[0,0,255],[255,232,0],[0,255,48],[255,94,0],[0,0,255],[255,232,0]]);
    assert_eq!(refreshed.bytes_copied,0);
    assert!(refreshed.metrics.analysis_bytes_read>0);
    assert_eq!(std::fs::read(&dat).unwrap(),original);assert_eq!(std::fs::read(&ext).unwrap(),original);
    assert_eq!(std::fs::read(&audio).unwrap(),[1_u8;1024]);
    assert_eq!(Manifest::load(dest.path()).unwrap().generation.unwrap().transform_version,incremental::TRANSFORM_VERSION);
    let again=export(dest.path(),&[track],&[]).unwrap();
    assert_eq!((again.bytes_copied,again.analysis_files),(0,0));
}
