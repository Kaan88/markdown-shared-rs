//! The named timers' arithmetic on fixed instants: the earliest wins, a name is moved rather
//! than added twice, a cancelled timer is gone, timers due together come one at a time (so a
//! callback can cancel or move a sibling), and an instant in the past is due at once.

use std::time::{Duration, Instant};

use super::NamedTimers;

#[test]
fn the_earliest_named_timer_is_the_one_the_loop_waits_for() {
    let t0 = Instant::now();
    let ms = Duration::from_millis;
    let mut t = NamedTimers::default();
    assert_eq!(t.next(), None);
    t.set("caret", t0 + ms(500));
    assert_eq!(t.next(), Some(t0 + ms(500)));
    t.set("autosave", t0 + ms(200));
    assert_eq!(t.next(), Some(t0 + ms(200)));
    t.set("toast", t0 + ms(50));
    assert_eq!(t.next(), Some(t0 + ms(50)));
}

#[test]
fn setting_a_name_again_moves_that_one_timer() {
    let t0 = Instant::now();
    let ms = Duration::from_millis;
    let mut t = NamedTimers::default();
    t.set("autosave", t0 + ms(200));
    t.set("caret", t0 + ms(900));
    // The same name at a later instant: one timer, moved, and the earliest is now the caret.
    t.set("autosave", t0 + ms(1500));
    assert_eq!(t.len(), 2);
    assert_eq!(t.at("autosave"), Some(t0 + ms(1500)));
    assert_eq!(t.next(), Some(t0 + ms(900)));
    // And moved back to be the earliest again.
    t.set("autosave", t0 + ms(100));
    assert_eq!(t.len(), 2);
    assert_eq!(t.next(), Some(t0 + ms(100)));
}

#[test]
fn a_cancelled_timer_is_gone_and_cancelling_it_again_answers_false() {
    let t0 = Instant::now();
    let ms = Duration::from_millis;
    let mut t = NamedTimers::default();
    t.set("caret", t0 + ms(100));
    t.set("autosave", t0 + ms(200));
    assert!(t.cancel("caret"));
    assert!(!t.cancel("caret"));
    assert!(!t.cancel("never set"));
    assert_eq!(t.at("caret"), None);
    assert_eq!(t.len(), 1);
    assert_eq!(t.next(), Some(t0 + ms(200)));
}

#[test]
fn two_timers_due_at_once_come_one_at_a_time_in_the_order_they_were_set() {
    let t0 = Instant::now();
    let now = t0 + Duration::from_millis(10);
    let mut t = NamedTimers::default();
    t.set("b", now);
    t.set("a", now);
    assert_eq!(t.take_earliest_due(now), Some("b".to_string()));
    assert_eq!(t.take_earliest_due(now), Some("a".to_string()));
    assert_eq!(t.take_earliest_due(now), None);
    assert_eq!(t.next(), None);
}

#[test]
fn a_timer_set_in_the_past_is_due_at_once_and_a_later_one_waits() {
    let t0 = Instant::now();
    let mut t = NamedTimers::default();
    t.set("late", t0 - Duration::from_secs(5));
    t.set("soon", t0 + Duration::from_secs(5));
    assert_eq!(t.next(), Some(t0 - Duration::from_secs(5)));
    assert_eq!(t.take_earliest_due(t0), Some("late".to_string()));
    assert_eq!(t.at("late"), None);
    assert_eq!(t.next(), Some(t0 + Duration::from_secs(5)));
    assert_eq!(t.take_earliest_due(t0), None);
}

#[test]
fn a_timer_that_is_not_yet_due_is_kept() {
    let t0 = Instant::now();
    let mut t = NamedTimers::default();
    t.set("later", t0 + Duration::from_millis(500));
    assert_eq!(t.take_earliest_due(t0), None);
    assert_eq!(t.next(), Some(t0 + Duration::from_millis(500)));
    assert_eq!(
        t.take_earliest_due(t0 + Duration::from_millis(500)),
        Some("later".to_string())
    );
}

#[test]
fn cancelling_a_sibling_from_a_callback_stops_it_from_firing() {
    let t0 = Instant::now();
    let now = t0 + Duration::from_millis(10);
    let mut t = NamedTimers::default();
    t.set("first", now);
    t.set("second", now);
    // The loop's shape: take one, call the application, look again.
    let mut fired = Vec::new();
    while let Some(name) = t.take_earliest_due(now) {
        fired.push(name.clone());
        if name == "first" {
            t.cancel("second");
        }
    }
    assert_eq!(fired, vec!["first".to_string()]);
}

#[test]
fn moving_a_sibling_from_a_callback_makes_it_fire_once_later() {
    let t0 = Instant::now();
    let now = t0 + Duration::from_millis(10);
    let later = now + Duration::from_millis(100);
    let mut t = NamedTimers::default();
    t.set("first", now);
    t.set("second", now);
    let mut fired = Vec::new();
    while let Some(name) = t.take_earliest_due(now) {
        fired.push(name.clone());
        if name == "first" {
            t.set("second", later);
        }
    }
    assert_eq!(fired, vec!["first".to_string()]);
    assert_eq!(t.next(), Some(later));
    assert_eq!(t.take_earliest_due(now), None);
    assert_eq!(t.take_earliest_due(later), Some("second".to_string()));
}
