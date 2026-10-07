//! Intelligent playlists: the rule in `djmdPlaylist.SmartList`, parsed and
//! evaluated over the columns.
//!
//! rekordbox stores the rule as a small XML document:
//!
//! ```xml
//! <NODE Id="123" LogicalOperator="1" AutomaticUpdate="1">
//!   <CONDITION PropertyName="genre" Operator="8" ValueUnit="" ValueLeft="House" ValueRight=""/>
//!   <CONDITION PropertyName="bpm" Operator="5" ValueUnit="" ValueLeft="120" ValueRight="130"/>
//! </NODE>
//! ```
//!
//! `LogicalOperator` 1 is "all of the following", 2 "any of". The operators
//! are numbered as rekordbox's picker lists them: equal, not equal, greater,
//! less, in range, in the last, not in the last, contains, does not contain,
//! starts with, ends with. The property names are rekordbox's own internal
//! ones (`name` is the title, `counter` the play count, `grouping` the
//! colour, `producer` the composer, `stockDate` the date added, `myTag` a
//! My Tag).
//!
//! A `myTag` condition names the tag by its `djmdMyTag.ID` in `ValueLeft`
//! and is answered from the track's tags (`djmdSongMyTag`) [OBS: static,
//! rekordbox 7.2.19 arm64 `db::getSmartlistCondition` reads `ValueLeft` with
//! `XmlElement::getIntAttribute` for `myTag`, and `db::operate` answers that
//! property only for operator 8 (the tag is among the track's) and 9 (it is
//! not, which an untagged track satisfies); every other operator is false].
//!
//! Read with `rbl_core::xml`'s scanner: the document is a flat handful of
//! elements with quoted attributes, and a rule that does not parse is
//! simply a playlist with nothing in it.
//!
//! The format is what rekordbox 6 and 7 write, as documented by the
//! community (pyrekordbox's `smartlist` module) rather than measured against
//! this library: there was no rekordbox library on the machine that wrote
//! this, so the value conventions marked `[ASSUME]` below want checking
//! against a recorded rule the first time one is to hand.

use rbl_core::xml::{tags, Tag};

use crate::strings::fold_smart;
use crate::{Library, Row, NO_ID};

/// How a group combines its conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Logic {
    /// Every condition must hold (`LogicalOperator="1"`).
    All,
    /// Any one condition suffices (`LogicalOperator="2"`).
    Any,
}

/// The comparison a condition makes, numbered as rekordbox numbers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    Equal,
    NotEqual,
    Greater,
    Less,
    InRange,
    InLast,
    NotInLast,
    Contains,
    NotContains,
    StartsWith,
    EndsWith,
}

impl Operator {
    /// The number rekordbox stores.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::Equal => "1",
            Self::NotEqual => "2",
            Self::Greater => "3",
            Self::Less => "4",
            Self::InRange => "5",
            Self::InLast => "6",
            Self::NotInLast => "7",
            Self::Contains => "8",
            Self::NotContains => "9",
            Self::StartsWith => "10",
            Self::EndsWith => "11",
        }
    }

    /// The operator with this number.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Some(match integer(code)? {
            1 => Self::Equal,
            2 => Self::NotEqual,
            3 => Self::Greater,
            4 => Self::Less,
            5 => Self::InRange,
            6 => Self::InLast,
            7 => Self::NotInLast,
            8 => Self::Contains,
            9 => Self::NotContains,
            10 => Self::StartsWith,
            11 => Self::EndsWith,
            _ => return None,
        })
    }
}

/// What a condition looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Property {
    Artist,
    Album,
    AlbumArtist,
    OriginalArtist,
    Composer,
    Remixer,
    MixName,
    Genre,
    Label,
    Key,
    Title,
    Comment,
    FileName,
    Bpm,
    Rating,
    Color,
    PlayCount,
    Duration,
    Year,
    DateAdded,
    DateCreated,
    DateReleased,
    /// A My Tag, by its `djmdMyTag.ID`.
    MyTag,
    /// A property the index does not hold. Never matches.
    Unsupported,
}

