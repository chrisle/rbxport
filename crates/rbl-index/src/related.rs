//! rekordbox's Related Tracks: the tracks that go with the one on the
//! player, under one of the section's criteria.
//!
//! Each criterion is a pass over the library's columns — a handful of
//! integer compares a row, the same cost as the filter bar — so the section
//! opens as fast as a playlist does. The track itself is left out: it is
//! related to nothing but itself.

use crate::key::camelot_rank;
use crate::smart::Date;
use crate::view::RelatedCriterion;
use crate::{Library, Row};

/// How far either side of the track's BPM `BPM + KEY` reaches, in percent.
/// [OBS] rekordbox's `BPM + KEY` preset stores `"BPM": {"Type": 1, "Diff":
/// {"Diff": 5, "HaDo": 1}}` (see `rbl_db::new_library`): five percent
/// either side of the track's own BPM, double and half tempo included.
const BPM_DIFF_PERCENT: u32 = 5;
/// [OBS] `HaDo` in the same preset: also match around half and double the
/// track's BPM.
const BPM_HALF_DOUBLE: bool = true;
/// How many days `Same genre in 30 days` looks back.
const RECENT_DAYS: i64 = 30;

/// The BPM window, in hundredths of a BPM and inclusive at both ends, that
/// rekordbox 7 builds `diff` percent either side of `centre_x100`.
///
/// [OBS] `db::RelatedTrackCriteria::BPM::createFilterCondition` (rekordbox
/// 7, arm64): the ends are `centre * (1 - diff * 0.01)` and `centre * (1 +
/// diff * 0.01)` worked in doubles, floored at zero and rounded to the
/// nearest integer, ties to even; the filter keeps a track whose BPM lies in
/// `[low, high]` (filter operation 6 compares `low <= bpm && bpm <= high`).
fn bpm_window(centre_x100: u32, diff: u32) -> (u32, u32) {
    let pct = f64::from(diff) * 0.01;
    let centre = f64::from(centre_x100);
    (round_even(centre * (1.0 - pct)), round_even(centre * (pct + 1.0)))
}

/// `x` floored at zero and rounded to the nearest whole number, ties to
/// even, as rekordbox rounds its BPM windows; past `u32::MAX` it is
/// `u32::MAX`.
fn round_even(x: f64) -> u32 {
    let whole = x.max(0.0).round_ties_even().min(f64::from(u32::MAX));
    // Whole, non-negative and at most u32::MAX: the conversion is exact.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let whole = whole as u32;
    whole
}

/// Whether `theirs_x100` is within `diff` percent of `centre_x100`, or, with
/// `half_double`, of half or double it — rekordbox's BPM criterion in its
/// "within N% of the track's BPM" mode.
///
/// [OBS] The same function: with `HaDo` set it adds two more windows, one
/// round half the BPM (`centre * 0.5`, rounded ties to even, as an integer)
/// and one round double it (`centre * 2`); the windows are OR-ed (the
/// condition's all-must-match flag is clear). A track with no BPM (`0`)
/// matches nothing, as `0` lies in no window round a positive centre.
fn bpm_matches(centre_x100: u32, theirs_x100: u32, diff: u32, half_double: bool) -> bool {
    let within = |centre: u32| {
        let (low, high) = bpm_window(centre, diff);
        (low..=high).contains(&theirs_x100)
    };
    if within(centre_x100) {
        return true;
    }
    if !half_double {
        return false;
    }
    within(round_even(f64::from(centre_x100) * 0.5)) || within(centre_x100.saturating_mul(2))
}

/// Whether two wheel positions are the same key, its relative, or the key
/// either side of it: `1A` goes with `1A`, `1B`, `12A` and `2A`.
///
/// [OBS] This is rekordbox 7's `Related key 2` (`"Key": {"Typ2": 2}`, the
/// `BPM + KEY` preset's setting): `db::RelatedTrackKeySearch::Algorithm`
/// keeps a track whose key is the track's own, the same wheel number with
/// the other letter, or the same letter one number either way (12 wraps to
/// 1). `MusicKey` numbers keys round the Camelot wheel (its name table pairs
/// `1A` with `Abm`, `8A` with `Am`, …). The same rule backs the Traffic
/// Light help text: "Related Key 2: 2A/2B/1A/3A".
fn keys_go_together(a: u32, b: u32) -> bool {
    if a == u32::MAX || b == u32::MAX {
        return false;
    }
    if a == b || a ^ 1 == b {
        return true;
    }
    // Same letter, one step round the wheel of twelve either way.
    (a & 1) == (b & 1) && ((a + 2) % 24 == b || (b + 2) % 24 == a)
}

