//! The track filter: BPM, key, rating and colour, combined with AND.
//!
//! rekordbox's Track Filter drops down between the browser header and the
//! column header. Each of its columns has a tick box; a ticked column narrows
//! the list to the values picked in it, and every ticked column has to agree.
//! The filter is part of [`ViewSpec`](crate::ViewSpec) so that Rust applies
//! it in the same pass as the search, over the same columnar library — the
//! frontend never sees more than a window of the result.
//!
//! The value lists the bar offers — which whole BPMs and which keys the
//! current list holds — are computed here too, from the list *before* the
//! filter is applied. A list computed after it would lose every value not
//! currently picked, and there would be no way to pick another.

use std::collections::{HashMap, HashSet};

use crate::{Library, Row, TrackSource, ViewSpec, NO_ID};

/// What the BPM column asks for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BpmFilter {
    /// Whole BPMs picked from the list; empty is `All`.
    pub values: Vec<u32>,
    /// The `MASTER PLAYER ± n%` pick, 0 to 6.
    pub tolerance_pct: u8,
    /// The master player's BPM, x100 as the library stores it. `None` when
    /// no deck is loaded, which greys the ± list and makes it inert.
    pub master_bpm_x100: Option<u32>,
}

/// One filter per column. `None` is an unticked column: no constraint.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackFilter {
    pub bpm: Option<BpmFilter>,
    /// Dense key ids (into `Library::keys`). Resolved from names by the
    /// caller, because `djmdKey` holds the same name under more than one id.
    pub keys: Option<Vec<u32>>,
    /// Star counts, 0 to 5.
    pub ratings: Option<Vec<u8>>,
    /// `djmdContent.ColorID`, 1 to 8 (see [`COLOR_NAMES`]).
    pub colors: Option<Vec<u8>>,
}

/// The eight colour comments, indexed by `ColorID - 1`.
///
/// `djmdColor` on the reference library, read-only: ids 1 to 8, `Commnt`
/// Pink, Red, Orange, Yellow, Green, Aqua, Blue, Purple, in `SortKey` order.
pub const COLOR_NAMES: [&str; 8] =
    ["Pink", "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple"];

impl TrackFilter {
    /// Whether the filter constrains anything at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bpm.is_none() && self.keys.is_none() && self.ratings.is_none() && self.colors.is_none()
    }

    /// The filter in a shape the row loop can test without allocating.
    pub(crate) fn compile(&self) -> Compiled {
        let ratings = self.ratings.as_ref().map(|list| {
            list.iter().fold(0_u8, |mask, &stars| mask | (1_u8 << stars.min(7)))
        });
        let colors = self.colors.as_ref().map(|list| {
            list.iter().fold(0_u16, |mask, &color| mask | (1_u16 << u32::from(color.min(15))))
        });
        let mut keys = self.keys.clone();
        if let Some(keys) = keys.as_mut() {
            keys.sort_unstable();
            keys.dedup();
        }
        let bpm = self.bpm.as_ref().map(|b| {
            let mut ranges: Vec<(u32, u32)> = Vec::with_capacity(b.values.len().max(1));
            if b.values.is_empty() {
                // `All` plus a tolerance: a band around the master player. With
                // no master player the ± list is inert and the column matches
                // everything, which is what an unticked one does.
                if let Some(master) = b.master_bpm_x100 {
                    ranges.push(band(master, b.tolerance_pct));
                }
                BpmRule { ranges, whole: Vec::new(), any: b.master_bpm_x100.is_none() }
            } else if b.tolerance_pct == 0 {
                // A picked whole BPM at ±0% is the bucket the list grouped it
                // into, not the exact hundredth: 127.98 sits under 128.
                let mut whole = b.values.clone();
                whole.sort_unstable();
                whole.dedup();
                BpmRule { ranges, whole, any: false }
            } else {
                for &value in &b.values {
                    ranges.push(band(value.saturating_mul(100), b.tolerance_pct));
                }
                BpmRule { ranges, whole: Vec::new(), any: false }
            }
        });
        Compiled { bpm, keys, ratings, colors }
    }
}

/// `centre ± pct%`, in hundredths, inclusive at both ends.
fn band(centre_x100: u32, pct: u8) -> (u32, u32) {
    let spread = u64::from(centre_x100) * u64::from(pct) / 100;
    let spread = u32::try_from(spread).unwrap_or(u32::MAX);
    (centre_x100.saturating_sub(spread), centre_x100.saturating_add(spread))
}

