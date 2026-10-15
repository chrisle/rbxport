//! Timestamps in the format rekordbox stores.
//!
//! `2026-02-24 00:53:12.292 +00:00` — millisecond precision and an explicit
//! UTC offset. Of the 1,602 playlists in the reference library, 1,575 carry
//! `+00:00` and the rest carry a local offset written by an older version, so
//! UTC is what a new row uses.
//!
//! Computed here rather than pulled from a date crate: the conversion is a
//! dozen lines, and it keeps a formatting dependency out of the write path.

use std::time::{SystemTime, UNIX_EPOCH};

/// Formats a moment as rekordbox writes it.
#[must_use]
pub fn format_utc(unix_seconds: i64, millis: u32) -> String {
    let (year, month, day) = civil_from_days(unix_seconds.div_euclid(86_400));
    let secs_of_day = unix_seconds.rem_euclid(86_400);
    let (hour, minute, second) =
        (secs_of_day / 3600, (secs_of_day % 3600) / 60, secs_of_day % 60);
    format!(
        "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}.{:03} +00:00",
        millis.min(999)
    )
}

/// The current moment, formatted for the database.
#[must_use]
pub fn now() -> String {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = i64::try_from(since.as_secs()).unwrap_or(0);
    format_utc(secs, since.subsec_millis())
}

/// Milliseconds since the epoch, as rekordbox stamps a stick's sync record.
#[must_use]
pub fn unix_millis() -> u64 {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
}

/// Milliseconds since the epoch of a moment as the database stores it
/// (`2026-10-12 23:18:03.154 +00:00`, or an older row's local offset).
/// `None` for anything else, a value with no offset among them: what zone
/// it was written in is not known.
#[must_use]
pub fn parse_millis(stamp: &str) -> Option<u64> {
    let moment = chrono::DateTime::parse_from_str(stamp.trim(), "%Y-%m-%d %H:%M:%S%.f %:z").ok()?;
    u64::try_from(moment.timestamp_millis()).ok()
}

/// Today's date where the machine is, `YYYY-MM-DD`.
///
/// The one place rekordbox writes local time rather than UTC: the date an
/// export was created, in `exportLibrary.db` and as the name of the day's
/// history. Late in the evening west of Greenwich the UTC date is already
/// tomorrow, which is how our exports came to be dated a day after
/// rekordbox's. The `time` crate refuses to read the local offset in a
/// process with threads without an `unsafe` opt-in; `chrono` reads the zone
/// database itself.
#[must_use]
pub fn local_date() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// The moment now where the machine is, `YYYY-MM-DD HH:MM:SS`, which is how
/// `djmdHistory.DateCreated` reads: local time, no offset, no milliseconds
/// (transcribed from the reference library into the fixture).
#[must_use]
pub fn local_stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Local date and 24-hour time for backup filenames, `YYYYMMDD-HHMM`.
#[must_use]
pub fn local_backup_stamp() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M").to_string()
}

/// Civil date from a count of days since 1970-01-01.
///
/// Hinnant's algorithm: shift the era so March is the first month, which makes
/// the leap day the last day of the year and removes every special case.
const fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11], March = 0
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}
