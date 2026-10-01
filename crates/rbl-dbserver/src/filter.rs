//! CDJ track-filter property messages. Setters carry little-endian values;
//! getters wrap the same conditions in big-endian FCND records.
use std::collections::BTreeMap;

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
}

impl Default for TrackFilter {
    fn default() -> Self {
        let conditions = [
            (6, 6, vec![12000, 12000]),
            (7, 0, vec![0, 0]),
            (12, 17, vec![1]),
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
}