/// The nearest whole BPM. `[ASSUME]` rekordbox groups its BPM list by the
/// rounded value — the capture only shows whole numbers, so truncation and
/// rounding look the same there.
#[must_use]
pub fn whole_bpm(bpm_x100: u32) -> u32 {
    bpm_x100.saturating_add(50) / 100
}

#[derive(Debug, Clone)]
struct BpmRule {
    ranges: Vec<(u32, u32)>,
    whole: Vec<u32>,
    /// Match every BPM: `All` with nothing to centre a band on.
    any: bool,
}

impl BpmRule {
    #[inline]
    fn matches(&self, bpm_x100: u32) -> bool {
        if self.any {
            return true;
        }
        if !self.whole.is_empty() {
            return self.whole.binary_search(&whole_bpm(bpm_x100)).is_ok();
        }
        self.ranges.iter().any(|&(lo, hi)| bpm_x100 >= lo && bpm_x100 <= hi)
    }
}

/// A [`TrackFilter`] with its lists turned into masks and sorted ids.
#[derive(Debug, Clone)]
pub(crate) struct Compiled {
    bpm: Option<BpmRule>,
    keys: Option<Vec<u32>>,
    ratings: Option<u8>,
    colors: Option<u16>,
}

impl Compiled {
    /// Whether a row passes every ticked column.
    #[inline]
    pub(crate) fn matches(&self, lib: &Library, row: Row) -> bool {
        let index = row as usize;
        if let Some(bpm) = &self.bpm {
            if !bpm.matches(lib.bpm_x100.get(index).copied().unwrap_or(0)) {
                return false;
            }
        }
        if let Some(keys) = &self.keys {
            let key = lib.key.get(index).copied().unwrap_or(NO_ID);
            if keys.binary_search(&key).is_err() {
                return false;
            }
        }
        if let Some(mask) = self.ratings {
            let stars = lib.rating.get(index).copied().unwrap_or(0).min(7);
            if mask & (1_u8 << stars) == 0 {
                return false;
            }
        }
        if let Some(mask) = self.colors {
            let color = lib.color.get(index).copied().unwrap_or(0).min(15);
            if mask & (1_u16 << u32::from(color)) == 0 {
                return false;
            }
        }
        true
    }
}

/// A value the bar can offer, with how many tracks of the list carry it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Counted<T> {
    pub value: T,
    pub count: u32,
}

/// One My Tag category and the tags under it, as named in `djmdMyTag`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TagCategory {
    pub name: String,
    pub tags: Vec<String>,
}

/// What the bar's lists hold for a source and query.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterValues {
    /// Whole BPMs present, ascending. Unanalysed tracks (BPM 0) are left out.
    pub bpms: Vec<Counted<u32>>,
    /// Key names present, in Camelot order; names the wheel does not know
    /// follow alphabetically. Tracks without a key are left out.
    pub keys: Vec<Counted<String>>,
    /// The My Tag categories, for the bar's tag columns. Inert: the bar does
    /// not filter on the tracks' tags (see [`Library::my_tags`]).
    pub tags: Vec<TagCategory>,
}

/// How many BPM entries the list may hold, so a library of nonsense BPMs
/// cannot push the response past the 64 KB IPC cap. A real library has a few
/// hundred at most: the reference one has 133.
const MAX_BPM_VALUES: usize = 1000;

impl Library {
    /// The BPMs and keys present in a view, before its filter is applied.
    ///
    /// One pass over the rows the source and query leave, into two small
    /// tallies. Sort and filter are ignored: the lists are what the user can
    /// pick from, so they must not shrink with what is picked.
    pub fn filter_values(&self, spec: &ViewSpec) -> FilterValues {
        self.filter_values_scoped(spec, crate::SearchField::All)
    }

    pub fn filter_values_scoped(&self, spec: &ViewSpec, field: crate::SearchField) -> FilterValues {
        let unfiltered = ViewSpec {
            source: spec.source.clone(),
            sort: crate::SortColumn::TrackNo,
            descending: false,
            query: spec.query.clone(),
            filter: TrackFilter::default(),
        };
        let view = self.open_view_scoped(&unfiltered, field);
        self.values_of(&view.rows)
    }

