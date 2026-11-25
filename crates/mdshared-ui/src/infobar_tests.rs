use super::*;
use crate::gfx::Gfx;
use crate::theme::{DARK, LIGHT};
use mdshared_text::Bitmap;

const ALL: [Severity; 4] = [
    Severity::Info,
    Severity::Success,
    Severity::Warning,
    Severity::Error,
];

fn sample() -> InfoBar {
    InfoBar::new(
        Severity::Info,
        "Update available",
        "A new version is ready. It can install when the application closes.",
    )
}

/// A painter over a small scratch bitmap, for the measuring the model does.
fn scratch() -> (Bitmap, Gfx) {
    (Bitmap::new(8, 8), Gfx::new())
}

fn at(bmp: &Bitmap, x: f32, y: f32) -> Rgb {
    let o = (y as usize * bmp.width as usize + x as usize) * 4;
    [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
}

fn count_in(bmp: &Bitmap, r: Rect, c: Rgb, tol: i32) -> usize {
    let (x0, x1) = (r.x.max(0.0) as usize, r.right().max(0.0) as usize);
    let (y0, y1) = (r.y.max(0.0) as usize, r.bottom().max(0.0) as usize);
    let mut n = 0;
    for y in y0..y1.min(bmp.height as usize) {
        for x in x0..x1.min(bmp.width as usize) {
            let p = at(bmp, x as f32, y as f32);
            if p.iter()
                .zip(c)
                .all(|(a, b)| (*a as i32 - b as i32).abs() <= tol)
            {
                n += 1;
            }
        }
    }
    n
}

fn shot(pal: &Palette, w: u32, h: u32, draw: impl FnOnce(&mut Painter<'_>)) -> Bitmap {
    let mut bmp = Bitmap::new(w, h);
    let mut gfx = Gfx::new();
    {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        p.fill(Rect::new(0.0, 0.0, w as f32, h as f32), pal.page);
        draw(&mut p);
    }
    bmp
}

#[test]
fn a_long_message_makes_the_bar_taller_and_a_narrow_one_taller_still() {
    let (mut bmp, mut gfx) = scratch();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    let short = InfoBar::new(Severity::Info, "Title", "One line.");
    let long = InfoBar::new(
        Severity::Info,
        "Title",
        "This message is long enough that it needs more than one line in the same column, \
         and another line after that, so the bar has to grow to hold it.",
    );
    let (short_h, long_h) = (short.height(&mut p, 420.0), long.height(&mut p, 420.0));
    assert!(long_h > short_h, "{long_h} is not taller than {short_h}");
    assert!(
        long.height(&mut p, 180.0) > long_h,
        "a narrower column should wrap the same message into more lines"
    );
    // Both heights include the padding at each side exactly once.
    assert!(short_h >= 2.0 * PAD);
}

#[test]
fn hit_finds_both_actions_the_close_button_and_nothing_else() {
    let (mut bmp, mut gfx) = scratch();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    let bar = sample().action(7, "Install").action(9, "Details");
    let rect = Rect::new(0.0, 0.0, 460.0, bar.height(&mut p, 460.0));
    let l = bar.layout(&mut p, rect);
    assert_eq!(l.actions.len(), 2);
    for &(id, r) in &l.actions {
        assert_eq!(
            bar.hit(&mut p, rect, r.cx(), r.cy()),
            Some(Hit::Action(id)),
            "the centre of action {id}"
        );
    }
    let close = l.close.expect("a closable bar has a close button");
    assert_eq!(
        bar.hit(&mut p, rect, close.cx(), close.cy()),
        Some(Hit::Close)
    );
    // A point on the message, and one outside the bar, belong to nothing.
    assert_eq!(
        bar.hit(&mut p, rect, l.message.x + 1.0, l.message.y + 1.0),
        None
    );
    assert_eq!(bar.hit(&mut p, rect, -20.0, -20.0), None);
    assert_eq!(bar.hit(&mut p, rect, rect.right() + 20.0, rect.cy()), None);
    assert_eq!(bar.hit(&mut p, rect, rect.cx(), rect.bottom() + 5.0), None);
}

#[test]
fn a_bar_that_cannot_be_closed_has_no_close_button_or_hit() {
    let (mut bmp, mut gfx) = scratch();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    let bar = sample().closable(false);
    let rect = Rect::new(0.0, 0.0, 460.0, bar.height(&mut p, 460.0));
    let l = bar.layout(&mut p, rect);
    assert!(l.close.is_none());
    let corner = (
        rect.right() - PAD - CLOSE_SIZE / 2.0,
        rect.y + PAD + CLOSE_SIZE / 2.0,
    );
    assert_eq!(bar.hit(&mut p, rect, corner.0, corner.1), None);
    assert!(!bar.is_closable());
}

#[test]
fn a_third_action_is_not_taken() {
    let bar = sample()
        .action(1, "One")
        .action(2, "Two")
        .action(3, "Three");
    assert_eq!(bar.actions().len(), MAX_ACTIONS);
    assert_eq!(
        bar.actions().iter().map(|a| a.id).collect::<Vec<_>>(),
        [1, 2]
    );
}

#[test]
fn the_stack_places_bars_inside_the_area_without_overlapping() {
    let (mut bmp, mut gfx) = scratch();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    let bars = vec![
        InfoBar::new(Severity::Info, "One", "The first bar."),
        InfoBar::new(Severity::Success, "Two", "The second, with an action.").action(4, "Undo"),
        InfoBar::new(Severity::Warning, "Three", "The third."),
        InfoBar::new(Severity::Error, "Four", "The fourth."),
    ];
    let area = Rect::new(40.0, 30.0, 500.0, 600.0);
    let rects = stack_layout(&mut p, area, &bars);
    assert_eq!(rects.len(), bars.len());
    for (bar, r) in bars.iter().zip(&rects) {
        assert_eq!(r.h, bar.height(&mut p, area.w));
        assert_eq!(r.x, area.x);
        assert_eq!(r.w, area.w);
        assert!(r.y >= area.y && r.bottom() <= area.bottom());
    }
    for pair in rects.windows(2) {
        assert!(
            pair[0].bottom() + STACK_GAP <= pair[1].y + 1e-3,
            "{:?} and {:?} are too close",
            pair[0],
            pair[1]
        );
        assert!(pair[0].bottom() <= pair[1].y, "the bars overlap");
    }
}

#[test]
fn a_stack_that_is_too_short_keeps_every_rectangle_inside_it() {
    let (mut bmp, mut gfx) = scratch();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    let bars = vec![
        InfoBar::new(Severity::Info, "One", "The first bar."),
        InfoBar::new(Severity::Info, "Two", "The second bar."),
        InfoBar::new(Severity::Info, "Three", "The third bar."),
        InfoBar::new(Severity::Info, "Four", "The fourth bar."),
    ];
    let area = Rect::new(10.0, 20.0, 300.0, 90.0);
    let rects = stack_layout(&mut p, area, &bars);
    assert_eq!(rects.len(), bars.len());
    for pair in rects.windows(2) {
        assert!(pair[0].bottom() <= pair[1].y + 1e-3);
    }
    for r in &rects {
        assert!(r.y >= area.y && r.bottom() <= area.bottom() + 1e-3);
    }
    // The last bars have no room at all rather than crossing the bottom.
    assert!(rects.iter().any(|r| r.h == 0.0));
}

#[test]
fn every_severity_draws_its_glyph_in_its_own_colour() {
    for pal in [&LIGHT, &DARK] {
        let mut inks = Vec::new();
        for severity in ALL {
            let ink = severity_colors(severity, pal).ink;
            inks.push(ink);
            let bmp = shot(pal, 420, 120, |p| {
                let bar = InfoBar::new(severity, "Title", "A message about the news.");
                let rect = Rect::new(0.0, 0.0, 420.0, bar.height(p, 420.0));
                draw_infobar(p, pal, rect, &bar, None);
            });
            let glyph = Rect::new(PAD, PAD, ICON, ICON + TITLE_H);
            assert!(
                count_in(&bmp, glyph.inset(-2.0), ink, 8) > 0,
                "{severity:?} put no {ink:?} ink in its glyph"
            );
        }
        for (i, a) in inks.iter().enumerate() {
            for b in &inks[i + 1..] {
                assert_ne!(a, b, "two severities share an ink");
            }
        }
    }
}

/// The device pixels of `r` (logical, at `scale`) as BGRA bytes, row after row.
fn pixels_in(bmp: &Bitmap, r: Rect, scale: f32) -> Vec<u8> {
    let (x0, x1) = ((r.x * scale) as usize, (r.right() * scale) as usize);
    let (y0, y1) = ((r.y * scale) as usize, (r.bottom() * scale) as usize);
    let mut out = Vec::new();
    for y in y0..y1.min(bmp.height as usize) {
        let o = y * bmp.width as usize * 4;
        out.extend_from_slice(&bmp.pixels[o + x0 * 4..o + x1.min(bmp.width as usize) * 4]);
    }
    out
}

/// Every device pixel where `a` and `b` differ.
fn differences(a: &Bitmap, b: &Bitmap) -> Vec<(usize, usize)> {
    let w = a.width as usize;
    a.pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.pixels.as_chunks::<4>().0)
        .enumerate()
        .filter(|(_, (p, q))| p != q)
        .map(|(i, _)| (i % w, i / w))
        .collect()
}

#[test]
fn the_close_cross_is_drawn_only_when_the_bar_is_closable() {
    // The cross is checked against the same card with no text at all, so no antialiased letter
    // can stand in for it or hide it.
    for pal in [&LIGHT, &DARK] {
        let (mut bmp, mut gfx) = scratch();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        let rect = Rect::new(0.0, 0.0, 420.0, sample().height(&mut p, 420.0));
        let close = sample().parts(&mut p, rect).close.expect("closable");
        let draw = |bar: InfoBar| {
            shot(pal, 420, 120, move |p| {
                draw_infobar(p, pal, rect, &bar, None)
            })
        };
        let with_text = draw(sample());
        let bare_open = draw(InfoBar::new(Severity::Info, "", ""));
        let bare_shut = draw(InfoBar::new(Severity::Info, "", "").closable(false));
        let fill = severity_colors(Severity::Info, pal).fill;
        let area = close.w as usize * close.h as usize;
        assert_eq!(
            count_in(&bare_shut, close, fill, 0),
            area,
            "a bar that cannot be closed drew something where the cross goes"
        );
        assert!(
            count_in(&bare_open, close, fill, 0) < area,
            "the close cross is missing from the closable bar"
        );
        assert!(
            pixels_in(&with_text, close, 1.0) == pixels_in(&bare_open, close, 1.0),
            "the bar's text reaches into the close button"
        );
    }
}

#[test]
fn the_parts_are_the_rectangles_hit_answers_for() {
    let (mut bmp, mut gfx) = scratch();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    let bars = [
        sample().action(7, "Install").action(9, "Details"),
        sample().action(3, "Undo").closable(false),
        InfoBar::new(Severity::Error, "", "").action(1, "OK"),
        InfoBar::new(Severity::Warning, "Short", "")
            .action(4, "Retry")
            .action(5, "Cancel"),
    ];
    for bar in &bars {
        // Its own height, and squeezed so small that the controls crowd each other.
        for (w, squeeze) in [(460.0, false), (300.0, false), (60.0, true)] {
            let h = if squeeze { 30.0 } else { bar.height(&mut p, w) };
            let rect = Rect::new(20.0, 10.0, w, h);
            let parts = bar.parts(&mut p, rect);
            assert_eq!(parts.actions.len(), bar.actions().len());
            assert_eq!(parts.close.is_some(), bar.is_closable());
            for (&(id, _), action) in parts.actions.iter().zip(bar.actions()) {
                assert_eq!(id, action.id, "the parts are not in the drawing order");
            }
            if !squeeze {
                for &(id, r) in &parts.actions {
                    assert_eq!(bar.hit(&mut p, rect, r.cx(), r.cy()), Some(Hit::Action(id)));
                }
                if let Some(c) = parts.close {
                    assert_eq!(bar.hit(&mut p, rect, c.cx(), c.cy()), Some(Hit::Close));
                }
            }
            // A second, plain way to answer every point: the close square first, then the
            // first action that holds it, and only inside the bar.
            let mut y = rect.y - 4.0;
            while y < rect.bottom() + 4.0 {
                let mut x = rect.x - 4.0;
                while x < rect.right() + 4.0 {
                    let expected = if !rect.contains(x, y) {
                        None
                    } else if parts.close.is_some_and(|c| c.contains(x, y)) {
                        Some(Hit::Close)
                    } else {
                        parts
                            .actions
                            .iter()
                            .find(|(_, r)| r.contains(x, y))
                            .map(|&(id, _)| Hit::Action(id))
                    };
                    assert_eq!(bar.hit(&mut p, rect, x, y), expected, "at ({x}, {y})");
                    x += 1.5;
                }
                y += 1.5;
            }
        }
    }
}

#[test]
fn an_action_under_a_short_text_stays_below_the_close_button() {
    let (mut bmp, mut gfx) = scratch();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    for (title, message) in [("", ""), ("T", ""), ("", "Hi."), ("Title", "Short.")] {
        let bar = InfoBar::new(Severity::Info, title, message).action(1, "OK");
        let rect = Rect::new(0.0, 0.0, 300.0, bar.height(&mut p, 300.0));
        let parts = bar.parts(&mut p, rect);
        let close = parts.close.expect("closable");
        let (_, ok) = parts.actions[0];
        assert!(
            ok.y >= rect.y + PAD + CLOSE_SIZE + GAP,
            "{title:?}/{message:?}: the action's top {} is beside the close button",
            ok.y
        );
        assert!(ok.y >= close.bottom(), "{ok:?} overlaps {close:?}");
        assert!(
            ok.bottom() + PAD <= rect.bottom() + 1e-3,
            "the card is too short for {ok:?}"
        );
    }
    // The point the two once shared is the close button's alone.
    let bar = InfoBar::new(Severity::Info, "", "").action(1, "OK");
    let rect = Rect::new(0.0, 0.0, 300.0, bar.height(&mut p, 300.0));
    assert_eq!(bar.hit(&mut p, rect, 276.0, 28.0), Some(Hit::Close));
    let (_, ok) = bar.parts(&mut p, rect).actions[0];
    assert!(!ok.contains(276.0, 28.0));
}

/// `draw_infobar` as it was before `draw_infobar_with` existed, kept to prove it still draws
/// the same pixels.
fn old_draw_infobar(p: &mut Painter, pal: &Palette, rect: Rect, bar: &InfoBar, hover: Option<Hit>) {
    let colors = severity_colors(bar.severity, pal);
    let l = bar.layout(p, rect);
    let clip = p.clip(rect);
    p.frame(rect, RADIUS, colors.fill, colors.line);
    p.icon(
        bar.severity.glyph(),
        ICON,
        l.icon.cx(),
        l.icon.cy(),
        colors.ink,
    );
    if !bar.title.is_empty() {
        p.text(
            Face::Semibold,
            TITLE_TEXT,
            l.title.x,
            l.title.cy(),
            &bar.title,
            pal.text,
            l.title.w,
        );
    }
    if !bar.message.is_empty() {
        p.paragraph(
            Face::Ui,
            MESSAGE_TEXT,
            l.message.x,
            l.message.y,
            l.message.w,
            &bar.message,
            pal.text,
        );
    }
    for (k, (action, (_, button))) in bar.actions.iter().zip(&l.actions).enumerate() {
        bar_button(
            p,
            pal,
            button.x,
            button.cy(),
            &action.label,
            k == 0,
            hover == Some(Hit::Action(action.id)),
            false,
        );
    }
    if let Some(close) = l.close {
        if hover == Some(Hit::Close) {
            p.rbox(close, 5.0, mix(colors.fill, pal.text, 0.10));
        }
        p.icon(glyph::CLOSE, 10.0, close.cx(), close.cy(), pal.muted);
    }
    p.restore(clip);
}

#[test]
fn draw_infobar_draws_what_it_drew_before_the_look_was_added() {
    let bars = [
        sample().action(7, "Install").action(9, "Details"),
        sample().action(7, "Install").closable(false),
        InfoBar::new(Severity::Error, "", ""),
    ];
    let hovers = [
        None,
        Some(Hit::Action(7)),
        Some(Hit::Action(9)),
        Some(Hit::Action(99)),
        Some(Hit::Close),
    ];
    for pal in [&LIGHT, &DARK, &HIGH_CONTRAST_LIGHT, &HIGH_CONTRAST_DARK] {
        for bar in &bars {
            for hover in hovers {
                let draw = |f: &dyn Fn(&mut Painter, Rect)| {
                    shot(pal, 460, 140, |p| {
                        let rect = Rect::new(4.0, 4.0, 440.0, bar.height(p, 440.0));
                        f(p, rect)
                    })
                };
                let old = draw(&|p, r| old_draw_infobar(p, pal, r, bar, hover));
                let now = draw(&|p, r| draw_infobar(p, pal, r, bar, hover));
                let with = draw(&|p, r| {
                    draw_infobar_with(
                        p,
                        pal,
                        r,
                        bar,
                        Look {
                            hover,
                            ..Look::default()
                        },
                    )
                });
                assert!(
                    old.pixels == now.pixels,
                    "draw_infobar changed, hover {hover:?}"
                );
                assert!(
                    old.pixels == with.pixels,
                    "the hover-only look differs, {hover:?}"
                );
            }
        }
    }
}

#[test]
fn a_press_shows_only_on_the_control_under_the_pointer() {
    for pal in [&LIGHT, &DARK] {
        let bar = sample().action(7, "Install").action(9, "Details");
        let (mut bmp, mut gfx) = scratch();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        let rect = Rect::new(0.0, 0.0, 460.0, bar.height(&mut p, 460.0));
        let parts = bar.parts(&mut p, rect);
        let draw = |look: Look| {
            shot(pal, 460, 140, |p| {
                draw_infobar_with(p, pal, rect, &bar, look)
            })
        };
        let rest = draw(Look::default());
        let mut controls: Vec<(Hit, Rect)> = parts
            .actions
            .iter()
            .map(|&(id, r)| (Hit::Action(id), r))
            .collect();
        controls.push((Hit::Close, parts.close.unwrap()));
        for (hit, r) in controls {
            let hovered = draw(Look {
                hover: Some(hit),
                ..Look::default()
            });
            let pressed = draw(Look {
                hover: Some(hit),
                pressed: Some(hit),
                ..Look::default()
            });
            let left = draw(Look {
                pressed: Some(hit),
                ..Look::default()
            });
            let changed = differences(&hovered, &pressed);
            assert!(
                !changed.is_empty(),
                "{hit:?} has no press look on {}",
                pal.dark
            );
            for &(x, y) in &changed {
                assert!(
                    r.contains(x as f32 + 0.5, y as f32 + 0.5),
                    "pressing {hit:?} changed ({x}, {y}), outside {r:?}"
                );
            }
            assert!(
                left.pixels == rest.pixels,
                "a press the pointer has left still shows on {hit:?}"
            );
        }
    }
}

#[test]
fn the_focused_control_gets_the_accent_ring_just_outside_it() {
    for pal in [&LIGHT, &DARK] {
        let bar = sample().action(7, "Install").action(9, "Details");
        let (mut bmp, mut gfx) = scratch();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        let rect = Rect::new(0.0, 0.0, 460.0, bar.height(&mut p, 460.0));
        let parts = bar.parts(&mut p, rect);
        let draw = |look: Look| {
            shot(pal, 460, 140, |p| {
                draw_infobar_with(p, pal, rect, &bar, look)
            })
        };
        let rest = draw(Look::default());
        let mut controls: Vec<(Hit, Rect)> = parts
            .actions
            .iter()
            .map(|&(id, r)| (Hit::Action(id), r))
            .collect();
        controls.push((Hit::Close, parts.close.unwrap()));
        for (hit, r) in controls {
            for hover in [None, Some(hit)] {
                let plain = draw(Look {
                    hover,
                    ..Look::default()
                });
                let focused = draw(Look {
                    hover,
                    focused: Some(hit),
                    ..Look::default()
                });
                let changed = differences(&plain, &focused);
                assert!(!changed.is_empty(), "{hit:?} took no focus ring");
                for &(x, y) in &changed {
                    let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
                    assert!(
                        r.inset(-4.0).contains(cx, cy) && !r.inset(1.0).contains(cx, cy),
                        "the ring of {hit:?} drew at ({x}, {y}), not just outside {r:?}"
                    );
                }
                assert!(
                    count_in(&focused, r.inset(-4.0), pal.accent, 8)
                        > count_in(&plain, r.inset(-4.0), pal.accent, 8),
                    "the ring of {hit:?} is not in the accent"
                );
            }
        }
        // Focus on something the bar does not have marks nothing.
        let missing = Some(Hit::Action(99));
        let look = Look {
            focused: missing,
            pressed: missing,
            hover: missing,
            high_contrast: false,
        };
        assert!(
            draw(look).pixels == rest.pixels,
            "an action the bar lacks was marked"
        );
        let shut = sample().closable(false);
        let plain = shot(pal, 460, 140, |p| draw_infobar(p, pal, rect, &shut, None));
        let focused = shot(pal, 460, 140, |p| {
            draw_infobar_with(
                p,
                pal,
                rect,
                &shut,
                Look {
                    focused: Some(Hit::Close),
                    ..Look::default()
                },
            )
        });
        assert!(
            plain.pixels == focused.pixels,
            "a bar with no close button drew its ring"
        );
    }
}

#[test]
fn a_wrapped_message_stays_above_the_actions_and_inside_the_card_at_scale_one_and_a_half() {
    const SCALE: f32 = 1.5;
    let title = "This file has changed";
    let message = "Someone else saved it while you were editing, so the copy on disk is not \
                   the one in front of you. Reload it or keep yours.";
    let full = InfoBar::new(Severity::Warning, title, message)
        .action(1, "Reload")
        .action(2, "Keep mine");
    for pal in [&LIGHT, &DARK] {
        for w in [240.0, 330.0] {
            let mut bmp = Bitmap::new(8, 8);
            let mut gfx = Gfx::new();
            let mut p = Painter::new(&mut bmp, &mut gfx, SCALE);
            let rect = Rect::new(10.0, 10.0, w, full.height(&mut p, w));
            let parts = full.parts(&mut p, rect);
            let one_line = p.paragraph_height(Face::Ui, MESSAGE_TEXT, f32::INFINITY, message);
            let lines = full.layout(&mut p, rect).message.h;
            assert!(lines > 2.0 * one_line, "{w} wide, the message did not wrap");
            let actions_top = parts
                .actions
                .iter()
                .map(|(_, r)| r.y)
                .fold(f32::MAX, f32::min);
            for (_, r) in &parts.actions {
                assert!(
                    r.bottom() + PAD <= rect.bottom() + 1e-3,
                    "{r:?} leaves the card"
                );
            }
            // The message's own ink: the same card and title with and without it, both with no
            // actions and closable, so its column and place are the full bar's.
            let draw = |bar: InfoBar| {
                let size = |v: f32| ((v + 20.0) * SCALE) as u32;
                let mut bmp = Bitmap::new(size(w), size(rect.h));
                let mut gfx = Gfx::new();
                {
                    let mut p = Painter::new(&mut bmp, &mut gfx, SCALE);
                    p.fill(Rect::new(0.0, 0.0, w + 20.0, rect.h + 20.0), pal.page);
                    draw_infobar(&mut p, pal, rect, &bar, None);
                }
                bmp
            };
            let with = draw(InfoBar::new(Severity::Warning, title, message));
            let without = draw(InfoBar::new(Severity::Warning, title, ""));
            let ink = differences(&with, &without);
            assert!(!ink.is_empty(), "the message drew nothing");
            let inside = rect.inset(PAD);
            for &(x, y) in &ink {
                let (lx, ly) = (x as f32 / SCALE, y as f32 / SCALE);
                let (rx, by) = ((x + 1) as f32 / SCALE, (y + 1) as f32 / SCALE);
                assert!(
                    by <= actions_top,
                    "{w} wide on {}: ink at device ({x}, {y}) reaches the actions at {actions_top}",
                    pal.dark
                );
                assert!(
                    lx >= inside.x && rx <= inside.right() && ly >= inside.y,
                    "{w} wide: ink at device ({x}, {y}) is outside the card's padding"
                );
            }
        }
    }
}

#[test]
fn the_card_is_filled_in_the_severity_colour_in_both_themes() {
    for pal in [&LIGHT, &DARK] {
        for severity in ALL {
            let colors = severity_colors(severity, pal);
            let bmp = shot(pal, 420, 120, |p| {
                let bar = InfoBar::new(severity, "Title", "A message.");
                let rect = Rect::new(0.0, 0.0, 420.0, bar.height(p, 420.0));
                draw_infobar(p, pal, rect, &bar, None);
            });
            let inside = Rect::new(2.0, 30.0, 2.0, 10.0);
            assert_eq!(
                count_in(&bmp, inside, colors.fill, 1),
                inside.w as usize * inside.h as usize,
                "{severity:?} is not filled with {:?}",
                colors.fill
            );
        }
    }
}

#[test]
fn high_contrast_palettes_get_their_own_inks() {
    for severity in ALL {
        let light = severity_colors(severity, &LIGHT);
        let dark = severity_colors(severity, &DARK);
        let hc_light = severity_colors(severity, &HIGH_CONTRAST_LIGHT);
        let hc_dark = severity_colors(severity, &HIGH_CONTRAST_DARK);
        assert_ne!(light.ink, hc_light.ink);
        assert_ne!(dark.ink, hc_dark.ink);
        assert_eq!(hc_light.fill, HIGH_CONTRAST_LIGHT.page);
        assert_eq!(hc_dark.fill, HIGH_CONTRAST_DARK.page);
        assert_eq!(hc_light.line, hc_light.ink);
        assert_eq!(hc_dark.line, hc_dark.ink);
    }
}

#[test]
fn the_system_contrast_theme_gets_the_high_contrast_colours_when_told() {
    // Desert-like: white window, black text, a dark blue highlight. Not one of the toolkit's
    // two high-contrast palettes, so only the flag makes it high contrast.
    let colors = crate::tokens::SystemColors {
        window: rgb(0xfffaef),
        window_text: rgb(0x3d3d3d),
        highlight: rgb(0x903909),
        highlight_text: rgb(0xfff5e3),
        hot_light: rgb(0x1c5e75),
        gray_text: rgb(0x676767),
        button_face: rgb(0xfffaef),
        button_text: rgb(0x3d3d3d),
    };
    let pal = crate::tokens::Tokens::from_system_colors(&colors).palette;
    for severity in ALL {
        let told = severity_colors_in(severity, &pal, true);
        assert_eq!(told.fill, pal.page, "{severity:?}");
        assert_eq!(told.line, told.ink, "{severity:?}");
        let untold = severity_colors(severity, &pal);
        assert_ne!(
            untold.fill, pal.page,
            "{severity:?}: untold is the old light look"
        );
    }
    // The look's flag reaches the drawing: the card is the page, not the tinted fill.
    let bar = sample();
    let rect = Rect::new(0.0, 0.0, 360.0, 120.0);
    let card = |high_contrast| {
        let bmp = shot(&pal, 360, 120, |p| {
            draw_infobar_with(
                p,
                &pal,
                rect,
                &bar,
                Look {
                    high_contrast,
                    ..Look::default()
                },
            )
        });
        at(&bmp, 354.0, 60.0)
    };
    assert_eq!(card(true), pal.page);
    assert_eq!(card(false), severity_colors(Severity::Info, &pal).fill);
}

#[test]
fn the_dark_error_ink_reads_on_the_dark_surfaces() {
    use crate::cover::contrast;
    let ink = severity_colors(Severity::Error, &DARK).ink;
    assert_eq!(ink, DARK_ERROR_INK);
    let fill = severity_colors(Severity::Error, &DARK).fill;
    for ground in [DARK.page, DARK.pane, DARK.bar, DARK.popup, fill] {
        assert!(
            contrast(ink, ground) >= 4.5,
            "{ground:?}: {}",
            contrast(ink, ground)
        );
    }
    // Light and the high-contrast palettes keep the palette's danger.
    assert_eq!(severity_colors(Severity::Error, &LIGHT).ink, LIGHT.danger);
    assert_eq!(
        severity_colors(Severity::Error, &HIGH_CONTRAST_DARK).ink,
        HIGH_CONTRAST_DARK.danger
    );
}

#[test]
fn an_empty_bar_is_the_padding_high_and_has_no_hits() {
    let (mut bmp, mut gfx) = scratch();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    let bare = InfoBar::new(Severity::Info, "", "").closable(false);
    assert_eq!(bare.height(&mut p, 300.0), 2.0 * PAD);
    let rect = Rect::new(0.0, 0.0, 300.0, 2.0 * PAD);
    assert_eq!(bare.hit(&mut p, rect, rect.cx(), rect.cy()), None);
    // With a close button it is the close button's height, so the cross fits.
    let closable = InfoBar::new(Severity::Info, "", "");
    assert_eq!(closable.height(&mut p, 300.0), 2.0 * PAD + CLOSE_SIZE);
}

#[test]
fn nonsense_sizes_give_finite_heights_and_no_panics() {
    let (mut bmp, mut gfx) = scratch();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    let bar = sample().action(1, "Install");
    for width in [0.0, -50.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let h = bar.height(&mut p, width);
        assert!(h.is_finite() && h > 0.0, "{width} gave {h}");
    }
    let rect = Rect::new(0.0, 0.0, 300.0, bar.height(&mut p, 300.0));
    assert_eq!(bar.hit(&mut p, rect, f32::NAN, 4.0), None);
    let before = stack_layout(
        &mut p,
        Rect::new(f32::NAN, f32::INFINITY, f32::NAN, -3.0),
        &[bar.clone(), bar],
    );
    for r in &before {
        assert!(r.x.is_finite() && r.y.is_finite() && r.w.is_finite() && r.h.is_finite());
        assert_eq!(r.w, 0.0);
        assert_eq!(r.h, 0.0);
    }
}

#[test]
#[ignore]
fn infobar_sheets() {
    use std::path::Path;
    const W: f32 = 860.0;
    const H: f32 = 840.0;
    let out = |name: &str| {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/specimen")
            .join(name)
    };
    let bars = || {
        vec![
            InfoBar::new(
                Severity::Info,
                "Update available",
                "A new version is ready to install.",
            ),
            InfoBar::new(
                Severity::Info,
                "Update available",
                "A new version is ready to install.",
            )
            .action(1, "Install")
            .action(2, "Later"),
            InfoBar::new(Severity::Success, "Saved", "Your changes are on disk."),
            InfoBar::new(Severity::Success, "Saved", "Your changes are on disk.")
                .action(3, "Open folder"),
            InfoBar::new(
                Severity::Warning,
                "This file has changed",
                "Someone else saved it while you were editing.",
            ),
            InfoBar::new(
                Severity::Warning,
                "This file has changed",
                "Someone else saved it while you were editing.",
            )
            .action(4, "Reload")
            .action(5, "Keep mine")
            .closable(false),
            InfoBar::new(
                Severity::Error,
                "Could not save",
                "The folder is read-only.",
            ),
            InfoBar::new(
                Severity::Error,
                "Could not save",
                "The folder is read-only. Choose another place to save the document.",
            )
            .action(6, "Save as…")
            .action(7, "Details"),
        ]
    };
    for (scale, sfx) in [(1.0, ""), (1.5, "-150")] {
        for (pal, theme) in [(&LIGHT, "light"), (&DARK, "dark")] {
            let mut bmp = Bitmap::new((W * scale) as u32, (H * scale) as u32);
            let mut gfx = Gfx::new();
            {
                let mut p = Painter::new(&mut bmp, &mut gfx, scale);
                p.fill(Rect::new(0.0, 0.0, W, H), pal.page);
                let area = Rect::new(30.0, 30.0, W - 60.0, H - 60.0);
                let list = bars();
                let rects = stack_layout(&mut p, area, &list);
                for (bar, rect) in list.iter().zip(&rects) {
                    draw_infobar(&mut p, pal, *rect, bar, None);
                }
            }
            bmp.write_png(&out(&format!("infobar-{theme}{sfx}.png")))
                .unwrap();
        }
    }
}
