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

#[test]
fn the_close_cross_is_drawn_only_when_the_bar_is_closable() {
    for pal in [&LIGHT, &DARK] {
        let close = Rect::new(420.0 - PAD - CLOSE_SIZE, PAD, CLOSE_SIZE, CLOSE_SIZE);
        let draw = |on: bool| {
            shot(pal, 420, 120, move |p| {
                let bar = sample().closable(on);
                let rect = Rect::new(0.0, 0.0, 420.0, bar.height(p, 420.0));
                draw_infobar(p, pal, rect, &bar, None);
            })
        };
        let open = draw(true);
        let shut = draw(false);
        assert!(
            count_in(&open, close, pal.muted, 24) > 0,
            "the close cross is missing from the closable bar"
        );
        assert_eq!(
            count_in(&shut, close, pal.muted, 24),
            0,
            "a bar that cannot be closed drew a close cross"
        );
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