    /// The tallies over a set of rows. Split out so a test can drive it
    /// without a source.
    pub(crate) fn values_of(&self, rows: &[Row]) -> FilterValues {
        let mut bpm_counts: HashMap<u32, u32> = HashMap::new();
        // Keyed by dense key id; folded into names afterwards, because two
        // ids can carry the same name and the bar lists names.
        let mut key_counts: Vec<u32> = vec![0; self.keys.len()];
        for &row in rows {
            let index = row as usize;
            let whole = whole_bpm(self.bpm_x100.get(index).copied().unwrap_or(0));
            if whole > 0 {
                *bpm_counts.entry(whole).or_insert(0) += 1;
            }
            let key = self.key.get(index).copied().unwrap_or(NO_ID);
            if let Some(slot) = key_counts.get_mut(key as usize) {
                *slot += 1;
            }
        }

        let mut bpms: Vec<Counted<u32>> =
            bpm_counts.into_iter().map(|(value, count)| Counted { value, count }).collect();
        bpms.sort_unstable_by_key(|c| c.value);
        bpms.truncate(MAX_BPM_VALUES);

        let mut by_name: HashMap<&str, u32> = HashMap::new();
        for (id, &count) in key_counts.iter().enumerate() {
            let name = self.keys.name(u32::try_from(id).unwrap_or(NO_ID));
            if count > 0 && !name.trim().is_empty() {
                *by_name.entry(name).or_insert(0) += count;
            }
        }
        let mut keys: Vec<Counted<String>> = by_name
            .into_iter()
            .map(|(name, count)| Counted { value: name.to_owned(), count })
            .collect();
        keys.sort_by_cached_key(|c| (camelot_rank(&c.value), crate::strings::fold(&c.value)));

        FilterValues { bpms, keys, tags: self.my_tags().to_vec() }
    }

    /// Dense key ids whose name is one of `names`.
    ///
    /// The bar picks by name; the filter tests by id. `djmdKey` holds the
    /// same name under more than one id on the reference library (`A` twice),
    /// so every id carrying the name is taken.
    #[must_use]
    pub fn key_ids_named(&self, names: &[String]) -> Vec<u32> {
        let mut out = Vec::new();
        for id in 0..self.keys.len() {
            let dense = u32::try_from(id).unwrap_or(NO_ID);
            let name = self.keys.name(dense);
            if names.iter().any(|wanted| wanted == name) {
                out.push(dense);
            }
        }
        out
    }

    /// The rows a source yields with no query and no filter. What the values
    /// are counted over when a caller already has them.
    #[must_use]
    pub fn source_rows(&self, source: &TrackSource) -> Vec<Row> {
        self.source_rows_on(source, &crate::smart::Date::today())
    }

    /// [`source_rows`](Self::source_rows) with an explicit date for relative
    /// Smart Playlist conditions.
    #[must_use]
    pub fn source_rows_on(&self, source: &TrackSource, today: &crate::smart::Date) -> Vec<Row> {
        match source {
            TrackSource::History(index) => {
                self.histories().members.get(*index).cloned().unwrap_or_default()
            }
            TrackSource::Collection | TrackSource::Playlist(_) | TrackSource::PlaylistFolder(_) | TrackSource::SmartPlaylist(_) => {
                let playlists = self.playlists();
                self.source_rows_unlocked_on(&playlists, source, today)
            }
            TrackSource::Related { track, criterion } => self.related_rows(*track, *criterion),
            TrackSource::TagList => self.tag_list(),
        }
    }

    /// [`source_rows`](Self::source_rows) for a caller already holding the
    /// playlists, which the lock would otherwise wait on for ever. A history
    /// source is not answered here.
    #[must_use]
    pub fn source_rows_unlocked(&self, playlists: &crate::Playlists, source: &TrackSource) -> Vec<Row> {
        self.source_rows_unlocked_on(playlists, source, &crate::smart::Date::today())
    }

