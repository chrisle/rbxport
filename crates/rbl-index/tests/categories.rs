use rbl_index::testing::{library_from, TestTrack};

fn track(
    id: u64,
    path: &'static str,
    bpm_x100: u32,
    duration: u32,
    rating: u8,
    color: u8,
    year: u16,
) -> TestTrack {
    TestTrack {
        id,
        path,
        bpm_x100,
        length_sec: duration,
        rating,
        color,
        year,
        ..TestTrack::default()
    }
}

#[test]
fn bpm_uses_half_up_buckets_and_raw_percentage_ranges() {
    let lib = library_from(&[
        track(1, "z.mp3", 12_749, 0, 0, 0, 0),
        track(2, "a.mp3", 12_750, 0, 0, 0, 0),
        track(3, "b.mp3", 13_050, 0, 0, 0, 0),
        track(4, "c.mp3", 0, 0, 0, 0, 0),
        track(5, "d.mp3", 50_000, 0, 0, 0, 0),
    ]);

    assert_eq!(lib.bpm_buckets(), [12_700, 12_800, 13_100]);
    assert!(lib.bpm_matches(1, 12_800, 0));
    assert!(!lib.bpm_matches(0, 12_800, 0));
    assert!(lib.bpm_matches(0, 12_800, 1));
    assert!(!lib.bpm_matches(2, 12_800, 1));
    assert!(!lib.bpm_matches(4, 50_000, 0));
}

#[test]
fn scalar_categories_have_captured_order_and_boundaries() {
    let mut lib = library_from(&[
        track(1, "z.mp3", 0, 59, 1, 1, 2024),
        track(2, "A.mp3", 0, 60, 5, 8, 2020),
        track(3, "a.mp3", 0, 10_799, 1, 0, 1999),
        track(4, "x.mp3", 0, 10_800, 0, 9, 0),
        track(5, "y.mp3", 0, 120, 0, 0, 3000),
    ]);
    lib.bitrate[..5].copy_from_slice(&[320, 0, 128, 320, 0]);

    assert_eq!(lib.bitrates(), [320, 128, 0]);
    assert!(lib.bitrate_matches(1, 0));
    assert_eq!(lib.ratings(), [5, 1, 0]);
    assert!(lib.rating_matches(2, 1));
    assert_eq!(lib.color_ids(), [1, 2, 3, 4, 5, 6, 7, 8]);
    assert!(lib.color_matches(1, 8));
    assert!(!lib.color_matches(2, 0));
    assert_eq!(lib.duration_minute_buckets(), [179, 2, 1, 0]);
    assert!(lib.duration_minute_matches(1, 1));
    assert!(!lib.duration_minute_matches(3, 180));
    assert_eq!(lib.release_decades(), [2020, 1990]);
    assert_eq!(lib.release_years(2020), [2024, 2020]);
    assert!(lib.release_year_matches(0, 2024));
    assert!(!lib.release_year_matches(4, 3000));
    assert_eq!(lib.filename_rows(), [1, 2, 3, 4, 0]);
}