impl Property {
    /// rekordbox's internal name for the property, the one in the XML.
    /// `Unsupported` has none; it is written as an empty name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Artist => "artist",
            Self::Album => "album",
            Self::AlbumArtist => "albumArtist",
            Self::OriginalArtist => "originalArtist",
            Self::Composer => "producer",
            Self::Remixer => "remixedBy",
            Self::MixName => "mixName",
            Self::Genre => "genre",
            Self::Label => "label",
            Self::Key => "key",
            Self::Title => "name",
            Self::Comment => "comments",
            Self::FileName => "fileName",
            Self::Bpm => "bpm",
            Self::Rating => "rating",
            Self::Color => "grouping",
            Self::PlayCount => "counter",
            Self::Duration => "duration",
            Self::Year => "year",
            Self::DateAdded => "stockDate",
            Self::DateCreated => "dateCreated",
            Self::DateReleased => "dateReleased",
            Self::MyTag => "myTag",
            Self::Unsupported => "",
        }
    }

    /// The property rekordbox calls `name` in the XML.
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        for (candidate, property) in [
            ("artist", Self::Artist),
            ("album", Self::Album),
            ("albumArtist", Self::AlbumArtist),
            ("originalArtist", Self::OriginalArtist),
            ("producer", Self::Composer),
            ("remixedBy", Self::Remixer),
            ("mixName", Self::MixName),
            ("genre", Self::Genre),
            ("label", Self::Label),
            ("key", Self::Key),
            ("name", Self::Title),
            ("comments", Self::Comment),
            ("fileName", Self::FileName),
            ("bpm", Self::Bpm),
            ("rating", Self::Rating),
            ("grouping", Self::Color),
            ("counter", Self::PlayCount),
            ("duration", Self::Duration),
            ("year", Self::Year),
            ("stockDate", Self::DateAdded),
            ("dateCreated", Self::DateCreated),
            ("dateReleased", Self::DateReleased),
            ("myTag", Self::MyTag),
        ] {
            if name.eq_ignore_ascii_case(candidate) {
                return property;
            }
        }

        Self::Unsupported
    }
}

/// One line of the rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Condition {
    pub property: Property,
    pub operator: Operator,
    pub left: String,
    pub right: String,
    /// The unit of an "in the last" count: `day`, `week`, `month` or `year`.
    pub unit: String,
}

/// A group of conditions, or of groups.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub logic: Logic,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Condition(Condition),
    Group(Group),
}

/// A parsed rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartRule {
    pub root: Group,
}

impl SmartRule {
    /// Parses the XML. `None` when its first element is not a complete `NODE`.
    #[must_use]
    pub fn parse(xml: &str) -> Option<Self> {
        let mut tags = tags(smart_document(xml)?).into_iter();
        let Tag::Open { name, attributes, closed } = tags.next()? else {
            return None;
        };
        if !name.eq_ignore_ascii_case("NODE") {
            return None;
        }

        let logic = if integer(attribute(&attributes, "LogicalOperator")) == Some(2) {
            Logic::Any
        } else {
            Logic::All
        };
        let mut root = Group { logic, items: Vec::new() };
        if closed {
            return Some(Self { root });
        }

        let mut open = vec![name];
        for tag in tags {
            match tag {
                Tag::Open { name, attributes, closed } => {
                    if open.len() == 1 && name.eq_ignore_ascii_case("CONDITION") {
                        if let Some(condition) = condition(&attributes) {
                            root.items.push(Item::Condition(condition));
                        }
                    }

                    if !closed {
                        open.push(name);
                    }
                }
                Tag::Close { .. } if open.len() == 1 => {
                    return Some(Self { root });
                }
                Tag::Close { name } => {
                    let opened = open.pop()?;
                    if !opened.eq_ignore_ascii_case(&name) {
                        return None;
                    }
                }
            }
        }
        None
    }

    /// The rule as `djmdPlaylist.SmartList` holds it, for the playlist with
    /// `id`: the root `NODE` names the playlist, `AutomaticUpdate` is on,
    /// and each condition is one `CONDITION`. The shape is the one parsed
    /// above, written back without whitespace between the elements
    /// [ASSUME: no rekordbox-written rule has been captured to copy its
    /// spacing; the reader here and rekordbox's both ignore it].
    #[must_use]
    pub fn to_xml(&self, id: u64) -> String {
        use std::fmt::Write as _;
        fn write_group(out: &mut String, group: &Group, id: Option<u64>) {
            let logic = match group.logic {
                Logic::All => "1",
                Logic::Any => "2",
            };
            match id {
                Some(id) => {
                    let _ = write!(out, "<NODE Id=\"{id}\" LogicalOperator=\"{logic}\" AutomaticUpdate=\"1\">");
                }
                None => {
                    let _ = write!(out, "<NODE LogicalOperator=\"{logic}\">");
                }
            }
            for item in &group.items {
                match item {
                    Item::Condition(c) => {
                        let _ = write!(
                            out,
                            "<CONDITION PropertyName=\"{}\" Operator=\"{}\" ValueUnit=\"{}\" ValueLeft=\"{}\" ValueRight=\"{}\"/>",
                            c.property.name(),
                            c.operator.code(),
                            rbl_core::xml::escape(&c.unit),
                            rbl_core::xml::escape(&c.left),
                            rbl_core::xml::escape(&c.right),
                        );
                    }
                    Item::Group(g) => write_group(out, g, None),
                }
            }
            out.push_str("</NODE>");
        }
        let mut out = String::with_capacity(128 + self.root.items.len() * 96);
        write_group(&mut out, &self.root, Some(id));
        out
    }

