# System short dates

The browser table and Information summary display the operating system's short
Gregorian date style. There is no application date-format preference. Stored
`YYYY-MM-DD` values remain calendar days even when their text includes a timezone;
filesystem epochs instead use the machine's local day. Raw values used for edits,
sorting and persistence are unchanged.

Native row responses include labels only for requested visible date columns.
One formatter and a transient day-to-label map serve each existing bounded page
(at most 128 rows and four date fields). Information details return their saved
Date Created label with the same response. The existing view/source/request and
Information track-ID guards apply to labels as well as raw data. Session snapshots
discard labels so a prior process's locale output is not reused at startup.

* macOS uses the current-user Foundation short-date formatter with an explicit
  Gregorian calendar. This matches the installed reference's CF short-pattern
  selection, including its Gregorian property. Stored days are constructed with
  Gregorian components in UTC; epoch dates use the local timezone.
* Windows uses `GetDateFormatEx` with the user-default locale, `DATE_SHORTDATE`,
  a null explicit pattern and user overrides enabled. Gregorian `SYSTEMTIME`
  components represent stored days; epoch conversion uses the local timezone.
* Linux owns an environment-derived `LC_TIME` locale and uses `strftime_l("%x")`.
  It never changes the process's locale. This implements the LC_TIME/LANG contract;
  additional desktop-specific date preferences are not established here.

Malformed dates are blank. If native formatting is unavailable, a valid stored
calendar day retains its ISO representation; an epoch has no UTC fallback that
could display the wrong local day. Non-UTF-8 Linux output is treated as unavailable.
The browser-only mock uses `Intl.DateTimeFormat` short style with a Gregorian
calendar; it demonstrates browser-locale behavior, not native custom preferences.
The Information Release Date editor retains its existing control order.

## Evidence and checks

Native tests cover Gregorian validation, requested-field filtering, raw-value
preservation, repeated-day deduplication and formatter failure. macOS tests compare
Foundation's current resolved pattern with the CF short/Gregorian path and exercise
custom patterns and mixed civil-day/epoch formatting across UTC+14 and UTC-11.
Windows tests use deterministic explicit patterns and separately compare the
current-user result with the read-only `LOCALE_SSHORTDATE` value, without changing
OS preferences. Linux tests use an owned C locale and isolated timezone children.
The Windows validation workflow explicitly runs the owning `system_dates` tests.

The application command fixture checks row/details labels and unchanged database
values. Chromium/WebKit tests check verbatim native-label presentation and bounded
visible-column requests; they do not simulate the native OS formatter.
