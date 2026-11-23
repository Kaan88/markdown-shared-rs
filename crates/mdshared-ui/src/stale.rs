//! Offering to close the tabs that have not been opened for a while, as both applications do:
//! time is counted in days the application was used, not calendar days, so a week away does not
//! make every tab stale at once. A day of use is a start on a date the application had not been
//! started on yet ([`UseDays::start`]); each tab keeps the day of use it was last shown on, and
//! one not shown in the last [`DAYS`] of them is offered for closing, once, at start, on a bar
//! ([`crate::notice::draw_offer_bar`]). Which tabs may be offered (never a pinned one, the one
//! showing, or one with unsaved changes) is each application's to say.

use std::time::{SystemTime, UNIX_EPOCH};

/// How many days of use a tab may go unopened before it is offered.
pub const DAYS: u32 = 7;

/// The bar's buttons.
pub const CLOSE_THEM: &str = "Close them";
pub const KEEP: &str = "Keep";

/// The days the application was used: how many, and the date (days since 1970, UTC) of the last.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UseDays {
    pub count: u32,
    pub date: u64,
}

impl UseDays {
    /// At start on `today` (see [`today`]): one more day of use when the date is new.
    pub fn start(self, today: u64) -> UseDays {
        if today == self.date {
            self
        } else {
            // The count is read from the session: one at its top stays there (audit 4, E12).
            UseDays {
                count: self.count.saturating_add(1),
                date: today,
            }
        }
    }

    /// `count date`, the settings line's value.
    pub fn to_setting(self) -> String {
        format!("{} {}", self.count, self.date)
    }

    pub fn parse(value: &str) -> Option<UseDays> {
        let mut parts = value.split_whitespace();
        let count = parts.next()?.parse().ok()?;
        let date = parts.next()?.parse().ok()?;
        Some(UseDays { count, date })
    }
}

/// Today's date, in days since 1970 (UTC).
pub fn today() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 86_400)
}

/// Whether a tab last shown on day of use `seen` is to be offered on day `day`.
pub fn is_stale(seen: u32, day: u32) -> bool {
    day.saturating_sub(seen) >= DAYS
}

/// `seen=` line: `day path-or-name`.
pub fn seen_setting(day: u32, key: &str) -> String {
    format!("{day} {key}")
}

/// A `seen=` line's day and key.
pub fn parse_seen(value: &str) -> Option<(u32, &str)> {
    let (day, key) = value.split_once(' ')?;
    Some((day.parse().ok()?, key))
}

/// What the bar says about `n` tabs, for `app` (its display name).
pub fn label(n: usize, app: &str) -> String {
    if n == 1 {
        format!("1 tab has not been opened in the last {DAYS} days you used {app}.")
    } else {
        format!("{n} tabs have not been opened in the last {DAYS} days you used {app}.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_day_of_use_is_a_start_on_a_new_date() {
        let d = UseDays::default().start(100);
        assert_eq!(
            d,
            UseDays {
                count: 1,
                date: 100
            }
        );
        assert_eq!(d.start(100), d, "a second start the same day");
        assert_eq!(d.start(130).count, 2, "a month away is one day");
        assert_eq!(UseDays::parse(&d.to_setting()), Some(d));
        assert_eq!(UseDays::parse("x"), None);
        // A count at its top, from a session line, stays at its top.
        let top = UseDays::parse("4294967295 1").expect("a count");
        assert_eq!(top.start(2).count, u32::MAX);
    }

    #[test]
    fn a_tab_is_stale_after_a_week_of_use() {
        assert!(!is_stale(10, 16));
        assert!(is_stale(10, 17));
        assert!(!is_stale(20, 17), "seen later than now: never");
        assert_eq!(parse_seen(&seen_setting(4, "a b.md")), Some((4, "a b.md")));
    }

    #[test]
    fn the_label_counts_the_tabs() {
        assert_eq!(
            label(1, "mdview"),
            "1 tab has not been opened in the last 7 days you used mdview."
        );
        assert!(label(3, "mdedit").starts_with("3 tabs have"));
    }
}
