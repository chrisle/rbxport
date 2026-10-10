//! Minimal `DeviceSQL` page builder.
//!
//! Enough to construct valid pages for tests today, and the foundation of the
//! export writer. Laying a page out is the fiddly part of the format: rows go
//! into a heap growing forward from the page header while their offsets go into
//! an index growing *backwards* from the end of the page, sixteen to a group,
//! each group preceded by a presence bitmap.

/// Bytes of page header before the heap starts.
pub const PAGE_HEADER_LEN: usize = 0x28;
/// Bytes each row group occupies at the end of a page.
pub const ROW_GROUP_LEN: usize = 0x24;

/// The longest row a page of `page_size` bytes holds: an empty page less
/// its header, one row group, and the slack [`FileBuilder`] keeps before it
/// starts a new page. A row cannot continue onto another page, so anything
/// longer cannot be written.
#[must_use]
pub const fn max_row_len(page_size: usize) -> usize {
    page_size.saturating_sub(PAGE_HEADER_LEN + ROW_GROUP_LEN + 8)
}

/// Encodes a string in the short-ASCII `DeviceSQL` form.
///
/// The length byte is "incremented, doubled, and incremented again", which is
/// why a naive `len` byte produces strings that read as garbage.
pub fn short_ascii(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mangled = ((bytes.len() + 1) * 2 + 1).min(0xff);
    let mut out = Vec::with_capacity(bytes.len() + 1);
    out.push(u8::try_from(mangled).unwrap_or(0xff));
    out.extend_from_slice(bytes);
    out
}

/// Encodes a string in the long ASCII form rekordbox uses once the short
/// form's length byte runs out: `0x40`, a `u16` length that counts these
/// four header bytes and the text, a pad byte, then the text with no
/// terminator [OBS 7.2.11: `40 85 00 00` before a 129-byte path].
pub fn long_ascii(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let len = 4 + bytes.len();
    let mut out = Vec::with_capacity(len);
    out.push(0x40);
    out.extend_from_slice(&u16::try_from(len).unwrap_or(u16::MAX).to_le_bytes());
    out.push(0x00);
    out.extend_from_slice(bytes);
    out
}

/// Encodes a string in the long UTF-16LE form, for text ASCII cannot carry.
pub fn long_utf16le(text: &str) -> Vec<u8> {
    let units: Vec<u16> = text.encode_utf16().collect();
    // The length counts the 4-byte header and the two trailing NUL bytes.
    let len = 4 + units.len() * 2 + 2;
    let mut out = Vec::with_capacity(len);
    out.push(0x90);
    out.extend_from_slice(&u16::try_from(len).unwrap_or(u16::MAX).to_le_bytes());
    out.push(0x00);
    for unit in units {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out.extend_from_slice(&[0, 0]);
    out
}

/// Picks the encoding rekordbox would: short ASCII when it fits, long ASCII
/// for longer ASCII, UTF-16 for anything else.
pub fn device_sql_string(text: &str) -> Vec<u8> {
    if text.is_ascii() {
        if text.len() < 0x7e { short_ascii(text) } else { long_ascii(text) }
    } else {
        long_utf16le(text)
    }
}

/// How a table's data pages are filled and what their header says at
/// `0x20` and `0x22`. Every other header field is the same either way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PageStyle {
    /// `0x20` the row count, `0x22` 0 (the count again past 255 rows),
    /// and a new page once less than the next row plus eight bytes is
    /// free: rbxport's `export.pdb` pages and `exportExt.pdb`'s tag and
    /// master pages, whose bytes this style keeps as they always were.
    #[default]
    Counted,
    /// `0x20` 1 and `0x22` the last row's index, each row group's last
    /// word 0 except the last group's, which holds only the last row's
    /// bit, and rows added for as long as they and their row index fit
    /// the page to the last byte: rekordbox's `exportExt.pdb` type-4 pages
    /// [OBS: a rekordbox 7 export, #316: 222 sixteen-byte rows a page, 4
    /// bytes free, `01 00 dd 00`, the last group `ff 3f 00 20`; its last
    /// page of 128 rows `01 00 7f 00`, `ff ff 00 80`].
    LastRowIndex,
}

/// Builds one page of rows.
pub struct PageBuilder {
    page_size: usize,
    page_index: u32,
    page_type: u32,
    next_page: u32,
    heap: Vec<u8>,
    row_offsets: Vec<u16>,
    style: PageStyle,
}