    /// How many conditions name something the index cannot answer.
    #[must_use]
    pub fn unsupported(&self) -> usize {
        fn count(group: &Group) -> usize {
            group
                .items
                .iter()
                .map(|item| match item {
                    Item::Condition(c) => usize::from(c.property == Property::Unsupported),
                    Item::Group(g) => count(g),
                })
                .sum()
        }
        count(&self.root)
    }

    /// The rows of the library the rule admits, in collection order.
    #[must_use]
    pub fn evaluate(&self, library: &Library) -> Vec<Row> {
        self.evaluate_on(library, &Date::today())
    }

    /// As [`evaluate`](Self::evaluate), with the date the relative
    /// conditions count back from.
    #[must_use]
    pub fn evaluate_on(&self, library: &Library, today: &Date) -> Vec<Row> {
        let compiled = CompiledGroup::from(&self.root, library, today);
        let count = u32::try_from(library.len()).unwrap_or(u32::MAX);
        (0..count).filter(|&row| compiled.matches(library, row)).collect()
    }
}

fn smart_document(mut xml: &str) -> Option<&str> {
    loop {
        xml = xml.trim_start_matches(char::is_whitespace);
        if let Some(comment) = xml.strip_prefix("<!--") {
            let end = comment.find("-->")?;
            xml = comment.get(end + 3..)?;
            continue;
        }
        if let Some(instruction) = xml.strip_prefix("<?") {
            let end = instruction.find("?>")?;
            xml = instruction.get(end + 2..)?;
            continue;
        }

        return xml.starts_with('<').then_some(xml);
    }
}

fn condition(attributes: &[(String, String)]) -> Option<Condition> {
    let operator = Operator::from_code(attribute(attributes, "Operator"))?;
    Some(Condition {
        property: Property::from_name(attribute(attributes, "PropertyName")),
        operator,
        left: attribute(attributes, "ValueLeft").to_owned(),
        right: attribute(attributes, "ValueRight").to_owned(),
        unit: attribute(attributes, "ValueUnit").to_owned(),
    })
}

fn attribute<'a>(attributes: &'a [(String, String)], name: &str) -> &'a str {
    attributes
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
        .unwrap_or_default()
}

fn integer(text: &str) -> Option<i32> {
    text.trim().parse().ok()
}

// ---------------------------------------------------------------- evaluation

/// A rule with its values parsed once, so the row loop compares integers and
/// folded strings without allocating.
struct CompiledGroup {
    logic: Logic,
    items: Vec<CompiledItem>,
}

enum CompiledItem {
    Condition(CompiledCondition),
    Group(CompiledGroup),
}

struct CompiledCondition {
    property: Property,
    operator: Operator,
    /// The text value, folded, for the string properties.
    text: String,
    /// The numeric bounds, for the number and date properties: the value,
    /// or the two ends of a range, or the earliest day of "in the last".
    low: i64,
    high: i64,
    /// Whether the date operands needed by this operator converted. rekordbox
    /// excludes an invalid rule value except that a valid track date remains
    /// unequal to it.
    date_bounds_valid: bool,
}

impl CompiledGroup {
    fn from(group: &Group, library: &Library, today: &Date) -> Self {
        Self {
            logic: group.logic,
            items: group
                .items
                .iter()
                .map(|item| match item {
                    Item::Condition(c) => CompiledItem::Condition(CompiledCondition::from(c, library, today)),
                    Item::Group(g) => CompiledItem::Group(Self::from(g, library, today)),
                })
                .collect(),
        }
    }

    fn matches(&self, library: &Library, row: Row) -> bool {
        if self.items.is_empty() {
            return false;
        }

        match self.logic {
            Logic::All => self.items.iter().all(|item| item.matches(library, row)),
            Logic::Any => self.items.iter().any(|item| item.matches(library, row)),
        }
    }
}

impl CompiledItem {
    fn matches(&self, library: &Library, row: Row) -> bool {
        match self {
            Self::Condition(c) => c.matches(library, row),
            Self::Group(g) => g.matches(library, row),
        }
    }
}

