use super::*;

/// A monitor whose work area is its whole rectangle.
fn screen(name: &str, at: (i32, i32), size: (u32, u32), scale: f32) -> Monitor {
    Monitor {
        name: Some(name.to_owned()),
        position: at,
        size,
        scale,
        work: Rect::new(0.0, 0.0, size.0 as f32 / scale, size.1 as f32 / scale),
    }
}

/// A state saved by a window on `monitor`, not maximized.
fn saved(rect: Rect, monitor: &Monitor) -> WindowState {
    WindowState {
        rect,
        maximized: false,
        scale: monitor.scale,
        monitor: monitor.id(),
    }
}

/// Whether `rect` lies wholly inside `work`, in physical screen px.
fn inside(rect: ScreenRect, work: ScreenRect) -> bool {
    i64::from(rect.x) >= i64::from(work.x)
        && i64::from(rect.y) >= i64::from(work.y)
        && i64::from(rect.x) + i64::from(rect.w) <= i64::from(work.x) + i64::from(work.w)
        && i64::from(rect.y) + i64::from(rect.h) <= i64::from(work.y) + i64::from(work.h)
}

fn physical_work(monitor: &Monitor) -> ScreenRect {
    ScreenRect {
        x: monitor.position.0 + (monitor.work.x * monitor.scale).round() as i32,
        y: monitor.position.1 + (monitor.work.y * monitor.scale).round() as i32,
        w: (monitor.work.w * monitor.scale).round() as u32,
        h: (monitor.work.h * monitor.scale).round() as u32,
    }
}

const GOOD: &str = "x=100.00;y=50.00;w=1200.00;h=800.00;max=0;scale=1.0000;\
                    mon=;monx=0;mony=0;monw=3840;monh=2160";

#[test]
fn a_state_round_trips_through_its_settings_value() {
    let state = WindowState {
        rect: Rect::new(100.0, 50.0, 1200.0, 800.0),
        maximized: true,
        scale: 1.25,
        monitor: MonitorId::new(Some("DELL U2720Q".to_owned()), (1920, -160), (3840, 2160)),
    };
    let line = state.to_setting();
    assert_eq!(
        line,
        "x=100.00;y=50.00;w=1200.00;h=800.00;max=1;scale=1.2500;mon=DELL U2720Q;\
         monx=1920;mony=-160;monw=3840;monh=2160"
    );
    assert_eq!(WindowState::from_setting(&line), Some(state));
    // The same value written again is the same text, byte for byte.
    let back = WindowState::from_setting(&line).unwrap();
    assert_eq!(back.to_setting(), line);
}

#[test]
fn a_monitor_with_no_name_is_written_as_an_empty_field_and_read_back_as_none() {
    let mut state = saved(
        Rect::new(10.0, 20.0, 300.0, 200.0),
        &screen("Panel", (0, 0), (1920, 1080), 1.0),
    );
    state.monitor.name = None;
    let line = state.to_setting();
    assert!(line.contains("mon=;monx="), "{line}");
    assert_eq!(WindowState::from_setting(&line), Some(state));
}

#[test]
fn a_name_with_the_field_separators_and_line_breaks_round_trips() {
    let mut state = saved(
        Rect::new(10.0, 20.0, 300.0, 200.0),
        &screen("Panel", (0, 0), (1920, 1080), 1.0),
    );
    state.monitor.name = Some("a;b=c%25\nsecond line".to_owned());
    let line = state.to_setting();
    assert!(!line.contains('\n') && !line.contains('\r'));
    assert!(
        line.contains("mon=a%3Bb%3Dc%2525%0Asecond line;monx="),
        "{line}"
    );
    let back = WindowState::from_setting(&line).unwrap();
    assert_eq!(back.monitor.name, state.monitor.name);
    assert_eq!(back.to_setting(), line);
}

#[test]
fn a_damaged_or_partial_value_is_none() {
    let damaged = [
        String::new(),
        "   ".to_owned(),
        "junk".to_owned(),
        "ratio:0.3".to_owned(),
        "x=100;y=50".to_owned(),
        // A field with no value is not a field.
        GOOD.replace(";monh=2160", ";monh"),
        // A field with no '=' is not a field.
        format!("{GOOD};oops"),
        // Nonsense sizes.
        GOOD.replace("w=1200.00", "w=0.00"),
        GOOD.replace("w=1200.00", "w=-1.00"),
        GOOD.replace("w=1200.00", "w=NaN"),
        GOOD.replace("h=800.00", "h=inf"),
        GOOD.replace("w=1200.00", "w=200000.00"),
        // Nonsense positions.
        GOOD.replace("x=100.00", "x=NaN"),
        GOOD.replace("x=100.00", "x=2000000.00"),
        // Nonsense scales.
        GOOD.replace("scale=1.0000", "scale=0.0000"),
        GOOD.replace("scale=1.0000", "scale=17.0000"),
        GOOD.replace("scale=1.0000", "scale=NaN"),
        // A flag and monitor fields that are not the ones written.
        GOOD.replace("max=0", "max=2"),
        GOOD.replace("max=0", "max=true"),
        GOOD.replace("monw=3840", "monw=0"),
        GOOD.replace("monw=3840", "monw=-1"),
        GOOD.replace("monx=0", "monx=abc"),
        GOOD.replace("mony=0", "mony=1.5"),
        // A name longer than any monitor's.
        GOOD.replace("mon=;", &format!("mon={};", "x".repeat(600))),
        // A whole value too long to be a settings value.
        format!("{GOOD};pad={}", "x".repeat(MAX_LINE)),
    ];
    for line in damaged {
        assert_eq!(WindowState::from_setting(&line), None, "{line}");
    }
}

