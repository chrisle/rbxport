//! Waveform payloads from band columns, and whole analysis files from a
//! track's analysis.
//!
//! The encodings are the ones the app's own drawing reads (`src/canvas/
//! waveform.ts`) and `rbl-link` serves to players, written back the other
//! way:
//!
//! - `PWAV`, `PWV2`, `PWV3`: one byte a column, five bits of height and
//!   three of "whiteness" — how much of the column is treble.
//! - `PWV4`: six bytes a column, the first the raw peak out of 127 and the
//!   last three the red, green and blue, which track the low, mid and high
//!   bands [OBS]. Bytes 1 and 2 are `[UNKNOWN]` and written as zero.
//! - `PWV5`: a big-endian `u16` a column, `rrrgggbbbhhhhh00`, red, green
//!   and blue again the low, mid and high bands [OBS].
//! - `PWV6`, `PWV7`: three bytes a column, low, mid and high out of 127.
//!
//! `PWAV` is 400 columns and `PWV2` 100; `PWV4` and `PWV6` are 1,200; the
//! scrolling `PWV3`, `PWV5` and `PWV7` are 150 a second [OBS]. A coarser
//! peak preview uses the loudest column of each bucket. `PWV6` uses an
//! independent energy envelope when supplied to [`author_with_overview`].
//!
//! The preview heights (`PWAV`, `PWV2`) come from the three bands; the
//! scrolling details' (`PWV3`, `PWV5`) from the column's sample peak, on
//! rekordbox's squared curve — see [`detail_height`]. The bands are not all
//! written at one scale: `PWV6` takes [`band`], while `PWV7` keeps the full
//! seven-bit detail range; `PWV4`'s colour channels a weighted seven-bit
//! narrowing, `PWV5`'s a colour normalised to its strongest band. Each of
//! those is what rekordbox's own files hold; see the functions for the
//! measurements.

use rbl_core::FourCc;

use crate::write::{beat_grid_section, AnlzBuilder};
use crate::{Anlz, Beat, Section};

/// One column of the analysed waveform, every value out of 255.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BandColumn {
    pub low: u8,
    pub mid: u8,
    pub high: u8,
    /// The column's overall peak. `PWV4`'s first byte, and nothing else:
    /// the height the other sections draw comes from the bands — see
    /// [`band_height`].
    pub peak: u8,
}

/// Columns in the overview waveforms `PWV4` and `PWV6`.
pub const OVERVIEW_COLUMNS: usize = 1200;
/// Columns in the `PWAV` preview.
pub const PREVIEW_COLUMNS: usize = 400;
/// Columns in the `PWV2` tiny preview.
pub const TINY_COLUMNS: usize = 100;

/// `n` columns from any number: the loudest column of each bucket.
#[must_use]
pub fn resample(columns: &[BandColumn], n: usize) -> Vec<BandColumn> {
    if n == 0 {
        return Vec::new();
    }
    if columns.is_empty() {
        return vec![BandColumn::default(); n];
    }
    let mut out = Vec::with_capacity(n);
    for bucket in 0..n {
        let start = bucket * columns.len() / n;
        let end = ((bucket + 1) * columns.len() / n).max(start + 1).min(columns.len());
        let mut loudest = BandColumn::default();
        for column in columns.get(start..end).unwrap_or(&[]) {
            loudest.low = loudest.low.max(column.low);
            loudest.mid = loudest.mid.max(column.mid);
            loudest.high = loudest.high.max(column.high);
            loudest.peak = loudest.peak.max(column.peak);
        }
        out.push(loudest);
    }
    out
}

/// Legacy peak-derived `PWV6` scale, used only when no independent overview
/// is supplied. The production analyser supplies its own energy envelope.
///
/// A band here is the peak of the filtered signal over the column, which on
/// a loud master sits near the top of its range almost everywhere. Written
/// at the field's full scale that draws as a solid block with its top cut
/// off — every column the same height. rekordbox's own values sit far
/// lower: on the nine files of `RBX-BPM-MULTIBPM-TEST`, analysed by
/// rekordbox and by this, ours were 2.9x its across all three bands before
/// this and are 0.96x its (0.68 to 1.23) after [OBS 7.2.11].
///
/// This is `PWV6`'s scale and not the format's: `PWV7` and `PWV4`'s colour
/// channels carry the same bands about three times hotter, reaching the
/// full 127 on every one of those nine tracks, and the canvas reads both
/// against 127 (`BAND_FULL_SCALE` in `src/canvas/waveform.ts`).
const BAND_FULL_SCALE: u32 = 44;

/// A band magnitude 0..=255 as the `PWV6` overview carries it.
fn band(value: u8) -> u8 {
    u8::try_from(u32::from(value) * BAND_FULL_SCALE / 255).unwrap_or(0)
}

/// The tallest height each five-bit tag writes, of the 31 the field allows.
///
/// The three do not share a ceiling. Over the nine reference tracks,
/// rekordbox's `PWV3` and `PWV5` reach exactly 31 on every one of them, its
/// `PWAV` tops out at 24-25 and never higher, and its `PWV2` at 14-15
/// [OBS 7.2.11] - the last matching the older reading that no `PWV2` byte
/// of 150 reference files exceeds 15. So the preview tags are drawn into
/// less than the field holds and the two detail tags into all of it.
const PWAV_CEILING: u32 = 25;
const PWV2_CEILING: u32 = 15;
const DETAIL_CEILING: u32 = 31;

