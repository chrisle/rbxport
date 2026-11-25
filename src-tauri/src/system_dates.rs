//! System short dates, formatted once per requested row batch, never per cell.
//! Stored dates are Gregorian civil days; filesystem timestamps are local days.
use std::collections::{BTreeMap, HashMap};
use time::{Date, Month, OffsetDateTime};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Value {
    Day(Date),
    Epoch(i64),
}

fn day(text: &str) -> Option<Date> {
    let bytes = text.as_bytes().get(..10)?;
    if bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(at, byte)| !matches!(at, 4 | 7) && !byte.is_ascii_digit())
    {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let year: i32 = text[..4].parse().ok()?;
    if !(1..=9999).contains(&year) {
        return None;
    }
    Date::from_calendar_date(
        year,
        Month::try_from(text[5..7].parse::<u8>().ok()?).ok()?,
        text[8..10].parse().ok()?,
    )
    .ok()
}

fn epoch(seconds: f64) -> Option<i64> {
    if !seconds.is_finite() {
        return None;
    }
    // Date's supported Unix range is far inside i64, so this cannot saturate.
    #[allow(clippy::cast_possible_truncation)]
    let seconds = seconds.floor() as i64;
    let date = OffsetDateTime::from_unix_timestamp(seconds).ok()?.date();
    (1..=9999).contains(&date.year()).then_some(seconds)
}

const COLUMNS: [&str; 4] = ["dateAdded", "releaseDate", "dateCreated", "fileCreated"];
fn value(row: &crate::dto::RowDto, column: &str) -> Option<Value> {
    match column {
        "dateAdded" => day(&row.date_added).map(Value::Day),
        "releaseDate" => day(&row.release_date).map(Value::Day),
        "dateCreated" => day(row.extra.as_ref()?.get(column)?.as_str()?).map(Value::Day),
        "fileCreated" => epoch(row.extra.as_ref()?.get(column)?.as_f64()?).map(Value::Epoch),
        _ => None,
    }
}

/// `fetch_rows` has a 128-row cap. Its transient deduplication map therefore
/// holds at most 512 date values, then drops with this response. The existing
/// view page cache owns the labels and invalidates them with their source.
pub fn rows(rows: &mut [crate::dto::RowDto], requested: &[String]) {
    let wanted: Vec<_> = COLUMNS
        .into_iter()
        .filter(|key| requested.iter().any(|item| item == key))
        .collect();
    if wanted.is_empty() {
        return;
    }
    let mut formatter = platform::Formatter::new();
    rows_with(rows, &wanted, |value| format(&mut formatter, value));
}

fn rows_with(
    rows: &mut [crate::dto::RowDto],
    wanted: &[&str],
    mut print: impl FnMut(Value) -> String,
) {
    let mut cache = HashMap::new();
    for row in rows {
        let mut labels = BTreeMap::new();
        for &column in wanted {
            let label = value(row, column).map_or_else(String::new, |value| {
                cache.entry(value).or_insert_with(|| print(value)).clone()
            });
            labels.insert(column.to_owned(), label);
        }
        row.date_labels = labels;
    }
}

fn format(formatter: &mut Option<platform::Formatter>, value: Value) -> String {
    formatter
        .as_mut()
        .and_then(|f| f.format(value))
        .unwrap_or_else(|| match value {
            // An unavailable native locale must not reinterpret a stored day.
            Value::Day(day) => day.to_string(),
            // No UTC fallback: that could display the wrong local filesystem day.
            Value::Epoch(_) => String::new(),
        })
}

pub fn stored(text: &str) -> String {
    day(text).map_or_else(String::new, |day| {
        format(&mut platform::Formatter::new(), Value::Day(day))
    })
}

#[cfg(target_os = "macos")]
mod platform {
    use super::Value;
    use objc2::rc::Retained;
    use objc2_foundation::{
        NSCalendar, NSCalendarIdentifierGregorian, NSDate, NSDateComponents, NSDateFormatter,
        NSDateFormatterStyle, NSLocale, NSTimeZone,
    };