#[test]
fn a_field_that_appears_twice_keeps_the_last_one_and_an_unknown_field_is_ignored() {
    let line = format!("{};later=42;", GOOD.replace("x=100.00", "x=5.00;x=100.00"));
    let state = WindowState::from_setting(&line).unwrap();
    assert_eq!(state.rect.x, 100.0);
    assert_eq!(state.monitor.size, (3840, 2160));
}

#[test]
fn a_window_is_restored_on_the_monitor_it_was_saved_on() {
    let primary = screen("Primary", (0, 0), (1920, 1080), 1.0);
    let second = screen("Second", (1920, 0), (2560, 1440), 1.0);
    let state = saved(Rect::new(100.0, 80.0, 800.0, 600.0), &second);
    let placement = restore(&state, &[primary, second]).unwrap();
    assert_eq!(placement.monitor, 1);
    assert_eq!(
        placement.rect,
        ScreenRect {
            x: 2020,
            y: 80,
            w: 800,
            h: 600
        }
    );
    assert!(!placement.maximized);
}

#[test]
fn a_window_whose_only_other_monitor_is_gone_goes_to_the_one_left() {
    let primary = screen("Primary", (0, 0), (1920, 1080), 1.0);
    let gone = screen("Gone", (1920, 0), (2560, 1440), 1.0);
    let state = saved(Rect::new(100.0, 80.0, 800.0, 600.0), &gone);
    let placement = restore(&state, std::slice::from_ref(&primary)).unwrap();
    assert_eq!(placement.monitor, 0);
    assert_eq!(
        placement.rect,
        ScreenRect {
            x: 100,
            y: 80,
            w: 800,
            h: 600
        }
    );
}

#[test]
fn a_monitor_with_the_same_name_at_another_place_is_not_the_saved_monitor() {
    let primary = screen("Primary", (1920, 0), (1920, 1080), 1.0);
    let moved = screen("Second", (0, 0), (2560, 1440), 1.0);
    let saved_on = screen("Second", (1920, 0), (2560, 1440), 1.0);
    let state = saved(Rect::new(100.0, 80.0, 800.0, 600.0), &saved_on);
    let placement = restore(&state, &[primary, moved]).unwrap();
    assert_eq!(placement.monitor, 0);
    assert_eq!(placement.rect.x, 2020);
}

#[test]
fn a_monitor_whose_size_changed_is_not_the_saved_one_but_the_nearest() {
    // Its resolution changed: not the same monitor, but where the window was, so it stays.
    let primary = screen("Primary", (0, 0), (1920, 1080), 1.0);
    let other = screen("Second", (1920, 0), (1920, 1080), 1.0);
    let saved_on = screen("Second", (1920, 0), (2560, 1440), 1.0);
    let state = saved(Rect::new(100.0, 80.0, 800.0, 600.0), &saved_on);
    let placement = restore(&state, &[primary, other]).unwrap();
    assert_eq!(placement.monitor, 1);
    assert_eq!(placement.rect.x, 2020);
}

#[test]
fn a_window_whose_monitor_is_gone_goes_to_the_nearest_of_the_rest() {
    let left = screen("Left", (-1920, 0), (1920, 1080), 1.0);
    let primary = screen("Primary", (0, 0), (1920, 1080), 1.0);
    let right = screen("Right", (1920, 0), (1920, 1080), 1.0);
    let gone = screen("Gone", (3840, 0), (1920, 1080), 1.0);
    let state = saved(Rect::new(100.0, 80.0, 800.0, 600.0), &gone);
    let placement = restore(&state, &[primary, left, right]).unwrap();
    assert_eq!(placement.monitor, 2);
    assert_eq!(placement.rect.x, 2020);
}

#[test]
fn a_window_is_rescaled_to_the_scale_of_the_monitor_it_lands_on() {
    // The same physical panel, saved at 1x and read back when it runs at 2x: the logical
    // rectangle stays the same, so the physical one doubles.
    let before = screen("Panel", (0, 0), (3840, 2160), 1.0);
    let after = screen("Panel", (0, 0), (3840, 2160), 2.0);
    let state = saved(Rect::new(100.0, 80.0, 800.0, 600.0), &before);
    let placement = restore(&state, &[after]).unwrap();
    assert_eq!(placement.monitor, 0);
    assert_eq!(
        placement.rect,
        ScreenRect {
            x: 200,
            y: 160,
            w: 1600,
            h: 1200
        }
    );
    // Saved at 2x and read back at 1x: the physical rectangle is the logical one.
    let before = screen("Panel", (0, 0), (3840, 2160), 2.0);
    let after = screen("Panel", (0, 0), (3840, 2160), 1.0);
    let state = saved(Rect::new(100.0, 80.0, 800.0, 600.0), &before);
    let placement = restore(&state, &[after]).unwrap();
    assert_eq!(
        placement.rect,
        ScreenRect {
            x: 100,
            y: 80,
            w: 800,
            h: 600
        }
    );
}