/// A preview column's drawn height, up to `ceiling`.
///
/// The loudest of the three bands, not the column's raw sample peak taken
/// linearly: on a loud master the peak over a whole preview bucket, the
/// byte rekordbox writes as `PWV4`'s height and the one ours already
/// matches, is pinned at full scale for most of the track, while the
/// `PWAV` heights it draws keep their dynamics, sitting at 10-21 of its 25
/// [OBS 7.2.11]. Taking that peak put every column at the top: a solid
/// block. The scrolling details, whose 1/150 s columns are short enough to
/// keep the peak's dynamics, use [`detail_height`] instead.
fn band_height(column: BandColumn, ceiling: u32) -> u32 {
    let loudest = u32::from(band(column.low).max(band(column.mid)).max(band(column.high)));
    (loudest * ceiling / BAND_FULL_SCALE).min(ceiling)
}

/// A scrolling detail column's height, 0..=31: the column's sample peak
/// squared, rounded into the five bits.
///
/// rekordbox's `PWV3` height is `round(31 * (peak / 255)^2)` of the same
/// 1/150 s sample peak this analysis takes: against it over eleven
/// rekordbox-analysed tracks (three of them not used to find the curve)
/// the mean error is 0.14 to 0.42 of a step, where the band-derived
/// height it replaces was 2.6 to 4.8 out, its 95th percentile 16-28 of 31
/// where rekordbox's is 21-31 [OBS 7.2.11, issue #158].
/// rekordbox's `PWV5` height is the same as its `PWV3` on 63-93% of
/// columns, differing either way on the rest, so the one curve serves both.
fn detail_height(column: BandColumn) -> u32 {
    const FULL: u32 = 255 * 255;
    let peak = u32::from(column.peak);
    ((DETAIL_CEILING * peak * peak + FULL / 2) / FULL).min(DETAIL_CEILING)
}

/// The one-byte encoding: `height` in the low five bits, whiteness above.
fn mono_byte(column: BandColumn, height: u32) -> u8 {
    let total = u32::from(column.low) + u32::from(column.mid) + u32::from(column.high);
    let whiteness = (u32::from(column.high) * 7 + total / 2).checked_div(total).unwrap_or(0);
    u8::try_from((whiteness.min(7) << 5) | height.min(DETAIL_CEILING)).unwrap_or(0)
}

/// The lowest height rekordbox writes in each preview tag. A silent column
/// is not 0: over 1,558 rekordbox 7 `.DAT` files of a USB export, the
/// smallest `PWAV` height is 2 in 1,552 of them and never lower, and the
/// smallest `PWV2` byte is 1 in 1,143 and never 0 [OBS, issue #278].
///
/// Players read 0 as "not analysed yet": the Nexus firmware's preview check
/// (`TotalWaveWacher_CheckWaveDataComplete`, below) reports a preview with a
/// zero column as incomplete, and the player then builds its own from the
/// audio and saves it over the stick's [static].
const PWAV_FLOOR: u8 = 2;
const PWV2_FLOOR: u8 = 1;

/// `PWAV`: 400 columns, one byte each, the height at least [`PWAV_FLOOR`].
#[must_use]
pub fn pwav(columns: &[BandColumn]) -> Vec<u8> {
    resample(columns, PREVIEW_COLUMNS).into_iter().map(|c| device_preview_byte(mono_byte(c, band_height(c, PWAV_CEILING)))).collect()
}

/// `PWV2`: 100 columns, one byte each: the height alone, 1..=15.
///
/// Unlike `PWAV`, rekordbox puts no whiteness above the height here: no
/// `PWV2` byte in 1,590 rekordbox-written `.DAT` files (a rekordbox 7 USB
/// export and a rekordbox 7 share tree) is above 15 [OBS, issue #278]. Players
/// depend on that. The Nexus player firmware checks the 900-byte preview it
/// loads and throws all of it away, `PWAV` with it, when any `PWV2` byte is
/// above 15 (`TotalWaveWacher_CheckWaveDataComplete` at `0x000d7c18`, XDJ-RX2
/// 1.43, which shares the XDJ-1000MK2's code base) [static]. See
/// [`with_device_preview`] for files already written with whiteness.
#[must_use]
pub fn pwv2(columns: &[BandColumn]) -> Vec<u8> {
    resample(columns, TINY_COLUMNS).into_iter().map(tiny_preview_byte).collect()
}

/// One `PWV2` column: its height and nothing else.
fn tiny_preview_byte(column: BandColumn) -> u8 {
    device_tiny_preview_byte(u8::try_from(band_height(column, PWV2_CEILING)).unwrap_or(0))
}

/// The tallest byte a player accepts in `PWV2`: see [`pwv2`].
pub const PWV2_MAX_BYTE: u8 = 15;

/// One `PWAV` byte as rekordbox writes it: the whiteness kept, the height
/// at least [`PWAV_FLOOR`]. A byte rekordbox wrote comes back unchanged.
#[must_use]
pub fn device_preview_byte(byte: u8) -> u8 {
    (byte & 0xe0) | (byte & 0x1f).max(PWAV_FLOOR)
}

