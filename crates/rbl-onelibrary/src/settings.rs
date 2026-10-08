//! The per-stick settings `exportLibrary.db` carries: the device name, which
//! browse categories and sort options a player offers and in what order, the
//! sub-column, and the eight colour comments.
//!
//! These are the tabs rekordbox shows for a selected device. On the stick
//! they are plain rows — `property`, `category`, `sort`, `color` — so they
//! can be read back and updated in place without rebuilding the database.
//! An export carries them forward: a sync rebuilds `exportLibrary.db` from
//! scratch, and losing a renamed colour comment on every sync would make
//! the Color tab pointless.
//!
//! What was checked against a real stick (`TEST`, rekordbox 7.2.8, read
//! read-only on 2026-09-09) [OBS]:
//!
//! - `sort.isVisible = 1` rows ordered by `sequenceNo` are exactly the Active
//!   Sort Options rekordbox lists, and the `isVisible = 0` rows are the
//!   Inactive ones. All seventeen rows agree with the capture.
//! - `category` agrees for 21 of 22 rows. The one that does not: `DATE ADDED`
//!   is `isVisible = 1, sequenceNo = 10` on the stick and in `master.db`
//!   alike, yet rekordbox draws it in the Inactive list. Which source it
//!   draws from, or whether it hides that one category regardless, is
//!   `[UNKNOWN]` — toggling a category in rekordbox and diffing the stick's
//!   `category` table against `djmdCategory` would settle it.
//! - No `sort` row has `isSelectedAsSubColumn = 1` on that stick, and the
//!   Column tab shows "Not Specified", which is consistent. The reference
//!   stick `build.rs` was transcribed from has it set on `COMMENTS`, so the
//!   column is the flag; what rekordbox lists as choices is `[ASSUME]` the
//!   sort options, since that is the table the flag lives in.
//! - `property.backGroundColorType` is "Background Color : `OneLibrary`"
//!   [OBS rekordbox 7.2.14, 2026-10-08]. It was 0 while the menu showed
//!   "Default Color" and 8 for Purple. It stayed 8 while "Background
//!   Color : Device Library" was changed twice; that menu is a byte of
//!   `export.pdb`'s `property` row (`rbl_pdb::rows::PdbProperty`). Both
//!   menus use the track-colour order: 0 Default, 1 Pink, 2 Red, 3 Orange,
//!   4 Yellow, 5 Green, 6 Aqua, 7 Blue, 8 Purple. Purple (8), Yellow (4)
//!   and Blue (7) were observed; the other values are `[ASSUME]` from that
//!   order.

use std::path::Path;

use rusqlite::{params, Connection, OpenFlags};

use crate::build::reference;
use crate::{key, unlock, Result};

/// One browse category or sort option: a row of `category` or `sort`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuSlot {
    /// `category_id` / `sort_id`: the row's own key, stable across exports.
    pub id: i64,
    /// `menuItem_id`, what the row means.
    pub menu_item: i64,
    /// The menu item's name, without the U+FFFA / U+FFFB annotation marks.
    pub name: String,
    /// Position among the visible rows; 0 for a hidden one.
    pub seq: i64,
    pub visible: bool,
}

/// A colour comment: `color.color_id` and `color.name`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorName {
    pub id: i64,
    pub name: String,
}

/// Everything the device tabs read from `exportLibrary.db`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StickSettings {
    /// `property.deviceName`.
    pub device_name: String,
    /// `property.backGroundColorType`: "Background Color : `OneLibrary`",
    /// 0 Default to 8 Purple. See the module notes.
    pub background_color_type: i64,
    /// Every `category` row, in `category_id` order.
    pub categories: Vec<MenuSlot>,
    /// Every `sort` row, in `sort_id` order.
    pub sorts: Vec<MenuSlot>,
    /// `menuItem_id` of the sort row flagged `isSelectedAsSubColumn`, if any.
    pub sub_column: Option<i64>,
    /// The eight colour comments in `color_id` order.
    pub colors: Vec<ColorName>,
}