impl CompiledCondition {
    fn from(condition: &Condition, library: &Library, today: &Date) -> Self {
        let mut date_bounds_valid = true;
        let (low, high) = match condition.property {
            Property::Bpm => (whole(&condition.left), whole(&condition.right)),
            Property::Duration => (seconds(&condition.left), seconds(&condition.right)),
            Property::Rating | Property::PlayCount | Property::Year => {
                (whole(&condition.left), whole(&condition.right))
            }
            Property::Color => (color_id(&condition.left), color_id(&condition.right)),
            Property::MyTag => (i64::from(my_tag_key(&condition.left)), 0),
            Property::DateAdded | Property::DateCreated | Property::DateReleased => {
                match condition.operator {
                    Operator::InLast | Operator::NotInLast => {
                        let back = relative_count(&condition.left);
                        (today.minus(back, &condition.unit).days(), today.days())
                    }
                    Operator::InRange => {
                        let low = smart_date(&condition.left);
                        let high = smart_date(&condition.right);
                        date_bounds_valid = low.is_some() && high.is_some();
                        (low.unwrap_or_default(), high.unwrap_or_default())
                    }
                    _ => {
                        let low = smart_date(&condition.left);
                        date_bounds_valid = low.is_some();
                        (low.unwrap_or_default(), 0)
                    }
                }
            }
            _ => (0, 0),
        };
        let text = match condition.property {
            // A key is picked from the library's own names, so the folded
            // name is what to compare [ASSUME: rekordbox writes the name,
            // not the `djmdKey` id, into `ValueLeft`].
            Property::Key => fold_smart(condition.left.trim()),
            _ => fold_smart(&condition.left),
        };
        let _ = library;
        Self {
            property: condition.property,
            operator: condition.operator,
            text,
            low,
            high,
            date_bounds_valid,
        }
    }

    #[inline]
    fn matches(&self, lib: &Library, row: Row) -> bool {
        let index = row as usize;
        match self.property {
            Property::Artist => self.text_matches(
                lib.artists
                    .name(lib.artist.get(index).copied().unwrap_or(NO_ID)),
            ),
            Property::Album => self.text_matches(
                lib.albums
                    .name(lib.album.get(index).copied().unwrap_or(NO_ID)),
            ),
            Property::AlbumArtist => self.text_matches(lib.search_extra[1].get(index)),
            Property::OriginalArtist => self.text_matches(lib.search_extra[3].get(index)),
            Property::Composer => self.text_matches(lib.search_extra[0].get(index)),
            Property::Remixer => self.text_matches(lib.search_extra[2].get(index)),
            Property::MixName => self.text_matches(lib.search_extra[4].get(index)),
            Property::Genre => self.text_matches(
                lib.genres
                    .name(lib.genre.get(index).copied().unwrap_or(NO_ID)),
            ),
            Property::Label => self.text_matches(
                lib.labels
                    .name(lib.label.get(index).copied().unwrap_or(NO_ID)),
            ),
            Property::Key => {
                self.text_matches(lib.keys.name(lib.key.get(index).copied().unwrap_or(NO_ID)))
            }
            Property::Title => self.text_matches(lib.title.get(index)),
            // Not folded ahead of time: comments and file names are searched
            // by the query through the haystack, not compared on their own,
            // so a rule on them folds per row. A few milliseconds over the
            // whole library, once per open, not per keystroke.
            Property::Comment => self.text_matches(lib.comment.get(index)),
            Property::FileName => self.text_matches(lib.file_name.get(index)),
            Property::Bpm => {
                self.number_matches(i64::from(lib.bpm_x100.get(index).copied().unwrap_or(0)))
            }
            Property::Rating => {
                self.number_matches(i64::from(lib.rating.get(index).copied().unwrap_or(0)))
            }
            Property::Color => {
                self.number_matches(i64::from(lib.color.get(index).copied().unwrap_or(0)))
            }
            Property::PlayCount => {
                self.number_matches(i64::from(lib.play_count.get(index).copied().unwrap_or(0)))
            }
            Property::Duration => {
                self.number_matches(i64::from(lib.length_sec.get(index).copied().unwrap_or(0)))
            }
            Property::Year => {
                self.number_matches(i64::from(lib.year.get(index).copied().unwrap_or(0)))
            }
            Property::DateAdded | Property::DateCreated => {
                self.date_matches(lib.date_added.get(index))
            }
            Property::DateReleased => self.date_matches(lib.release_date.get(index)),
            Property::MyTag => self.tag_matches(lib.my_tag_keys(index)),
            Property::Unsupported => false,
        }
    }

    fn text_matches(&self, text: &str) -> bool {
        let folded = fold_smart(text);
        let folded = folded.as_str();
        let wanted = self.text.as_str();
        match self.operator {
            Operator::Equal => folded == wanted,
            Operator::NotEqual => folded != wanted,
            Operator::Contains => {
                !folded.is_empty() && !wanted.is_empty() && folded.contains(wanted)
            }
            Operator::NotContains => {
                !folded.is_empty() && !wanted.is_empty() && !folded.contains(wanted)
            }
            Operator::StartsWith => {
                !folded.is_empty() && !wanted.is_empty() && folded.starts_with(wanted)
            }
            Operator::EndsWith => {
                !folded.is_empty() && !wanted.is_empty() && folded.ends_with(wanted)
            }
            // Ordering a name makes no sense; rekordbox does not offer it.
            Operator::Greater
            | Operator::Less
            | Operator::InRange
            | Operator::InLast
            | Operator::NotInLast => false,
        }
    }