/// One `PWV2` byte as rekordbox writes it and players accept it: the
/// five-bit height without the whiteness bits earlier versions of this
/// encoder put above it, within [`PWV2_FLOOR`]..=[`PWV2_MAX_BYTE`]. A byte
/// rekordbox wrote comes back unchanged.
#[must_use]
pub fn device_tiny_preview_byte(byte: u8) -> u8 {
    (byte & 0x1f).clamp(PWV2_FLOOR, PWV2_MAX_BYTE)
}

/// A section with its `PWAV` or `PWV2` bytes brought to what rekordbox
/// writes and players accept; every other section unchanged. Analysis
/// written before issue #278 carried whiteness in `PWV2`, which makes a
/// Nexus player (XDJ-1000MK2, XDJ-RX2) reject the track's whole preview, and
/// zero-height columns, which it reads as unfinished; an export repairs both
/// rather than asking for every track to be analysed again.
#[must_use]
pub fn with_device_preview(section: &Section) -> Section {
    let repair: fn(u8) -> u8 = if section.tag == FourCc::new(b"PWAV") {
        device_preview_byte
    } else if section.tag == FourCc::new(b"PWV2") {
        device_tiny_preview_byte
    } else {
        return section.clone();
    };
    Section { tag: section.tag, header: section.header.clone(), payload: section.payload.iter().map(|&b| repair(b)).collect() }
}

/// `PWV3`: every column, one byte each, at [`detail_height`].
#[must_use]
pub fn pwv3(columns: &[BandColumn]) -> Vec<u8> {
    columns.iter().copied().map(|c| mono_byte(c, detail_height(c))).collect()
}

/// `PWV4`'s mid and high channels as fractions of the plain seven-bit
/// narrowing: `PWV4_MID_GAIN.0 / PWV4_MID_GAIN.1` and likewise for high;
/// low keeps the plain narrowing.
///
/// The XDJ-AZ scales a column's three channels so the strongest is full
/// before it draws them (firmware 1.30, the `PWV4` reader `FUN_017a8e1c`),
/// so the colour drawn depends only on their ratios, which these gains
/// set. Compared that way, scaled to 255, with rekordbox's own `PWV4` for
/// the same audio, these gains leave a mean error of 26 to 34 a channel on
/// eleven rekordbox-analysed tracks (three not used to choose them), where
/// the unweighted channels were 20 to 68 out and the mid, high and low
/// order this used to write 61 to 121 [OBS 7.2.11, issue #158].
const PWV4_MID_GAIN: (u32, u32) = (4, 5);
const PWV4_HIGH_GAIN: (u32, u32) = (13, 20);

/// `PWV4`: 1,200 columns of six bytes.
#[must_use]
pub fn pwv4(columns: &[BandColumn]) -> Vec<u8> {
    let weigh = |value: u8, (num, den): (u32, u32)| u8::try_from(u32::from(value >> 1) * num / den).unwrap_or(127);
    let mut out = Vec::with_capacity(OVERVIEW_COLUMNS * 6);
    for column in resample(columns, OVERVIEW_COLUMNS) {
        // The three colour channels are seven-bit like the height beside
        // them, and unlike `PWV6` they use all of it: rekordbox reaches
        // exactly 127 on every one of the nine reference tracks and no
        // byte goes above it [OBS 7.2.11]. So the band is narrowed to the
        // field rather than scaled the way `band` scales it — written
        // straight from the 0..=255 band these ran to 255, which put half
        // of them outside the field.
        //
        // The channels are low, mid and high in that order: rekordbox's
        // byte 3 runs with its own `PWV6` low band at r = 0.83 to 0.96 on
        // every one of eleven tracks, and with ours at 0.93 to 0.99, where
        // the mid this used to write there managed 0.43 to 0.84 [OBS 7.2.11,
        // issue #158].
        out.extend_from_slice(&[
            column.peak >> 1,
            0,
            0,
            column.low >> 1,
            weigh(column.mid, PWV4_MID_GAIN),
            weigh(column.high, PWV4_HIGH_GAIN),
        ]);
    }
    out
}

/// `PWV5`'s three colour channels for a column, `(red, green, blue)`: the
/// low, mid and high bands as shares of the strongest of them, so the
/// colour says which bands the column holds and the height alone how loud
/// it is.
///
/// What rekordbox writes, over eight rekordbox-analysed tracks [OBS 7.2.11,
/// issue #158]:
///
/// - red runs with the column's low band, green with its mid and blue with
///   its high (r = 0.76 to 0.86, 0.46 to 0.80 and 0.59 to 0.82 against each band's share
///   of the column). This used to write mid, high and low.
/// - The colour is normalised: the strongest channel is 7 in 80% of columns
///   and 5 or 6 in nearly all the rest. Green tops out at 5 — 51% of columns
///   have it there, 2.6% at 6 and 0.2% at 7. The plain top three bits written here
///   before left all three channels at 1 or 2 on most columns.
/// - High weighs 1.5 times the other two in choosing the strongest; that
///   weight and green's ceiling of 5 give the least error of the
///   combinations tried, 0.66 to 0.91 of a step a channel on all eleven
///   tracks (three not used to choose them), where this was 3.0 to 3.3.
///
/// The XDJ-AZ draws these bits as they are, each shifted up into a byte,
/// without normalising (firmware 1.30,
/// `DetailedWaveformDataProvider<DetailedWaveform_RGB>` slot 7 at
/// `0x01b94d90`), so the plain bits drew dim, washed-out columns.
fn pwv5_colour(column: BandColumn) -> (u16, u16, u16) {
    let low = 2 * u32::from(column.low);
    let mid = 2 * u32::from(column.mid);
    let high = 3 * u32::from(column.high);
    let strongest = low.max(mid).max(high);
    if strongest == 0 {
        return (0, 0, 0);
    }
    let share = |value: u32, full: u32| u16::try_from((2 * full * value + strongest) / (2 * strongest)).unwrap_or(0);
    (share(low, 7), share(mid, 5), share(high, 7))
}

