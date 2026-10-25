//! Issue #527: AIFF-C files that label plain PCM with a compression type
//! symphonia 0.5 refuses ("aifc: Compression type not implemented").
#![allow(clippy::unwrap_used, clippy::panic, clippy::cast_possible_truncation, clippy::cast_precision_loss)]

use std::path::Path;

use rbl_audio::compatibility::{convert, inspect, Format};

const RATE: u32 = 44_100;
const FRAMES: usize = 4_410;

fn sample(i: usize) -> f64 {
    (i as f64 * 2.0 * std::f64::consts::PI * 440.0 / f64::from(RATE)).sin() * 0.5
}

/// Encodes one sample as the file stores it.
type Encode = fn(f64) -> Vec<u8>;

/// A mono 44.1 kHz AIFF-C tone with compression type `code`, samples
/// written by `encode`.
fn aifc(path: &Path, code: [u8; 4], bits: u16, encode: Encode) {
    let data: Vec<u8> = (0..FRAMES).flat_map(|i| encode(sample(i))).collect();
    let mut comm = Vec::new();
    comm.extend_from_slice(&1_u16.to_be_bytes());
    comm.extend_from_slice(&u32::try_from(FRAMES).unwrap().to_be_bytes());
    comm.extend_from_slice(&bits.to_be_bytes());
    // 44100 as an 80-bit extended float.
    comm.extend_from_slice(&[0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0]);
    comm.extend_from_slice(&code);
    comm.extend_from_slice(&[0, 0]);
    let mut body = b"AIFC".to_vec();
    body.extend_from_slice(b"FVER");
    body.extend_from_slice(&4_u32.to_be_bytes());
    body.extend_from_slice(&0xA280_5140_u32.to_be_bytes());
    body.extend_from_slice(b"COMM");
    body.extend_from_slice(&u32::try_from(comm.len()).unwrap().to_be_bytes());
    body.extend_from_slice(&comm);
    body.extend_from_slice(b"SSND");
    body.extend_from_slice(&u32::try_from(data.len() + 8).unwrap().to_be_bytes());
    body.extend_from_slice(&[0; 8]);
    body.extend_from_slice(&data);
    let mut file = b"FORM".to_vec();
    file.extend_from_slice(&u32::try_from(body.len()).unwrap().to_be_bytes());
    file.extend_from_slice(&body);
    std::fs::write(path, file).unwrap();
}

fn in24(v: f64) -> Vec<u8> {
    let s = (v * 8_388_607.0) as i32;
    s.to_be_bytes()[1..].to_vec()
}

fn fl32(v: f64) -> Vec<u8> {
    (v as f32).to_be_bytes().to_vec()
}

fn in32(v: f64) -> Vec<u8> {
    ((v * 2_147_483_647.0) as i32).to_be_bytes().to_vec()
}

fn fl64(v: f64) -> Vec<u8> {
    v.to_be_bytes().to_vec()
}

fn in16(v: f64) -> Vec<u8> {
    ((v * 32767.0) as i16).to_be_bytes().to_vec()
}

#[test]
fn aliased_pcm_aifc_decodes_to_the_written_samples() {
    let dir = tempfile::tempdir().unwrap();
    let cases: [([u8; 4], u16, Encode); 4] =
        [(*b"in24", 24, in24), (*b"in32", 32, in32), (*b"FL32", 32, fl32), (*b"FL64", 64, fl64)];
    for (code, bits, encode) in cases {
        let name = format!("issue-527-{}.aif", String::from_utf8_lossy(&code));
        let path = dir.path().join(&name);
        aifc(&path, code, bits, encode);

        let audio = rbl_audio::decode_mono(&path, None).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(audio.sample_rate, RATE, "{name}");
        assert_eq!(audio.samples.len(), FRAMES, "{name}");
        for (i, got) in audio.samples.iter().enumerate() {
            assert!((f64::from(*got) - sample(i)).abs() < 1e-4, "{name} sample {i}: {got}");
        }
    }
}

#[test]
fn mismatched_alias_width_is_rejected_before_decode_or_export() {
    let dir = tempfile::tempdir().unwrap();
    let cases: [([u8; 4], u16, u16, Encode); 4] = [
        (*b"in24", 24, 16, in16),
        (*b"in32", 32, 16, in16),
        (*b"FL32", 32, 64, fl64),
        (*b"FL64", 64, 32, fl32),
    ];
    for (code, expected_bits, bits, encode) in cases {
        let label = String::from_utf8_lossy(&code);
        let source = dir.path().join(format!("issue-527-{label}-{bits}.aif"));
        aifc(&source, code, bits, encode);
        let original = std::fs::read(&source).unwrap();

        let decode_error = rbl_audio::decode_mono(&source, None).unwrap_err().to_string();
        assert!(decode_error.contains(label.as_ref()) && decode_error.contains(&bits.to_string()), "{decode_error}");
        assert!(decode_error.contains(&format!("requires {expected_bits}-bit")), "{decode_error}");

        let inspect_error = inspect(&source).unwrap_err().to_string();
        assert!(inspect_error.contains(label.as_ref()) && inspect_error.contains(&bits.to_string()), "{inspect_error}");

        let out = dir.path().join(format!("issue-527-{label}-{bits}-out.aiff"));
        let export_error = convert(&source, &out, Format::Aiff).unwrap_err().to_string();
        assert!(export_error.contains(label.as_ref()) && export_error.contains(&bits.to_string()), "{export_error}");
        assert!(!out.exists(), "invalid input must not create an export");
        assert_eq!(std::fs::read(&source).unwrap(), original, "source is read only");
    }
}

#[test]
fn aliased_aifc_is_converted_for_export_not_copied() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("issue-527-in24.aif");
    aifc(&source, *b"in24", 24, in24);

    let properties = inspect(&source).unwrap();
    assert!(!properties.compatible, "a player reads 'in24' itself");
    assert_eq!(properties.bit_depth, Some(24));

    let out = dir.path().join("issue-527-out.aiff");
    convert(&source, &out, Format::Aiff).unwrap();
    assert!(inspect(&out).unwrap().compatible);
}

#[test]
fn a_compression_type_still_refused_is_named_in_the_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("issue-527-ima4.aif");
    aifc(&path, *b"ima4", 16, |v| ((v * 32767.0) as i16).to_be_bytes().to_vec());

    let error = rbl_audio::decode_mono(&path, None).unwrap_err().to_string();
    assert!(error.contains("Compression type not implemented"), "{error}");
    assert!(error.contains("'ima4'"), "{error}");
}