    fn number_matches(&self, value: i64) -> bool {
        match self.operator {
            Operator::Equal => value == self.low,
            Operator::NotEqual => value != self.low,
            Operator::Greater => value > self.low,
            Operator::Less => value < self.low,
            Operator::InRange => value >= self.low && value <= self.high,
            Operator::InLast
            | Operator::NotInLast
            | Operator::Contains
            | Operator::NotContains
            | Operator::StartsWith
            | Operator::EndsWith => false,
        }
    }

    /// rekordbox's `myTag` comparison: whether the tag is among the
    /// track's. Only "contains" and "does not contain" answer; see the
    /// module notes.
    fn tag_matches(&self, keys: &[i32]) -> bool {
        let wanted = i32::try_from(self.low).unwrap_or_default();
        match self.operator {
            Operator::Contains => keys.contains(&wanted),
            Operator::NotContains => !keys.contains(&wanted),
            _ => false,
        }
    }

    fn date_matches(&self, text: &str) -> bool {
        let Some(days) = smart_date(text) else {
            return false;
        };
        if !self.date_bounds_valid {
            return self.operator == Operator::NotEqual;
        }
        match self.operator {
            Operator::Equal => days == self.low,
            Operator::NotEqual => days != self.low,
            Operator::Greater => days > self.low,
            Operator::Less | Operator::NotInLast => days < self.low,
            Operator::InRange => days >= self.low && days <= self.high,
            Operator::InLast => days >= self.low,
            Operator::Contains | Operator::NotContains | Operator::StartsWith | Operator::EndsWith => false,
        }
    }
}

/// Convert the ten-character date values used by Smart Playlists.
///
/// rekordbox reads the year, month and day from fixed positions, ignores the
/// two separators, and lets the C calendar routines normalize overflowing
/// fields. The conversion reports only positive `_mktime64` values, whose
/// supported calendar ends in 3000.
fn smart_date(text: &str) -> Option<i64> {
    if text.contains('\0') {
        return None;
    }

    let mut chars = text.chars();
    let year = positional_number(&mut chars, 4)?;
    chars.next()?;
    let month = positional_number(&mut chars, 2)?;
    chars.next()?;
    let day = positional_number(&mut chars, 2)?;
    if chars.next().is_some() {
        return None;
    }

    let total_months = year.checked_mul(12)?.checked_add(month)?.checked_sub(1)?;
    let normalized_year = total_months.div_euclid(12);
    let normalized_month = total_months.rem_euclid(12) + 1;
    let days = days_from_civil(normalized_year, normalized_month, 1).checked_add(day - 1)?;
    let normalized = Date::from_days(days);

    (days > 0 && normalized.year <= 3000).then_some(days)
}

fn positional_number(chars: &mut impl Iterator<Item = char>, width: usize) -> Option<i64> {
    (0..width).try_fold(0_i64, |value, _| {
        let character = chars.next()?;
        Some(value * 10 + i64::from(u32::from(character)) - i64::from(u32::from('0')))
    })
}

/// A duration in whole seconds, from `300` or `5:00`.
fn seconds(text: &str) -> i64 {
    let text = text.trim();
    if let Some((minutes, secs)) = text.split_once(':') {
        return whole(minutes) * 60 + whole(secs);
    }
    whole(text)
}

/// A numeric rule value, truncated and clamped to rekordbox's signed range.
#[allow(
    clippy::cast_possible_truncation,
    reason = "the clamp defines the conversion range"
)]
fn whole(text: &str) -> i64 {
    let value = text.trim().parse::<f64>().unwrap_or(0.0);
    (value.trunc() as i64).clamp(i64::from(i32::MIN), i64::from(i32::MAX))
}

/// A relative-date count. Unlike the other numeric fields, rekordbox accepts
/// only a whole integer here and treats invalid or negative input as zero.
fn relative_count(text: &str) -> i64 {
    text.trim().parse::<i64>().unwrap_or(0).max(0)
}