impl Library {
    /// The rows related to `track` under `criterion`, in collection order.
    /// A track past the end of the library relates to nothing.
    #[must_use]
    pub fn related_rows(&self, track: Row, criterion: RelatedCriterion) -> Vec<Row> {
        let at = track as usize;
        if at >= self.len() {
            return Vec::new();
        }
        let rows = 0..u32::try_from(self.len()).unwrap_or(u32::MAX);
        match criterion {
            RelatedCriterion::BpmAndKey => {
                let bpm = self.bpm_x100.get(at).copied().unwrap_or(0);
                // The preset's key setting also has `"Typ1": 1`, which makes
                // rekordbox take the key from the search's extra deck
                // information rather than the track's tag [OBS]; that this is
                // the deck's key-shifted "Current key" is [UNKNOWN] (the UI
                // offers "Track's key" and "Current key"). This uses the
                // track's key, which is the same with no key shift.
                let key = self.key.get(at).map_or(u32::MAX, |&id| camelot_rank(self.keys.name(id)));
                // A track with no BPM or no key is matched on what it has:
                // [OBS] rekordbox's BPM search returns without narrowing when
                // the track's BPM is 0, and its key search does the same
                // when the track's key is not one of the 24. With neither
                // there is nothing to relate it by [ASSUME].
                if bpm == 0 && key == u32::MAX {
                    return Vec::new();
                }
                rows.filter(|&r| {
                    if r == track {
                        return false;
                    }
                    let other = usize::try_from(r).unwrap_or(usize::MAX);
                    if bpm != 0 {
                        let theirs = self.bpm_x100.get(other).copied().unwrap_or(0);
                        if !bpm_matches(bpm, theirs, BPM_DIFF_PERCENT, BPM_HALF_DOUBLE) {
                            return false;
                        }
                    }
                    if key != u32::MAX {
                        let theirs = self.key.get(other).map_or(u32::MAX, |&id| camelot_rank(self.keys.name(id)));
                        if !keys_go_together(key, theirs) {
                            return false;
                        }
                    }
                    true
                })
                .collect()
            }
            RelatedCriterion::SameGenreRecent => {
                let genre = self.genre.get(at).copied().unwrap_or(0);
                if genre == 0 {
                    return Vec::new();
                }
                let since = Date::today().days_ago(RECENT_DAYS).days();
                rows.filter(|&r| {
                    if r == track {
                        return false;
                    }
                    let other = usize::try_from(r).unwrap_or(usize::MAX);
                    self.genre.get(other).copied() == Some(genre)
                        && Date::parse(self.date_added.get(other)).is_some_and(|added| added.days() >= since)
                })
                .collect()
            }
            RelatedCriterion::SameArtist => {
                let artist = self.artist.get(at).copied().unwrap_or(0);
                if artist == 0 {
                    return Vec::new();
                }
                rows.filter(|&r| r != track && self.artist.get(usize::try_from(r).unwrap_or(usize::MAX)).copied() == Some(artist))
                    .collect()
            }
            RelatedCriterion::Suggestion => {
                let followed = self.followed_in_history(track);
                if followed.is_empty() {
                    return self.related_rows(track, RelatedCriterion::BpmAndKey);
                }
                followed
            }
        }
    }