impl Default for StickSettings {
    /// The reference stick's rows, which is what a fresh export writes.
    fn default() -> Self {
        let name_of = |menu_item: i64| {
            reference::MENU_ITEMS
                .iter()
                .find(|(id, _, _)| *id == menu_item)
                .map_or_else(String::new, |(_, _, name)| (*name).to_owned())
        };
        Self {
            device_name: String::new(),
            background_color_type: 0,
            categories: reference::CATEGORIES
                .iter()
                .map(|&(id, menu_item, seq, visible)| MenuSlot {
                    id,
                    menu_item,
                    name: name_of(menu_item),
                    seq,
                    visible: visible != 0,
                })
                .collect(),
            sorts: reference::SORTS
                .iter()
                .map(|&(id, menu_item, seq, visible, _)| MenuSlot {
                    id,
                    menu_item,
                    name: name_of(menu_item),
                    seq,
                    visible: visible != 0,
                })
                .collect(),
            sub_column: reference::SORTS
                .iter()
                .find(|(_, _, _, _, sub)| *sub != 0)
                .map(|(_, menu_item, _, _, _)| *menu_item),
            colors: reference::COLORS
                .iter()
                .enumerate()
                .map(|(i, name)| ColorName {
                    id: i64::try_from(i).unwrap_or(0) + 1,
                    name: (*name).to_owned(),
                })
                .collect(),
        }
    }
}

/// Strips the U+FFFA / U+FFFB annotation marks rekordbox wraps menu names in.
fn plain(name: &str) -> String {
    name.trim_matches(|c| c == '\u{FFFA}' || c == '\u{FFFB}').to_owned()
}

impl StickSettings {
    /// Reads the settings out of an existing `exportLibrary.db`, read-only.
    pub fn read(path: &Path) -> Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|source| crate::Error::Open { path: path.display().to_string(), source })?;
        unlock(&conn, &key::passphrase()?)?;
        Self::read_from(&conn)
    }

    fn read_from(conn: &Connection) -> Result<Self> {
        let (device_name, background_color_type): (String, i64) = conn
            .query_row(
                "SELECT COALESCE(deviceName, ''), COALESCE(backGroundColorType, 0)
                 FROM property LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or_default();

        let slots = |sql: &str| -> Result<Vec<MenuSlot>> {
            let mut stmt = conn.prepare(sql)?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(MenuSlot {
                        id: r.get(0)?,
                        menu_item: r.get(1)?,
                        name: plain(&r.get::<_, String>(2).unwrap_or_default()),
                        seq: r.get::<_, i64>(3).unwrap_or(0),
                        visible: r.get::<_, i64>(4).unwrap_or(0) != 0,
                    })
                })?
                .filter_map(std::result::Result::ok)
                .collect();
            Ok(rows)
        };
        let categories = slots(
            "SELECT c.category_id, c.menuItem_id, COALESCE(m.name, ''), c.sequenceNo, c.isVisible
             FROM category c LEFT JOIN menuItem m ON m.menuItem_id = c.menuItem_id
             ORDER BY c.category_id",
        )?;
        let sorts = slots(
            "SELECT s.sort_id, s.menuItem_id, COALESCE(m.name, ''), s.sequenceNo, s.isVisible
             FROM sort s LEFT JOIN menuItem m ON m.menuItem_id = s.menuItem_id
             ORDER BY s.sort_id",
        )?;
        let sub_column: Option<i64> = conn
            .query_row(
                "SELECT menuItem_id FROM sort WHERE isSelectedAsSubColumn = 1
                 ORDER BY sort_id LIMIT 1",
                [],
                |r| r.get(0),
            )
            .ok();

        let mut stmt = conn.prepare("SELECT color_id, COALESCE(name, '') FROM color ORDER BY color_id")?;
        let colors = stmt
            .query_map([], |r| Ok(ColorName { id: r.get(0)?, name: r.get(1)? }))?
            .filter_map(std::result::Result::ok)
            .collect();

        Ok(Self { device_name, background_color_type, categories, sorts, sub_column, colors })
    }

    /// Updates the rows in place, in one transaction.
    ///
    /// Only rows that already exist are touched; a slot naming a row the
    /// stick does not have is skipped rather than inserted, so a database
    /// from a newer rekordbox is never given rows it did not ask for.
    pub fn write(&self, path: &Path) -> Result<()> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|source| crate::Error::Open { path: path.display().to_string(), source })?;
        unlock(&conn, &key::passphrase()?)?;

        crate::durable_writes(&conn)?;
        let tx = rusqlite::Transaction::new_unchecked(&conn, rusqlite::TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE property SET deviceName = ?1, backGroundColorType = ?2",
            params![self.device_name, self.background_color_type],
        )?;
        // rekordbox numbers the visible rows 1.. in their order and gives a
        // hidden row 0 [OBS 7.2.11, BPM taken off the sort list]; the same
        // change made here and there then leaves the same rows.
        for slot in &renumbered(&self.categories) {
            tx.execute(
                "UPDATE category SET sequenceNo = ?1, isVisible = ?2 WHERE category_id = ?3",
                params![slot.seq, i64::from(slot.visible), slot.id],
            )?;
        }
        for slot in &renumbered(&self.sorts) {
            let sub = i64::from(self.sub_column == Some(slot.menu_item));
            tx.execute(
                "UPDATE sort SET sequenceNo = ?1, isVisible = ?2, isSelectedAsSubColumn = ?3
                 WHERE sort_id = ?4",
                params![slot.seq, i64::from(slot.visible), sub, slot.id],
            )?;
        }
        for color in &self.colors {
            tx.execute(
                "UPDATE color SET name = ?1 WHERE color_id = ?2",
                params![color.name, color.id],
            )?;
        }
        tx.commit()?;

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_reference_rows() {
        let settings = StickSettings::default();
        assert_eq!(settings.categories.len(), 22);
        assert_eq!(settings.sorts.len(), 17);
        assert_eq!(settings.colors.len(), 8);
        // The sub-column the reference stick carries.
        assert_eq!(settings.sub_column, Some(21));
        let artist = settings.categories.iter().find(|s| s.menu_item == 2).unwrap();
        assert_eq!(artist.name, "ARTIST");
        assert!(artist.visible);
        assert_eq!(artist.seq, 1);
    }

    #[test]
    fn annotation_marks_are_stripped_from_names() {
        assert_eq!(plain("\u{FFFA}GENRE\u{FFFB}"), "GENRE");
        assert_eq!(plain("GENRE"), "GENRE");
    }
}

