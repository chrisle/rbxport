//! Numeric selector categories used by Link Export menus.

use std::collections::BTreeSet;

use crate::{strings::fold, Library, Row};

/// Rekordbox's highest BPM value admitted to the selector root.
const MAX_MENU_BPM_X100: u32 = 49_949;

/// Rekordbox ignores duration values above 179:59 in its TIME menu.
const MAX_MENU_DURATION_SEC: u32 = 10_799;

/// The highest release year admitted to the YEAR hierarchy.
const MAX_MENU_YEAR: u16 = 2_999;

/// Rounds a stored x100 BPM half-up to the whole-BPM selector value.
#[must_use]
pub fn bpm_bucket(bpm_x100: u32) -> u32 {
    bpm_x100.saturating_add(50) / 100 * 100
}

impl Library {
    /// Distinct nonzero BPM selector values, rounded half-up and ascending.
    #[must_use]
    pub fn bpm_buckets(&self) -> Vec<u32> {
        self.bpm_x100
            .iter()
            .copied()
            .filter(|&bpm| bpm != 0 && bpm <= MAX_MENU_BPM_X100)
            .map(bpm_bucket)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// Whether a row belongs to a BPM selector and its ±percentage range.
    ///
    /// At zero percent the selected whole-BPM bucket is retained: 127.98 is
    /// under 128.00. Wider ranges compare the stored x100 value directly.
    #[must_use]
    pub fn bpm_matches(&self, row: Row, selected_x100: u32, tolerance_pct: u32) -> bool {
        let Some(&bpm) = self.bpm_x100.get(row as usize) else {
            return false;
        };
        if bpm == 0 || bpm > MAX_MENU_BPM_X100 {
            return false;
        }
        if tolerance_pct == 0 {
            return bpm_bucket(bpm) == selected_x100;
        }

        let spread = u64::from(selected_x100) * u64::from(tolerance_pct) / 100;
        let spread = u32::try_from(spread).unwrap_or(u32::MAX);
        (selected_x100.saturating_sub(spread)..=selected_x100.saturating_add(spread)).contains(&bpm)
    }

    /// Distinct bitrates in kbps, including zero, descending.
    #[must_use]
    pub fn bitrates(&self) -> Vec<u32> {
        descending(self.bitrate.iter().take(self.count).copied())
    }

    #[must_use]
    pub fn bitrate_matches(&self, row: Row, bitrate: u32) -> bool {
        self.bitrate.get(row as usize) == Some(&bitrate)
    }

    /// Distinct ratings used by the library, descending.
    #[must_use]
    pub fn ratings(&self) -> Vec<u32> {
        descending(self.rating.iter().take(self.count).copied().map(u32::from))
    }

    #[must_use]
    pub fn rating_matches(&self, row: Row, rating: u32) -> bool {
        self.rating.get(row as usize).copied().map(u32::from) == Some(rating)
    }

    /// The fixed rekordbox palette. Empty colors remain visible in the menu.
    #[must_use]
    pub fn color_ids(&self) -> Vec<u32> {
        (1..=8).collect()
    }

    #[must_use]
    pub fn color_matches(&self, row: Row, color: u32) -> bool {
        (1..=8).contains(&color)
            && self.color.get(row as usize).copied().map(u32::from) == Some(color)
    }

    /// Distinct floor(duration / 60) buckets, descending.
    #[must_use]
    pub fn duration_minute_buckets(&self) -> Vec<u32> {
        descending(
            self.length_sec
                .iter()
                .take(self.count)
                .copied()
                .filter(|&seconds| seconds <= MAX_MENU_DURATION_SEC)
                .map(|seconds| seconds / 60),
        )
    }

    #[must_use]
    pub fn duration_minute_matches(&self, row: Row, minute: u32) -> bool {
        self.length_sec
            .get(row as usize)
            .is_some_and(|&seconds| seconds <= MAX_MENU_DURATION_SEC && seconds / 60 == minute)
    }

    /// Distinct release-year decades, excluding zero and years after 2999.
    #[must_use]
    pub fn release_decades(&self) -> Vec<u32> {
        descending(
            self.year
                .iter()
                .take(self.count)
                .copied()
                .filter(|&year| valid_year(year))
                .map(|year| u32::from(year / 10 * 10)),
        )
    }

    /// Distinct release years in a decade, descending.
    #[must_use]
    pub fn release_years(&self, decade: u32) -> Vec<u32> {
        descending(
            self.year
                .iter()
                .take(self.count)
                .copied()
                .filter(|&year| valid_year(year))
                .map(u32::from)
                .filter(|&year| year / 10 * 10 == decade),
        )
    }

    #[must_use]
    pub fn release_year_matches(&self, row: Row, year: u32) -> bool {
        year != 0
            && year <= u32::from(MAX_MENU_YEAR)
            && self.year.get(row as usize).copied().map(u32::from) == Some(year)
    }

    /// Every row ordered by folded filename, with source order breaking ties.
    #[must_use]
    pub fn filename_rows(&self) -> Vec<Row> {
        let mut rows: Vec<Row> = (0..u32::try_from(self.count).unwrap_or(u32::MAX)).collect();
        rows.sort_by_cached_key(|&row| (fold(self.file_name.get(row as usize)), row));
        rows
    }
}

fn valid_year(year: u16) -> bool {
    year != 0 && year <= MAX_MENU_YEAR
}

fn descending(values: impl IntoIterator<Item = u32>) -> Vec<u32> {
    values
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .rev()
        .collect()
}
