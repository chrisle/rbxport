//! Beat-grid edits recovered from rekordbox 7.2.11's `BeatGridAdjustment`.
//! See the private pre-release beat-grid audit for Ghidra addresses/evidence.
//! Times are integer milliseconds; interval calculations round ties to even.
use crate::Beat;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Edit {
    Nudge(i32),
    Double,
    Halve,
    /// Align the nearest beat to the playhead and make it beat 1.
    Downbeat { time_ms: u32 },
    /// Change BPM while holding the first editable beat.
    Tempo { bpm_x100: u16, anchor_ms: u32 },
    /// TAP changes BPM, aligns to the first tap, and sets beat 1 there.
    Tap { bpm: f64, anchor_ms: u32 },
    /// Move the beat nearest the playhead by milliseconds, holding the start.
    Stretch { by_ms: i32, time_ms: u32 },
    Align { time_ms: u32 },
}

const MAX_BEATS: usize = 1 << 20;
pub const MIN_BPM_X100: u16 = 4_000;
pub const MAX_BPM_X100: u16 = 49_900;

#[must_use]
pub fn tempo_x100(beats: &[Beat]) -> u16 { beats.first().map_or(0, |b| b.tempo_x100) }

#[must_use]
pub fn nearest_index(beats: &[Beat], time_ms: u32) -> Option<usize> {
    beats.iter().enumerate().min_by_key(|(_, b)| b.time_ms.abs_diff(time_ms)).map(|(i, _)| i)
}

fn start_index(beats: &[Beat], from_ms: Option<u32>) -> usize {
    from_ms.and_then(|ms| nearest_index(beats, ms)).unwrap_or(0)
}

fn number(offset: i64) -> u16 { u16::try_from(offset.rem_euclid(4) + 1).unwrap_or(1) }
fn renumber(beats: &mut [Beat], at: usize, value: u16) {
    for (i, beat) in beats.iter_mut().enumerate() {
        beat.beat_number = number(i64::try_from(i).unwrap_or(0) - i64::try_from(at).unwrap_or(0) + i64::from(value) - 1);
    }
}
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "nonnegative, bounded millisecond/BPM result")]
fn rounded(value: f64) -> u32 { value.round_ties_even().clamp(0.0, f64::from(u32::MAX)) as u32 }

fn stretch_interval(beats: &[Beat], start: usize, time_ms: u32, by_ms: i32) -> Option<f64> {
    let first = beats.get(start)?;
    let mut target = nearest_index(beats, time_ms)?;
    if target == start && time_ms >= first.time_ms { target += 1; }
    if target <= start { return None; }
    let end = beats.get(target)?;
    let count = u32::try_from(target - start).ok()?;
    Some((f64::from(end.time_ms) + f64::from(by_ms.clamp(-120, 120)) - f64::from(first.time_ms)) / f64::from(count))
}

/// Validate without saturating tempo values or silently changing their meaning.
pub fn validate(beats: &[Beat], from_ms: Option<u32>, edit: Edit) -> Result<(), &'static str> {
    let start = start_index(beats, from_ms);
    let tail = &beats[start..];
    let valid = match edit {
        Edit::Double => tail.iter().all(|b| (2_000..=24_950).contains(&b.tempo_x100)),
        Edit::Halve => tail.iter().all(|b| b.tempo_x100 >= 8_000),
        Edit::Tempo { bpm_x100, .. } => (MIN_BPM_X100..=MAX_BPM_X100).contains(&bpm_x100),
        Edit::Tap { bpm, .. } => (40.0..500.0).contains(&bpm),
        Edit::Stretch { time_ms, by_ms } => stretch_interval(beats, start, time_ms, by_ms)
            .is_none_or(|interval| (60_000.0 / 499.0..=1500.0).contains(&interval)),
        _ => true,
    };
    if valid { Ok(()) } else { Err("The beat-grid tempo must be between 40 and 499 BPM.") }
}

#[must_use]
pub fn is_dynamic_from(beats: &[Beat], from_ms: Option<u32>) -> bool {
    let tail = &beats[start_index(beats, from_ms)..];
    tail.first().is_some_and(|first| tail.iter().any(|b| b.tempo_x100 != first.tempo_x100))
}

