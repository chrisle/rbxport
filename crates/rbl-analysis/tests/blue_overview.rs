//! Native LoudViewMaker output for synthetic PCM, independent of our encoder.
#![allow(clippy::pedantic, clippy::unwrap_used, clippy::panic)]
#[path = "rekordbox_waveform/signals.rs"]
mod signals;
use rbl_analysis::wave_creator::WaveCreator;
#[test]
fn blue_previews_match_native_reference() {
    for signal in &signals::SIGNALS {
        let mut creator = WaveCreator::new(signal.rate);
        creator.push(&(signal.frames)(signal.rate));
        let ours = creator.finish();
        let reference = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/rekordbox_waveform")
                .join(format!("{}.blue-preview", signal.name)),
        )
        .unwrap();
        assert_eq!(ours.pwav, reference[..400], "{} PWAV", signal.name);
        assert_eq!(ours.pwv2, reference[400..], "{} PWV2", signal.name);
    }
}

#[test]
fn previews_are_independent_of_decoder_packets() {
    let signal = &signals::SIGNALS[0];
    let frames = (signal.frames)(signal.rate);
    let mut whole = WaveCreator::new(signal.rate);
    whole.push(&frames);
    let whole = whole.finish();
    let mut packets = WaveCreator::new(signal.rate);
    for chunk in frames.chunks(137) {
        packets.push(chunk);
    }
    let packets = packets.finish();
    assert_eq!(packets.pwav, whole.pwav);
    assert_eq!(packets.pwv2, whole.pwv2);
}

#[test]
fn empty_and_short_silence_have_complete_previews() {
    for count in [0, 1, 293, 588, 589, 44100] {
        let mut creator = WaveCreator::new(44100);
        creator.push(&vec![[0, 0]; count]);
        let waves = creator.finish();
        assert_eq!(waves.pwav, vec![0xa2; 400]);
        assert_eq!(waves.pwv2, vec![1; 100]);
    }
}

#[test]
fn long_preview_matches_reference() {
    let frames = (signals::SIGNALS[0].frames)(44100);
    let mut creator = WaveCreator::new(44100);
    for _ in 0..10 {
        creator.push(&frames);
    }
    let ours = creator.finish();
    let reference = include_bytes!("rekordbox_waveform/kick-hats-long.blue-preview");
    assert_eq!(ours.pwav, reference[..400]);
    assert_eq!(ours.pwv2, reference[400..]);
}

#[test]
fn fractional_final_block_rounds_the_overview_grid_up() {
    let frames = (signals::SIGNALS[0].frames)(44100);
    let mut creator = WaveCreator::new(44100);
    creator.push(&frames);
    creator.push(&frames[..137]);
    let ours = creator.finish();
    let reference = include_bytes!("rekordbox_waveform/kick-hats-tail.blue-preview");
    assert_eq!(ours.pwav, reference[..400]);
    assert_eq!(ours.pwv2, reference[400..]);
}
