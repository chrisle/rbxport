//! Carry only Device Library menu ownership when no `OneLibrary` exists.
use std::{collections::BTreeSet, path::Path};
use rbl_onelibrary::{build::reference::MENU_ITEMS, settings::{MenuSlot, StickSettings}};
use rbl_pdb::{PageType, Pdb, TableRef, build::PAGE_HEADER_LEN};
use crate::{ExportError, Result};

fn invalid() -> ExportError {
    ExportError::Conflict("The Device Library's category, sort or column settings are malformed or unsupported; its settings were left unchanged.".into())
}
fn u16_at(bytes: &[u8], at: usize) -> u16 { u16::from_le_bytes([bytes[at], bytes[at + 1]]) }
fn u32_at(bytes: &[u8], at: usize) -> u32 { u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) }

/// Unlike the observational PDB reader, a writer must not silently drop an
/// unreadable page or borrow the next row's bytes for a truncated menu row.
fn rows<'a>(pdb: &Pdb<'a>, bytes: &'a [u8], table: &TableRef) -> Result<Vec<&'a [u8]>> {
    let size = usize::try_from(pdb.page_size).map_err(|_| invalid())?;
    let mut seen = BTreeSet::new();
    let mut ends = std::collections::BTreeMap::new();
    let mut result = Vec::new();
    let mut index = table.first_page;
    loop {
        // Keep the same traversal bound as the existing PDB reader.
        if !seen.insert(index) || seen.len() > 100_000 { return Err(invalid()); }
        let offset = usize::try_from(index).ok().and_then(|n| n.checked_mul(size)).ok_or_else(invalid)?;
        let page = bytes.get(offset..offset.checked_add(size).ok_or_else(invalid)?).ok_or_else(invalid)?;
        let kind = match table.page_type { PageType::Categories => 17, PageType::Sorts => 18, _ => return Err(invalid()) };
        if u32_at(page, 8) != kind { return Err(invalid()); }
        if page[0x1b] & 0x40 == 0 {
            let packed = u32_at(page, 0x18) & 0x1fff;
            // Old RBXport pages used the unpacked count beyond 255 rows.
            let old = u32::from(u16_at(page, 0x22));
            let count = if packed == 0xff && old > packed && old != 0x1fff { old } else { packed };
            let count = usize::try_from(count).map_err(|_| invalid())?;
            let groups = count.div_ceil(16);
            let index_bytes = groups.checked_mul(0x24).ok_or_else(invalid)?;
            let heap_end = PAGE_HEADER_LEN + usize::from(u16_at(page, 0x1e));
            if heap_end > size.checked_sub(index_bytes).ok_or_else(invalid)? { return Err(invalid()); }
            // Decode every PRESENT entry here. The observational reader skips
            // offsets beyond the file, which would silently erase a setting.
            for group in 0..groups {
                let base = size - group * 0x24;
                let present = u16_at(page, base - 4);
                for row in 0..16.min(count - group * 16) {
                    if present & (1 << row) == 0 { continue; }
                    let start = PAGE_HEADER_LEN + usize::from(u16_at(page, base - 6 - 2 * row));
                    let end = start.checked_add(8).ok_or_else(invalid)?;
                    if end > heap_end { return Err(invalid()); }
                    if ends.insert(offset + start, offset + end).is_some() { return Err(invalid()); }
                    result.push(page.get(start..end).ok_or_else(invalid)?);
                }
            }
        }
        if index == table.last_page { break; }
        index = u32_at(page, 0x0c);
        if index == 0 { return Err(invalid()); }
    }
    if ends.iter().zip(ends.keys().skip(1)).any(|((_, end), next)| end > next) { return Err(invalid()); }
    Ok(result)
}

fn decode(rows: &[&[u8]], category: bool) -> Result<(Vec<MenuSlot>, Option<i64>)> {
    let mut ids = BTreeSet::new();
    let mut items = BTreeSet::new();
    let mut positions = BTreeSet::new();
    let mut selected = None;
    let mut slots = Vec::new();
    for row in rows {
        let menu_item = i64::from(u16_at(row, 0));
        let id = i64::from(u16_at(row, 2));
        let name = MENU_ITEMS.iter().find(|(item, _, _)| *item == menu_item).map(|(_, _, name)| *name).ok_or_else(invalid)?;
        if !ids.insert(id) || !items.insert(menu_item) { return Err(invalid()); }
        let (visible, seq) = if category {
            let reference = rbl_pdb::reference::CATEGORIES.iter().find(|known| known[..2] == row[..2]).ok_or_else(invalid)?;
            if row[4] != reference[4] || row[5] & !1 != reference[5] & !1 { return Err(invalid()); }
            (row[5] & 1 == 0, i64::from(u16_at(row, 6)))
        } else {
            if row[4] & !3 != 0 || row[6..8] != [0, 0] { return Err(invalid()); }
            if row[4] & 2 != 0 && selected.replace(menu_item).is_some() { return Err(invalid()); }
            (row[4] & 1 == 0, i64::from(row[5]))
        };
        if (visible && (seq == 0 || !positions.insert(seq))) || (!visible && seq != 0) { return Err(invalid()); }
        slots.push(MenuSlot { id, menu_item, name: name.into(), seq, visible });
    }
    Ok((slots, selected))
}

pub(crate) fn read(path: &Path, defaults: Option<&StickSettings>) -> Result<Option<StickSettings>> {
    if !path.try_exists()? { return Ok(None); }
    let bytes = std::fs::read(path)?;
    let pdb = Pdb::parse(&bytes).map_err(|_| invalid())?;
    let tables = usize::try_from(u32_at(&bytes, 8)).map_err(|_| invalid())?;
    if tables != pdb.tables.len() { return Err(invalid()); }
    let find = |kind| {
        let mut tables = pdb.tables.iter().filter(|table| table.page_type == kind);
        let first = tables.next();
        if tables.next().is_some() { Err(invalid()) } else { Ok(first) }
    };
    let category = find(PageType::Categories)?;
    let sort = find(PageType::Sorts)?;
    // Minimal old libraries with neither table own no menu choices.
    let (category, sort) = match (category, sort) {
        (None, None) => return Ok(None),
        (Some(category), Some(sort)) => (category, sort),
        _ => return Err(invalid()),
    };
    let (categories, _) = decode(&rows(&pdb, &bytes, category)?, true)?;
    let (sorts, sub_column) = decode(&rows(&pdb, &bytes, sort)?, false)?;
    if categories.is_empty() || sorts.is_empty() { return Err(invalid()); }
    // Names, colours and background retain their existing ownership rules.
    let mut settings = defaults.cloned().unwrap_or_default();
    settings.categories = categories;
    settings.sorts = sorts;
    settings.sub_column = sub_column;
    Ok(Some(settings))
}