/// Normalize to the actual track bounds (zero is valid), preserving bar phase.
fn fit(mut beats: Vec<Beat>, end_ms: u32) -> Vec<Beat> {
    beats.retain(|b| b.time_ms <= end_ms);
    if beats.is_empty() { return beats; }
    if beats.len() == 1 {
        let first = beats[0];
        if first.time_ms == 0 { return beats; }
        while beats.len() < MAX_BEATS {
            let last = beats[beats.len() - 1];
            let Some(time_ms) = last.time_ms.checked_add(first.time_ms).filter(|time| *time <= end_ms) else { break };
            beats.push(Beat { time_ms, beat_number: number(i64::from(last.beat_number)), ..last });
        }
        return beats;
    }
    let interval = beats[1].time_ms.saturating_sub(beats[0].time_ms);
    if interval > 0 && beats[0].time_ms >= interval {
        let first = beats[0];
        beats.insert(0, Beat { time_ms: first.time_ms - interval, beat_number: number(i64::from(first.beat_number) - 2), ..first });
    }
    while beats.len() < MAX_BEATS {
        let n = beats.len();
        let last = beats[n - 1];
        let interval = last.time_ms.saturating_sub(beats[n - 2].time_ms);
        let Some(next) = last.time_ms.checked_add(interval) else { break };
        if interval == 0 || next > end_ms { break; }
        beats.push(Beat { time_ms: next, beat_number: number(i64::from(last.beat_number)), ..last });
    }
    beats
}

fn move_by(beats: &[Beat], start: usize, by: i64, end_ms: u32) -> Vec<Beat> {
    let moved = beats.iter().enumerate().filter_map(|(i, b)| {
        let time = i64::from(b.time_ms) + if i >= start { by } else { 0 };
        let time_ms = u32::try_from(time).ok()?;
        (time_ms <= end_ms).then_some(Beat { time_ms, ..*b })
    }).collect();
    fit(moved, end_ms)
}

fn respace(beats: &[Beat], start: usize, interval: f64, bpm_x100: u16, end_ms: u32) -> Vec<Beat> {
    let anchor = beats[start].time_ms;
    let out = beats.iter().enumerate().map(|(i, b)| {
        if i < start { return *b; }
        let count = u32::try_from(i - start).unwrap_or(u32::MAX);
        Beat { time_ms: anchor.saturating_add(rounded(f64::from(count) * interval)), tempo_x100: bpm_x100, ..*b }
    }).collect();
    fit(out, end_ms)
}

/// Production entry point: the caller supplies the real duration, never an
/// arbitrary half-beat margin. Invalid edits leave the grid unchanged; writers
/// call `validate` first so users receive an actionable error.
#[must_use]
pub fn apply_with_duration(beats: &[Beat], from_ms: Option<u32>, edit: Edit, end_ms: u32) -> Vec<Beat> {
    if beats.is_empty() || validate(beats, from_ms, edit).is_err() { return beats.to_vec(); }
    let start = start_index(beats, from_ms);
    match edit {
        Edit::Nudge(ms) => move_by(beats, start, i64::from(ms), end_ms),
        Edit::Align { time_ms } | Edit::Downbeat { time_ms } => {
            let at = nearest_index(beats, time_ms).unwrap_or(0);
            let mut out = move_by(beats, start, i64::from(time_ms) - i64::from(beats[at].time_ms), end_ms);
            if matches!(edit, Edit::Downbeat { .. }) {
                if let Some(at) = nearest_index(&out, time_ms) { renumber(&mut out, at, 1); }
            }
            out
        }
        Edit::Stretch { by_ms, time_ms } => {
            let Some(interval) = stretch_interval(beats, start, time_ms, by_ms) else { return beats.to_vec() };
            let bpm = u16::try_from(rounded(6_000_000.0 / interval)).unwrap_or(MAX_BPM_X100);
            respace(beats, start, interval, bpm, end_ms)
        }
        Edit::Tempo { bpm_x100, .. } => respace(beats, start, 6_000_000.0 / f64::from(bpm_x100), bpm_x100, end_ms),
        Edit::Tap { bpm, anchor_ms } => {
            let bpm_x100 = u16::try_from(rounded(bpm * 100.0)).unwrap_or(MAX_BPM_X100);
            let out = respace(beats, start, 60_000.0 / bpm, bpm_x100, end_ms);
            apply_with_duration(&out, from_ms, Edit::Downbeat { time_ms: anchor_ms }, end_ms)
        }
        Edit::Double => {
            let mut out = beats[..start].to_vec();
            for (i, beat) in beats.iter().enumerate().skip(start) {
                let tempo_x100 = beat.tempo_x100 * 2;
                out.push(Beat { tempo_x100, ..*beat });
                let interval = if let Some(next) = beats.get(i + 1) { next.time_ms.saturating_sub(beat.time_ms) }
                    else if i > start { beat.time_ms.saturating_sub(beats[i - 1].time_ms) }
                    else { end_ms.saturating_sub(beat.time_ms) };
                if interval > 1 { out.push(Beat { time_ms: beat.time_ms.saturating_add(interval / 2), tempo_x100, ..*beat }); }
            }
            let first_number = if beats[start].beat_number % 2 == 1 { 1 } else { 3 };
            renumber(&mut out, start, first_number);
            fit(out, end_ms)
        }
        Edit::Halve => {
            let mut out = beats[..start].to_vec();
            let original = beats[start].beat_number;
            let skip = usize::from(original % 2 == 0);
            out.extend(beats[start + skip..].iter().step_by(2).map(|b| Beat { tempo_x100: b.tempo_x100 / 2, ..*b }));
            let value = if original == 1 || original == 4 { 1 } else { 4 };
            renumber(&mut out, start, value);
            fit(out, end_ms)
        }
    }
}