/// A My Tag id as rekordbox compares it: the text read as a 32-bit signed
/// integer the way JUCE's `String::getIntValue` reads it — leading
/// whitespace skipped, an optional sign, then decimal digits up to the
/// first non-digit, anything unreadable 0 [OBS: static, rekordbox 7.2.19
/// reads both the rule's `ValueLeft` and compares the track's tags as
/// 32-bit integers]. Ids past 2^31 wrap [ASSUME: the compiled loop's
/// two's-complement overflow; `djmdMyTag.ID` values that large have not
/// been seen in a captured library, and rekordbox's
/// `DatabaseMediator::fixMyTagIdsInSmartlistCriteria` rewrites a rule's
/// `ValueLeft` with `setAttribute(…, int)`, which would write the wrapped
/// value]. Reading the track's `djmdSongMyTag.MyTagID` the same way is
/// [ASSUME]: where rekordbox fills a track's tag list was not traced.
#[must_use]
pub fn my_tag_key(text: &str) -> i32 {
    let mut chars = text.trim_start().chars().peekable();
    let negative = match chars.peek() {
        Some('-') => {
            chars.next();
            true
        }
        Some('+') => {
            chars.next();
            false
        }
        _ => false,
    };
    let mut value: i32 = 0;
    for c in chars {
        let Some(digit) = c.to_digit(10) else { break };
        let digit = i32::try_from(digit).unwrap_or_default();
        value = value.wrapping_mul(10).wrapping_add(digit);
    }
    if negative {
        value.wrapping_neg()
    } else {
        value
    }
}

/// A colour by its `ColorID`, or by name for a rule written with one.
fn color_id(text: &str) -> i64 {
    let text = text.trim();
    if let Ok(id) = text.parse::<i64>() {
        return id;
    }
    crate::filter::COLOR_NAMES
        .iter()
        .position(|name| name.eq_ignore_ascii_case(text))
        .map_or(0, |i| i64::try_from(i).unwrap_or(0) + 1)
}

/// A calendar day, for the date conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Date {
    pub year: i64,
    pub month: i64,
    pub day: i64,
}

impl Date {
    /// The machine's local calendar date.
    #[must_use]
    pub fn today() -> Self {
        Self::parse(&rbl_core::time::local_date()).unwrap_or(Self {
            year: 1970,
            month: 1,
            day: 1,
        })
    }

    /// The first ten characters of a rekordbox date: `YYYY-MM-DD`, whatever
    /// follows.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let mut parts = text.get(..10)?.split('-');
        let year = parts.next()?.parse().ok()?;
        let month = parts.next()?.parse().ok()?;
        let day = parts.next()?.parse().ok()?;
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return None;
        }
        Some(Self { year, month, day })
    }

    /// Days since 1970-01-01, for comparing.
    #[must_use]
    pub fn days(self) -> i64 {
        days_from_civil(self.year, self.month, self.day)
    }

    /// The inclusive start of a relative-date window. A case-insensitive
    /// singular `month` steps the calendar; every other spelling counts days,
    /// with today as the first day.
    #[must_use]
    pub fn minus(self, count: i64, unit: &str) -> Self {
        if unit.eq_ignore_ascii_case("month") {
            return self.months_back(count);
        }

        self.days_ago(count.max(1) - 1)
    }

    pub(crate) fn days_ago(self, count: i64) -> Self {
        Self::from_days(self.days() - count)
    }

    fn months_back(self, count: i64) -> Self {
        let total = self.year * 12 + (self.month - 1) - count;
        let year = total.div_euclid(12);
        let month = total.rem_euclid(12) + 1;
        Self { year, month, day: self.day.min(days_in_month(year, month)) }
    }

    fn from_days(days: i64) -> Self {
        let (year, month, day) = civil_from_days(days);
        Self { year, month, day }
    }
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
    }
}

