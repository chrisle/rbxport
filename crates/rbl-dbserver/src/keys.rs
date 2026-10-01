//! The KEY menu: the 24 keys round the Camelot wheel, numbered as rekordbox
//! numbers them for a player (measured: the KEY menu lists ids 1–24 in this
//! order, and every track row carries its key by that id).

/// Key names by id, `1` first: `1A, 1B, 2A, 2B, … 12A, 12B` as rekordbox
/// spells them.
pub const NAMES: [&str; 24] = [
    "Abm", "B", "Ebm", "F#", "Bbm", "Db", "Fm", "Ab", "Cm", "Eb", "Gm", "Bb", "Dm", "F", "Am", "C",
    "Em", "G", "Bm", "D", "F#m", "A", "Dbm", "E",
];

/// The key's name, or "" for an id off the wheel.
pub fn name(id: u32) -> &'static str {
    usize::try_from(id)
        .ok()
        .and_then(|i| i.checked_sub(1))
        .and_then(|i| NAMES.get(i))
        .copied()
        .unwrap_or("")
}

/// Keys that mix with `id`, widening with `distance`: the key alone, then
/// with its relative major or minor, then with its two neighbours on the
/// wheel (measured: `Abm` → `Abm, B` → `Abm, B, Dbm, Ebm`).
pub fn related(id: u32, distance: u32) -> Vec<u32> {
    if !(1..=24).contains(&id) {
        return Vec::new();
    }
    // Position on the wheel (0-based hour) and whether minor (A) or major (B).
    let index = id - 1;
    let hour = index / 2;
    let minor = index % 2 == 0;
    let at = |hour: u32, minor: bool| (hour % 12) * 2 + u32::from(!minor) + 1;
    let mut out = vec![id];
    if distance >= 1 {
        out.push(at(hour, !minor));
    }
    if distance >= 2 {
        out.push(at((hour + 11) % 12, minor));
        out.push(at(hour + 1, minor));
    }
    out
}

/// The related-keys menu row for a distance: the names joined with ", ".
pub fn related_text(id: u32, distance: u32) -> String {
    related(id, distance)
        .into_iter()
        .map(name)
        .collect::<Vec<_>>()
        .join(", ")
}
