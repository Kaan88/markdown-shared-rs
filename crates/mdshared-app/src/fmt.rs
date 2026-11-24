//! The small texts every application writes next to its data: a size in bytes, a count with
//! thousands separators, a length of time, a date, and the order a person expects names in.
//! Pure functions of numbers and strings: no clock is read, no locale is asked, nothing is
//! allocated but the answer.
//!
//! Deliberately not here: translation and locales. The separators, the units and the order of a
//! date are the ones below on every system, as the rest of the applications' texts are English
//! on every system. A date is written year first, which no reader takes for another day.

use std::cmp::Ordering;

/// A size in bytes as a person reads it: `"0 B"`, `"999 B"`, `"1.9 KB"`, `"12.3 MB"`,
/// `"1.5 GB"`, `"2.0 TB"`. The units are the binary ones under their everyday names (a KB is
/// 1024 bytes), as the file managers of Windows show them, with one decimal from KB upwards.
/// The unit changes when the bytes reach the next one, not when the rounded number does, so the
/// last few bytes below a megabyte read `"1024.0 KB"`: the number never claims more than is
/// there.
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut unit = 0;
    let mut scale: u64 = 1024;
    while unit + 1 < UNITS.len() && bytes >= scale * 1024 {
        scale *= 1024;
        unit += 1;
    }
    format!("{:.1} {}", bytes as f64 / scale as f64, UNITS[unit])
}

/// A count with a comma between each three digits: `"7"`, `"1,284"`, `"31,000"`,
/// `"1,000,000"`.
pub fn count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A length of time in whole seconds as a player shows it: `"0:07"`, `"3:07"`, `"59:59"`,
/// `"1:02:45"`, `"100:00:00"`. Minutes and seconds are two digits once there is a unit above
/// them; the leading unit is as long as it is. There are no days: a length of two days is 48
/// hours.
pub fn duration(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// A moment as the fields of a calendar and a clock.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Civil {
    /// The year of the proleptic Gregorian calendar; it may be negative.
    pub year: i64,
    /// 1 to 12.
    pub month: u8,
    /// 1 to 31.
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    /// 0 is Monday, 6 is Sunday.
    pub weekday: u8,
}

impl Civil {
    /// The calendar fields of `unix_secs` (seconds since 1970-01-01 00:00 UTC, as a file's
    /// modification time is kept; leap seconds do not exist in it) on a clock `offset_minutes`
    /// ahead of UTC. The offset is the caller's to find: this module asks the system nothing.
    /// Every `i64` is answered; years far outside history come out as the arithmetic gives
    /// them.
    pub fn from_unix(unix_secs: i64, offset_minutes: i32) -> Civil {
        let local = unix_secs.saturating_add(i64::from(offset_minutes) * 60);
        let (days, of_day) = (local.div_euclid(86_400), local.rem_euclid(86_400));
        // Days since 1970-01-01 to a date, after Howard Hinnant's `civil_from_days`: the
        // calendar repeats every 400 years (146,097 days), counted from a 1 March so that the
        // leap day is the last day of its year.
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let day_of_era = z.rem_euclid(146_097);
        let year_of_era =
            (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let shifted_month = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
        let month = if shifted_month < 10 {
            shifted_month + 3
        } else {
            shifted_month - 9
        };
        let year = year_of_era + era * 400 + i64::from(month <= 2);
        Civil {
            year,
            month: month as u8,
            day: day as u8,
            hour: (of_day / 3600) as u8,
            minute: (of_day / 60 % 60) as u8,
            second: (of_day % 60) as u8,
            // 1970-01-01 was a Thursday.
            weekday: (days + 3).rem_euclid(7) as u8,
        }
    }
}

/// The day of a moment: `"2026-11-24"`. See [`Civil::from_unix`] for the two numbers.
pub fn date(unix_secs: i64, offset_minutes: i32) -> String {
    let c = Civil::from_unix(unix_secs, offset_minutes);
    format!("{:04}-{:02}-{:02}", c.year, c.month, c.day)
}

/// The day and the minute of a moment: `"2026-11-24 14:05"`, on a 24-hour clock. What a list of
/// files shows under "Modified".
pub fn date_time(unix_secs: i64, offset_minutes: i32) -> String {
    let c = Civil::from_unix(unix_secs, offset_minutes);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        c.year, c.month, c.day, c.hour, c.minute
    )
}

