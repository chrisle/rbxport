//! Static-reference save vectors: the payload is opaque and must survive
//! beat-number-only changes, but cannot survive timing/tempo changes.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use rbl_anlz::{parse, Beat};
use rbl_anlz::write::AnlzBuilder;
use rbl_core::FourCc;
fn fixture() -> (rbl_anlz::Anlz, Vec<Beat>) {
    let beats = vec![Beat {beat_number:1,tempo_x100:12000,time_ms:0},Beat {beat_number:2,tempo_x100:12000,time_ms:500}];
    let mut header = vec![0;44];
    for (at,value) in [(4,0x0100_0002_u32),(12,(1<<16)|0x2ee0),(16,0),(20,(2<<16)|0x2ee0),(24,500),(28,2),(32,24503)] { header[at..at+4].copy_from_slice(&value.to_be_bytes()); }
    let mut builder=AnlzBuilder::new();
    builder.raw(FourCc::new(b"PQT2"),header,vec![0x12,0x34,0x56,0x78]);
    (parse(&builder.finish()).unwrap(),beats)
}
#[test]
fn renumbering_preserves_payload_and_updates_checksum() {
    let (file,old)=fixture(); let mut new=old.clone(); new[0].beat_number=4;new[1].beat_number=1;
    let saved=parse(&file.with_extended_grid_edit(&old,&new,0).unwrap()).unwrap();
    let section=saved.section(b"PQT2").unwrap();
    assert_eq!(section.payload,[0x12,0x34,0x56,0x78]);
    assert_eq!(&section.header[32..36],24505_u32.to_be_bytes());
    assert_eq!(&section.header[12..16],((4_u32<<16)|0x2ee0).to_be_bytes());
}
#[test]
fn retiming_or_an_invalid_old_checksum_clears_payload() {
    let (file,old)=fixture();let mut new=old.clone();new[1].time_ms+=1;
    let saved=parse(&file.with_extended_grid_edit(&old,&new,0).unwrap()).unwrap();
    assert_eq!(saved.section(b"PQT2").unwrap().payload, [] as [u8; 0]);
    let mut bad=file.clone();bad.sections[0].header[32]=1;
    let saved=parse(&bad.with_extended_grid_edit(&old,&old,0).unwrap()).unwrap();
    assert_eq!(saved.section(b"PQT2").unwrap().payload, [] as [u8; 0]);
}