impl PageBuilder {
    pub fn new(page_size: usize, page_index: u32, page_type: u32, next_page: u32) -> Self {
        Self {
            page_size,
            page_index,
            page_type,
            next_page,
            heap: Vec::new(),
            row_offsets: Vec::new(),
            style: PageStyle::Counted,
        }
    }

    /// The page with its header and fill in `style`.
    #[must_use]
    pub fn with_style(mut self, style: PageStyle) -> Self {
        self.style = style;
        self
    }

    /// Whether a row of `len` bytes still goes on this page in its style.
    pub fn fits(&self, len: usize) -> bool {
        match self.style {
            PageStyle::Counted => self.free_space() >= len + 8,
            PageStyle::LastRowIndex => {
                let heap = self.heap.len().div_ceil(4) * 4 + len;
                PAGE_HEADER_LEN + heap + groups_for(self.row_offsets.len() + 1) * ROW_GROUP_LEN <= self.page_size
            }
        }
    }

    /// Appends a row, returning its offset within the heap.
    ///
    /// Rows are aligned to four bytes, as rekordbox's own files are.
    pub fn push_row(&mut self, row: &[u8]) -> u16 {
        while !self.heap.len().is_multiple_of(4) {
            self.heap.push(0);
        }
        let offset = u16::try_from(self.heap.len()).unwrap_or(u16::MAX);
        self.heap.extend_from_slice(row);
        self.row_offsets.push(offset);
        offset
    }

    /// Bytes still available for rows, accounting for the index this page needs.
    pub fn free_space(&self) -> usize {
        let groups = self.row_offsets.len().div_ceil(16).max(1);
        let index = groups * ROW_GROUP_LEN;
        self.page_size
            .saturating_sub(PAGE_HEADER_LEN)
            .saturating_sub(self.heap.len())
            .saturating_sub(index)
    }

    pub fn row_count(&self) -> usize {
        self.row_offsets.len()
    }

    /// Renders the page. Every row is marked present.
    ///
    /// The header fields follow rekordbox's own data pages [OBS 7.2.11]:
    /// `0x18` the row count, `0x19` and `0x1a` derived from it, `0x1b` the
    /// flags `0x24`, `0x1c` the free bytes, `0x1e` the used bytes rounded
    /// up to four, `0x20` the row count again. Each row group ends in the
    /// present mask twice.
    pub fn finish(self) -> Vec<u8> {
        self.finish_with(0)
    }