/// The order a person expects names in: `"track 2"` before `"track 10"`, and `"readme"` next to
/// `"README"`.
///
/// A run of ASCII digits is compared as the number it writes, however long (no overflow: the
/// digits are compared, not converted). Everything else is compared character by character in
/// Unicode lower case, the folding the folder tree sorts by. Names that are equal that way are
/// not equal here unless they are the same string: fewer leading zeros come first (`"a1"`
/// before `"a01"`), and then the plain order of the strings decides (`"README"` before
/// `"readme"`). So the order is total, and a sort by it is the same on every run.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    // The first difference that folding or the value of a number hides.
    let mut zeros = Ordering::Equal;
    loop {
        let (ca, cb) = match (x.peek().copied(), y.peek().copied()) {
            (None, None) => break,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ca), Some(cb)) => (ca, cb),
        };
        if ca.is_ascii_digit() && cb.is_ascii_digit() {
            let run = |it: &mut std::iter::Peekable<std::str::Chars>| {
                let mut digits = String::new();
                while let Some(d) = it.peek().copied().filter(char::is_ascii_digit) {
                    digits.push(d);
                    it.next();
                }
                digits
            };
            let (da, db) = (run(&mut x), run(&mut y));
            let (na, nb) = (da.trim_start_matches('0'), db.trim_start_matches('0'));
            // Without leading zeros the longer number is the larger, and equal lengths compare
            // digit by digit.
            let by_value = na.len().cmp(&nb.len()).then_with(|| na.cmp(nb));
            if by_value != Ordering::Equal {
                return by_value;
            }
            zeros = zeros.then(da.len().cmp(&db.len()));
            continue;
        }
        let folded = ca.to_lowercase().cmp(cb.to_lowercase());
        if folded != Ordering::Equal {
            return folded;
        }
        x.next();
        y.next();
    }
    zeros.then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_change_unit_at_each_power_of_1024() {
        for (bytes, text) in [
            (0, "0 B"),
            (1, "1 B"),
            (1023, "1023 B"),
            (1024, "1.0 KB"),
            (1946, "1.9 KB"),
            (1_048_575, "1024.0 KB"),
            (1_048_576, "1.0 MB"),
            (12_897_485, "12.3 MB"),
            (1_073_741_823, "1024.0 MB"),
            (1_073_741_824, "1.0 GB"),
            (1_610_612_736, "1.5 GB"),
            (2_199_023_255_552, "2.0 TB"),
            // Past the last unit the number grows.
            (u64::MAX, "16777216.0 TB"),
        ] {
            assert_eq!(size(bytes), text, "{bytes}");
        }
    }

    #[test]
    fn counts_get_a_comma_every_three_digits() {
        for (n, text) in [
            (0, "0"),
            (7, "7"),
            (999, "999"),
            (1000, "1,000"),
            (1284, "1,284"),
            (31_000, "31,000"),
            (123_456, "123,456"),
            (1_000_000, "1,000,000"),
            (u64::MAX, "18,446,744,073,709,551,615"),
        ] {
            assert_eq!(count(n), text);
        }
    }

    #[test]
    fn durations_read_as_a_player_shows_them() {
        for (secs, text) in [
            (0, "0:00"),
            (7, "0:07"),
            (59, "0:59"),
            (60, "1:00"),
            (187, "3:07"),
            (3599, "59:59"),
            (3600, "1:00:00"),
            (3765, "1:02:45"),
            (86_400, "24:00:00"),
            (360_000, "100:00:00"),
        ] {
            assert_eq!(duration(secs), text);
        }
        assert!(duration(u64::MAX).ends_with(":15"));
    }

    #[test]
    fn dates_are_right_at_the_epoch_at_leap_days_and_before_1970() {
        for (secs, offset, text) in [
            (0, 0, "1970-01-01 00:00"),
            (-1, 0, "1969-12-31 23:59"),
            (86_399, 0, "1970-01-01 23:59"),
            (86_400, 0, "1970-01-02 00:00"),
            // A leap day, the day after it, and the century that has none.
            (951_782_400, 0, "2000-02-29 00:00"),
            (951_868_800, 0, "2000-03-01 00:00"),
            (4_107_456_000, 0, "2100-02-28 00:00"),
            (4_107_542_400, 0, "2100-03-01 00:00"),
            (1_709_164_800, 0, "2024-02-29 00:00"),
            // The day this was written, in three zones.
            (1_795_529_100, 0, "2026-11-24 14:05"),
            (1_795_529_100, 180, "2026-11-24 17:05"),
            (1_795_529_100, -900, "2026-11-23 23:05"),
            (1_795_529_100, 600, "2026-11-25 00:05"),
            // The last second of a year, and the first of the next one in a zone ahead.
            (1_798_761_599, 0, "2026-12-31 23:59"),
            (1_798_761_599, 1, "2027-01-01 00:00"),
            (-2_208_988_800, 0, "1900-01-01 00:00"),
        ] {
            assert_eq!(date_time(secs, offset), text, "{secs} at {offset}");
            assert_eq!(date(secs, offset), &text[..10]);
        }
        let c = Civil::from_unix(1_795_529_100, 0);
        assert_eq!((c.year, c.month, c.day), (2026, 11, 24));
        assert_eq!((c.hour, c.minute, c.second), (14, 5, 0));
        assert_eq!(c.weekday, 1, "a Tuesday");
        assert_eq!(Civil::from_unix(0, 0).weekday, 3, "a Thursday");
        assert_eq!(Civil::from_unix(-86_400, 0).weekday, 2);
        // Every day of four centuries follows the one before it, so no month is the wrong length.
        let mut last = Civil::from_unix(-100 * 365 * 86_400, 0);
        for day in 1..(400 * 366) {
            let c = Civil::from_unix((day - 100 * 365) * 86_400, 0);
            let next_day = c.day == last.day + 1 && c.month == last.month && c.year == last.year;
            let next_month = c.day == 1
                && ((c.month == last.month + 1 && c.year == last.year)
                    || (c.month == 1 && last.month == 12 && c.year == last.year + 1));
            assert!(next_day || next_month, "{last:?} then {c:?}");
            if next_month {
                let leap = last.year % 4 == 0 && (last.year % 100 != 0 || last.year % 400 == 0);
                let length = match last.month {
                    2 if leap => 29,
                    2 => 28,
                    4 | 6 | 9 | 11 => 30,
                    _ => 31,
                };
                assert_eq!(last.day, length, "{last:?}");
            }
            assert_eq!(c.weekday, (last.weekday + 1) % 7);
            last = c;
        }
        // The ends of the range do not panic.
        let _ = date_time(i64::MAX, i32::MAX);
        let _ = date_time(i64::MIN, i32::MIN);
    }

    #[test]
    fn names_sort_with_numbers_as_numbers_and_case_folded() {
        let mut names = vec![
            "track 10.flac",
            "Track 2.flac",
            "track 1.flac",
            "readme",
            "README",
            "a01",
            "a1",
            "a2",
            "a",
            "Zebra",
            "apple",
            "Émile",
            "émile",
            "file99999999999999999999999",
            "file100000000000000000000000",
            "",
            "10",
            "9",
            "1.10",
            "1.9",
        ];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            names,
            vec![
                "",
                "1.9",
                "1.10",
                "9",
                "10",
                "a",
                "a1",
                "a01",
                "a2",
                "apple",
                "file99999999999999999999999",
                "file100000000000000000000000",
                "README",
                "readme",
                "track 1.flac",
                "Track 2.flac",
                "track 10.flac",
                "Zebra",
                "Émile",
                "émile",
            ]
        );
        assert_eq!(natural_cmp("abc", "abc"), Ordering::Equal);
        assert_eq!(natural_cmp("abc", "abd"), Ordering::Less);
        assert_eq!(natural_cmp("a10b", "a10a"), Ordering::Greater);
        assert_eq!(natural_cmp("a", "a1"), Ordering::Less);
        // A digit against a letter is a character like any other.
        assert_eq!(natural_cmp("a1", "ab"), Ordering::Less);
        assert_eq!(natural_cmp("x007", "x7"), Ordering::Greater);
        assert_eq!(natural_cmp("x007y", "x7z"), Ordering::Less);
    }

    #[test]
    fn the_order_is_total_and_agrees_with_itself() {
        let names = [
            "", "0", "00", "1", "01", "a", "A", "a0", "a00", "a1b", "A1B", "a1B2", "a01b2", "b",
            "ß", "ss", "10a", "9a", "é", "E",
        ];
        for a in names {
            assert_eq!(natural_cmp(a, a), Ordering::Equal);
            for b in names {
                let ab = natural_cmp(a, b);
                assert_eq!(ab, natural_cmp(b, a).reverse(), "{a:?} {b:?}");
                assert_eq!(ab == Ordering::Equal, a == b, "{a:?} {b:?}");
                for c in names {
                    if ab == Ordering::Less && natural_cmp(b, c) == Ordering::Less {
                        assert_eq!(natural_cmp(a, c), Ordering::Less, "{a:?} {b:?} {c:?}");
                    }
                }
            }
        }
    }
}
