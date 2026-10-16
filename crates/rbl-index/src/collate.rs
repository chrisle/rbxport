//! The order rekordbox puts names in when it sorts a folder.
//!
//! Sort Items on a playlist folder (or the Playlists heading) puts the
//! folder's children in name order. rekordbox compares the two names with an
//! ICU collator made for the language the user picked, with ICU's default
//! options [OBS static, rekordbox 7.2.19 macOS arm64:
//! `browse::PlaylistItemSorter::compareElements` @0x1015933a8 hands the two
//! names to `browse::SorterBase::compare` @0x1015920a0, which converts both to
//! `icu_51::UnicodeString` and calls the collator's `compare`; the collator
//! is `icu_51::Collator::createInstance` in `SorterBase::setLocale`
//! @0x1015888f0, for the user's language and region from the `language`
//! setting (`SorterBase::SorterBase` @0x1015887d0), `en_US` first, and no
//! `setStrength` or `setAttribute` follows].
//!
//! ICU's defaults are tertiary strength with no numeric ordering, so the
//! order is: case and accents aside first, then unaccented before accented,
//! then lower case before upper case; spaces and punctuation before digits,
//! digits before letters; and digits one at a time, so "10" sorts before
//! "2". ICU4X's collator is the same algorithm over the same CLDR root
//! tailorings [ASSUME: rekordbox links ICU 51; for Latin, Greek, Cyrillic
//! and kana names the root order has not moved since, but a script whose
//! CLDR tailoring changed after ICU 51 could order differently].

use std::cmp::Ordering;

use icu_collator::options::CollatorOptions;
use icu_collator::{Collator, CollatorBorrowed, CollatorPreferences};
use icu_locale_core::Locale;

/// Compares names the way rekordbox's Sort Items does, for one language.
pub struct NameCollator {
    collator: Option<CollatorBorrowed<'static>>,
}

impl NameCollator {
    /// A collator for `language`, a BCP 47 tag such as `en`, `ja` or
    /// `zh-TW`. `None`, or a tag ICU does not know, gets the root order,
    /// which is what English uses.
    #[must_use]
    pub fn new(language: Option<&str>) -> Self {
        let locale = language
            .map(|tag| tag.replace('_', "-"))
            .and_then(|tag| tag.parse::<Locale>().ok())
            .unwrap_or(Locale::UNKNOWN);
        let collator = Collator::try_new(CollatorPreferences::from(&locale), CollatorOptions::default())
            .or_else(|_| Collator::try_new(CollatorPreferences::default(), CollatorOptions::default()))
            .ok();
        Self { collator }
    }

    /// `a` against `b`. Without a collator, which compiled-in data should
    /// never leave us, the names compare as plain text.
    #[must_use]
    pub fn compare(&self, a: &str, b: &str) -> Ordering {
        match &self.collator {
            Some(collator) => collator.compare(a, b),
            None => a.cmp(b),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(language: Option<&str>, names: &[&str]) -> Vec<String> {
        let collator = NameCollator::new(language);
        let mut out: Vec<String> = names.iter().map(|&n| n.to_owned()).collect();
        out.sort_by(|a, b| collator.compare(a, b));
        out
    }

    #[test]
    fn orders_names_as_icu_tertiary_does() {
        // ICU's default (tertiary, non-numeric) order: blanks and
        // punctuation, then digits one at a time, then letters with case and
        // accents settled last — lower before upper, plain before accented.
        let names = ["b", "B", "Ä", "a", "A", "é", "e", "E", "10", "2", "_x", " z", "#1", "Zebra", "zebra"];
        assert_eq!(
            sorted(Some("en"), &names),
            [" z", "_x", "#1", "10", "2", "a", "A", "Ä", "b", "B", "e", "E", "é", "zebra", "Zebra"],
        );
    }

    #[test]
    fn case_and_accents_do_not_split_a_run_of_names() {
        // "apple" and "Apple" sit together ahead of "banana": case is a
        // tie-break, not the first thing compared.
        assert_eq!(
            sorted(None, &["banana", "Apple", "Éclair", "apple", "eclair"]),
            ["apple", "Apple", "banana", "eclair", "Éclair"],
        );
    }

    #[test]
    fn an_unknown_or_missing_language_is_the_root_order() {
        let names = ["b", "a", "B", "A"];
        assert_eq!(sorted(Some("not a tag!"), &names), sorted(None, &names));
        assert_eq!(sorted(Some("en_US"), &names), ["a", "A", "b", "B"]);
    }

    #[test]
    fn a_language_tailoring_applies() {
        // Swedish puts Ä after Z; the root order files it with A.
        assert_eq!(sorted(Some("sv"), &["Ä", "Z", "A"]), ["A", "Z", "Ä"]);
        assert_eq!(sorted(Some("en"), &["Ä", "Z", "A"]), ["A", "Ä", "Z"]);
    }
}
