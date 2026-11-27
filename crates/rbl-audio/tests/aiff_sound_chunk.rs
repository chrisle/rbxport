//! An AIFF sound chunk contains eight bytes of control data before the PCM.
//! A later metadata chunk must not increase the length or appear as samples,
//! however many chunks come before the sound chunk, and on every route that
//! decodes the file: analysis, export conversion, and loop export.

#![allow(clippy::unwrap_used, clippy::panic, clippy::cast_possible_truncation)]

use std::path::Path;

use rbl_audio::compatibility::{Format, convert, inspect};
use rbl_audio::{MonoStream, write_range_wav};

const PCM: [[u8; 3]; 5] = [
    [0x20, 0, 0],
    [0xC0, 0, 0],
    [0, 0, 0],
    [0x40, 0, 0],
    [0xE0, 0, 0],
];
const SAMPLES: [f32; 5] = [0.25, -0.5, 0.0, 0.5, -0.25];

fn chunk(file: &mut Vec<u8>, name: [u8; 4], data: &[u8]) {
    file.extend(name);
    file.extend(u32::try_from(data.len()).unwrap().to_be_bytes());
    file.extend(data);
    if data.len() % 2 != 0 {
        file.push(0);
    }
}

fn generated_aiff(frames: usize, compression: Option<[u8; 4]>) -> Vec<u8> {
    let width = if compression.is_none() { 2 } else { 3 };
    let mut comm = 1_u16.to_be_bytes().to_vec();
    comm.extend(u32::try_from(frames).unwrap().to_be_bytes());
    comm.extend(u16::try_from(width * 8).unwrap().to_be_bytes());
    comm.extend([0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0]);

    let mut body = if let Some(code) = compression {
        comm.extend(code);
        comm.extend([0, 0]);
        let mut form = b"AIFC".to_vec();
        chunk(&mut form, *b"FVER", &0xA280_5140_u32.to_be_bytes());
        form
    } else {
        b"AIFF".to_vec()
    };
    chunk(&mut body, *b"JUNK", &[0x5A]); // Exercise odd-size chunk padding before COMM.
    chunk(&mut body, *b"COMM", &comm);
    let mut sound = vec![0; 8];
    for sample in &PCM[..frames] {
        sound.extend(&sample[..width]);
    }
    chunk(&mut body, *b"SSND", &sound);
    chunk(&mut body, *b"JUNK", &[0x11, 0x22, 0x33, 0x44]);

    let mut file = b"FORM".to_vec();
    file.extend(u32::try_from(body.len()).unwrap().to_be_bytes());
    file.extend(body);
    file
}

/// What the stream declares for `path`, then everything it decodes from it.
fn stream_pcm(path: &Path) -> (Option<u64>, Vec<f32>) {
    let mut stream = MonoStream::open(path).unwrap();
    let declared = stream.expected_frames();
    let mut decoded = Vec::new();
    while let Some(packet) = stream.next_chunk() {
        decoded.extend_from_slice(packet);
    }
    (declared, decoded)
}

fn check_exact_pcm(frames: usize, compression: Option<[u8; 4]>) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("generated.aif");
    let original = generated_aiff(frames, compression);
    std::fs::write(&path, &original).unwrap();
    let (total, decoded) = stream_pcm(&path);
    assert_eq!(std::fs::read(path).unwrap(), original);
    assert_eq!(
        total,
        Some(u64::try_from(frames).unwrap()),
        "declared PCM length"
    );
    assert_eq!(
        decoded,
        SAMPLES[..frames],
        "tail metadata must not be decoded"
    );
}

#[test]
fn plain_aiff_uses_only_the_pcm_after_the_sound_chunk_prefix() {
    check_exact_pcm(1, None);
    check_exact_pcm(5, None);
}

#[test]
fn aliased_aiff_c_uses_only_the_pcm_after_the_sound_chunk_prefix() {
    check_exact_pcm(1, Some(*b"in24"));
    check_exact_pcm(5, Some(*b"in24"));
}

#[test]
fn wav_conversion_excludes_metadata_after_the_aiff_sound_chunk() {
    let dir = tempfile::tempdir().unwrap();
    let frames = 5;
    let expected = Some(u64::try_from(frames).unwrap());
    let mut observed = Vec::new();
    let mut converted_samples = Vec::new();
    for (index, compression) in [None, Some(*b"in24")].into_iter().enumerate() {
        let source = dir.path().join(format!("source-{index}.aif"));
        let output = dir.path().join(format!("converted-{index}.wav"));
        std::fs::write(&source, generated_aiff(frames, compression)).unwrap();
        convert(&source, &output, Format::Wav).unwrap();
        let declared = inspect(&source).unwrap().frames;
        let converted = inspect(&output).unwrap().frames;
        observed.push((declared, converted));
        converted_samples.push(rbl_audio::decode_mono(&output, None).unwrap().samples);
    }
    assert_eq!(
        observed,
        [(expected, expected); 2],
        "source and converted PCM frame counts"
    );
    for samples in converted_samples {
        assert_eq!(samples.len(), frames);
        for (converted, expected) in samples.into_iter().zip(SAMPLES) {
            assert!((converted - expected).abs() <= 1.0 / 32768.0);
        }
    }
}

/// A header walk that gave up after a fixed number of chunks left a file
/// with more of them before its sound chunk (`APPL` chunks, as some editors
/// write) shown as it is: nine frames for five, the last four the header of
/// the chunk after the samples. The walk is bounded by the FORM, not a count.
#[test]
fn many_chunks_before_the_sound_chunk_are_walked_to_it() {
    let dir = tempfile::tempdir().unwrap();
    for (index, compression) in [None, Some(*b"in24")].into_iter().enumerate() {
        let path = dir.path().join(format!("many-chunks-{index}.aif"));
        let mut original = generated_aiff(5, compression);
        let sound = original.windows(4).position(|word| word == b"SSND").unwrap();
        let mut extra = Vec::new();
        for _ in 0..65 {
            chunk(&mut extra, *b"APPL", b"TEST");
        }
        original.splice(sound..sound, extra);
        let form_len = u32::try_from(original.len() - 8).unwrap();
        original[4..8].copy_from_slice(&form_len.to_be_bytes());
        std::fs::write(&path, &original).unwrap();
        assert_eq!(stream_pcm(&path), (Some(5), SAMPLES.to_vec()), "{compression:?}");
    }
}

/// Export Loop As WAV reads the file through the same view. A range running
/// past the end of a plain AIFF ends with its five frames, not nine with the
/// header of the chunk after them; an aliased AIFF-C is exported, not refused.
#[test]
fn a_loop_exported_past_the_end_holds_only_the_pcm() {
    let dir = tempfile::tempdir().unwrap();
    for (index, compression) in [None, Some(*b"in24")].into_iter().enumerate() {
        let source = dir.path().join(format!("source-{index}.aif"));
        let out = dir.path().join(format!("loop-{index}.wav"));
        std::fs::write(&source, generated_aiff(5, compression)).unwrap();
        // From the start to a second in: well past the five frames.
        let written = write_range_wav(&source, 0.0, 1.0, &out).unwrap_or_else(|e| panic!("{compression:?}: {e}"));
        let samples = rbl_audio::decode_mono(&out, None).unwrap().samples;
        assert_eq!((written, inspect(&out).unwrap().frames, samples.len()), (5, Some(5), 5), "{compression:?}");
        // As the export quantises them: 16-bit, truncated toward zero.
        let expected: Vec<f32> = SAMPLES.iter().map(|s| f32::from((s * 32767.0) as i16) / 32768.0).collect();
        assert_eq!(samples, expected, "{compression:?}");
    }
}