    /// The tracks that came right after `track` in the history sessions,
    /// the most often first and, among equals, the one played that way
    /// most recently first. Sessions are in the histories' order, which is
    /// by date, so a later session is a later play.
    fn followed_in_history(&self, track: Row) -> Vec<Row> {
        let histories = self.histories();
        let mut count: std::collections::HashMap<Row, (u32, usize)> = std::collections::HashMap::new();
        for (session, members) in histories.members.iter().enumerate() {
            for pair in members.windows(2) {
                if pair[0] == track && pair[1] != track {
                    let entry = count.entry(pair[1]).or_insert((0, 0));
                    entry.0 += 1;
                    entry.1 = session;
                }
            }
        }
        let mut ranked: Vec<(Row, (u32, usize))> = count.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        ranked.into_iter().map(|(row, _)| row).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_goes_with_itself_its_relative_and_its_neighbours() {
        let a1 = camelot_rank("Abm");
        let b1 = camelot_rank("B");
        let a2 = camelot_rank("Ebm");
        let a12 = camelot_rank("Dbm");
        let a3 = camelot_rank("Bbm");
        assert!(keys_go_together(a1, a1));
        assert!(keys_go_together(a1, b1));
        assert!(keys_go_together(a1, a2));
        assert!(keys_go_together(a1, a12));
        assert!(!keys_go_together(a1, a3));
        assert!(!keys_go_together(a1, camelot_rank("F#")));
        assert!(!keys_go_together(u32::MAX, a1));
    }

    #[test]
    fn the_bpm_window_is_five_percent_either_side_rounded_ties_to_even() {
        assert_eq!(bpm_window(12800, 5), (12160, 13440));
        // 12810 × 0.95 = 12169.5 and × 1.05 = 13450.5: both ties go even.
        assert_eq!(bpm_window(12810, 5), (12170, 13450));
        assert_eq!(bpm_window(12800, 0), (12800, 12800));
        assert_eq!(bpm_window(0, 5), (0, 0));
    }

    #[test]
    fn a_bpm_matches_inside_the_window_at_its_own_half_or_double_tempo() {
        // Both ends are in.
        assert!(bpm_matches(12800, 12160, 5, true));
        assert!(bpm_matches(12800, 13440, 5, true));
        assert!(!bpm_matches(12800, 12159, 5, true));
        assert!(!bpm_matches(12800, 13441, 5, true));
        // Half 170 is 85, ±5% is 80.75 to 89.25; double is 340, 323 to 357.
        assert!(bpm_matches(17000, 8500, 5, true));
        assert!(bpm_matches(17000, 8075, 5, true));
        assert!(!bpm_matches(17000, 8074, 5, true));
        assert!(bpm_matches(17000, 34000, 5, true));
        assert!(bpm_matches(17000, 35700, 5, true));
        assert!(!bpm_matches(17000, 35701, 5, true));
        // Without half/double only the track's own tempo counts.
        assert!(!bpm_matches(17000, 8500, 5, false));
        assert!(!bpm_matches(17000, 34000, 5, false));
        // Half of an odd hundredth rounds ties to even: 12801 / 2 is 6400.
        assert!(bpm_matches(12801, 6080, 5, true));
        // No BPM is in no window.
        assert!(!bpm_matches(12800, 0, 5, true));
    }

    #[test]
    fn bpm_and_key_takes_half_and_double_tempo_tracks_in_a_related_key() {
        use crate::testing::{library_from, TestTrack};
        let track = |id: u64, bpm: u32, key: &'static str| TestTrack { id, title: "t", artist: "a", bpm_x100: bpm, key, ..TestTrack::default() };
        let lib = library_from(&[
            track(1, 17400, "Am"), // the track: 174, 8A
            track(2, 8700, "Am"),  // half tempo, same key
            track(3, 34800, "C"),  // double tempo, 8B
            track(4, 17400, "Em"), // same tempo, 9A
            track(5, 8700, "F#"),  // half tempo, 2B: not a related key
            track(6, 12800, "Am"), // same key, 128 is no tempo of 174's
            track(7, 18270, "Dm"), // 182.7 is the top of the window, 7A
            track(8, 18271, "Am"), // just past it
        ]);
        assert_eq!(lib.related_rows(0, RelatedCriterion::BpmAndKey), vec![1, 2, 3, 6]);
    }

    #[test]
    fn a_suggestion_is_what_followed_the_track_in_past_sets_or_its_bpm_and_key_matches() {
        use crate::testing::{add_history, library_from, TestTrack};
        let track = |id: u64, bpm: u32, key: &'static str| TestTrack { id, title: "t", artist: "a", bpm_x100: bpm, key, ..TestTrack::default() };
        let mut lib = library_from(&[
            track(1, 12800, "Am"),
            track(2, 12800, "Am"),
            track(3, 12800, "Em"),
            track(4, 17000, "C"),
            track(5, 12800, "Am"),
        ]);
        // Three sets: 0 was followed by 3 twice and by 1 once, the latest
        // set having 3 after it; 4 was never followed.
        add_history(&mut lib, "one", &[0, 3, 4]);
        add_history(&mut lib, "two", &[2, 0, 1]);
        add_history(&mut lib, "three", &[0, 3]);
        assert_eq!(lib.related_rows(0, RelatedCriterion::Suggestion), vec![3, 1]);
        // Never played after anything: what goes with it by BPM and key.
        let fallback = lib.related_rows(1, RelatedCriterion::Suggestion);
        assert_eq!(fallback, lib.related_rows(1, RelatedCriterion::BpmAndKey));
        // Rows, not ids: the 170 BPM track is row 3.
        assert_eq!(fallback, vec![0, 2, 4]);
    }
}