    /// [`source_rows_unlocked`](Self::source_rows_unlocked) with an explicit
    /// date for relative Smart Playlist conditions.
    #[must_use]
    pub fn source_rows_unlocked_on(
        &self,
        playlists: &crate::Playlists,
        source: &TrackSource,
        today: &crate::smart::Date,
    ) -> Vec<Row> {
        match source {
            TrackSource::Collection | TrackSource::History(_) => {
                (0..u32::try_from(self.len()).unwrap_or(u32::MAX)).collect()
            }
            TrackSource::Playlist(index) => playlists.members.get(*index).cloned().unwrap_or_default(),
            TrackSource::PlaylistFolder(folder) => {
                if !playlists.is_folder(*folder) {
                    return Vec::new();
                }
                let mut seen = HashSet::new();
                let mut rows = Vec::new();
                for index in 0..playlists.len() {
                    if playlists.is_folder(index) {
                        continue;
                    }
                    let mut parent = playlists.parent.get(index).copied().unwrap_or(NO_ID);
                    let mut depth = 0;
                    while parent != NO_ID && depth < playlists.len() {
                        if parent as usize == *folder {
                            let members = if playlists.is_smart(index) {
                                playlists
                                    .smart_rule(index)
                                    .map(|rule| rule.evaluate_on(self, today))
                                    .unwrap_or_default()
                            } else {
                                playlists.members.get(index).cloned().unwrap_or_default()
                            };
                            rows.extend(members.into_iter().filter(|row| seen.insert(*row)));
                            break;
                        }
                        parent = playlists.parent.get(parent as usize).copied().unwrap_or(NO_ID);
                        depth += 1;
                    }
                }
                rows
            }
            // A rule that does not parse admits nothing, which is what
            // rekordbox shows for a rule it cannot read.
            TrackSource::SmartPlaylist(index) => {
                playlists
                    .smart_rule(*index)
                    .map(|rule| rule.evaluate_on(self, today))
                    .unwrap_or_default()
            }
            TrackSource::Related { track, criterion } => self.related_rows(*track, *criterion),
            TrackSource::TagList => self.tag_list(),
        }
    }
}

/// Where a key sits on the Camelot wheel: `(number, letter)`, with A (minor)
/// before B (major), which is the order rekordbox's key list uses in the
/// capture — Abm, B, Ebm, F#, Bbm, Db, Fm, Ab, Cm… Unknown names sort last.
///
/// The same table as `src/lib/camelot.ts`, including its aliases.
fn camelot_rank(name: &str) -> (u8, u8) {
    const MINOR: [&str; 12] =
        ["Abm", "Ebm", "Bbm", "Fm", "Cm", "Gm", "Dm", "Am", "Em", "Bm", "F#m", "Dbm"];
    const MAJOR: [&str; 12] = ["B", "F#", "Db", "Ab", "Eb", "Bb", "F", "C", "G", "D", "A", "E"];
    let name = match name.trim() {
        "G#m" => "Abm",
        "D#m" => "Ebm",
        "A#m" => "Bbm",
        "C#m" => "Dbm",
        "Gbm" => "F#m",
        "Gb" => "F#",
        "C#" => "Db",
        "G#" => "Ab",
        "D#" => "Eb",
        "A#" => "Bb",
        other => other,
    };
    if let Some(at) = MINOR.iter().position(|&k| k == name) {
        return (u8::try_from(at).unwrap_or(u8::MAX), 0);
    }
    if let Some(at) = MAJOR.iter().position(|&k| k == name) {
        return (u8::try_from(at).unwrap_or(u8::MAX), 1);
    }
    (u8::MAX, 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_is_inclusive_and_symmetric() {
        assert_eq!(band(12800, 0), (12800, 12800));
        assert_eq!(band(12800, 2), (12544, 13056));
        assert_eq!(band(100, 6), (94, 106));
        assert_eq!(band(0, 6), (0, 0));
    }

    #[test]
    fn whole_bpm_rounds_to_nearest() {
        assert_eq!(whole_bpm(12798), 128);
        assert_eq!(whole_bpm(12849), 128);
        assert_eq!(whole_bpm(12850), 129);
        assert_eq!(whole_bpm(0), 0);
        assert_eq!(whole_bpm(u32::MAX), u32::MAX / 100);
    }

    #[test]
    fn camelot_order_matches_the_capture() {
        let mut names = vec!["Cm", "F#", "B", "Ebm", "Abm", "Db", "Fm", "Ab", "Bbm", "zz", "A"];
        names.sort_by_key(|n| (camelot_rank(n), n.to_lowercase()));
        assert_eq!(names, ["Abm", "B", "Ebm", "F#", "Bbm", "Db", "Fm", "Ab", "Cm", "A", "zz"]);
        assert_eq!(camelot_rank("G#m"), camelot_rank("Abm"));
    }
}