    pub struct Formatter {
        native: Retained<NSDateFormatter>,
        calendar: Retained<NSCalendar>,
        utc: Retained<NSTimeZone>,
        local: Retained<NSTimeZone>,
    }
    impl Formatter {
        pub fn new() -> Option<Self> {
            Self::with_locale(&NSLocale::currentLocale())
        }
        fn with_locale(locale: &NSLocale) -> Option<Self> {
            // SAFETY: immutable Foundation identifier constant. The formatter
            // and calendar remain owned by this batch on its worker thread.
            #[allow(unsafe_code)]
            let calendar =
                NSCalendar::calendarWithIdentifier(unsafe { NSCalendarIdentifierGregorian })?;
            let utc = NSTimeZone::timeZoneForSecondsFromGMT(0);
            calendar.setTimeZone(&utc);
            let formatter = NSDateFormatter::new();
            formatter.setLocale(Some(locale));
            formatter.setDateStyle(NSDateFormatterStyle::ShortStyle);
            formatter.setTimeStyle(NSDateFormatterStyle::NoStyle);
            // Mirrors getUserShortDateFormat's explicit Gregorian property.
            formatter.setCalendar(Some(&calendar));
            Some(Self {
                native: formatter,
                calendar,
                utc,
                local: NSTimeZone::localTimeZone(),
            })
        }
        pub fn format(&mut self, value: Value) -> Option<String> {
            let date = match value {
                Value::Day(day) => {
                    self.native.setTimeZone(Some(&self.utc));
                    let components = NSDateComponents::new();
                    components.setYear(isize::try_from(day.year()).ok()?);
                    components.setMonth(isize::from(u8::from(day.month())));
                    components.setDay(isize::from(day.day()));
                    components.setHour(12);
                    self.calendar.dateFromComponents(&components)?
                }
                Value::Epoch(seconds) => {
                    self.native.setTimeZone(Some(&self.local));
                    #[allow(clippy::cast_precision_loss)]
                    NSDate::dateWithTimeIntervalSince1970(seconds as f64)
                }
            };
            Some(self.native.stringFromDate(&date).to_string())
        }
        #[cfg(test)]
        #[allow(clippy::unwrap_used)]
        pub(super) fn current_pattern_for_test() -> String {
            Self::new().unwrap().native.dateFormat().to_string()
        }
        #[cfg(test)]
        pub(super) fn test_offset(&mut self, seconds: isize) {
            self.local = NSTimeZone::timeZoneForSecondsFromGMT(seconds);
        }
        #[cfg(test)]
        #[allow(clippy::expect_used)]
        pub(super) fn for_test(locale: &str, pattern: Option<&str>) -> Self {
            use objc2_foundation::NSString;
            let locale = NSLocale::localeWithLocaleIdentifier(&NSString::from_str(locale));
            let result = Self::with_locale(&locale).expect("Gregorian formatter");
            if let Some(pattern) = pattern {
                result
                    .native
                    .setDateFormat(Some(&NSString::from_str(pattern)));
            }
            result
        }
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod platform {
    use super::Value;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::{
        Foundation::{FILETIME, SYSTEMTIME},
        Globalization::{GetDateFormatEx, DATE_SHORTDATE},
        System::Time::{
            FileTimeToSystemTime, SystemTimeToTzSpecificLocalTimeEx, DYNAMIC_TIME_ZONE_INFORMATION,
        },
    };

    // WinNls.h defines LOCALE_NAME_USER_DEFAULT as a null locale name.
    pub(super) const USER_DEFAULT: windows_sys::core::PCWSTR = null();
    pub struct Formatter;
    impl Formatter {
        pub fn new() -> Option<Self> {
            Some(Self)
        }
        pub fn format(&mut self, value: Value) -> Option<String> {
            let date = match value {
                Value::Day(day) => SYSTEMTIME {
                    wYear: u16::try_from(day.year()).ok()?,
                    wMonth: u16::from(u8::from(day.month())),
                    wDay: u16::from(day.day()),
                    wDayOfWeek: u16::from(day.weekday().number_days_from_sunday()),
                    ..Default::default()
                },
                Value::Epoch(seconds) => local_time(seconds, None)?,
            };
            print(&date, USER_DEFAULT, None)
        }
    }
    fn local_time(
        seconds: i64,
        zone: Option<&DYNAMIC_TIME_ZONE_INFORMATION>,
    ) -> Option<SYSTEMTIME> {
        let ticks = u64::try_from(seconds.checked_add(11_644_473_600)?)
            .ok()?
            .checked_mul(10_000_000)?;
        let bytes = ticks.to_le_bytes();
        let file = FILETIME {
            dwLowDateTime: u32::from_le_bytes(bytes[..4].try_into().ok()?),
            dwHighDateTime: u32::from_le_bytes(bytes[4..].try_into().ok()?),
        };
        let mut utc = SYSTEMTIME::default();
        let mut local = SYSTEMTIME::default();
        // SAFETY: initialized fixed-size structures and an optional borrowed
        // zone valid for this call. Null selects the current dynamic timezone;
        // Ex applies its historical DST rules for the timestamp's year.
        if unsafe { FileTimeToSystemTime(&file, &mut utc) } == 0
            || unsafe {
                SystemTimeToTzSpecificLocalTimeEx(
                    zone.map_or(null(), std::ptr::from_ref),
                    &utc,
                    &mut local,
                )
            } == 0
        {
            return None;
        }
        Some(local)
    }
    #[cfg(test)]
    pub(super) fn historical_epoch_for_test(
        seconds: i64,
        zone: &DYNAMIC_TIME_ZONE_INFORMATION,
    ) -> Option<SYSTEMTIME> {
        local_time(seconds, Some(zone))
    }
    fn print(
        date: &SYSTEMTIME,
        locale: windows_sys::core::PCWSTR,
        pattern: Option<&[u16]>,
    ) -> Option<String> {
        let pattern = pattern.map_or(null(), <[u16]>::as_ptr);
        let flags = if pattern.is_null() { DATE_SHORTDATE } else { 0 };
        // SAFETY: all input pointers live for both calls, the first queries
        // capacity, and the second receives exactly that initialized capacity.
        // No LOCALE_NOUSEROVERRIDE or alternate-calendar flag is passed.
        let size = unsafe { GetDateFormatEx(locale, flags, date, pattern, null_mut(), 0, null()) };
        if !(1..=4096).contains(&size) {
            return None;
        }
        let mut out = vec![0; usize::try_from(size).ok()?];
        let written = unsafe {
            GetDateFormatEx(locale, flags, date, pattern, out.as_mut_ptr(), size, null())
        };
        if written <= 0 || written > size {
            return None;
        }
        String::from_utf16(out.get(..usize::try_from(written - 1).ok()?)?).ok()
    }
    #[cfg(test)]
    pub(super) fn user_pattern_for_test(day: time::Date, pattern: &str) -> Option<String> {
        let pattern: Vec<_> = pattern.encode_utf16().chain([0]).collect();
        print(
            &SYSTEMTIME {
                wYear: u16::try_from(day.year()).ok()?,
                wMonth: u16::from(u8::from(day.month())),
                wDay: u16::from(day.day()),
                ..Default::default()
            },
            USER_DEFAULT,
            Some(&pattern),
        )
    }
    #[cfg(test)]
    pub(super) fn for_test(day: time::Date, locale: &str, pattern: Option<&str>) -> Option<String> {
        let locale: Vec<_> = locale.encode_utf16().chain([0]).collect();
        let pattern: Option<Vec<_>> = pattern.map(|text| text.encode_utf16().chain([0]).collect());
        print(
            &SYSTEMTIME {
                wYear: u16::try_from(day.year()).ok()?,
                wMonth: u16::from(u8::from(day.month())),
                wDay: u16::from(day.day()),
                ..Default::default()
            },
            locale.as_ptr(),
            pattern.as_deref(),
        )
    }
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
mod platform {
    use super::Value;
    pub struct Formatter(libc::locale_t);
    impl Formatter {
        pub fn new() -> Option<Self> {
            // SAFETY: newlocale creates an owned thread-independent LC_TIME
            // locale from the environment; it never installs a global locale.
            let locale =
                unsafe { libc::newlocale(libc::LC_TIME_MASK, c"".as_ptr(), std::ptr::null_mut()) };
            (!locale.is_null()).then_some(Self(locale))
        }
        pub fn format(&mut self, value: Value) -> Option<String> {
            // SAFETY: zero is valid for every tm field; only required calendar
            // components are used, and localtime_r fills the entire structure.
            let mut tm: libc::tm = unsafe { std::mem::zeroed() };
            match value {
                Value::Day(day) => {
                    tm.tm_year = day.year() - 1900;
                    tm.tm_mon = i32::from(u8::from(day.month())) - 1;
                    tm.tm_mday = i32::from(day.day());
                    tm.tm_wday = i32::from(day.weekday().number_days_from_sunday());
                    tm.tm_yday = i32::from(day.ordinal()) - 1;
                }
                Value::Epoch(seconds) => {
                    let seconds = libc::time_t::try_from(seconds).ok()?;
                    if unsafe { libc::localtime_r(&raw const seconds, &raw mut tm) }.is_null() {
                        return None;
                    }
                }
            }
            for size in [128, 512, 4096] {
                let mut bytes = vec![0_u8; size];
                // SAFETY: buffer, tm and owned locale remain valid; strftime_l
                // receives the exact capacity. %x is LC_TIME's date format.
                let count = unsafe {
                    libc::strftime_l(
                        bytes.as_mut_ptr().cast(),
                        bytes.len(),
                        c"%x".as_ptr(),
                        &raw const tm,
                        self.0,
                    )
                };
                if count != 0 {
                    bytes.truncate(count);
                    return String::from_utf8(bytes).ok();
                }
            }
            None
        }
    }
    #[cfg(test)]
    impl Formatter {
        #[allow(clippy::unwrap_used)]
        pub(super) fn for_test(locale: &str) -> Self {
            let locale = std::ffi::CString::new(locale).unwrap();
            let value = unsafe {
                libc::newlocale(libc::LC_TIME_MASK, locale.as_ptr(), std::ptr::null_mut())
            };
            assert!(!value.is_null());
            Self(value)
        }
    }
    impl Drop for Formatter {
        fn drop(&mut self) {
            unsafe {
                libc::freelocale(self.0);
            }
        }
    }
}

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
mod platform {
    pub struct Formatter;
    impl Formatter {
        pub fn new() -> Option<Self> {
            None
        }
        pub fn format(&mut self, _: super::Value) -> Option<String> {
            None
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    fn row() -> crate::dto::RowDto {
        crate::dto::RowDto {
            id: "fixture".into(),
            track_no: 1,
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            genre: String::new(),
            label: String::new(),
            comment: String::new(),
            bpm_x100: 0,
            key: String::new(),
            duration_sec: 0,
            rating: 0,
            analysed: 0,
            date_added: "2025-03-04 23:59:00 -11:00".into(),
            release_date: "2025-03-04".into(),
            date_labels: BTreeMap::new(),
            hot_cues: vec![],
            memory_cues: vec![],
            memory_cue_colours: vec![],
            artwork_hue: 0,
            has_artwork: false,
            file_name: String::new(),
            missing: false,
            extra: Some(serde_json::Map::from_iter([
                ("dateCreated".into(), serde_json::json!("2025-03-04")),
                ("fileCreated".into(), serde_json::json!(1_741_045_500)),
            ])),
            device_source: None,
        }
    }
    #[test]
    fn system_dates_validate_gregorian_days_without_reinterpreting_years_or_timezones() {
        for invalid in [
            "",
            "2025-02-29",
            "2024-02-30",
            "2025-13-01",
            "0000-01-01",
            "2025-01-00",
            "2025-界-01",
            "+025-03-04",
            "2025-+3-04",
            "2025-03-+4",
        ] {
            assert!(day(invalid).is_none(), "{invalid}");
        }
        assert_eq!(day("2024-02-29").unwrap().day(), 29);
        assert_eq!(day("0099-03-04").unwrap().year(), 99);
        assert_eq!(
            day("2025-03-04 23:59:00 -11:00"),
            day("2025-03-04 00:01:00 +14:00")
        );
        assert_eq!(epoch(-0.5), Some(-1));
        for invalid in [f64::NAN, f64::INFINITY, f64::MAX] {
            assert!(epoch(invalid).is_none());
        }
    }
    #[test]
    fn system_dates_deduplicate_only_requested_labels_and_preserve_stored_data() {
        let mut rows = vec![row(), row()];
        let mut calls = 0;
        rows_with(
            &mut rows,
            &["dateAdded", "releaseDate", "dateCreated"],
            |_| {
                calls += 1;
                "04.03.25 literal".into()
            },
        );
        assert_eq!(calls, 1, "six cells share one Gregorian date");
        assert_eq!(rows[0].date_labels.len(), 3);
        assert!(!rows[0].date_labels.contains_key("fileCreated"));
        assert_eq!(rows[0].date_labels["dateAdded"], "04.03.25 literal");
        assert_eq!(rows[0].date_added, "2025-03-04 23:59:00 -11:00");
        rows[0].date_added = "2025-02-29".into();
        rows_with(&mut rows, &["dateAdded"], |_| "new day".into());
        assert_eq!(
            rows[0].date_labels["dateAdded"], "",
            "invalid/stale labels cannot survive a new response"
        );
        assert_eq!(rows[0].date_labels.len(), 1);
    }
    #[test]
    fn system_dates_use_a_native_formatter_and_never_shift_failed_epoch_dates_to_utc() {
        assert_ne!(stored("2025-03-04"), "");
        assert_eq!(stored("2025-02-29"), "");
        assert_eq!(
            format(&mut None, Value::Day(day("2025-03-04").unwrap())),
            "2025-03-04"
        );
        assert_eq!(format(&mut None, Value::Epoch(0)), "");
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn system_dates_mac_short_styles_and_custom_literals_keep_native_widths() {
        let day = Value::Day(day("2025-03-04").unwrap());
        for (locale, expected) in [
            ("en_US", "3/4/25"),
            ("en_GB", "04/03/2025"),
            ("de_DE", "04.03.25"),
        ] {
            assert_eq!(
                platform::Formatter::for_test(locale, None)
                    .format(day)
                    .unwrap(),
                expected
            );
        }
        assert_eq!(
            platform::Formatter::for_test("en_US", Some("dd 'day' MM 'year' yy"))
                .format(day)
                .unwrap(),
            "04 day 03 year 25"
        );
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn system_dates_mac_civil_days_do_not_shift_but_epochs_follow_the_local_day() {
        let mut formatter = platform::Formatter::for_test("en_GB", None);
        let timestamp = day("2025-03-04")
            .unwrap()
            .with_hms(0, 15, 0)
            .unwrap()
            .assume_utc()
            .unix_timestamp();
        for (offset, expected) in [(-11 * 3600, "03/03/2025"), (14 * 3600, "04/03/2025")] {
            formatter.test_offset(offset);
            assert_eq!(
                formatter
                    .format(Value::Day(day("2025-03-04").unwrap()))
                    .unwrap(),
                "04/03/2025"
            );
            assert_eq!(formatter.format(Value::Epoch(timestamp)).unwrap(), expected);
            assert_eq!(
                formatter
                    .format(Value::Day(day("2025-03-04").unwrap()))
                    .unwrap(),
                "04/03/2025",
                "Day → Epoch → Day must restore the fixed calendar timezone"
            );
        }
    }
    #[cfg(windows)]
    #[test]
    fn system_dates_windows_native_short_styles_and_custom_literals() {
        let day = day("2025-03-04").unwrap();
        for (locale, pattern, expected) in [
            ("en-US", "M/d/yy", "3/4/25"),
            ("en-GB", "dd/MM/yyyy", "04/03/2025"),
            ("de-DE", "dd.MM.yy", "04.03.25"),
        ] {
            assert_eq!(
                platform::for_test(day, locale, Some(pattern)).unwrap(),
                expected
            );
        }
        assert_eq!(
            platform::for_test(day, "en-US", Some("dd 'day' MM 'year' yy")).unwrap(),
            "04 day 03 year 25"
        );
        assert!(platform::Formatter::new()
            .unwrap()
            .format(Value::Day(day))
            .is_some());
    }
    #[cfg(windows)]
    #[test]
    #[allow(unsafe_code)]
    fn system_dates_windows_default_uses_the_current_user_short_pattern() {
        use windows_sys::Win32::Globalization::{GetLocaleInfoEx, LOCALE_SSHORTDATE};
        let mut pattern = vec![0_u16; 4096];
        // SAFETY: initialized writable UTF-16 buffer; reads current user
        // locale metadata only, without LOCALE_NOUSEROVERRIDE.
        let count = unsafe {
            GetLocaleInfoEx(
                platform::USER_DEFAULT,
                LOCALE_SSHORTDATE,
                pattern.as_mut_ptr(),
                4096,
            )
        };
        assert!(count > 1);
        pattern.truncate(usize::try_from(count).unwrap());
        let pattern = String::from_utf16(&pattern[..pattern.len() - 1]).unwrap();
        let date = day("2025-03-04").unwrap();
        let expected = platform::user_pattern_for_test(date, &pattern).unwrap();
        assert_eq!(
            platform::Formatter::new()
                .unwrap()
                .format(Value::Day(date))
                .unwrap(),
            expected
        );
    }
    #[cfg(windows)]
    #[test]
    #[allow(unsafe_code)]
    fn system_dates_windows_epoch_uses_historical_dynamic_dst_without_changing_preferences() {
        use windows_sys::Win32::{
            Foundation::SYSTEMTIME,
            System::Time::{
                EnumDynamicTimeZoneInformation, GetTimeZoneInformationForYear,
                SystemTimeToTzSpecificLocalTime, DYNAMIC_TIME_ZONE_INFORMATION,
                TIME_ZONE_INFORMATION,
            },
        };
        // Enumerate read-only: never install a timezone or change user settings.
        let mut pacific = None;
        for index in 0..1024 {
            let mut zone = DYNAMIC_TIME_ZONE_INFORMATION::default();
            if unsafe { EnumDynamicTimeZoneInformation(index, &mut zone) } != 0 {
                break;
            }
            let end = zone.TimeZoneKeyName.iter().position(|&c| c == 0).unwrap();
            if String::from_utf16(&zone.TimeZoneKeyName[..end]).unwrap() == "Pacific Standard Time"
            {
                pacific = Some(zone);
                break;
            }
        }
        let zone = pacific.expect("Windows Pacific Standard Time registry entry");
        for (year, expected_day) in [(2006, 14), (2007, 15)] {
            let timestamp = time::Date::from_calendar_date(year, time::Month::March, 15)
                .unwrap()
                .with_hms(7, 30, 0)
                .unwrap()
                .assume_utc()
                .unix_timestamp();
            let local = platform::historical_epoch_for_test(timestamp, &zone).unwrap();
            assert_eq!(
                (local.wYear, local.wMonth, local.wDay),
                (u16::try_from(year).unwrap(), 3, expected_day)
            );
            // Independent read-only oracle: resolve this timestamp's year to
            // its specific registry rules, then convert with that ruleset.
            let mut year_rules = TIME_ZONE_INFORMATION::default();
            assert_ne!(
                unsafe {
                    GetTimeZoneInformationForYear(
                        u16::try_from(year).unwrap(),
                        &zone,
                        &mut year_rules,
                    )
                },
                0
            );
            let utc = SYSTEMTIME {
                wYear: u16::try_from(year).unwrap(),
                wMonth: 3,
                wDay: 15,
                wHour: 7,
                wMinute: 30,
                ..Default::default()
            };
            let mut expected = SYSTEMTIME::default();
            assert_ne!(
                unsafe { SystemTimeToTzSpecificLocalTime(&year_rules, &utc, &mut expected) },
                0
            );
            assert_eq!(
                (
                    local.wYear,
                    local.wMonth,
                    local.wDay,
                    local.wHour,
                    local.wMinute
                ),
                (
                    expected.wYear,
                    expected.wMonth,
                    expected.wDay,
                    expected.wHour,
                    expected.wMinute
                )
            );
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn system_dates_linux_epoch_follows_local_day_in_isolated_children() {
        const TEST: &str =
            "system_dates::tests::system_dates_linux_epoch_follows_local_day_in_isolated_children";
        if let Ok(expected) = std::env::var("RBX881_DATE_CHILD_EXPECTED") {
            let mut formatter = platform::Formatter::for_test("C");
            let date = day("2025-03-04").unwrap();
            let timestamp = date
                .with_hms(0, 15, 0)
                .unwrap()
                .assume_utc()
                .unix_timestamp();
            assert_eq!(formatter.format(Value::Day(date)).unwrap(), "03/04/25");
            assert_eq!(formatter.format(Value::Epoch(timestamp)).unwrap(), expected);
            assert_eq!(formatter.format(Value::Day(date)).unwrap(), "03/04/25");
            return;
        }
        // Child-only TZ: never mutate the application's global environment or
        // locale while other native tests/threads may be running.
        for (zone, expected) in [("UTC-14", "03/04/25"), ("UTC+11", "03/03/25")] {
            let result = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TEST])
                .env("TZ", zone)
                .env("RBX881_DATE_CHILD_EXPECTED", expected)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn system_dates_linux_uses_owned_locale_without_changing_global_locale() {
        let mut formatter = platform::Formatter::for_test("C");
        assert_eq!(
            formatter
                .format(Value::Day(day("2025-03-04").unwrap()))
                .unwrap(),
            "03/04/25"
        );
        assert_eq!(
            formatter
                .format(Value::Day(day("2024-02-29").unwrap()))
                .unwrap(),
            "02/29/24"
        );
    }
}

#[cfg(all(test, target_os = "macos"))]
#[allow(unsafe_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod mac_primary_contract {
    use std::ffi::{c_char, c_void, CStr};
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFLocaleCopyCurrent() -> *const c_void;
        fn CFDateFormatterCreate(
            allocator: *const c_void,
            locale: *const c_void,
            date: isize,
            time: isize,
        ) -> *const c_void;
        fn CFDateFormatterSetProperty(
            formatter: *const c_void,
            key: *const c_void,
            value: *const c_void,
        );
        fn CFDateFormatterGetFormat(formatter: *const c_void) -> *const c_void;
        fn CFStringGetLength(string: *const c_void) -> isize;
        fn CFStringGetCString(
            string: *const c_void,
            buffer: *mut c_char,
            capacity: isize,
            encoding: u32,
        ) -> bool;
        fn CFRelease(value: *const c_void);
        static kCFDateFormatterCalendarName: *const c_void;
        static kCFGregorianCalendar: *const c_void;
    }
    #[test]
    fn system_dates_mac_foundation_matches_the_primary_current_cf_gregorian_pattern() {
        // SAFETY: Copy/Create return owned CF objects, retained until after
        // the borrowed pattern is copied. Only immutable OS metadata is read.
        let pattern = unsafe {
            let locale = CFLocaleCopyCurrent();
            assert!(!locale.is_null());
            let formatter = CFDateFormatterCreate(std::ptr::null(), locale, 1, 0);
            assert!(!formatter.is_null());
            CFDateFormatterSetProperty(
                formatter,
                kCFDateFormatterCalendarName,
                kCFGregorianCalendar,
            );
            let pattern = CFDateFormatterGetFormat(formatter);
            assert!(!pattern.is_null());
            let size = CFStringGetLength(pattern).checked_mul(4).unwrap() + 1;
            assert!((1..=4096).contains(&size));
            let mut buffer = vec![0_u8; usize::try_from(size).unwrap()];
            assert!(CFStringGetCString(
                pattern,
                buffer.as_mut_ptr().cast(),
                size,
                0x0800_0100
            ));
            let text = CStr::from_ptr(buffer.as_ptr().cast())
                .to_str()
                .unwrap()
                .to_owned();
            CFRelease(formatter);
            CFRelease(locale);
            text
        };
        assert_eq!(
            super::platform::Formatter::current_pattern_for_test(),
            pattern
        );
    }
}