    /// [`finish`](Self::finish) with the page's sequence number, which
    /// rekordbox increments as it writes pages.
    pub fn finish_with(self, sequence: u32) -> Vec<u8> {
        let mut page = vec![0_u8; self.page_size];
        let num_rows = self.row_offsets.len();

        // Header.
        page[0x04..0x08].copy_from_slice(&self.page_index.to_le_bytes());
        page[0x08..0x0c].copy_from_slice(&self.page_type.to_le_bytes());
        page[0x0c..0x10].copy_from_slice(&self.next_page.to_le_bytes());
        page[0x10..0x14].copy_from_slice(&sequence.to_le_bytes());
        let rows_u16 = u16::try_from(num_rows).unwrap_or(u16::MAX);
        page[0x18] = u8::try_from(num_rows.min(0xff)).unwrap_or(0xff);
        // Two bytes rekordbox derives from the row count [OBS 7.2.11, twelve
        // pages of 1 to 61 rows]: the count times 32 in its low byte, and
        // the count over eight.
        page[0x19] = u8::try_from((num_rows * 32) & 0xff).unwrap_or(0);
        page[0x1a] = u8::try_from((num_rows / 8).min(0xff)).unwrap_or(0xff);
        page[0x1b] = 0x24; // ordinary data page
        let used = self.heap.len().div_ceil(4) * 4;
        // rekordbox's figure counts two bytes per row and four per group
        // against the free space, not the whole 36-byte group [OBS 7.2.11:
        // 3912 free with 8 rows in 124 bytes, 3270 with 27 rows in 724].
        let free = self
            .page_size
            .saturating_sub(PAGE_HEADER_LEN)
            .saturating_sub(used)
            .saturating_sub(2 * num_rows + 4 * groups_for(num_rows));
        page[0x1c..0x1e].copy_from_slice(&u16::try_from(free).unwrap_or(0).to_le_bytes());
        page[0x1e..0x20].copy_from_slice(&u16::try_from(used).unwrap_or(u16::MAX).to_le_bytes());
        match self.style {
            PageStyle::Counted => {
                page[0x20..0x22].copy_from_slice(&rows_u16.to_le_bytes());
                if num_rows > 0xff {
                    page[0x22..0x24].copy_from_slice(&rows_u16.to_le_bytes());
                }
            }
            PageStyle::LastRowIndex => {
                page[0x20..0x22].copy_from_slice(&1_u16.to_le_bytes());
                page[0x22..0x24].copy_from_slice(&rows_u16.saturating_sub(1).to_le_bytes());
            }
        }

        // Heap.
        let end = (PAGE_HEADER_LEN + self.heap.len()).min(page.len());
        page[PAGE_HEADER_LEN..end].copy_from_slice(&self.heap[..end - PAGE_HEADER_LEN]);

        // Row index, backwards from the end, sixteen rows per group.
        let groups = num_rows.div_ceil(16);
        for group in 0..groups {
            let base = self.page_size - group * ROW_GROUP_LEN;
            let in_group = if group < groups - 1 { 16 } else { num_rows - group * 16 };
            let mut present: u16 = 0;
            for row in 0..in_group {
                present |= 1 << row;
                let offset = self.row_offsets.get(group * 16 + row).copied().unwrap_or(0);
                let at = base - (6 + 2 * row);
                if at + 2 <= page.len() {
                    page[at..at + 2].copy_from_slice(&offset.to_le_bytes());
                }
            }
            // The mask, then the mask again, or in the last-row-index
            // style only the last row's bit in the last group and 0 in
            // the others.
            let second = match self.style {
                PageStyle::Counted => present,
                PageStyle::LastRowIndex if group == groups - 1 => 1 << (in_group - 1),
                PageStyle::LastRowIndex => 0,
            };
            for (at, word) in [(base - 4, present), (base - 2, second)] {
                if at + 2 <= page.len() {
                    page[at..at + 2].copy_from_slice(&word.to_le_bytes());
                }
            }
        }

        page
    }
}

/// Row groups a page with this many rows has: sixteen rows to a group.
fn groups_for(num_rows: usize) -> usize {
    num_rows.div_ceil(16)
}

/// The page that heads every table in a rekordbox file: flags `0x64`, no
/// rows, `0x1fff` in both row-count words, `0x03ec` at `0x24`, and a body
/// naming itself, its first data page (or `0x03ffffff` when it has none),
/// then 1004 copies of `ff 1f f8 ff` [OBS 7.2.11, every table of an empty
/// and of a 61-track export]. rekordbox reads a file whose tables start
/// with a plain data page as corrupted.
fn index_page(page_size: usize, page_index: u32, page_type: u32, next_page: u32, first_data_page: Option<u32>) -> Vec<u8> {
    let mut page = vec![0_u8; page_size];
    page[0x04..0x08].copy_from_slice(&page_index.to_le_bytes());
    page[0x08..0x0c].copy_from_slice(&page_type.to_le_bytes());
    page[0x0c..0x10].copy_from_slice(&next_page.to_le_bytes());
    page[0x10..0x14].copy_from_slice(&1_u32.to_le_bytes());
    page[0x1b] = 0x64;
    page[0x20..0x22].copy_from_slice(&0x1fff_u16.to_le_bytes());
    page[0x22..0x24].copy_from_slice(&0x1fff_u16.to_le_bytes());
    page[0x24..0x26].copy_from_slice(&0x03ec_u16.to_le_bytes());
    let body = PAGE_HEADER_LEN;
    page[body..body + 4].copy_from_slice(&page_index.to_le_bytes());
    page[body + 4..body + 8].copy_from_slice(&first_data_page.unwrap_or(0x03ff_ffff).to_le_bytes());
    page[body + 8..body + 12].copy_from_slice(&0x03ff_ffff_u32.to_le_bytes());
    let mut at = body + 18;
    for _ in 0..1004 {
        if at + 4 > page.len() {
            break;
        }
        page[at..at + 4].copy_from_slice(&[0xff, 0x1f, 0xf8, 0xff]);
        at += 4;
    }
    // The run ends on a half pattern.
    if at + 2 <= page.len() {
        page[at..at + 2].copy_from_slice(&[0xff, 0x1f]);
    }
    page
}