#[test]
fn a_window_larger_than_the_work_area_is_shrunk_to_fit() {
    let mut monitor = screen("Small", (0, 0), (1920, 1080), 1.0);
    monitor.work = Rect::new(0.0, 0.0, 1000.0, 600.0);
    let state = saved(Rect::new(100.0, 100.0, 1600.0, 900.0), &monitor);
    let placement = restore(&state, std::slice::from_ref(&monitor)).unwrap();
    assert_eq!(
        placement.rect,
        ScreenRect {
            x: 0,
            y: 0,
            w: 1000,
            h: 600
        }
    );
}

#[test]
fn a_window_partly_off_screen_is_moved_fully_inside() {
    let monitor = screen("Panel", (0, 0), (1920, 1080), 1.0);
    let state = saved(Rect::new(-200.0, 900.0, 800.0, 600.0), &monitor);
    let placement = restore(&state, std::slice::from_ref(&monitor)).unwrap();
    assert_eq!(
        placement.rect,
        ScreenRect {
            x: 0,
            y: 480,
            w: 800,
            h: 600
        }
    );
    assert!(inside(placement.rect, physical_work(&monitor)));
}

#[test]
fn a_monitor_left_of_the_primary_keeps_its_negative_position() {
    let left = screen("Left", (-1920, 0), (1920, 1080), 1.0);
    let state = saved(Rect::new(100.0, 80.0, 800.0, 600.0), &left);
    let placement = restore(&state, &[left]).unwrap();
    assert_eq!(
        placement.rect,
        ScreenRect {
            x: -1820,
            y: 80,
            w: 800,
            h: 600
        }
    );
}

#[test]
fn a_work_area_away_from_the_monitors_corner_is_respected() {
    // A panel along the top of the monitor.
    let mut monitor = screen("Panel", (0, 0), (1920, 1080), 1.0);
    monitor.work = Rect::new(0.0, 40.0, 1920.0, 1000.0);
    let top = saved(Rect::new(100.0, 0.0, 800.0, 600.0), &monitor);
    let bottom = saved(Rect::new(100.0, 900.0, 800.0, 600.0), &monitor);
    let first = restore(&top, std::slice::from_ref(&monitor)).unwrap();
    let second = restore(&bottom, std::slice::from_ref(&monitor)).unwrap();
    assert_eq!(first.rect.y, 40);
    assert_eq!(second.rect.y, 440);
    assert!(inside(first.rect, physical_work(&monitor)));
    assert!(inside(second.rect, physical_work(&monitor)));
}

#[test]
fn no_monitors_is_no_placement() {
    let monitor = screen("Panel", (0, 0), (1920, 1080), 1.0);
    let state = saved(Rect::new(100.0, 80.0, 800.0, 600.0), &monitor);
    assert_eq!(restore(&state, &[]), None);
}

#[test]
fn a_maximized_window_is_placed_maximized() {
    let monitor = screen("Panel", (0, 0), (1920, 1080), 1.0);
    let mut state = saved(Rect::new(100.0, 80.0, 800.0, 600.0), &monitor);
    state.maximized = true;
    let placement = restore(&state, &[monitor]).unwrap();
    assert!(placement.maximized);
}

#[test]
fn nonsense_values_still_give_a_real_rectangle() {
    let mut monitor = screen("Panel", (0, 0), (1920, 1080), 1.0);
    monitor.work = Rect::new(f32::NAN, f32::INFINITY, -4.0, 0.0);
    let mut state = saved(
        Rect::new(f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.0),
        &monitor,
    );
    state.scale = f32::NAN;
    let placement = restore(&state, std::slice::from_ref(&monitor)).unwrap();
    assert!(placement.rect.w >= 1 && placement.rect.h >= 1);
    // The work area that was given is nonsense, so the monitor's own rectangle is used.
    assert!(inside(
        placement.rect,
        ScreenRect {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080
        }
    ));

    // A monitor with no size at all and a scale that is not one is still answered, with the
    // smallest rectangle there is.
    let bad = Monitor {
        name: None,
        position: (i32::MAX, i32::MIN),
        size: (0, 0),
        scale: f32::NAN,
        work: Rect::new(f32::NAN, 0.0, 0.0, 0.0),
    };
    let state = saved(Rect::new(10.0, 10.0, 800.0, 600.0), &bad);
    let placement = restore(&state, &[bad]).unwrap();
    assert_eq!((placement.rect.w, placement.rect.h), (1, 1));
}
