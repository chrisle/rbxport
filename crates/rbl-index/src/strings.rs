//! Packed string column.
//!
//! 38k tracks means ~150k short strings. Storing them as `Vec<String>` costs an
//! allocation and 24 bytes of header each; packing them into one arena with
//! (offset, len) pairs cuts that to 8 bytes and keeps them contiguous, which is
//! what makes a full-column scan cache-friendly.

/// Byte arena plus offsets. Indices are row order.
#[derive(Debug, Default, Clone)]
pub struct StrColumn {
    bytes: Vec<u8>,
    spans: Vec<(u32, u32)>,
}

impl StrColumn {
    pub fn with_capacity(rows: usize, bytes: usize) -> Self {
        Self { bytes: Vec::with_capacity(bytes), spans: Vec::with_capacity(rows) }
    }

    pub fn push(&mut self, s: &str) {
        let start = u32::try_from(self.bytes.len()).unwrap_or(u32::MAX);
        self.bytes.extend_from_slice(s.as_bytes());
        let len = u32::try_from(s.len()).unwrap_or(0);
        self.spans.push((start, len));
    }

    #[inline]
    pub fn get(&self, row: usize) -> &str {
        let Some(&(start, len)) = self.spans.get(row) else { return "" };
        let (a, b) = (start as usize, start as usize + len as usize);
        self.bytes.get(a..b).map_or("", |s| std::str::from_utf8(s).unwrap_or(""))
    }

    pub fn len(&self) -> usize {
        self.spans.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// Replace rows in one pass so repeated edits do not leave dead strings
    /// growing in the packed arena.
    pub(crate) fn replace_rows(&mut self, values: &std::collections::HashMap<usize, String>) {
        let mut next = Self::with_capacity(self.len(), self.bytes.len());
        for row in 0..self.len() {
            next.push(values.get(&row).map_or_else(|| self.get(row), String::as_str));
        }
        *self = next;
    }

    /// Bytes held, for the memory budget.
    pub fn heap_bytes(&self) -> usize {
        self.bytes.capacity() + self.spans.capacity() * std::mem::size_of::<(u32, u32)>()
    }

    /// The arena and its spans, for the on-disk snapshot.
    pub(crate) fn raw(&self) -> (&[u8], &[(u32, u32)]) {
        (&self.bytes, &self.spans)
    }

    /// Rebuilds from a snapshot. Spans are not validated here because `get`
    /// already treats an out-of-range or non-UTF-8 span as an empty string, so
    /// a damaged file reads as blank fields rather than as a panic.
    pub(crate) fn from_raw(bytes: Vec<u8>, spans: Vec<(u32, u32)>) -> Self {
        Self { bytes, spans }
    }
}

/// Interning table for lookup columns (artist, album, genre, label, key).
///
/// rekordbox already normalises these into their own tables, so a row holds a
/// `u32` id and the name is stored once.
#[derive(Debug, Default, Clone)]
pub struct Interner {
    names: StrColumn,
    /// Lowercased for sorting and search, in the same row order.
    folded: StrColumn,
}

impl Interner {
    pub(crate) fn parts(&self) -> (&StrColumn, &StrColumn) {
        (&self.names, &self.folded)
    }

    pub(crate) fn from_parts(names: StrColumn, folded: StrColumn) -> Self {
        Self { names, folded }
    }

    pub fn push(&mut self, name: &str) -> u32 {
        let id = u32::try_from(self.names.len()).unwrap_or(u32::MAX);
        self.names.push(name);
        self.folded.push(&fold(name));
        id
    }

    #[inline]
    pub fn name(&self, id: u32) -> &str {
        if id == u32::MAX { "" } else { self.names.get(id as usize) }
    }

    #[inline]
    pub fn folded(&self, id: u32) -> &str {
        if id == u32::MAX { "" } else { self.folded.get(id as usize) }
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    pub fn heap_bytes(&self) -> usize {
        self.names.heap_bytes() + self.folded.heap_bytes()
    }
}

/// Case- and accent-insensitive folding used for both sorting and search.
///
/// Deliberately ASCII-fast: the common case is Latin text, and we only need a
/// consistent ordering, not a locale-correct collation. Punctuation and
/// symbols are dropped, as they are for Latin text, but a letter or digit
/// outside the Latin tables (kana, kanji, Cyrillic, and so on) is kept and
/// lowercased rather than silently discarded: dropping it made a query made
/// entirely of such characters fold to "", which callers treat as no filter
/// at all (see `rbxport#891`).
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            'a'..='z' | '0'..='9' | ' ' => out.push(ch),
            'A'..='Z' => out.push(ch.to_ascii_lowercase()),
            _ => {
                let deaccented = deaccent(ch);
                if !deaccented.is_empty() {
                    out.extend(deaccented.iter().copied());
                } else if ch.is_alphanumeric() {
                    out.extend(ch.to_lowercase());
                }
            }
        }
    }
    out
}