/// Convenience for callers without a duration: bound to the final stored beat.
#[must_use]
pub fn apply(beats: &[Beat], edit: Edit) -> Vec<Beat> { apply_from(beats, None, edit) }
#[must_use]
pub fn apply_from(beats: &[Beat], from_ms: Option<u32>, edit: Edit) -> Vec<Beat> {
    apply_with_duration(beats, from_ms, edit, beats.last().map_or(0, |b| b.time_ms))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn grid() -> Vec<Beat> { (0..9).map(|i| Beat { time_ms: i * 500, beat_number: u16::try_from(i % 4 + 1).unwrap_or(0), tempo_x100: 12000 }).collect() }
    fn times(beats: &[Beat]) -> Vec<u32> { beats.iter().map(|b| b.time_ms).collect() }
    #[test]
    fn recovered_stretch_uses_target_distance_and_even_rounding() {
        let out = apply_with_duration(&grid(), None, Edit::Stretch { by_ms: 1, time_ms: 1000 }, 4000);
        assert_eq!(&times(&out)[..5], &[0,500,1001,1502,2002]);
        assert_eq!(tempo_x100(&out),11988);
    }
    #[test]
    fn downbeat_aligns_and_renumbers() {
        let out = apply_with_duration(&grid(), None, Edit::Downbeat { time_ms: 1600 }, 4000);
        assert_eq!(times(&out), [100,600,1100,1600,2100,2600,3100,3600]);
        assert_eq!(out.iter().find(|b| b.time_ms==1600).map(|b|b.beat_number),Some(1));
    }
    #[test]
    fn double_includes_final_midpoint() {
        let out = apply_with_duration(&grid(), None, Edit::Double,4250);
        assert_eq!(times(&out),(0..18).map(|i|i*250).collect::<Vec<_>>());
    }
    #[test]
    fn halve_obeys_original_bar_phase() {
        let beats: Vec<_> = grid().iter().map(|b|Beat {beat_number:b.beat_number%4+1,..*b}).collect();
        let out = apply_with_duration(&beats,None,Edit::Halve,4000);
        assert_eq!(times(&out),[500,1500,2500,3500]);
        assert_eq!(out.first().map(|b|b.beat_number),Some(4));
    }
    #[test]
    fn typed_tempo_anchors_the_scope_start() {
        let out=apply_with_duration(&grid(),Some(2000),Edit::Tempo{bpm_x100:10000,anchor_ms:999},4000);
        assert_eq!(times(&out),[0,500,1000,1500,2000,2600,3200,3800]);
    }
    #[test]
    fn singleton_normalizes_using_its_distance_from_zero() {
        let beat = Beat {time_ms: 500, beat_number: 1, tempo_x100: 12000};
        assert_eq!(times(&apply_with_duration(&[beat],None,Edit::Nudge(0),2000)),[500,1000,1500,2000]);
    }
    #[test]
    fn nearest_ties_choose_the_earlier_beat() {
        assert_eq!(nearest_index(&grid(),750),Some(1));
    }
    #[test]
    fn invalid_tempo_is_rejected_without_clamping() {
        assert!(validate(&grid(),None,Edit::Tempo{bpm_x100:3999,anchor_ms:0}).is_err());
        let fast: Vec<_> = grid().iter().map(|b|Beat{tempo_x100:25000,..*b}).collect();
        assert!(validate(&fast,None,Edit::Double).is_err());
        assert_eq!(apply(&[],Edit::Double), [] as [Beat; 0]);
    }
}
