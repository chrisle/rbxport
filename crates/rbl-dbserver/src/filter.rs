//! CDJ track-filter property messages. Setters carry little-endian values;
//! getters wrap the same conditions in big-endian FCND records.
//!
//! My Tag conditions are kept apart from the property records: rekordbox
//! holds one condition per My Tag category (property `0x19`), set by
//! `3307`/`3407` rather than `3207`, and its `3107` reply did not change
//! when tags were ticked [OBS: #316 captures, rekordbox 7 serving an
//! XDJ-RX3 1.20].
use std::collections::BTreeMap;

/// The My Tag mode for "every ticked tag" (`TrackFilter::operateSign` 16).
pub const MY_TAG_AND: u8 = 16;
/// The My Tag mode for "any ticked tag" (`TrackFilter::operateSign` 17).
pub const MY_TAG_OR: u8 = 17;

/// rekordbox drops a My Tag condition holding more tags than this from the
/// filter, so the category stops narrowing the list [OBS static: rekordbox
/// 7.2.19 `TrackFilter::TrackFilter` @`0x100e77f38`, `count <= 0x32` for
/// property `0x19`].
const MY_TAG_FILTER_LIMIT: usize = 50;

/// The most ticked tags a category holds here. rekordbox has no limit
/// (`FilterSettingManager::addMyTagConditionParam` @`0x100e76a70` grows the
/// list); the ids come off the network, so RBX refuses a tick past this
/// rather than grow without bound [ASSUME: RBX policy, far above any real
/// category].
const MY_TAG_TICK_LIMIT: usize = 4096;

/// One My Tag category's condition: whether it is on, AND or OR, and the
/// tags ticked in it, in the order they were ticked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MyTagCondition {
    /// The `on` byte the player sent; bit 0 turns the category on.
    pub flags: u8,
    pub mode: u8,
    pub tags: Vec<u32>,
}

impl Default for MyTagCondition {
    /// Off, AND, nothing ticked: rekordbox's starting condition for every
    /// category (`FilterSettingManager::setDefaultMyTagConditions`
    /// @`0x100e77a0c`).
    fn default() -> Self {
        Self { flags: 0, mode: MY_TAG_AND, tags: Vec::new() }
    }
}

impl MyTagCondition {
    pub fn enabled(&self) -> bool {
        self.flags & 1 != 0
    }

    /// Whether the condition narrows a list at all: on, with between one
    /// and fifty tags ticked (`TrackFilter::TrackFilter` keeps it, and
    /// `getFilteringResult` @`0x100e78530` skips one with no tags).
    fn applies(&self) -> bool {
        self.enabled() && (1..=MY_TAG_FILTER_LIMIT).contains(&self.tags.len())
    }