/// Hinnant's days-from-civil.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Hinnant's civil-from-days.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn a_rule_parses_into_its_conditions() {
        let rule = SmartRule::parse(
            r#"<NODE Id="1" LogicalOperator="2" AutomaticUpdate="1">
                 <CONDITION PropertyName="genre" Operator="8" ValueUnit="" ValueLeft="Tech &amp; House" ValueRight=""/>
                 <CONDITION PropertyName="bpm" Operator="5" ValueUnit="" ValueLeft="120" ValueRight="130"/>
               </NODE>"#,
        )
        .unwrap();
        assert_eq!(rule.root.logic, Logic::Any);
        assert_eq!(rule.root.items.len(), 2);
        let Item::Condition(first) = &rule.root.items[0] else { panic!() };
        assert_eq!(first.property, Property::Genre);
        assert_eq!(first.operator, Operator::Contains);
        assert_eq!(first.left, "Tech & House");
        assert_eq!(rule.unsupported(), 0);
    }

    #[test]
    fn only_direct_conditions_of_the_first_node_are_kept() {
        let rule = SmartRule::parse(
            r#"<?xml version="1.0"?><NODE LogicalOperator="1"><NODE LogicalOperator="2">
               <CONDITION PropertyName="myTag" Operator="1" ValueLeft="7" ValueRight=""/>
               </NODE><CONDITION PropertyName="rating" Operator="3" ValueLeft="3" ValueRight=""/></NODE>"#,
        )
        .unwrap();
        assert_eq!(rule.root.items.len(), 1);
        let Item::Condition(condition) = &rule.root.items[0] else { panic!() };
        assert_eq!(condition.property, Property::Rating);
        assert_eq!(rule.unsupported(), 0);

        let wrapped = SmartRule::parse(
            r#"<NODE LogicalOperator="1"><WRAP><CONDITION PropertyName="rating" Operator="3" ValueLeft="3"/></WRAP></NODE>"#,
        )
        .unwrap();
        assert_eq!(wrapped.root.items.len(), 0);
    }

    #[test]
    fn a_smart_document_requires_a_complete_first_node() {
        assert!(SmartRule::parse("").is_none());
        assert!(SmartRule::parse("<NODE/>").is_some());
        assert!(SmartRule::parse("\u{feff}<NODE/>").is_none());
        assert!(SmartRule::parse("leading<NODE/>").is_none());
        assert!(SmartRule::parse("\0<NODE/>").is_none());
        assert!(SmartRule::parse("<ROOT><NODE/></ROOT>").is_none());
        assert!(SmartRule::parse("<CONDITION/><NODE/>").is_none());
        assert!(SmartRule::parse("<NODE><CONDITION/>").is_none());
        assert!(SmartRule::parse("<NODE><CONDITION></NODE>").is_none());
        assert!(SmartRule::parse("<!--before--><?probe?><NODE></NODES>").is_some());
    }

    #[test]
    fn attributes_follow_rekordbox_case_and_integer_rules() {
        let rule = SmartRule::parse(
            r#"<NODE LogicalOperator="+2"><CONDITION PropertyName="GENRE" Operator="0001" ValueLeft="House"/></NODE>"#,
        )
        .unwrap();
        assert_eq!(rule.root.logic, Logic::Any);
        let Item::Condition(condition) = &rule.root.items[0] else { panic!() };
        assert_eq!(condition.property, Property::Genre);
        assert_eq!(condition.operator, Operator::Equal);

        let rule = SmartRule::parse(
            r#"<NODE logicaloperator="2"><CONDITION PropertyName=" genre " Operator="+1" ValueLeft="House"/></NODE>"#,
        )
        .unwrap();
        assert_eq!(rule.root.logic, Logic::All);
        let Item::Condition(condition) = &rule.root.items[0] else { panic!() };
        assert_eq!(condition.property, Property::Unsupported);
        assert_eq!(condition.operator, Operator::Equal);
    }

    #[test]
    fn a_rule_written_out_reads_back_the_same() {
        let rule = SmartRule {
            root: Group {
                logic: Logic::Any,
                items: vec![
                    Item::Condition(Condition {
                        property: Property::Genre,
                        operator: Operator::Contains,
                        left: "Tech & \"House\"".to_owned(),
                        right: String::new(),
                        unit: String::new(),
                    }),
                    Item::Condition(Condition {
                        property: Property::DateAdded,
                        operator: Operator::InLast,
                        left: "30".to_owned(),
                        right: String::new(),
                        unit: "day".to_owned(),
                    }),
                ],
            },
        };
        let xml = rule.to_xml(4_290_236_987);
        assert!(xml.starts_with("<NODE Id=\"4290236987\" LogicalOperator=\"2\" AutomaticUpdate=\"1\"><CONDITION PropertyName=\"genre\" Operator=\"8\""));
        assert_eq!(SmartRule::parse(&xml).unwrap(), rule);
    }

    #[test]
    fn relative_dates_only_treat_singular_month_as_a_calendar_unit() {
        let d = Date::parse("2026-03-31 10:00:00").unwrap();
        assert_eq!(d.minus(1, "month"), Date { year: 2026, month: 2, day: 28 });
        assert_eq!(d.minus(2, "weeks"), Date { year: 2026, month: 3, day: 30 });
        assert_eq!(d.minus(1, "MONTH"), Date { year: 2026, month: 2, day: 28 });
        assert_eq!(d.minus(1, "year"), d);
        assert_eq!(d.minus(31, "day"), Date { year: 2026, month: 3, day: 1 });
        assert_eq!(Date::parse("2024-02-29").unwrap().days(), 19_782);
        assert!(Date::parse("not a date").is_none());
    }

    #[test]
    fn relative_date_counts_require_nonnegative_integers() {
        assert_eq!(relative_count("2"), 2);
        assert_eq!(relative_count("0"), 0);
        assert_eq!(relative_count("-1"), 0);
        assert_eq!(relative_count("1.5"), 0);
        assert_eq!(relative_count("not-a-number"), 0);
    }

    #[test]
    fn smart_dates_use_fixed_positions_and_normalize_the_calendar() {
        let january_31 = smart_date("2025-01-31").unwrap();
        assert_eq!(smart_date("2025/01/31"), Some(january_31));
        assert_eq!(smart_date("2025Ω01Ω31"), Some(january_31));
        assert_eq!(smart_date("2025-02-29"), smart_date("2025-03-01"));
        assert_eq!(smart_date("2025-13-01"), smart_date("2026-01-01"));
        assert_eq!(smart_date("2025-01-00"), smart_date("2024-12-31"));
        assert_eq!(smart_date("202A-01-31"), smart_date("2037-01-31"));
    }

    #[test]
    fn smart_dates_reject_values_outside_rekordbox_conversion() {
        assert!(smart_date("1970-01-01").is_none());
        assert!(smart_date("1969-12-31").is_none());
        assert!(smart_date("9999-12-31").is_none());
        assert!(smart_date("2025-1-31").is_none());
        assert!(smart_date("2025-01-31T00:00").is_none());
        assert!(smart_date(" 2025-01-31").is_none());
        assert!(smart_date("２０２５-０１-３１").is_none());
        assert!(smart_date("2025-0\0-31").is_none());
    }

    #[test]
    fn smart_date_comparisons_require_a_valid_track_date() {
        let condition = |operator: Operator, left: &str| {
            CompiledCondition::from(
                &Condition {
                    property: Property::DateAdded,
                    operator,
                    left: left.to_owned(),
                    right: String::new(),
                    unit: String::new(),
                },
                &Library::default(),
                &Date {
                    year: 2025,
                    month: 1,
                    day: 31,
                },
            )
        };

        assert!(!condition(Operator::NotEqual, "2025-01-31").date_matches("not-a-date"));
        assert!(condition(Operator::NotEqual, "not-a-date").date_matches("2025-01-31"));
        assert!(!condition(Operator::Equal, "not-a-date").date_matches("2025-01-31"));
    }

    #[test]
    fn my_tag_ids_are_read_as_juce_reads_an_int_attribute() {
        assert_eq!(my_tag_key("700002"), 700_002);
        assert_eq!(my_tag_key("  +42abc"), 42);
        assert_eq!(my_tag_key("-7"), -7);
        assert_eq!(my_tag_key(""), 0);
        assert_eq!(my_tag_key("tag"), 0);
        // Past 2^31 the id wraps, so the wrapped form a rewritten rule holds
        // still names the same tag.
        assert_eq!(my_tag_key("3000000000"), my_tag_key("-1294967296"));
    }

    #[test]
    fn a_my_tag_condition_answers_only_contains_and_does_not_contain() {
        let rule = SmartRule::parse(
            r#"<NODE LogicalOperator="1"><CONDITION PropertyName="myTag" Operator="8" ValueLeft="12" ValueRight=""/></NODE>"#,
        )
        .unwrap();
        let Item::Condition(condition) = &rule.root.items[0] else { panic!() };
        assert_eq!(condition.property, Property::MyTag);
        assert_eq!(rule.unsupported(), 0);

        let compiled = |operator: Operator| {
            CompiledCondition::from(
                &Condition {
                    property: Property::MyTag,
                    operator,
                    left: "12".to_owned(),
                    right: String::new(),
                    unit: String::new(),
                },
                &Library::default(),
                &Date { year: 2025, month: 1, day: 31 },
            )
        };
        assert!(compiled(Operator::Contains).tag_matches(&[3, 12]));
        assert!(!compiled(Operator::Contains).tag_matches(&[3]));
        assert!(!compiled(Operator::Contains).tag_matches(&[]));
        assert!(compiled(Operator::NotContains).tag_matches(&[]));
        assert!(compiled(Operator::NotContains).tag_matches(&[3]));
        assert!(!compiled(Operator::NotContains).tag_matches(&[12]));
        for operator in [Operator::Equal, Operator::NotEqual, Operator::Greater, Operator::StartsWith] {
            assert!(!compiled(operator).tag_matches(&[12]));
            assert!(!compiled(operator).tag_matches(&[]));
        }
    }

    #[test]
    fn values_are_read_the_way_rekordbox_writes_them() {
        assert_eq!(whole("128"), 128);
        assert_eq!(whole("128.5"), 128);
        assert_eq!(whole("12800"), 12_800);
        assert_eq!(whole("0.5"), 0);
        assert_eq!(whole("2147483648"), i64::from(i32::MAX));
        assert_eq!(seconds("5:30"), 330);
        assert_eq!(seconds("330"), 330);
        assert_eq!(color_id("Aqua"), 6);
        assert_eq!(color_id("3"), 3);
        assert_eq!(rbl_core::xml::unescape("a &lt;b&gt; &#39;c&#x27; &unknown; &"), "a <b> 'c' &unknown; &");
    }
}
