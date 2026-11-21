//! Leaving out at start the tabs barely looked at, as both applications offer (off by default):
//! each tab counts the time it showed in front of a focused window ([`Clock`]), added up across
//! runs and kept in the session as `focus=<seconds> <path>` lines ([`to_setting`], [`parse`]).
//! A tab counted for less than the setting's seconds is not reopened ([`left_out`]). Which tabs
//! always come back (the one that was showing, a pinned one, one with unsaved text) is each
//! application's to say; a tab with no count (from before the count) always comes back.

use std::time::Instant;

/// The settings: the switch's and the seconds' labels and help, and the seconds' range.
pub const SWITCH: &str = "Don't reopen tabs barely looked at";
pub const SECONDS: &str = "Looked at for less than";
pub const SECONDS_HELP: &str = "Only while the tab is in front and the window has focus.";
pub const SECS_DEFAULT: u32 = 10;
pub const SECS_MIN: u32 = 1;
pub const SECS_MAX: u32 = 600;

/// The clock of focused time: which tab it runs for, and since when.
#[derive(Clone, Copy, Debug)]
pub struct Clock<Id> {
    running: Option<(Id, Instant)>,
}

impl<Id> Default for Clock<Id> {
    fn default() -> Self {
        Clock { running: None }
    }
}

impl<Id: Clone + PartialEq> Clock<Id> {
    /// Called on every turn of the loop: `showing` is the tab in front while the window has the
    /// focus (`None` while it has not, or is minimized). When it changes, the tab counted until
    /// now gets its time through `add(id, ms)` and the clock runs for `showing`.
    pub fn tick(&mut self, showing: Option<Id>, now: Instant, add: impl FnMut(Id, u64)) {
        if self.running.as_ref().map(|(id, _)| id) == showing.as_ref() {
            return;
        }
        self.flush(now, add);
        self.running = showing.map(|id| (id, now));
    }

    /// Gives the running tab the time counted so far, the clock still running (before a session
    /// write).
    pub fn flush(&mut self, now: Instant, mut add: impl FnMut(Id, u64)) {
        if let Some((id, since)) = self.running.take() {
            add(
                id.clone(),
                now.saturating_duration_since(since).as_millis() as u64,
            );
            self.running = Some((id, now));
        }
    }
}

/// Whether a tab is left out at start: the setting is `on`, the tab may be left out (`keep` is
/// false: not the one showing, pinned or unsaved), and it has a count under `secs`.
pub fn left_out(on: bool, secs: u32, counted: Option<u64>, keep: bool) -> bool {
    on && !keep && counted.is_some_and(|s| s < secs as u64)
}

/// The value of a `focus` line: `<seconds> <path>`, the path as [`crate::settings::encode_path`]
/// writes it.
pub fn to_setting(secs: u64, path: &std::path::Path) -> String {
    format!("{secs} {}", crate::settings::encode_path(path))
}

/// Reads a `focus` line's value; `None` when it is not `<seconds> <path>`.
pub fn parse(value: &str) -> Option<(std::path::PathBuf, u64)> {
    let (secs, path) = value.split_once(' ')?;
    let secs = secs.parse().ok()?;
    (!path.is_empty()).then(|| (crate::settings::decode_path(path), secs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn only_the_tab_in_front_of_a_focused_window_is_counted() {
        let mut counted = [0u64; 2];
        let mut c = Clock::default();
        let t0 = Instant::now();
        let at = |s: u64| t0 + Duration::from_secs(s);
        let mut tick = |c: &mut Clock<usize>, id, s| c.tick(id, at(s), |i, ms| counted[i] += ms);
        tick(&mut c, Some(0), 0);
        tick(&mut c, Some(0), 2);
        tick(&mut c, None, 3);
        tick(&mut c, None, 60);
        tick(&mut c, Some(1), 60);
        c.flush(at(65), |i, ms| counted[i] += ms);
        c.tick(Some(0), at(70), |i, ms| counted[i] += ms);
        assert_eq!(counted, [3000, 10_000]);
    }

    #[test]
    fn a_tab_is_left_out_only_when_on_counted_and_short() {
        assert!(!left_out(false, 10, Some(3), false));
        assert!(left_out(true, 10, Some(3), false));
        assert!(!left_out(true, 10, Some(3), true));
        assert!(!left_out(true, 10, None, false));
        assert!(!left_out(true, 10, Some(10), false));
    }

    #[test]
    fn a_focus_line_round_trips() {
        let p = std::path::Path::new("C:/a b/c.md");
        assert_eq!(parse(&to_setting(42, p)), Some((p.into(), 42)));
        assert_eq!(parse("x C:/a"), None);
        assert_eq!(parse("5 "), None);
    }
}