/// Builds a whole file: header page listing the tables, then their pages.
pub struct FileBuilder {
    page_size: usize,
    /// (`page_type`, pages)
    tables: Vec<(u32, Vec<Vec<u8>>)>,
    /// The order rekordbox wrote the tables in, which is the order their
    /// pages past the first are allocated in; see [`Self::write_order`].
    write_order: Vec<u32>,
}

impl FileBuilder {
    pub fn new(page_size: usize) -> Self {
        Self { page_size, tables: Vec::new(), write_order: Vec::new() }
    }

    /// The order the tables were written in, by page type. Every table
    /// gets its index page and one data page up front, in table order; a
    /// table's further data pages and its empty candidate are allocated
    /// when it is written, so a table written early has its candidate
    /// before a later table's overflow. Types not named here follow the
    /// named ones in table order. Without this, `property` (type 19) is
    /// written first and the rest in table order, as `export.pdb` has it
    /// [OBS 7.2.11]; `exportExt.pdb` writes its type 7 before its type 3.
    pub fn write_order(&mut self, order: &[u32]) {
        self.write_order = order.to_vec();
    }

    /// Adds a table whose rows are already encoded. Its index page and its
    /// empty candidate are added when the file is finished.
    pub fn add_table(&mut self, page_type: u32, rows: &[Vec<u8>]) {
        self.add_table_numbered(page_type, rows, |_, _| {});
    }

    /// [`add_table`](Self::add_table), with each row told its index on its
    /// page before it is written: `exportExt.pdb`'s tag rows carry that
    /// index, times 32, in their third and fourth bytes, and it starts
    /// again on every page.
    pub fn add_table_numbered(&mut self, page_type: u32, rows: &[Vec<u8>], number: impl Fn(&mut Vec<u8>, u16)) {
        self.add_table_styled(page_type, rows, number, PageStyle::Counted);
    }

    /// [`add_table_numbered`](Self::add_table_numbered), with the data
    /// pages filled and headed in `style`.
    pub fn add_table_styled(&mut self, page_type: u32, rows: &[Vec<u8>], number: impl Fn(&mut Vec<u8>, u16), style: PageStyle) {
        // Page indices are assigned in `finish`, so use a placeholder for now
        // and patch the links afterwards.
        let mut pages: Vec<Vec<u8>> = Vec::new();
        let fresh = || PageBuilder::new(self.page_size, 0, page_type, 0).with_style(style);
        let mut builder = fresh();
        for row in rows {
            if !builder.fits(row.len()) {
                pages.push(std::mem::replace(&mut builder, fresh()).finish());
            }
            let mut row = row.clone();
            number(&mut row, u16::try_from(builder.row_count()).unwrap_or(u16::MAX));
            builder.push_row(&row);
        }
        pages.push(builder.finish());
        self.tables.push((page_type, pages));
    }