/// `PWV5`: every column as a big-endian word, `rrrgggbbbhhhhh00` — the
/// colour from [`pwv5_colour`], the height from [`detail_height`].
#[must_use]
pub fn pwv5(columns: &[BandColumn]) -> Vec<u8> {
    let mut out = Vec::with_capacity(columns.len() * 2);
    for &column in columns {
        let (red, green, blue) = pwv5_colour(column);
        let word = (red << 13)
            | (green << 10)
            | (blue << 7)
            | (u16::try_from(detail_height(column)).unwrap_or(0) << 2);
        out.extend_from_slice(&word.to_be_bytes());
    }
    out
}

/// `PWV6`: 1,200 columns of three bytes, low, mid and high out of 127.
#[must_use]
pub fn pwv6(columns: &[BandColumn]) -> Vec<u8> {
    let mut out = Vec::with_capacity(OVERVIEW_COLUMNS * 3);
    for column in resample(columns, OVERVIEW_COLUMNS) {
        out.extend_from_slice(&[band(column.low), band(column.mid), band(column.high)]);
    }
    out
}

/// `PWV7`: every column as three bytes.
#[must_use]
pub fn pwv7(columns: &[BandColumn]) -> Vec<u8> {
    let mut out = Vec::with_capacity(columns.len() * 3);
    for column in columns {
        // The detail uses the full seven-bit range. Scaling it like PWV6
        // made all nine reference tracks' scrolling envelopes only 31-41%
        // of rekordbox's height, despite matching their shape (r >= 0.97).
        out.extend_from_slice(&[column.low >> 1, column.mid >> 1, column.high >> 1]);
    }
    out
}

/// What one analysis produces on disk: the three files rekordbox keeps
/// beside each other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisFiles {
    pub dat: Vec<u8>,
    pub ext: Vec<u8>,
    pub two_ex: Vec<u8>,
}

/// The files already there for the track, whose sections this analysis
/// cannot produce are carried through.
#[derive(Debug, Clone, Copy, Default)]
pub struct Existing<'a> {
    pub dat: Option<&'a Anlz>,
    pub ext: Option<&'a Anlz>,
    pub two_ex: Option<&'a Anlz>,
}

/// The tags an analysis writes afresh — or, for `PVBR`, places itself (a real
/// table carried through, or the zero table written when there is none) — so a
/// carried file's copies of them are dropped rather than doubled.
const AUTHORED: [&[u8; 4]; 11] =
    [b"PPTH", b"PVBR", b"PQTZ", b"PWAV", b"PWV2", b"PWV3", b"PWV4", b"PWV5", b"PWV6", b"PWV7", b"PQT2"];

/// Authors the three analysis files for a track.
///
/// `audio_path` is what `PPTH` names: the file's own path, as
/// `djmdContent.FolderPath` holds it. The grid and the waveforms are written
/// from the analysis; everything else in an existing file — the cue lists,
/// `PSSI`, `PVDI`, tags nobody has named — is carried through in its own
/// order, because rekordbox authored it and this cannot. `PQT2` is the one
/// exception: it is an extended copy of the grid, and a stale one beside a
/// new grid is worse than none, so it is dropped (nothing here can write
/// one; see [`Anlz::has_extended_grid`]).
///
/// A file with nothing to carry gets the empty cue lists the share tree
/// holds: cues live in `djmdCue`, and every share-tree cue list in the
/// reference library is empty [OBS].
#[must_use]
pub fn author(audio_path: &str, beats: &[Beat], columns: &[BandColumn], existing: Existing<'_>) -> AnalysisFiles {
    author_with_overview(audio_path, beats, columns, None, existing)
}

