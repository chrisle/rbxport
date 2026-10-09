//! Which Collection tracks Auto Analysis offers to analyse at launch.

use rbl_index::testing::{library_from, TestTrack};

#[test]
fn only_unanalysed_tracks_with_a_file_are_offered() {
    let mut library = library_from(&[
        TestTrack { id: 1, title: "Analysed", bpm_x100: 12_800, path: "/music/a.mp3", ..Default::default() },
        TestTrack { id: 2, title: "Fresh", path: "/music/b.mp3", ..Default::default() },
        TestTrack { id: 3, title: "No file", ..Default::default() },
        TestTrack { id: 4, title: "Locked", path: "/music/d.mp3", ..Default::default() },
        TestTrack { id: 5, title: "Also fresh", path: "/music/e.mp3", ..Default::default() },
    ]);
    // A locked track carries bit 0x80 in `Analysed`, which the loader keeps
    // as analysed even when its BPM is 0.
    if let Some(locked) = library.analysed.get_mut(3) {
        *locked = 1;
    }

    assert_eq!(library.unanalysed_rows(), vec![1, 4]);
}