    /// Renders the file the way rekordbox lays one out [OBS 7.2.11]: the
    /// header page, then for each table its index page followed by its
    /// data pages — or, for a table with no rows, one zeroed page that the
    /// header names as the table's empty candidate — and after all tables
    /// one zeroed empty-candidate page for every table that had rows. Every
    /// last data page points at its table's empty candidate. The header
    /// carries the page count, `1` at `0x10`, and the sequence the pages
    /// reached, as rekordbox's does.
    pub fn finish(self) -> Vec<u8> {
        let page_size = self.page_size;
        let num_tables = self.tables.len();
        let mut out = vec![0_u8; page_size];
        let mut next_index: u32 = 1;
        let mut sequence: u32 = 1;
        // (page_type, first page, last page, empty candidate)
        let mut entries: Vec<(u32, u32, u32, u32)> = Vec::with_capacity(num_tables);
        // Tables with rows get their empty candidate after all the tables.
        let mut pages: Vec<Vec<u8>> = Vec::new();

        // Every table's index page and first page, in table order. A table
        // with rows keeps the rest of its pages back until it is "written".
        let mut held: Vec<(usize, Vec<Vec<u8>>)> = Vec::new();
        for (table_at, (page_type, mut table_pages)) in self.tables.into_iter().enumerate() {
            let has_rows = table_pages.iter().any(|p| p[0x18] != 0 || p[0x22] != 0 || p[0x23] != 0);
            let index_at = next_index;
            next_index += 1;
            if has_rows {
                let first_data = next_index;
                next_index += 1;
                pages.push(index_page(page_size, index_at, page_type, first_data, Some(first_data)));
                let mut first = table_pages.remove(0);
                first[0x04..0x08].copy_from_slice(&first_data.to_le_bytes());
                pages.push(first);
                held.push((table_at, table_pages));
                entries.push((page_type, index_at, first_data, u32::MAX));
            } else {
                // The zeroed page right after the index page is the candidate.
                let candidate = next_index;
                next_index += 1;
                pages.push(index_page(page_size, index_at, page_type, candidate, None));
                pages.push(vec![0_u8; page_size]);
                entries.push((page_type, index_at, index_at, candidate));
            }
        }
        // Then each table's further pages and its candidate, in the order
        // the tables were written: as asked, else `property` (type 19)
        // first and the rest in table order.
        let order = |page_type: u32| -> (usize, u32) {
            match self.write_order.iter().position(|&t| t == page_type) {
                Some(at) => (0, u32::try_from(at).unwrap_or(u32::MAX)),
                None if self.write_order.is_empty() && page_type == 19 => (0, 0),
                None => (1, page_type),
            }
        };
        held.sort_by_key(|&(table_at, _)| entries.get(table_at).map_or((2, 0), |e| order(e.0)));
        for (table_at, rest) in held {
            let Some(&(page_type, _, first_data, _)) = entries.get(table_at) else { continue };
            // The first data page is where the index page sent us.
            let mut last_page_at = pages.iter().position(|p| {
                crate::u4(p, 0x04) == first_data && crate::u4(p, 0x08) == page_type && p[0x1b] & 0x40 == 0
            });
            let mut last_index = first_data;
            if let Some(at) = last_page_at {
                sequence += 1;
                pages[at][0x10..0x14].copy_from_slice(&sequence.to_le_bytes());
            }
            for mut page in rest {
                let index = next_index;
                next_index += 1;
                page[0x04..0x08].copy_from_slice(&index.to_le_bytes());
                sequence += 1;
                page[0x10..0x14].copy_from_slice(&sequence.to_le_bytes());
                if let Some(at) = last_page_at {
                    pages[at][0x0c..0x10].copy_from_slice(&index.to_le_bytes());
                }
                pages.push(page);
                last_page_at = Some(pages.len() - 1);
                last_index = index;
            }
            let candidate = next_index;
            next_index += 1;
            pages.push(vec![0_u8; page_size]);
            if let Some(at) = last_page_at {
                pages[at][0x0c..0x10].copy_from_slice(&candidate.to_le_bytes());
            }
            if let Some(entry) = entries.get_mut(table_at) {
                entry.2 = last_index;
                entry.3 = candidate;
            }
        }
        // Zeroed pages at the end are not written: rekordbox's files stop
        // at the last page with anything in it, and name candidates past
        // the end [OBS 7.2.11: 41 pages in a blank export that names 45].
        while pages.last().is_some_and(|p| p.iter().all(|&b| b == 0)) {
            pages.pop();
        }

        out[0x04..0x08].copy_from_slice(&u32::try_from(page_size).unwrap_or(4096).to_le_bytes());
        out[0x08..0x0c].copy_from_slice(&u32::try_from(num_tables).unwrap_or(0).to_le_bytes());
        out[0x0c..0x10].copy_from_slice(&next_index.to_le_bytes()); // next unused page
        out[0x10..0x14].copy_from_slice(&1_u32.to_le_bytes());
        out[0x14..0x18].copy_from_slice(&sequence.to_le_bytes());
        for (i, (page_type, first, last, candidate)) in entries.into_iter().enumerate() {
            let at = 28 + i * 16;
            out[at..at + 4].copy_from_slice(&page_type.to_le_bytes());
            out[at + 4..at + 8].copy_from_slice(&candidate.to_le_bytes());
            out[at + 8..at + 12].copy_from_slice(&first.to_le_bytes());
            out[at + 12..at + 16].copy_from_slice(&last.to_le_bytes());
        }
        for page in pages {
            out.extend_from_slice(&page);
        }
        out
    }
}

/// The value an index page names in place of a first data page when its
/// table has none.
const NO_PAGE: u32 = 0x03ff_ffff;