/// Authors analysis with an independently measured PWV6 energy envelope.
/// Values are already seven-bit; this avoids peak reduction and a second gain.
/// Callers with only detail columns can pass `None` for the legacy approximation.
#[must_use]
pub fn author_with_overview(
    audio_path: &str, beats: &[Beat], columns: &[BandColumn],
    overview: Option<&[[u8; 3]; OVERVIEW_COLUMNS]>, existing: Existing<'_>,
) -> AnalysisFiles {
    let dat = {
        let mut builder = AnlzBuilder::new();
        builder.path(audio_path);
        // `PVBR` sits between the path and the grid in every rekordbox `.DAT`
        // (1,558 of 1,558 reference files [OBS]); rekordbox 6/7 reject a `.DAT`
        // that lacks it. A real VBR seek table cannot be re-derived from the
        // analysis here, so an existing one is carried through unchanged, and a
        // file that has none gets the zero table rekordbox itself writes for a
        // track with no VBR frames to index — see [`AnlzBuilder::vbr_table_zero`].
        match existing.dat.and_then(|file| file.section(b"PVBR")) {
            Some(pvbr) => { builder.copy_section(pvbr); }
            None => { builder.vbr_table_zero(); }
        }
        builder.beat_grid(beats);
        builder.waveform_preview(b"PWAV", &pwav(columns));
        builder.waveform_preview(b"PWV2", &pwv2(columns));
        carry_or(&mut builder, existing.dat, |b| {
            b.empty_cue_list_of(false, 0);
            b.empty_cue_list_of(false, 1);
        });
        builder.finish()
    };
    let ext = {
        let mut builder = AnlzBuilder::new();
        builder.path(audio_path);
        builder.waveform_scroll(b"PWV3", 1, &pwv3(columns));
        builder.waveform_scroll(b"PWV4", 6, &pwv4(columns));
        builder.waveform_scroll(b"PWV5", 2, &pwv5(columns));
        carry_or(&mut builder, existing.ext, |b| {
            b.empty_cue_list_of(true, 0);
            b.empty_cue_list_of(true, 1);
        });
        builder.finish()
    };
    let two_ex = {
        let mut builder = AnlzBuilder::new();
        builder.path(audio_path);
        let preview = overview.map_or_else(|| pwv6(columns), |bands| bands.iter().flatten().map(|v| (*v).min(127)).collect());
        builder.waveform_scroll(b"PWV6", 3, &preview);
        builder.waveform_scroll(b"PWV7", 3, &pwv7(columns));
        carry_or(&mut builder, existing.two_ex, |_| {});
        builder.finish()
    };
    AnalysisFiles { dat, ext, two_ex }
}

/// Copies an existing file's other sections, or writes the defaults for a
/// file that has none.
fn carry_or(builder: &mut AnlzBuilder, existing: Option<&Anlz>, defaults: impl FnOnce(&mut AnlzBuilder)) {
    match existing {
        Some(file) => {
            builder.header_extra(&file.header_extra);
            for section in file.sections.iter().filter(|s| !is_authored(s)) {
                builder.copy_section(section);
            }
        }
        None => defaults(builder),
    }
}

fn is_authored(section: &Section) -> bool {
    AUTHORED.iter().any(|tag| section.tag == FourCc::new(tag))
}

/// The grid as `Beat`s from the analyser's own beat list, which shares the
/// field names but not the type.
#[must_use]
pub fn beat_grid_of(beats: impl IntoIterator<Item = (u16, u16, u32)>) -> Vec<Beat> {
    beats
        .into_iter()
        .map(|(beat_number, tempo_x100, time_ms)| Beat { beat_number, tempo_x100, time_ms })
        .collect()
}

