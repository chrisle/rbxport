//! Existing-row Auto Gain updates. Encoding is based on pyrekordbox's MIT-licensed
//! `DjmdMixerParam._set_db` (Dylan Jones); see docs/reference/auto-gain.md.
use super::{next_usn, set_counter, Changed, Writer};
use crate::{DbError, Result};
use rusqlite::{params, Connection, TransactionBehavior};

#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    id: String,
    content: String,
    high: Option<i64>,
    low: Option<i64>,
}

/// Opaque, conflict-checked history for one atomic Auto Gain batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoGainEdit {
    before: Vec<Row>,
    after: Vec<Row>,
}

impl AutoGainEdit {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.before == self.after
    }
}

/// Convert finite decibels to the two unsigned 16-bit halves of a positive
/// IEEE-754 binary32 linear amplitude. This is a storage limit, not a claimed
/// rekordbox UI range. Nonfinite input, overflow and underflow-to-zero fail closed.
pub fn encode_auto_gain(db: f64) -> Result<(u16, u16)> {
    if !db.is_finite() {
        return Err(DbError::WriteRefused(
            "Auto Gain must be finite decibels".into(),
        ));
    }
    #[allow(clippy::cast_possible_truncation)]
    let factor = 10_f64.powf(db / 20.0) as f32;
    if !factor.is_finite() || factor <= 0.0 {
        return Err(DbError::WriteRefused(
            "Auto Gain is outside positive binary32 storage range".into(),
        ));
    }
    let bytes = factor.to_bits().to_be_bytes();
    Ok((
        u16::from_be_bytes([bytes[0], bytes[1]]),
        u16::from_be_bytes([bytes[2], bytes[3]]),
    ))
}

fn read_row(conn: &Connection, content: &str) -> Result<Row> {
    let mut stmt = conn.prepare(
        "SELECT m.ID, m.GainHigh, m.GainLow FROM djmdMixerParam m
        JOIN djmdContent c ON c.ID=m.ContentID WHERE m.ContentID=?1
        AND m.rb_local_deleted=0 AND c.rb_local_deleted=0 ORDER BY m.ID",
    )?;
    let rows = stmt
        .query_map([content], |r| {
            Ok(Row {
                id: r.get(0)?,
                content: content.to_owned(),
                high: r.get(1)?,
                low: r.get(2)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if rows.len() != 1 {
        return Err(DbError::WriteRefused(format!(
            "Auto Gain requires exactly one existing live mixer row for {content}"
        )));
    }
    rows.into_iter()
        .next()
        .ok_or_else(|| DbError::WriteRefused("Missing mixer row".into()))
}

impl Writer {
    /// Update existing mixer rows only, backed up through the normal Writer
    /// policy and committed as one undoable batch. Never creates a mixer row,
    /// modifies Peak, or runs analysis. Duplicate track IDs are refused.
    pub fn set_auto_gain_with_undo(
        &mut self,
        values: &[(String, f64)],
    ) -> Result<(Changed, AutoGainEdit)> {
        let mut desired = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (content, db) in values {
            if !seen.insert(content) {
                return Err(DbError::WriteRefused("Duplicate Auto Gain track ID".into()));
            }
            desired.push((content, encode_auto_gain(*db)?));
        }
        if desired.is_empty() {
            return Ok((
                Changed::default(),
                AutoGainEdit {
                    before: vec![],
                    after: vec![],
                },
            ));
        }
        self.prepare()?;
        let tx = self
            .library
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut edit = AutoGainEdit {
            before: vec![],
            after: vec![],
        };
        for (content, (high, low)) in desired {
            let before = read_row(&tx, content)?;
            let mut after = before.clone();
            after.high = Some(i64::from(high));
            after.low = Some(i64::from(low));
            if before != after {
                edit.before.push(before);
                edit.after.push(after);
            }
        }
        let changed = apply(&tx, &edit.after)?;
        tx.commit()?;
        Ok((changed, edit))
    }

    /// Undo (`undo=true`) or redo the entire batch, refusing intervening changes
    /// to row identity, membership or gain values before changing any row.
    pub fn restore_auto_gain(&mut self, edit: &AutoGainEdit, undo: bool) -> Result<Changed> {
        if edit.is_empty() {
            return Ok(Changed::default());
        }
        self.prepare()?;
        let tx = self
            .library
            .connection_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (expected, target) = if undo {
            (&edit.after, &edit.before)
        } else {
            (&edit.before, &edit.after)
        };
        for row in expected {
            if read_row(&tx, &row.content)? != *row {
                return Err(DbError::Conflict(format!(
                    "Auto Gain for {} changed since this edit",
                    row.content
                )));
            }
        }
        let changed = apply(&tx, target)?;
        tx.commit()?;
        Ok(changed)
    }
}

fn apply(conn: &Connection, rows: &[Row]) -> Result<Changed> {
    let mut changed = Changed::default();
    if rows.is_empty() {
        return Ok(changed);
    }
    let mut usn = next_usn(conn)?;
    let stamp = rbl_core::time::now();
    for row in rows {
        changed.rows += conn.execute(
            "UPDATE djmdMixerParam SET GainHigh=?1, GainLow=?2,
            rb_local_usn=?3, updated_at=?4 WHERE ID=?5 AND ContentID=?6 AND rb_local_deleted=0",
            params![row.high, row.low, usn, stamp, row.id, row.content],
        )?;
        changed.usn = usn;
        usn = usn
            .checked_add(1)
            .ok_or_else(|| DbError::WriteRefused("Auto Gain USN overflow".into()))?;
    }
    set_counter(conn, changed.usn)?;
    Ok(changed)
}