/// The slots with rekordbox's numbering: visible ones 1.. in their present
/// order (by sequence, then id, so an unnumbered newcomer lands last),
/// hidden ones 0.
#[must_use]
pub fn renumbered(slots: &[MenuSlot]) -> Vec<MenuSlot> {
    let mut visible: Vec<&MenuSlot> = slots.iter().filter(|s| s.visible).collect();
    visible.sort_by_key(|s| (s.seq <= 0, s.seq, s.id));
    let mut out: Vec<MenuSlot> = slots.iter().map(|s| MenuSlot { seq: 0, ..s.clone() }).collect();
    for (i, slot) in visible.iter().enumerate() {
        if let Some(target) = out.iter_mut().find(|o| o.id == slot.id) {
            target.seq = i64::try_from(i).unwrap_or(0) + 1;
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod renumber_tests {
    use super::*;

    fn slot(id: i64, seq: i64, visible: bool) -> MenuSlot {
        MenuSlot { id, menu_item: id, name: format!("S{id}"), seq, visible }
    }

    #[test]
    fn a_hidden_row_gets_zero_and_the_rest_close_the_gap() {
        // BPM (id 4) taken off a list numbered 1..7.
        let slots = vec![slot(0, 1, true), slot(1, 2, true), slot(4, 5, false), slot(5, 6, true), slot(12, 7, true)];
        let out = renumbered(&slots);
        let seqs: Vec<(i64, i64)> = out.iter().map(|s| (s.id, s.seq)).collect();
        assert_eq!(seqs, vec![(0, 1), (1, 2), (4, 0), (5, 3), (12, 4)]);
    }

    #[test]
    fn a_newcomer_without_a_number_lands_last() {
        let slots = vec![slot(2, 0, true), slot(0, 1, true), slot(1, 2, true)];
        let out = renumbered(&slots);
        assert_eq!(out.iter().find(|s| s.id == 2).unwrap().seq, 3);
    }
}