/// A `PQTZ` on its own, for callers that replace a grid in place.
#[must_use]
pub fn grid_section(beats: &[Beat]) -> Section {
    beat_grid_section(beats)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn ramp(n: usize) -> Vec<BandColumn> {
        (0..n)
            .map(|i| {
                let v = u8::try_from(i * 255 / n.saturating_sub(1).max(1)).unwrap_or(255);
                BandColumn { low: v, mid: v / 2, high: v / 4, peak: v }
            })
            .collect()
    }

    #[test]
    fn independent_overview_is_encoded_without_peak_gain() {
        let columns = ramp(1500);
        let mut overview = [[0; 3]; OVERVIEW_COLUMNS];
        overview[600] = [59, 42, 70];
        let files = author_with_overview("/track.wav", &[], &columns, Some(&overview), Existing::default());
        let parsed = crate::parse(&files.two_ex).unwrap();
        let preview = parsed.sections.iter().find(|s| s.tag == FourCc::new(b"PWV6")).unwrap();
        assert_eq!(&preview.payload[1800..1803], &[59, 42, 70]);
        assert!(preview.payload[..1800].iter().all(|&v| v == 0));
        let detail = parsed.sections.iter().find(|s| s.tag == FourCc::new(b"PWV7")).unwrap();
        assert_eq!(detail.payload, pwv7(&columns));
    }

    #[test]
    fn three_band_preview_uses_rekordbox_framing() {
        let columns = ramp(1200);
        let files = author("/track.wav", &[], &columns, Existing::default());
        let parsed = crate::parse(&files.two_ex).unwrap();
        let section = parsed.sections.iter().find(|s| s.tag == FourCc::new(b"PWV6")).unwrap();
        // Measured on all nine reference tracks: 20-byte total header,
        // followed immediately by 1,200 three-byte columns.
        assert_eq!(section.header, [0, 0, 0, 3, 0, 0, 4, 176]);
        assert_eq!(section.payload.len(), 3600);
    }

    #[test]
    fn coarser_waveforms_keep_the_loudest_column_of_each_bucket() {
        let mut columns = vec![BandColumn::default(); 3000];
        columns[1500] = BandColumn { low: 200, mid: 10, high: 10, peak: 200 };
        let overview = resample(&columns, 1200);
        assert_eq!(overview.len(), 1200);
        assert_eq!(overview[600].peak, 200, "the kick survives the resample");
        assert_eq!(resample(&[], 4), vec![BandColumn::default(); 4]);
        assert_eq!(resample(&columns, 0), [] as [BandColumn; 0]);
    }

    #[test]
    fn the_payloads_have_their_strides_and_ranges() {
        let columns = ramp(3000);
        assert_eq!(pwav(&columns).len(), 400);
        assert_eq!(pwv2(&columns).len(), 100);
        assert_eq!(pwv3(&columns).len(), 3000);
        assert_eq!(pwv4(&columns).len(), 7200);
        assert_eq!(pwv5(&columns).len(), 6000);
        assert_eq!(pwv6(&columns).len(), 3600);
        assert_eq!(pwv7(&columns).len(), 9000);
        // Every field stays inside its range, and a full-scale column stops
        // short of the top of it: the headroom rekordbox leaves, without
        // which a loud track draws as a block with its top cut off.
        assert!(pwv4(&columns).chunks(6).all(|c| c[0] <= 127));
        // `PWV4`'s colours use the whole seven bits, mid and high weighted
        // down; `PWV6`'s stop at `band`.
        assert!(pwv4(&columns).chunks(6).all(|c| c[3] <= 127 && c[4] <= 127 && c[5] <= 127));
        assert_eq!(pwv4(&[BandColumn { low: 255, mid: 255, high: 255, peak: 255 }])[3..6], [127, 101, 82]);
        assert!(pwv6(&columns).iter().all(|&b| b <= 44));
        assert!(pwv7(&columns).iter().all(|&b| b <= 127));
        assert_eq!(pwv7(&[BandColumn { low: 255, mid: 128, high: 64, peak: 255 }]), [127, 64, 32]);
        let full = BandColumn { low: 255, mid: 255, high: 255, peak: 255 };
        assert_eq!(band(255), 44);
        // Each five-bit tag reaches its own ceiling at full scale, and no
        // further: the preview tags stop short of the field, the detail
        // tags fill it.
        assert_eq!(band_height(full, PWAV_CEILING), 25);
        assert_eq!(band_height(full, DETAIL_CEILING), 31);
        assert_eq!(pwav(&columns).iter().map(|b| b & 0x1f).max(), Some(25));
        assert_eq!(pwv2(&columns).iter().max(), Some(&15), "PWV2 is height alone, no whiteness");
        assert_eq!(pwv3(&columns).iter().map(|b| b & 0x1f).max(), Some(31));
        assert_eq!(pwav(&[BandColumn::default()])[0] & 0x1f, 2, "a silent column at rekordbox's floor, not 0");
        // A treble-only column is white; a bass-only one is not.
        assert_eq!(mono_byte(BandColumn { low: 0, mid: 0, high: 255, peak: 255 }, PWAV_CEILING) >> 5, 7);
        assert_eq!(mono_byte(BandColumn { low: 255, mid: 0, high: 0, peak: 255 }, PWAV_CEILING) >> 5, 0);
        // A full column: high, weighted 1.5x, is the strongest band, so blue
        // is full and red and green take their shares; the height fills the
        // five bits.
        let word = u16::from_be_bytes(pwv5(&[full])[..2].try_into().unwrap());
        assert_eq!(word >> 7, 0b101_011_111, "red 5, green 3, blue 7");
        assert_eq!((word >> 2) & 0x1f, 31, "the peak-derived height, into all five bits");
    }

    fn pwv5_word(column: BandColumn) -> u16 {
        u16::from_be_bytes(pwv5(&[column])[..2].try_into().unwrap())
    }

    /// `(red, green, blue, height)` of a `PWV5` word.
    fn pwv5_fields(word: u16) -> [u16; 4] {
        [word >> 13, (word >> 10) & 7, (word >> 7) & 7, (word >> 2) & 0x1f]
    }

    #[test]
    fn colour_channels_are_low_mid_and_high() {
        // rekordbox's red is the low band, green the mid and blue the high,
        // in `PWV5` and in `PWV4`'s last three bytes alike (issue #158).
        let bass = BandColumn { low: 200, mid: 0, high: 0, peak: 200 };
        let body = BandColumn { low: 0, mid: 200, high: 0, peak: 200 };
        let hats = BandColumn { low: 0, mid: 0, high: 200, peak: 200 };
        assert_eq!(pwv5_fields(pwv5_word(bass))[..3], [7, 0, 0], "a kick is red");
        assert_eq!(pwv5_fields(pwv5_word(body))[..3], [0, 5, 0], "the mids are green, at most 5");
        assert_eq!(pwv5_fields(pwv5_word(hats))[..3], [0, 0, 7], "the hats are blue");
        assert_eq!(pwv4(&[bass])[3..6], [100, 0, 0]);
        assert_eq!(pwv4(&[body])[3..6], [0, 80, 0]);
        assert_eq!(pwv4(&[hats])[3..6], [0, 0, 65]);
    }

    #[test]
    fn pwv5_colour_is_normalised_so_a_quiet_column_is_as_saturated_as_a_loud_one() {
        // The XDJ-AZ draws the three bits as they are, so the strongest band
        // must reach the top of its field however quiet the column is; only
        // the height says how loud it is.
        let loud = pwv5_fields(pwv5_word(BandColumn { low: 240, mid: 120, high: 40, peak: 250 }));
        let quiet = pwv5_fields(pwv5_word(BandColumn { low: 60, mid: 30, high: 10, peak: 80 }));
        assert_eq!(loud[0], 7);
        assert_eq!(loud[..3], quiet[..3]);
        assert!(quiet[3] < loud[3]);
        assert_eq!(pwv5_word(BandColumn::default()), 0, "silence is black and flat");
    }

    #[test]
    fn detail_height_is_rekordboxs_squared_peak() {
        // rekordbox's median `PWV3`/`PWV5` height for a sample peak, read off
        // eight rekordbox-analysed tracks (issue #158): 56 -> 1, 104 -> 5,
        // 136 -> 9, 168 -> 13, 200 -> 19, 232 -> 26, 255 -> 31.
        for (peak, height) in [(0, 0), (56, 1), (104, 5), (136, 9), (168, 13), (200, 19), (232, 26), (255, 31)] {
            let column = BandColumn { low: peak / 2, mid: peak / 2, high: peak / 4, peak };
            assert_eq!(u16::from(pwv3(&[column])[0] & 0x1f), height, "PWV3 at peak {peak}");
            assert_eq!(pwv5_fields(pwv5_word(column))[3], height, "PWV5 at peak {peak}");
        }
    }

    #[test]
    fn pwv5_matches_rekordboxs_own_words_for_these_columns() {
        // Columns of two tracks rekordbox 7.2.11 analysed and this crate's
        // analysis did not see while its colour and height were chosen
        // ("Language (Sam WOLFE Remix)" and "YES BITCH (Sam WOLFE Remix)"):
        // the bands this analysis measures for the column, then the `PWV5`
        // word and `PWV3` height in rekordbox's own `.EXT` for it. These are
        // columns where the two agree exactly, picked to cover the colours;
        // across all columns the mean error is 0.7-0.8 of a step a channel,
        // not zero (issue #158).
        let rekordbox = [
            ((44, 102, 109, 186), 0x4fc0_u16, 16_u8),
            ((217, 94, 55, 198), 0xe9cc, 19),
            ((189, 45, 35, 222), 0xe55c, 23),
            ((179, 112, 106, 252), 0xef78, 30),
            ((233, 76, 21, 253), 0xe8fc, 31),
            ((176, 173, 127, 239), 0xd7ec, 27),
            ((162, 90, 57, 190), 0xee44, 17),
            ((41, 68, 149, 182), 0x2bc0, 16),
            ((173, 97, 41, 236), 0xed6c, 27),
            ((133, 216, 145, 253), 0x97fc, 31),
        ];
        for ((low, mid, high, peak), word, height) in rekordbox {
            let column = BandColumn { low, mid, high, peak };
            assert_eq!(pwv5_word(column), word, "PWV5 for {column:?}");
            assert_eq!(pwv3(&[column])[0] & 0x1f, height, "PWV3 height for {column:?}");
        }
    }

    #[test]
    fn pwv2_carries_the_height_alone_as_rekordbox_does() {
        // No PWV2 byte rekordbox writes is above 15 (1,590 of 1,590 files,
        // issue #278), and a Nexus player drops the whole preview when one
        // is. A treble-only column, which takes whiteness 7 in PWAV, is
        // still just its height here.
        let treble = BandColumn { low: 0, mid: 0, high: 255, peak: 255 };
        assert_eq!(pwv2(&[treble]), vec![15; TINY_COLUMNS]);
        assert_eq!(pwav(&[treble])[0], 0b111_11001, "PWAV keeps whiteness 7 over its height of 25");
        let columns = ramp(3000);
        assert!(pwv2(&columns).iter().all(|&b| b <= PWV2_MAX_BYTE));
        assert_eq!(pwv2(&columns).iter().max(), Some(&15));
        assert_eq!(pwv2(&[BandColumn::default()]), vec![1; TINY_COLUMNS], "a silent column at rekordbox's floor, not 0");
    }

    #[test]
    fn an_export_repairs_old_previews_and_leaves_rekordboxs_alone() {
        // Bytes this encoder wrote before issue #278: whiteness 2 over a
        // height of 8, whiteness 7 over 15, a silent column, whiteness 1 over 15.
        let ours = Section::new(b"PWV2", vec![0; 8], vec![0x48, 0xef, 0x00, 0x2f]);
        assert_eq!(with_device_preview(&ours).payload, [8, 15, 1, 15]);
        assert_eq!(with_device_preview(&ours).header, ours.header);
        // PWAV keeps its whiteness; only a height under rekordbox's floor moves.
        let pwav = Section::new(b"PWAV", vec![0; 8], vec![0xa0, 0x00, 0xe5, 0x21]);
        assert_eq!(with_device_preview(&pwav).payload, [0xa2, 0x02, 0xe5, 0x22]);
        // rekordbox's own previews (bytes from a rekordbox 7 USB export) are
        // in range already and come back byte for byte.
        let rekordbox_pwv2 = Section::new(b"PWV2", vec![0; 8], vec![0x08, 0x0a, 0x0a, 0x04, 0x0c, 0x0d, 0x0e, 0x01]);
        assert_eq!(with_device_preview(&rekordbox_pwv2), rekordbox_pwv2);
        let rekordbox_pwav = Section::new(b"PWAV", vec![0; 8], vec![0xa2, 0xa2, 0xa5, 0xa4, 0xa7]);
        assert_eq!(with_device_preview(&rekordbox_pwav), rekordbox_pwav);
        // Other sections are not touched.
        let pwv3 = Section::new(b"PWV3", vec![0; 12], vec![0x00, 0xe0]);
        assert_eq!(with_device_preview(&pwv3), pwv3);
        assert_eq!(device_tiny_preview_byte(0xff), 15);
    }

    #[test]
    fn a_real_vbr_table_is_preserved_across_re_analysis_and_never_doubled() {
        let beats = beat_grid_of([(1, 12_800, 0)]);
        let columns = ramp(600);
        // A previous `.DAT` whose `PVBR` holds a real seek table (a nonzero
        // payload rekordbox derived from the file's frames), plus a section
        // this app cannot author.
        let mut previous = crate::parse(&author("/Music/track.mp3", &beats, &columns, Existing::default()).dat).unwrap();
        let pvbr = previous.sections.iter_mut().find(|s| s.tag == FourCc::new(b"PVBR")).unwrap();
        pvbr.payload.iter_mut().enumerate().for_each(|(i, b)| *b = u8::try_from(i % 251).unwrap_or(0));
        let real_table = previous.section(b"PVBR").unwrap().clone();
        previous.sections.push(Section::new(b"PVDI", vec![0; 8], vec![7, 7, 7]));

        let again = author("/Music/track.mp3", &beats, &columns, Existing { dat: Some(&previous), ..Existing::default() });
        let reread = crate::parse(&again.dat).unwrap();
        // Exactly one `PVBR`, and it is the real table carried through, not the
        // zero table nor a duplicate.
        assert_eq!(reread.sections.iter().filter(|s| s.tag == FourCc::new(b"PVBR")).count(), 1);
        assert_eq!(reread.section(b"PVBR"), Some(&real_table));
        assert!(reread.section(b"PVDI").is_some(), "sections this app cannot write are still carried");
    }

    #[test]
    fn authored_files_parse_and_carry_what_they_cannot_write() {
        let beats = beat_grid_of([(1, 12_800, 0), (2, 12_800, 469), (3, 12_800, 938), (4, 12_800, 1406)]);
        let columns = ramp(600);
        let fresh = author("/Music/track.mp3", &beats, &columns, Existing::default());
        let dat = crate::parse(&fresh.dat).unwrap();
        assert_eq!(dat.path().as_deref(), Some("/Music/track.mp3"));
        // rekordbox writes `PVBR` on every `.DAT` — between the path and the
        // grid — and rejects one that lacks it. A fresh file gets the zero
        // table (a zero header word and 401 zero words) rekordbox writes for a
        // track with no VBR frames to index.
        let tags: Vec<FourCc> = dat.sections.iter().map(|s| s.tag).collect();
        assert_eq!(tags[0], FourCc::new(b"PPTH"));
        assert_eq!(tags[1], FourCc::new(b"PVBR"));
        assert_eq!(tags[2], FourCc::new(b"PQTZ"));
        let pvbr = dat.section(b"PVBR").unwrap();
        assert_eq!(pvbr.len_header(), 16);
        assert_eq!(pvbr.len_tag(), 1620);
        assert!(pvbr.header.iter().chain(&pvbr.payload).all(|&b| b == 0));
        assert_eq!(dat.beat_grid().unwrap(), beats);
        assert_eq!(dat.waveform(b"PWAV").unwrap().1.len(), 400);
        assert_eq!(dat.sections.iter().filter(|s| s.is_cue_list()).count(), 2);
        let ext = crate::parse(&fresh.ext).unwrap();
        assert_eq!(ext.waveform(b"PWV5").unwrap(), (2, &pwv5(&columns)[..]));
        assert_eq!(ext.waveform(b"PWV4").unwrap().0, 6);
        let two = crate::parse(&fresh.two_ex).unwrap();
        assert_eq!(two.waveform(b"PWV7").unwrap(), (3, &pwv7(&columns)[..]));

        // A second analysis carries the phrase section through and drops the
        // stale extended grid, keeping the file's own header bytes.
        let mut previous = ext.clone();
        previous.header_extra = vec![9; 16];
        previous.sections.push(Section::new(b"PSSI", vec![0; 8], vec![1, 2, 3]));
        previous.sections.push(Section::new(b"PQT2", vec![0; 32], vec![0; 8]));
        let again = author("/Music/track.mp3", &beats, &columns, Existing { ext: Some(&previous), ..Existing::default() });
        let reread = crate::parse(&again.ext).unwrap();
        assert!(reread.section(b"PSSI").is_some());
        assert!(!reread.has_extended_grid());
        assert_eq!(reread.header_extra, vec![9; 16]);
        assert_eq!(reread.sections.iter().filter(|s| s.tag == FourCc::new(b"PWV3")).count(), 1);
    }
}