/// Case and width folding for Smart Playlist string comparisons.
///
/// Unlike search folding, punctuation and whitespace remain significant.
/// Rekordbox's rule comparator ignores ASCII case, precomposed accents,
/// combining marks, and full-width ASCII differences.
pub(crate) fn fold_smart(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.split('\0').next().unwrap_or("").chars() {
        if ('\u{0300}'..='\u{036f}').contains(&ch) {
            continue;
        }

        let ch = match ch {
            '\u{ff01}'..='\u{ff5e}' => char::from_u32(u32::from(ch) - 0xfee0).unwrap_or(ch),
            '\u{3000}' => ' ',
            '\u{30a1}'..='\u{30f6}' => char::from_u32(u32::from(ch) - 0x60).unwrap_or(ch),
            '\u{03c2}' => '\u{03c3}',
            '\u{212b}' => 'a',
            '\u{0130}' => 'i',
            _ => ch,
        };

        match ch {
            'A'..='Z' => out.push(ch.to_ascii_lowercase()),
            '\u{fb00}' => out.push_str("ff"),
            '\u{fb01}' => out.push_str("fi"),
            '\u{fb02}' => out.push_str("fl"),
            '\u{fb03}' => out.push_str("ffi"),
            '\u{fb04}' => out.push_str("ffl"),
            '\u{fb05}' | '\u{fb06}' => out.push_str("st"),
            'œ' | 'Œ' => out.push_str("oe"),
            _ => {
                let deaccented = deaccent(ch);
                if deaccented.is_empty() {
                    out.extend(ch.to_lowercase());
                } else {
                    out.extend(deaccented.iter().copied());
                }
            }
        }
    }
    out
}

/// Maps the accented Latin-1/Latin-A range onto ASCII; anything else is kept
/// lowercased so non-Latin titles still sort and match consistently.
#[allow(
    clippy::match_same_arms,
    reason = "one arm per vowel group reads better than a merged arm"
)]
fn deaccent(ch: char) -> &'static [char] {
    match ch {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => &['a'],
        'è' | 'é' | 'ê' | 'ë' | 'È' | 'É' | 'Ê' | 'Ë' => &['e'],
        'ì' | 'í' | 'î' | 'ï' | 'Ì' | 'Í' | 'Î' | 'Ï' => &['i'],
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' => &['o'],
        'ù' | 'ú' | 'û' | 'ü' | 'Ù' | 'Ú' | 'Û' | 'Ü' => &['u'],
        'ñ' | 'Ñ' => &['n'],
        'ç' | 'Ç' => &['c'],
        'ß' => &['s', 's'],
        'æ' | 'Æ' => &['a', 'e'],
        'ø' | 'Ø' => &['o'],
        _ => &[],
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn packs_and_returns_strings() {
        let mut c = StrColumn::default();
        c.push("Airplane Mode");
        c.push("");
        c.push("Ébano");
        assert_eq!(c.get(0), "Airplane Mode");
        assert_eq!(c.get(1), "");
        assert_eq!(c.get(2), "Ébano");
        assert_eq!(c.get(99), "");
    }

    #[test]
    fn folds_case_and_accents() {
        assert_eq!(fold("Björk"), "bjork");
        assert_eq!(fold("DÉJÀ VU"), "deja vu");
        assert_eq!(fold("Straße"), "strasse");
        assert_eq!(fold("Tiësto"), "tiesto");
    }

    #[test]
    fn folding_drops_punctuation_so_search_ignores_it() {
        assert_eq!(fold("Wh0 - House (Remix)"), "wh0  house remix");
    }

    #[test]
    fn folding_preserves_non_latin_letters_instead_of_dropping_them() {
        // rbxport#891: a Japanese-only query used to fold to "", which the
        // track view treats as no filter at all.
        assert_eq!(fold("ナイト・テスト曲"), "ナイト・テスト曲".replace('・', ""));
        assert!(!fold("ナイト・テスト曲").is_empty());
    }

    #[test]
    fn smart_folding_preserves_punctuation_and_normalizes_width() {
        assert_eq!(fold_smart("Ａlphá-Beta"), "alpha-beta");
        assert_eq!(fold_smart("カタカナ"), fold_smart("かたかな"));
        assert_eq!(fold_smart("Straße"), "strasse");
        assert_eq!(fold_smart("before\0after"), "before");
    }

    #[test]
    fn interner_returns_empty_for_the_null_id() {
        let mut i = Interner::default();
        let a = i.push("ARTBAT");
        assert_eq!(i.name(a), "ARTBAT");
        assert_eq!(i.name(u32::MAX), "");
        assert_eq!(i.folded(a), "artbat");
    }
}