/// Splits rows into pages the way [`FileBuilder::add_table`] does: a page
/// rolls over when its free space is less than the next row plus eight bytes.
fn paginate(page_size: usize, rows: &[Vec<u8>]) -> Vec<Vec<&[u8]>> {
    let mut pages: Vec<Vec<&[u8]>> = Vec::new();
    let mut current: Vec<&[u8]> = Vec::new();
    let mut scratch = PageBuilder::new(page_size, 0, 0, 0);
    for row in rows {
        if scratch.free_space() < row.len() + 8 && !current.is_empty() {
            pages.push(std::mem::take(&mut current));
            scratch = PageBuilder::new(page_size, 0, 0, 0);
        }
        scratch.push_row(row);
        current.push(row);
    }
    if !current.is_empty() {
        pages.push(current);
    }
    pages
}

/// Replaces every row of one table inside an existing file, leaving every
/// other table's pages byte for byte as they were.
///
/// The table keeps its index page. Its data pages are reused in chain order;
/// when the new rows need more, the table's empty candidate is taken and a
/// fresh candidate is allocated at the file's next unused page, which is how
/// [`FileBuilder`] lays a growing table out. Pages the rows no longer need
/// are zeroed and left out of the chain. Each page written takes the next
/// sequence number, and the header records the last one, the table's last
/// page, its candidate and the next unused page.
///
/// `None` when the file does not hold the table, its chain does not lead
/// from the index page to the last page the header names, or the pages it
/// would take are not free: the caller must not write anything then.
#[must_use]
pub fn replace_table(file: &[u8], page_type: u32, rows: &[Vec<u8>]) -> Option<Vec<u8>> {
    let word = |at: usize| -> Option<u32> { Some(u32::from_le_bytes(file.get(at..at + 4)?.try_into().ok()?)) };
    let page_size = usize::try_from(word(4)?).ok()?;
    if !(512..=65_536).contains(&page_size) || !page_size.is_power_of_two() {
        return None;
    }
    let num_tables = usize::try_from(word(8)?).ok()?;
    let mut next_unused = word(0x0c)?;
    let mut sequence = word(0x14)?;
    let entry_at = (0..num_tables.min(64)).map(|i| 28 + i * 16).find(|&at| word(at) == Some(page_type))?;
    let mut candidate = word(entry_at + 4)?;
    let first = word(entry_at + 8)?;
    let last = word(entry_at + 12)?;
    let page_at = |index: u32| -> Option<usize> { usize::try_from(index).ok()?.checked_mul(page_size) };

    // The index page heads the chain; it must be one, and of this table.
    let index_at = page_at(first)?;
    let index_page = file.get(index_at..index_at + page_size)?;
    if index_page[0x1b] & 0x40 == 0 || crate::u4(index_page, 0x08) != page_type {
        return None;
    }
    // The data pages, walked the way a reader walks them: from the index
    // page's next pointer to the last page the header names.
    let mut data_pages: Vec<u32> = Vec::new();
    if last != first {
        let mut at = crate::u4(index_page, 0x0c);
        loop {
            if at == first || data_pages.contains(&at) || data_pages.len() > 1_000_000 {
                return None;
            }
            let offset = page_at(at)?;
            let page = file.get(offset..offset + page_size)?;
            if page[0x1b] & 0x40 != 0 || crate::u4(page, 0x08) != page_type || crate::u4(page, 0x04) != at {
                return None;
            }
            data_pages.push(at);
            if at == last {
                break;
            }
            at = crate::u4(page, 0x0c);
        }
    }

    let chunks = paginate(page_size, rows);
    // Where each page goes: the table's own pages first, then its candidate,
    // then fresh pages at the end of the file.
    let mut assigned: Vec<u32> = Vec::with_capacity(chunks.len());
    for k in 0..chunks.len() {
        if let Some(&index) = data_pages.get(k) {
            assigned.push(index);
        } else {
            assigned.push(candidate);
            candidate = next_unused;
            next_unused = next_unused.checked_add(1)?;
        }
    }
    // Taken pages that were not the table's must hold nothing: a candidate
    // or an unused page with data in it means the file is not what the
    // header says, and writing over it would destroy another table.
    let is_free = |index: u32| -> bool {
        page_at(index).is_some_and(|offset| file.get(offset..offset + page_size).is_none_or(|p| p.iter().all(|&b| b == 0)))
    };
    if assigned.iter().filter(|i| !data_pages.contains(i)).any(|&i| !is_free(i)) || !is_free(candidate) {
        return None;
    }

    let mut out = file.to_vec();
    let needed = assigned.iter().map(|&i| page_at(i).map(|o| o + page_size)).max().flatten().unwrap_or(0);
    if needed > out.len() {
        out.resize(needed, 0);
    }
    for (k, chunk) in chunks.iter().enumerate() {
        let index = assigned[k];
        let next = assigned.get(k + 1).copied().unwrap_or(candidate);
        let mut builder = PageBuilder::new(page_size, index, page_type, next);
        for row in chunk {
            builder.push_row(row);
        }
        sequence = sequence.checked_add(1)?;
        let offset = page_at(index)?;
        out.get_mut(offset..offset + page_size)?.copy_from_slice(&builder.finish_with(sequence));
    }
    // Pages the table no longer uses hold nothing.
    for &index in data_pages.iter().skip(chunks.len()) {
        let offset = page_at(index)?;
        out.get_mut(offset..offset + page_size)?.fill(0);
    }
    // The index page points at the first data page, or at the candidate
    // with no data page named when the table is empty, as a fresh file has it.
    let (index_next, first_data) = assigned.first().map_or((candidate, NO_PAGE), |&f| (f, f));
    out.get_mut(index_at + 0x0c..index_at + 0x10)?.copy_from_slice(&index_next.to_le_bytes());
    out.get_mut(index_at + PAGE_HEADER_LEN + 4..index_at + PAGE_HEADER_LEN + 8)?.copy_from_slice(&first_data.to_le_bytes());

    let new_last = assigned.last().copied().unwrap_or(first);
    out.get_mut(0x0c..0x10)?.copy_from_slice(&next_unused.to_le_bytes());
    out.get_mut(0x14..0x18)?.copy_from_slice(&sequence.to_le_bytes());
    out.get_mut(entry_at + 4..entry_at + 8)?.copy_from_slice(&candidate.to_le_bytes());
    out.get_mut(entry_at + 12..entry_at + 16)?.copy_from_slice(&new_last.to_le_bytes());
    // rekordbox's files stop at the last page with anything in it.
    while out.len() > page_size && out[out.len() - page_size..].iter().all(|&b| b == 0) {
        out.truncate(out.len() - page_size);
    }
    Some(out)
}

