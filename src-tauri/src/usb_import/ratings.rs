//! Only device edits since our export baseline can replace desktop ratings.
use std::{collections::{BTreeMap, HashMap}, path::Path};
use rbl_export::{Manifest, snapshot::Snapshot};
use super::{AppResult, AppState, ImportReport, err, write_error};

pub(super) fn import(state: &AppState, root: &Path, manifest: Option<&Manifest>, matched: &HashMap<u32, String>, db_id: u64, report: &mut ImportReport) -> AppResult<()> {
    // Library Protection is checked by Sync Manager. Even an unchanged or
    // unrecognised stick must also pass the native rekordbox-running guard.
    state.write(|_| Ok(())).map_err(write_error)?;
    let Some((manifest, baseline)) = manifest.and_then(|m| m.baseline.as_ref().map(|b| (m, b))) else {
        report.warnings.push("Track ratings were not imported: no export baseline for this library was found. Sync this USB with rating import disabled first.".into());
        return Ok(());
    };
    let current = Snapshot::read(root).map_err(err)?;
    let records: HashMap<_, _> = manifest.tracks.iter().map(|t| (t.export_id, t)).collect();
    let mut changes = BTreeMap::new();
    for (name, before, after) in [
        ("Device Library", &baseline.legacy, &current.legacy),
        ("OneLibrary", &baseline.one, &current.one),
    ] {
        let Some(after) = after else { continue; };
        let Some(before) = before else {
            report.warnings.push(format!("{name} ratings were not imported: this format has no export baseline."));
            continue;
        };
        let before: HashMap<_, _> = before.tracks.iter().map(|t| (t.id, t)).collect();
        for track in &after.tracks {
            let Some(old) = before.get(&track.id) else { continue; };
            if track.rating == old.rating { continue; }
            let identity_matches = name != "OneLibrary"
                || current.identity.get(&track.id) == baseline.identity.get(&track.id)
                    && current.identity.get(&track.id).is_some_and(|(owner, id)| *owner == db_id && matched.get(&track.id).is_some_and(|local| local == &id.to_string()));
            let path_matches = records.get(&track.id).is_some_and(|record|
                record.library_id != 0 && track.path == old.path
                    && track.path.trim_start_matches('/') == record.audio.trim_start_matches('/'));
            if !identity_matches || !path_matches || !matched.contains_key(&track.id) {
                report.skipped += 1;
                report.warnings.push(format!("{name} track {} rating was skipped: its library identity or source path could not be verified.", track.id));
                continue;
            }
            if track.rating > 5 || old.rating > 5 {
                return Err(err(format!("{name} track {} has an invalid rating. Sync this USB with rating import disabled to regenerate 0–5 star ratings.", track.id)));
            }
            if let Some(previous) = changes.insert(track.id, track.rating) {
                if previous != track.rating {
                    return Err(err(format!("The device libraries disagree on the rating for track {}. Reconcile its ratings in rekordbox before importing or syncing.", track.id)));
                }
            }
        }
    }
    let baseline_ratings: HashMap<_, _> = baseline.one.as_ref().or(baseline.legacy.as_ref())
        .into_iter().flat_map(|library| &library.tracks).map(|track| (track.id, track.rating)).collect();
    let mut conflicts = 0;
    // Resolve against the live database under the edit gate, not the cached index.
    let mut edits = Vec::new();
    for (export_id, rating) in changes {
        let Some(id) = matched.get(&export_id) else { continue; };
        let desktop = state.read_db(|db| Ok(db.connection().query_row(
            "SELECT COALESCE(Rating,0) FROM djmdContent WHERE ID=?1 AND rb_local_deleted=0", [id], |row| row.get::<_, u32>(0))?)).map_err(write_error)?;
        if desktop == rating { continue; }
        if baseline_ratings.get(&export_id).is_some_and(|old| *old != desktop) { conflicts += 1; }
        edits.push((id, u8::try_from(rating).map_err(err)?));
    }
    if !edits.is_empty() {
        state.write(|writer| {
            for (id, rating) in &edits { writer.set_rating(id, *rating)?; }
            Ok(())
        }).map_err(write_error)?;
        report.ratings = edits.len();
    }
    if conflicts > 0 {
        let noun = if conflicts == 1 { "rating" } else { "ratings" };
        report.warnings.push(format!("Device ratings replaced {conflicts} {noun} also edited in your library."));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
