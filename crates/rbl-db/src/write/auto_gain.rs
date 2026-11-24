//! Auto Gain updates. Encoding is based on pyrekordbox's MIT-licensed
//! `DjmdMixerParam._set_db` (Dylan Jones); see docs/reference/auto-gain.md.
use super::{next_usn, set_counter, Changed, Writer};
use crate::{DbError, Result};
use rusqlite::{params, types::Value, Connection, OptionalExtension, TransactionBehavior};

#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    id: String,
    content: String,
    high: Option<i64>,
    low: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
struct CreatedRow {
    row: Row,
    uuid: String,
    // Include every stored column except the liveness/revision fields that
    // Undo/Redo own. Deleting a row must not discard another writer's work.
    snapshot: Vec<(String, Value)>,
}

/// Opaque, conflict-checked history for one atomic Auto Gain batch.
#[derive(Debug, Clone, PartialEq)]
pub struct AutoGainEdit {
    before: Vec<Row>,
    after: Vec<Row>,
    created: Vec<CreatedRow>,
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

fn read_row(conn: &Connection, content: &str) -> Result<Option<Row>> {
    let live = conn
        .query_row(
            "SELECT 1 FROM djmdContent WHERE ID=?1 AND rb_local_deleted=0",
            [content],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !live {
        return Err(DbError::WriteRefused(format!(
            "{content} is not a live track"
        )));
    }
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
    if rows.len() > 1 {
        return Err(DbError::WriteRefused(format!(
            "Auto Gain requires at most one live mixer row for {content}"
        )));
    }
    Ok(rows.into_iter().next())
}

impl Writer {
    /// Set gain as one backed-up, undoable batch. Existing rows keep their
    /// identity and Peak. A track without a live mixer row gets a fresh UUID
    /// row with unity Peak; older tombstones are never reused. No analysis
    /// runs. Duplicate track IDs and duplicate live mixer rows are refused.
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
            desired.push((
                content,
                encode_auto_gain(*db)?,
                self.rng.uuid4(),
                self.rng.uuid4(),
            ));
        }
        if desired.is_empty() {
            return Ok((
                Changed::default(),
                AutoGainEdit {
                    before: vec![],
                    after: vec![],
                    created: vec![],
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
            created: vec![],
        };
        for (content, (high, low), id, uuid) in desired {
            if let Some(before) = read_row(&tx, content)? {
                let mut after = before.clone();
                after.high = Some(i64::from(high));
                after.low = Some(i64::from(low));
                if before != after {
                    edit.before.push(before);
                    edit.after.push(after);
                }
            } else {
                edit.created.push(CreatedRow {
                    row: Row {
                        id,
                        content: content.clone(),
                        high: Some(i64::from(high)),
                        low: Some(i64::from(low)),
                    },
                    uuid,
                    snapshot: vec![],
                });
            }
        }
        let mut changed = apply(&tx, &edit.after)?;
        let added = insert(&tx, &mut edit.created)?;
        changed.rows += added.rows;
        changed.usn = changed.usn.max(added.usn);
        edit.after
            .extend(edit.created.iter().map(|created| created.row.clone()));
        check_rows(&tx, &edit.after)?;
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
        check_rows(&tx, expected)?;
        for created in &edit.created {
            if !undo && read_row(&tx, &created.row.content)?.is_some() {
                return Err(DbError::Conflict(
                    "A replacement Auto Gain row exists".into(),
                ));
            }
            if snapshot(&tx, &created.row, !undo)? != created.snapshot {
                return Err(DbError::Conflict(
                    "A created Auto Gain row changed since this edit".into(),
                ));
            }
        }
        let created_ids: std::collections::HashSet<_> = edit
            .created
            .iter()
            .map(|created| created.row.id.as_str())
            .collect();
        let existing: Vec<_> = target
            .iter()
            .filter(|row| !created_ids.contains(row.id.as_str()))
            .cloned()
            .collect();
        let mut changed = apply(&tx, &existing)?;
        let toggled = set_deleted(&tx, &edit.created, undo)?;
        changed.rows += toggled.rows;
        changed.usn = changed.usn.max(toggled.usn);
        check_rows(&tx, target)?;
        for created in &edit.created {
            if snapshot(&tx, &created.row, undo)? != created.snapshot {
                return Err(DbError::Conflict(
                    "A created Auto Gain row changed during publication".into(),
                ));
            }
        }
        tx.commit()?;
        Ok(changed)
    }
}

fn apply(conn: &Connection, rows: &[Row]) -> Result<Changed> {
    let mut changed = Changed::default();
    if rows.is_empty() {
        return Ok(changed);
    }
    let first = next_usn(conn)?;
    let stamp = rbl_core::time::now();
    for (index, row) in rows.iter().enumerate() {
        let usn = batch_usn(first, index)?;
        let count = conn.execute(
            "UPDATE djmdMixerParam SET GainHigh=?1, GainLow=?2,
            rb_local_usn=?3, updated_at=?4 WHERE ID=?5 AND ContentID=?6 AND rb_local_deleted=0",
            params![row.high, row.low, usn, stamp, row.id, row.content],
        )?;
        if count != 1 {
            return Err(DbError::Conflict("Auto Gain row was not updated".into()));
        }
        changed.rows += count;
        changed.usn = usn;
    }
    set_counter(conn, changed.usn)?;
    Ok(changed)
}

fn check_rows(conn: &Connection, rows: &[Row]) -> Result<()> {
    for row in rows {
        if read_row(conn, &row.content)?.as_ref() != Some(row) {
            return Err(DbError::Conflict(format!(
                "Auto Gain for {} changed since this edit",
                row.content
            )));
        }
    }
    Ok(())
}

fn batch_usn(first: i64, index: usize) -> Result<i64> {
    first
        .checked_add(
            i64::try_from(index)
                .map_err(|_| DbError::WriteRefused("Auto Gain USN overflow".into()))?,
        )
        .ok_or_else(|| DbError::WriteRefused("Auto Gain USN overflow".into()))
}

fn snapshot(conn: &Connection, row: &Row, deleted: bool) -> Result<Vec<(String, Value)>> {
    let mut statement = conn.prepare(
        "SELECT * FROM djmdMixerParam WHERE ID=?1 AND ContentID=?2 AND rb_local_deleted=?3",
    )?;
    let columns: Vec<_> = statement
        .column_names()
        .into_iter()
        .enumerate()
        .filter(|(_, name)| !matches!(*name, "rb_local_deleted" | "rb_local_usn" | "updated_at"))
        .map(|(index, name)| (index, name.to_owned()))
        .collect();
    Ok(
        statement.query_row(params![row.id, row.content, deleted], |stored| {
            columns
                .iter()
                .map(|(index, name)| Ok((name.clone(), stored.get(*index)?)))
                .collect::<rusqlite::Result<_>>()
        })?,
    )
}

fn insert(conn: &Connection, created: &mut [CreatedRow]) -> Result<Changed> {
    if created.is_empty() {
        return Ok(Changed::default());
    }
    let first = next_usn(conn)?;
    let stamp = rbl_core::time::now();
    let mut changed = Changed::default();
    for (index, item) in created.iter_mut().enumerate() {
        let usn = batch_usn(first, index)?;
        // AppSyncDBController::addAutoGainInfo (rekordbox 7.2.19) uses
        // separate UUIDs for ID/UUID and splits both binary32 values.
        let count = conn.execute("INSERT INTO djmdMixerParam (ID,ContentID,GainHigh,GainLow,PeakHigh,PeakLow,UUID,rb_data_status,rb_local_data_status,rb_local_deleted,rb_local_synced,rb_local_usn,created_at,updated_at) VALUES (?1,?2,?3,?4,16256,0,?5,0,0,0,0,?6,?7,?7)",
            params![item.row.id,item.row.content,item.row.high,item.row.low,item.uuid,usn,stamp])?;
        if count != 1 {
            return Err(DbError::Conflict("Auto Gain row was not created".into()));
        }
        item.snapshot = snapshot(conn, &item.row, false)?;
        if !item
            .snapshot
            .contains(&("UUID".into(), Value::Text(item.uuid.clone())))
            || !item
                .snapshot
                .contains(&("PeakHigh".into(), Value::Integer(0x3f80)))
            || !item
                .snapshot
                .contains(&("PeakLow".into(), Value::Integer(0)))
        {
            return Err(DbError::Conflict(
                "New Auto Gain row changed during publication".into(),
            ));
        }
        changed.rows += count;
        changed.usn = usn;
    }
    set_counter(conn, changed.usn)?;
    Ok(changed)
}

fn set_deleted(conn: &Connection, created: &[CreatedRow], deleted: bool) -> Result<Changed> {
    if created.is_empty() {
        return Ok(Changed::default());
    }
    let first = next_usn(conn)?;
    let stamp = rbl_core::time::now();
    let mut changed = Changed::default();
    for (index, item) in created.iter().enumerate() {
        let usn = batch_usn(first, index)?;
        let count = conn.execute("UPDATE djmdMixerParam SET rb_local_deleted=?1,rb_local_usn=?2,updated_at=?3 WHERE ID=?4 AND ContentID=?5 AND rb_local_deleted=?6",
            params![deleted,usn,stamp,item.row.id,item.row.content,!deleted])?;
        if count != 1 {
            return Err(DbError::Conflict(
                "Created Auto Gain row was not updated".into(),
            ));
        }
        changed.rows += count;
        changed.usn = usn;
    }
    set_counter(conn, changed.usn)?;
    Ok(changed)
}