/// Replaces the rows of a table that fits one data page — the colours, on
/// every export — inside an existing file, keeping the page where it is
/// and its links as they were. rekordbox does this when a colour comment
/// is renamed on the device panel [OBS 7.2.11]: the name in `export.pdb`
/// follows the one in `exportLibrary.db`. `None` when the table is not
/// there, spans more than one page, or the rows would not fit.
#[must_use]
pub fn replace_single_page_table(file: &[u8], page_type: u32, rows: &[Vec<u8>]) -> Option<Vec<u8>> {
    let page_size = usize::try_from(u32::from_le_bytes(file.get(4..8)?.try_into().ok()?)).ok()?;
    let num_tables = usize::try_from(u32::from_le_bytes(file.get(8..12)?.try_into().ok()?)).ok()?;
    let (first, last) = (0..num_tables).find_map(|i| {
        let at = 28 + i * 16;
        let entry = file.get(at..at + 16)?;
        (u32::from_le_bytes(entry[0..4].try_into().ok()?) == page_type).then(|| {
            (
                u32::from_le_bytes(entry[8..12].try_into().unwrap_or([0; 4])),
                u32::from_le_bytes(entry[12..16].try_into().unwrap_or([0; 4])),
            )
        })
    })?;
    // The index page names the data page; a table on one page has it as
    // its last page too.
    let index_at = usize::try_from(first).ok()? * page_size;
    let data_index = u32::from_le_bytes(file.get(index_at + PAGE_HEADER_LEN + 4..index_at + PAGE_HEADER_LEN + 8)?.try_into().ok()?);
    if data_index != last || data_index == 0x03ff_ffff {
        return None;
    }
    let data_at = usize::try_from(data_index).ok()? * page_size;
    let old = file.get(data_at..data_at + page_size)?;
    let next = u32::from_le_bytes(old.get(0x0c..0x10)?.try_into().ok()?);
    let sequence = u32::from_le_bytes(old.get(0x10..0x14)?.try_into().ok()?);
    let mut builder = PageBuilder::new(page_size, data_index, page_type, next);
    for row in rows {
        if builder.free_space() < row.len() + 8 {
            return None;
        }
        builder.push_row(row);
    }
    let page = builder.finish_with(sequence);
    let mut out = file.to_vec();
    out.get_mut(data_at..data_at + page_size)?.copy_from_slice(&page);
    Some(out)
}
