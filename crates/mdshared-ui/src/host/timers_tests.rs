//! The named timers' arithmetic on fixed instants: the earliest wins, a name is moved rather
//! than added twice, a cancelled timer is gone, a turn snapshots its due names in firing order,
//! and an instant in the past is due at once.

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
fn a_turn_snapshots_its_due_names_in_firing_order_and_takes_each_once() {
    let t0 = Instant::now();
    let now = t0 + Duration::from_millis(10);
    let mut t = NamedTimers::default();
    t.set("b", now);
    t.set("a", now);
    t.set("later", now + Duration::from_millis(5));
    assert_eq!(t.due_at(now), vec!["b".to_string(), "a".to_string()]);
    assert!(t.take_if_due("b", now));
    assert!(t.take_if_due("a", now));
    assert!(!t.take_if_due("a", now));
    assert!(!t.take_if_due("later", now), "not due yet");
    assert_eq!(t.next(), Some(now + Duration::from_millis(5)));
}

#[test]
fn a_timer_set_in_the_past_is_due_at_once_and_a_later_one_waits() {
    let t0 = Instant::now();
    let mut t = NamedTimers::default();
    t.set("late", t0 - Duration::from_secs(5));
    t.set("soon", t0 + Duration::from_secs(5));
    assert_eq!(t.next(), Some(t0 - Duration::from_secs(5)));
    assert_eq!(t.due_at(t0), vec!["late".to_string()]);
    assert!(t.take_if_due("late", t0));
    assert_eq!(t.at("late"), None);
    assert_eq!(t.next(), Some(t0 + Duration::from_secs(5)));
    assert!(t.due_at(t0).is_empty());
}

#[test]
fn a_timer_moved_later_mid_turn_is_not_taken_by_that_turn() {
    let t0 = Instant::now();
    let now = t0 + Duration::from_millis(10);
    let later = now + Duration::from_millis(100);
    let mut t = NamedTimers::default();
    t.set("a", now);
    t.set("b", now);
    assert_eq!(t.due_at(now), vec!["a".to_string(), "b".to_string()]);
    t.set("b", later);
    assert!(t.take_if_due("a", now));
    assert!(!t.take_if_due("b", now), "moved later since the snapshot");
    assert_eq!(t.next(), Some(later));
}

#[test]
fn a_timer_that_is_not_yet_due_is_kept() {
    let t0 = Instant::now();
    let mut t = NamedTimers::default();
    t.set("later", t0 + Duration::from_millis(500));
    assert!(t.due_at(t0).is_empty());
    assert!(!t.take_if_due("later", t0));
    assert_eq!(t.next(), Some(t0 + Duration::from_millis(500)));
    assert_eq!(
        t.due_at(t0 + Duration::from_millis(500)),
        vec!["later".to_string()]
    );
    assert!(t.take_if_due("later", t0 + Duration::from_millis(500)));
}