    /// rekordbox's `operateSign` for a track's tags: 16 needs every ticked
    /// tag on the track, 17 any one; a track with no tags passes neither.
    fn matches(&self, track: &[u32]) -> bool {
        if track.is_empty() {
            return false;
        }
        match self.mode {
            MY_TAG_AND => self.tags.iter().all(|tag| track.contains(tag)),
            MY_TAG_OR => self.tags.iter().any(|tag| track.contains(tag)),
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
struct Condition {
    flags: u8,
    operator: u8,
    values: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct TrackFilter {
    pub enabled: bool,
    conditions: BTreeMap<u16, Condition>,
    /// By category id; a category not here has the default condition.
    my_tags: BTreeMap<u32, MyTagCondition>,
}

impl Default for TrackFilter {
    fn default() -> Self {
        let conditions = [
            // XDJ-XZ 1.26 initializes the Track Filter to BPM 130.0 at
            // +/-6%, Camelot key 8B (wire id 16), rating 3, and no colour
            // [OBS static: InitDeviceFilterInfo @0x002135e0]. The BPM and
            // key setters encode these as inclusive ranges / compatible
            // key ids; keep the returned defaults consistent with those
            // values even while the conditions are disabled.
            (6, 6, vec![12220, 13780]),
            (7, 0, vec![3, 0]),
            (12, 17, vec![16, 15, 14, 18]),
            (15, 17, vec![0]),
        ]
        .into_iter()
        .map(|(id, operator, values)| {
            (
                id,
                Condition {
                    flags: 0,
                    operator,
                    values,
                },
            )
        })
        .collect();
        Self {
            enabled: false,
            conditions,
            my_tags: BTreeMap::new(),
        }
    }
}

impl TrackFilter {
    pub fn update(&mut self, property: u32, bytes: &[u8]) -> bool {
        let Ok(property) = u16::try_from(property) else {
            return false;
        };
        if !self.conditions.contains_key(&property) || bytes.len() < 4 {
            return false;
        }
        let count = usize::from(u16::from_le_bytes([bytes[2], bytes[3]]));
        if count > 24 || bytes.len() != 4 + count * 4 {
            return false;
        }
        let operator = bytes[1];
        if !matches!(operator, 0..=7 | 16 | 17) {
            return false;
        }
        let values = bytes[4..]
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        self.conditions.insert(
            property,
            Condition {
                flags: bytes[0],
                operator,
                values,
            },
        );
        true
    }

    /// A category's My Tag condition.
    pub fn my_tag_condition(&self, category: u32) -> MyTagCondition {
        self.my_tags.get(&category).cloned().unwrap_or_default()
    }

    /// Turns a category on or off and sets its mode, as
    /// `FilterSettingManager::setMyTagCondition` (@`0x100e76a18`) does:
    /// the ticked tags are kept. The caller has checked that the category
    /// exists and that the mode is 16 or 17.
    pub fn set_my_tag_condition(&mut self, category: u32, flags: u8, mode: u8) {
        let condition = self.my_tags.entry(category).or_default();
        condition.flags = flags;
        condition.mode = mode;
    }

    /// Ticks (`on`) or unticks a tag in a category, as
    /// `FilterSettingManager::addMyTagConditionParam` (@`0x100e76a70`) and
    /// `deleteMyTagConditionParam` (@`0x100e76bb0`) do: a tag already ticked
    /// is not added twice, and unticking one that is not ticked succeeds.
    /// rekordbox does not check that the tag is under the category. `false`
    /// when the category already holds the most ticks RBX keeps.
    pub fn tick_my_tag(&mut self, category: u32, tag: u32, on: bool) -> bool {
        let condition = self.my_tags.entry(category).or_default();
        if !on {
            if let Some(at) = condition.tags.iter().position(|t| *t == tag) {
                condition.tags.remove(at);
            }
            return true;
        }
        if condition.tags.contains(&tag) {
            return true;
        }
        if condition.tags.len() >= MY_TAG_TICK_LIMIT {
            return false;
        }
        condition.tags.push(tag);
        true
    }

    /// Unticks every tag in a category (`3407` with tag `0xffffffff`).
    pub fn clear_my_tags(&mut self, category: u32) {
        self.my_tags.entry(category).or_default().tags.clear();
    }

    /// Whether any My Tag condition would narrow a list.
    pub fn has_my_tag_conditions(&self) -> bool {
        self.my_tags.values().any(MyTagCondition::applies)
    }

    /// Whether a track with these My Tags passes every category that is on:
    /// categories combine with AND, as all of rekordbox's filter conditions
    /// do (`TrackFilter::getFilteringResult` @`0x100e783a0`). The master
    /// switch is the caller's to check.
    pub fn matches_my_tags(&self, track: &[u32]) -> bool {
        self.my_tags
            .values()
            .filter(|c| c.applies())
            .all(|c| c.matches(track))
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for (&property, c) in &self.conditions {
            let payload_len = 20 + u32::try_from(c.values.len()).unwrap_or(0) * 4;
            out.extend_from_slice(&(payload_len + 8).to_le_bytes());
            out.extend_from_slice(&1_u32.to_le_bytes());
            out.extend_from_slice(b"FCND");
            out.extend_from_slice(&20_u32.to_be_bytes());
            out.extend_from_slice(&payload_len.to_be_bytes());
            out.extend_from_slice(&0_u16.to_be_bytes());
            out.extend_from_slice(&property.to_be_bytes());
            out.extend_from_slice(&[c.flags, c.operator]);
            out.extend_from_slice(&u16::try_from(c.values.len()).unwrap_or(0).to_be_bytes());
            for value in &c.values {
                out.extend_from_slice(&value.to_be_bytes());
            }
        }
        out
    }

    pub fn matches(&self, bpm: u32, key: u32, rating: u32, colour: u32) -> bool {
        !self.enabled
            || self.conditions.iter().all(|(&property, c)| {
                if c.flags & 1 == 0 {
                    return true;
                }
                let value = match property {
                    6 => bpm,
                    7 => rating,
                    12 => key,
                    15 => colour,
                    _ => return true,
                };
                if c.values.is_empty() {
                    return false;
                }
                // TrackFilter::operateSign in rekordbox 7.2.11, 0x100c944dc.
                match c.operator {
                    0 => value == c.values[0],
                    1 => value != c.values[0],
                    2 => value > c.values[0],
                    3 => value >= c.values[0],
                    4 => value < c.values[0],
                    5 => value <= c.values[0],
                    6 => (c.values[0]..=*c.values.get(1).unwrap_or(&0)).contains(&value),
                    7 => !(c.values[0]..=*c.values.get(1).unwrap_or(&0)).contains(&value),
                    16 => c.values.iter().all(|&expected| expected == value),
                    17 => c.values.contains(&value),
                    _ => false,
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn property_setters_roundtrip_to_captured_fcnd_records() {
        let mut filter = TrackFilter::default();
        let set = |flags, op, values: &[u32]| {
            let mut bytes = vec![flags, op, u8::try_from(values.len()).unwrap_or(0), 0];
            for value in values {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            bytes
        };
        assert!(filter.update(6, &set(1, 6, &[12690, 14310])));
        assert!(filter.update(12, &set(1, 17, &[19, 20, 17, 21])));
        assert!(filter.update(7, &set(0, 0, &[3, 0])));
        assert!(filter.update(15, &set(0, 17, &[0])));
        let encoded = filter.encode();
        // BPM record captured from rekordbox 7.2.11, 2026-09-20.
        assert_eq!(
            &encoded[..36],
            &[
                36, 0, 0, 0, 1, 0, 0, 0, 70, 67, 78, 68, 0, 0, 0, 20, 0, 0, 0, 28, 0, 0, 0, 6, 1,
                6, 0, 2, 0, 0, 49, 146, 0, 0, 55, 230,
            ]
        );
        filter.enabled = true;
        assert!(filter.matches(13000, 19, 0, 0));
        assert!(!filter.matches(12000, 19, 0, 0));
        assert!(!filter.matches(13000, 1, 0, 0));
        let before = filter.encode();
        assert!(!filter.update(6, &[1, 6, 2, 0, 1]));
        assert!(!filter.update(99, &set(1, 17, &[1])));
        assert_eq!(filter.encode(), before);
    }

    #[test]
    fn default_property_records_match_xdj_xz_filter_initialization() {
        let encoded = TrackFilter::default().encode();
        let mut expected = Vec::new();
        let mut record = |property: u16, operator: u8, values: &[u32]| {
            let payload_len = 20 + values.len() * 4;
            expected.extend_from_slice(&u32::try_from(payload_len + 8).unwrap_or(0).to_le_bytes());
            expected.extend_from_slice(&1_u32.to_le_bytes());
            expected.extend_from_slice(b"FCND");
            expected.extend_from_slice(&20_u32.to_be_bytes());
            expected.extend_from_slice(&u32::try_from(payload_len).unwrap_or(0).to_be_bytes());
            expected.extend_from_slice(&0_u16.to_be_bytes());
            expected.extend_from_slice(&property.to_be_bytes());
            expected.extend_from_slice(&[0, operator]);
            expected.extend_from_slice(&u16::try_from(values.len()).unwrap_or(0).to_be_bytes());
            for value in values {
                expected.extend_from_slice(&value.to_be_bytes());
            }
        };
        record(6, 6, &[12_220, 13_780]);
        record(7, 0, &[3, 0]);
        record(12, 17, &[16, 15, 14, 18]);
        record(15, 17, &[0]);
        assert_eq!(encoded, expected);
    }
    #[test]
    fn rating_operators_and_colour_ids_match_the_server_comparisons() {
        let mut filter = TrackFilter {
            enabled: true,
            ..TrackFilter::default()
        };
        assert!(filter.update(7, &[1, 0, 2, 0, 3, 0, 0, 0, 0, 0, 0, 0]));
        assert!(filter.matches(12000, 1, 3, 0));
        assert!(!filter.matches(12000, 1, 4, 0));
        assert!(filter.update(7, &[1, 3, 2, 0, 3, 0, 0, 0, 0, 0, 0, 0]));
        assert!(filter.matches(12000, 1, 4, 0));
        assert!(filter.update(15, &[1, 17, 2, 0, 2, 0, 0, 0, 5, 0, 0, 0]));
        assert!(filter.matches(12000, 1, 4, 5));
        assert!(!filter.matches(12000, 1, 4, 3));
    }

    #[test]
    fn my_tag_ticks_stop_at_the_bound_and_stay_out_of_the_property_records() {
        let mut filter = TrackFilter::default();
        let before = filter.encode();
        for tag in 0..4096_u32 {
            assert!(filter.tick_my_tag(2, tag, true));
        }
        assert!(!filter.tick_my_tag(2, u32::MAX - 1, true));
        assert!(filter.tick_my_tag(2, 0, true), "a tick already held is no growth");
        assert_eq!(filter.my_tag_condition(2).tags.len(), MY_TAG_TICK_LIMIT);
        filter.set_my_tag_condition(2, 1, MY_TAG_OR);
        assert_eq!(filter.encode(), before);
        filter.clear_my_tags(2);
        assert_eq!(filter.my_tag_condition(2).tags, Vec::<u32>::new());
        assert_eq!(filter.my_tag_condition(2).mode, MY_TAG_OR);
        assert_eq!(filter.my_tag_condition(9), MyTagCondition::default());
    }
}
