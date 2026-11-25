//! The pieces of the title row every application with this look shares: its height, the window
//! caption buttons, the tab strip's widths and one tab's drawing, and the fill behind a button.
//! The caller owns the layout and hit recording; these functions take plain values and rectangles.

use crate::anim::{Animator, AsId, Id, document};
use crate::gfx::{Face, Painter, Rect, Rgb, mix};
use crate::glyph;
use crate::theme::Palette;

/// Height of the title row and of the command bar under it, logical px.
pub const TITLE_H: f32 = 40.0;
pub const BAR_H: f32 = 44.0;

pub const WIN_BTN_W: f32 = 46.0;
pub const TAB_MIN: f32 = 90.0;
pub const TAB_MAX: f32 = 220.0;
/// Tabs squeeze below `TAB_MIN` only when nothing else fits; under this they would be unusable.
pub const TAB_FLOOR: f32 = 36.0;
/// Where the tabs start, after the logo.
pub const TABS_X: f32 = 50.0;
/// macOS: the window keeps the system's title bar, transparent and under the title row
/// ([`crate::window::identify`]), so the system draws the close, minimise and zoom buttons at the
/// row's left, resizes the window, and gives it its shadow and corners. The application draws no
/// window buttons, no logo and no resize band of its own there, and starts the tabs at
/// [`TABS_LEFT`].
pub const NATIVE_FRAME: bool = cfg!(target_os = "macos");
/// Where the tabs start: after the logo, or on macOS after the system's three buttons.
pub const TABS_LEFT: f32 = if NATIVE_FRAME { 80.0 } else { TABS_X };
/// The new-tab button after the last tab, and the drag area kept free beside it.
pub const NEW_TAB_W: f32 = 34.0;
pub const DRAG_MIN: f32 = 56.0;

/// The fill behind a button: pressed, hovered, toggled on, or none. `pressed` is "the press that
/// is held started on this button"; it only shows while the pointer is still over it.
pub fn button_fill(
    pal: &Palette,
    hovered: bool,
    pressed: bool,
    on: bool,
    open: bool,
) -> Option<Rgb> {
    if open || (pressed && hovered) {
        Some(pal.press)
    } else if hovered {
        Some(if on {
            mix(pal.on, pal.text, 0.06)
        } else {
            pal.hover
        })
    } else if on {
        Some(pal.on)
    } else {
        None
    }
}

/// The three window caption buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WinButton {
    Min,
    Max,
    Close,
}

/// The rectangle of caption button number `n` counted from the right edge (0 is the one at the
/// edge) of a title row whose right edge is `right`.
pub fn win_button_rect(right: f32, n: usize) -> Rect {
    Rect::new(
        right - (n as f32 + 1.0) * WIN_BTN_W,
        0.0,
        WIN_BTN_W,
        TITLE_H,
    )
}

/// Draws one caption button into `b`. `maximized` picks the restore glyph for `Max`.
pub fn draw_win_button(
    p: &mut Painter,
    pal: &Palette,
    kind: WinButton,
    b: Rect,
    maximized: bool,
    hovered: bool,
    pressed: bool,
) {
    let g = match kind {
        WinButton::Close => glyph::WIN_CLOSE,
        WinButton::Max if maximized => glyph::WIN_RESTORE,
        WinButton::Max => glyph::WIN_MAX,
        WinButton::Min => glyph::WIN_MIN,
    };
    let pressed = pressed && hovered;
    let mut color = pal.text;
    if kind == WinButton::Close && hovered {
        p.fill(
            b,
            if pressed {
                mix(pal.danger, [0, 0, 0], 0.15)
            } else {
                pal.danger
            },
        );
        color = [255, 255, 255];
    } else if hovered {
        p.fill(b, if pressed { pal.press } else { pal.hover });
    }
    p.icon(g, 10.0, b.cx(), b.cy(), color);
}

/// The width a tab with this label would like, before `tab_widths` fits them to the row.
pub fn tab_natural_width(p: &mut Painter, label: &str) -> f32 {
    p.measure(Face::Semibold, 12.5, label) + 74.0
}

/// Widths for tabs that would like `natural` px each, fitting in `avail`: each between
/// `TAB_MIN` and `TAB_MAX`; when they do not fit, the widest are capped first (so short labels
/// keep their size), then all shrink evenly down to `TAB_FLOOR`.
pub fn tab_widths(natural: &[f32], avail: f32) -> Vec<f32> {
    let wants: Vec<f32> = natural
        .iter()
        .map(|w| w.clamp(TAB_MIN, TAB_MAX).round())
        .collect();
    let sum: f32 = wants.iter().sum();
    if sum <= avail || wants.is_empty() {
        return wants;
    }
    let fits = |cap: f32| wants.iter().map(|w| w.min(cap)).sum::<f32>() <= avail;
    let (mut lo, mut hi) = (TAB_FLOOR, TAB_MAX);
    if !fits(lo) {
        return vec![TAB_FLOOR; wants.len()];
    }
    for _ in 0..40 {
        let mid = (lo + hi) / 2.0;
        if fits(mid) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let cap = lo.floor();
    wants.iter().map(|w| w.min(cap)).collect()
}

/// What decides how one tab looks.
#[derive(Clone, Copy, Debug, Default)]
pub struct TabState {
    pub active: bool,
    /// The pointer is over the tab or its close button.
    pub hovered: bool,
    /// The tab is being dragged to a new place.
    pub dragging: bool,
    /// The file is gone: warning glyph, struck-through label.
    pub missing: bool,
    /// The next tab is quiet too (not active, not hovered), so a hairline separates the two.
    pub next_quiet: bool,
    pub close_hovered: bool,
    /// A press that started on this tab's close button is held.
    pub close_pressed: bool,
    /// The tab is pinned: a pin stands where the close button would, always shown, and
    /// pressing it unpins the tab.
    pub pinned: bool,
}

/// Where one drawn tab can be hit: the tab, and its close button when shown.
#[derive(Clone, Copy, Debug)]
pub struct TabDrawn {
    pub tab: Rect,
    pub close: Option<Rect>,
}

/// Draws the tab at `x`, `w` wide, in a title row `row_w` wide. The caller sets the clip that
/// cuts off tabs which do not fit and records the hits (the tab first, then the close button).
pub fn draw_tab(
    p: &mut Painter,
    pal: &Palette,
    x: f32,
    w: f32,
    row_w: f32,
    label: &str,
    s: &TabState,
) -> TabDrawn {
    let tab = Rect::new(x, 6.0, w, TITLE_H - 6.0);
    if s.active {
        // The active tab joins the bar: square bottom, outward curves where it meets it.
        let old = p.clip(Rect::new(0.0, 0.0, row_w, TITLE_H));
        p.rbox(Rect::new(x, 6.0, w, TITLE_H), 8.0, pal.bar);
        for (cx, sq) in [(x - 8.0, x - 8.0), (x + w + 8.0, x + w)] {
            let corner = Rect::new(sq, TITLE_H - 8.0, 8.0, 8.0);
            let o2 = p.clip(corner);
            p.fill(corner, pal.bar);
            p.rbox(
                Rect::new(cx - 8.0, TITLE_H - 16.0, 16.0, 16.0),
                8.0,
                pal.title,
            );
            p.restore(o2);
        }
        p.restore(old);
    } else if s.hovered || s.dragging {
        p.rbox(Rect::new(x + 2.0, 9.0, w - 4.0, 28.0), 6.0, pal.hover);
    } else if s.next_quiet {
        // A hairline between two quiet tabs.
        let px = p.px();
        p.fill(Rect::new(x + w - px, 15.0, px, 16.0), pal.line);
    }
    let cy = 23.0;
    let show_close = (s.active || s.hovered || s.pinned) && w >= 64.0;
    let tiny = w < 64.0;
    let icon_x = if tiny { x + w / 2.0 } else { x + 19.0 };
    let (g, gc) = if s.missing {
        (glyph::WARN, pal.warn)
    } else if s.active {
        (glyph::DOC, pal.accent)
    } else {
        (glyph::DOC, pal.muted)
    };
    p.icon(g, 13.0, icon_x, cy, gc);
    if !tiny {
        let tx = x + 34.0;
        let max = w - 34.0 - if show_close { 30.0 } else { 12.0 };
        let (face, color) = if s.active {
            (Face::Semibold, pal.text)
        } else if s.hovered {
            (Face::Ui, pal.text)
        } else {
            (Face::Ui, pal.muted)
        };
        let color = if s.missing { pal.faint } else { color };
        let drawn = p.text(face, 12.5, tx, cy, label, color, max);
        if s.missing {
            let px = p.px().max(1.0);
            p.fill(Rect::new(tx, cy + 0.5, drawn, px), pal.faint);
        }
    }
    let close = show_close.then(|| {
        let b = Rect::new(x + w - 29.0, cy - 10.0, 20.0, 20.0);
        if let Some(fill) = button_fill(pal, s.close_hovered, s.close_pressed, false, false) {
            p.rbox(b, 4.0, fill);
        }
        let (g, size) = if s.pinned {
            (glyph::PIN, 12.0)
        } else {
            (glyph::CLOSE, 8.5)
        };
        p.icon(
            g,
            size,
            b.cx(),
            b.cy(),
            if s.close_hovered { pal.text } else { pal.muted },
        );
        b
    });
    TabDrawn { tab, close }
}

/// Draws the new-tab button after the last tab (which ended at `x`) and returns its rectangle.
/// `avail` is the width the tabs had, so the button stays inside it when they are crowded.
pub fn draw_new_tab_button(
    p: &mut Painter,
    pal: &Palette,
    x: f32,
    avail: f32,
    hovered: bool,
    pressed: bool,
) -> Rect {
    let b = Rect::new((x + 4.0).min(TABS_X + avail + 4.0), 8.0, 30.0, 30.0);
    if let Some(fill) = button_fill(pal, hovered, pressed, false, false) {
        p.rbox(b, 6.0, fill);
    }
    p.icon(glyph::PLUS, 12.0, b.cx(), b.cy() - 0.5, pal.text);
    b
}

// ---- with motion -------------------------------------------------------------------------------
//
// The hover look of each piece fades in and out by `anim::document::HOVER` (roadmap D4). A press
// shows at once, as on Windows. At rest each of these calls its plain function as it is, so a
// window that does not animate (or a frame after the fade) draws exactly what it drew before;
// only between the two ends is the hover look laid over the rest look at its opacity.

/// [`button_fill`] with motion: the fill to draw and its opacity, `None` for none. The hover
/// colour fades in over the rest look (none, or `on`); a press and an open menu are at once.
/// `id` names the button in `anim`.
#[allow(clippy::too_many_arguments)]
pub fn button_fill_animated(
    anim: &mut Animator,
    id: impl AsId,
    pal: &Palette,
    hovered: bool,
    pressed: bool,
    on: bool,
    open: bool,
) -> Option<(Rgb, f32)> {
    let t = anim.toggle(Id::new(id).with("hover"), hovered, document::HOVER);
    if open || (pressed && hovered) || t >= 1.0 || t <= 0.0 {
        return button_fill(pal, hovered || t >= 1.0, pressed, on, open).map(|c| (c, 1.0));
    }
    let lit = button_fill(pal, true, false, on, false).unwrap_or(pal.hover);
    match button_fill(pal, false, false, on, false) {
        Some(rest) => Some((mix(rest, lit, t), 1.0)),
        None => Some((lit, t)),
    }
}

/// [`draw_win_button`] with motion: the hover fill (red for Close, its glyph turning white)
/// fades in and out. `id` names the button in `anim`.
#[allow(clippy::too_many_arguments)]
pub fn draw_win_button_animated(
    p: &mut Painter,
    pal: &Palette,
    anim: &mut Animator,
    id: impl AsId,
    kind: WinButton,
    b: Rect,
    maximized: bool,
    hovered: bool,
    pressed: bool,
) {
    let t = anim.toggle(Id::new(id).with("hover"), hovered, document::HOVER);
    if (pressed && hovered) || t >= 1.0 || t <= 0.0 {
        return draw_win_button(p, pal, kind, b, maximized, hovered || t >= 1.0, pressed);
    }
    let g = match kind {
        WinButton::Close => glyph::WIN_CLOSE,
        WinButton::Max if maximized => glyph::WIN_RESTORE,
        WinButton::Max => glyph::WIN_MAX,
        WinButton::Min => glyph::WIN_MIN,
    };
    let color = if kind == WinButton::Close {
        p.fill_a(b, pal.danger, t);
        mix(pal.text, [255, 255, 255], t)
    } else {
        p.fill_a(b, pal.hover, t);
        pal.text
    };
    p.icon(g, 10.0, b.cx(), b.cy(), color);
}

/// [`draw_tab`] with motion: a quiet tab's hover fill and its label's colour fade in and out,
/// and the hairline beside it the other way. The active tab, a dragged one and the close
/// button's own hover are as [`draw_tab`] draws them; the close button shows while the tab is
/// hovered, at once. `id` names the tab in `anim` (its document, not its place, so a tab that
/// moves keeps its fade).
#[allow(clippy::too_many_arguments)]
pub fn draw_tab_animated(
    p: &mut Painter,
    pal: &Palette,
    anim: &mut Animator,
    id: impl AsId,
    x: f32,
    w: f32,
    row_w: f32,
    label: &str,
    s: &TabState,
) -> TabDrawn {
    let quiet = !s.active && !s.dragging;
    let t = anim.toggle(Id::new(id).with("hover"), s.hovered, document::HOVER);
    if !quiet || t >= 1.0 || t <= 0.0 {
        let s = TabState {
            hovered: s.hovered || (quiet && t >= 1.0),
            ..*s
        };
        return draw_tab(p, pal, x, w, row_w, label, &s);
    }
    p.rbox_a(Rect::new(x + 2.0, 9.0, w - 4.0, 28.0), 6.0, pal.hover, t);
    if s.next_quiet {
        let px = p.px();
        p.fill_a(Rect::new(x + w - px, 15.0, px, 16.0), pal.line, 1.0 - t);
    }
    let cy = 23.0;
    let show_close = (s.hovered || s.pinned) && w >= 64.0;
    let tiny = w < 64.0;
    let icon_x = if tiny { x + w / 2.0 } else { x + 19.0 };
    let (g, gc) = if s.missing {
        (glyph::WARN, pal.warn)
    } else {
        (glyph::DOC, pal.muted)
    };
    p.icon(g, 13.0, icon_x, cy, gc);
    if !tiny {
        let tx = x + 34.0;
        let max = w - 34.0 - if show_close { 30.0 } else { 12.0 };
        let color = if s.missing {
            pal.faint
        } else {
            mix(pal.muted, pal.text, t)
        };
        let drawn = p.text(Face::Ui, 12.5, tx, cy, label, color, max);
        if s.missing {
            let px = p.px().max(1.0);
            p.fill(Rect::new(tx, cy + 0.5, drawn, px), pal.faint);
        }
    }
    let close = show_close.then(|| {
        let b = Rect::new(x + w - 29.0, cy - 10.0, 20.0, 20.0);
        if let Some(fill) = button_fill(pal, s.close_hovered, s.close_pressed, false, false) {
            p.rbox(b, 4.0, fill);
        }
        let (g, size) = if s.pinned {
            (glyph::PIN, 12.0)
        } else {
            (glyph::CLOSE, 8.5)
        };
        p.icon(
            g,
            size,
            b.cx(),
            b.cy(),
            if s.close_hovered { pal.text } else { pal.muted },
        );
        b
    });
    TabDrawn {
        tab: Rect::new(x, 6.0, w, TITLE_H - 6.0),
        close,
    }
}

/// [`draw_new_tab_button`] with motion: its hover fill fades in and out. `id` names it in
/// `anim`.
#[allow(clippy::too_many_arguments)]
pub fn draw_new_tab_button_animated(
    p: &mut Painter,
    pal: &Palette,
    anim: &mut Animator,
    id: impl AsId,
    x: f32,
    avail: f32,
    hovered: bool,
    pressed: bool,
) -> Rect {
    let fill = button_fill_animated(anim, id, pal, hovered, pressed, false, false);
    match fill {
        Some((_, a)) if a < 1.0 => {
            let b = Rect::new((x + 4.0).min(TABS_X + avail + 4.0), 8.0, 30.0, 30.0);
            p.rbox_a(b, 6.0, pal.hover, a);
            p.icon(glyph::PLUS, 12.0, b.cx(), b.cy() - 0.5, pal.text);
            b
        }
        _ => draw_new_tab_button(p, pal, x, avail, hovered || fill.is_some(), pressed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// At rest, and with an animator that does not animate, each animated piece draws what its
    /// plain function draws, pixel for pixel; on its way, something else.
    #[test]
    fn the_animated_pieces_are_the_plain_ones_at_rest() {
        use crate::anim::{Clock, ManualClock, Policy};
        use crate::gfx::Gfx;
        use mdshared_text::Bitmap;
        let pal = crate::theme::LIGHT;
        let area = Rect::new(0.0, 0.0, 400.0, TITLE_H);
        let shot = |f: &mut dyn FnMut(&mut Painter)| {
            let mut bmp = Bitmap::new(400, TITLE_H as u32);
            let mut gfx = Gfx::new();
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            p.fill(area, pal.title);
            f(&mut p);
            bmp.pixels
        };
        let close = win_button_rect(400.0, 0);
        let min = win_button_rect(400.0, 2);
        let tab = |hovered| TabState {
            hovered,
            next_quiet: true,
            ..TabState::default()
        };
        let plain = |hovered: bool| {
            shot(&mut |p| {
                draw_win_button(p, &pal, WinButton::Close, close, false, hovered, false);
                draw_win_button(p, &pal, WinButton::Min, min, false, hovered, false);
                draw_tab(p, &pal, 50.0, 120.0, 400.0, "Notes.md", &tab(hovered));
                draw_new_tab_button(p, &pal, 170.0, 300.0, hovered, false);
            })
        };
        let moving = |anim: &mut Animator, hovered: bool| {
            shot(&mut |p| {
                let (a, s) = (&mut *anim, hovered);
                draw_win_button_animated(
                    p,
                    &pal,
                    a,
                    "close",
                    WinButton::Close,
                    close,
                    false,
                    s,
                    false,
                );
                draw_win_button_animated(p, &pal, a, "min", WinButton::Min, min, false, s, false);
                draw_tab_animated(p, &pal, a, "tab", 50.0, 120.0, 400.0, "Notes.md", &tab(s));
                draw_new_tab_button_animated(p, &pal, a, "new", 170.0, 300.0, s, false);
            })
        };
        let clock = ManualClock::new();
        let mut anim = Animator::new();
        let mut frame = |hovered: bool| {
            anim.begin_frame(clock.now());
            let px = moving(&mut anim, hovered);
            anim.end_frame();
            px
        };
        assert!(frame(false) == plain(false), "first seen: at rest");
        // The motion starts on the frame that asks; one later it is on its way.
        frame(true);
        clock.advance_ms(40);
        let on_way = frame(true);
        assert!(
            on_way != plain(true) && on_way != plain(false),
            "on its way"
        );
        clock.advance_ms(500);
        assert!(frame(true) == plain(true), "arrived");
        frame(false);
        clock.advance_ms(40);
        let back = frame(false);
        assert!(
            back != plain(true) && back != plain(false),
            "on its way back"
        );
        clock.advance_ms(500);
        assert!(frame(false) == plain(false), "back at rest");
        let mut still = Animator::new();
        still.set_policy(Policy::Never);
        for hovered in [false, true] {
            still.begin_frame(clock.now());
            assert!(moving(&mut still, hovered) == plain(hovered));
            still.end_frame();
        }
    }

    #[test]
    fn tabs_keep_natural_width_when_room() {
        assert_eq!(tab_widths(&[100.0, 150.0], 1000.0), vec![100.0, 150.0]);
        assert_eq!(tab_widths(&[40.0, 400.0], 1000.0), vec![TAB_MIN, TAB_MAX]);
        assert!(tab_widths(&[], 10.0).is_empty());
    }

    #[test]
    fn crowded_tabs_cap_the_widest_first() {
        let w = tab_widths(&[100.0, 220.0, 220.0], 440.0);
        assert_eq!(w[0], 100.0);
        assert!(w[1] == w[2] && w[1] <= 170.0 && w[1] >= 160.0);
        assert!(w.iter().sum::<f32>() <= 440.0);
    }

    #[test]
    fn many_tabs_squeeze_to_the_floor() {
        let w = tab_widths(&[200.0; 8], 500.0);
        assert!(w.iter().all(|&x| (TAB_FLOOR..TAB_MIN).contains(&x)));
        assert!(w.iter().sum::<f32>() <= 500.0);
        assert_eq!(tab_widths(&[200.0; 20], 100.0), vec![TAB_FLOOR; 20]);
    }

    #[test]
    fn caption_buttons_sit_from_the_right() {
        assert_eq!(win_button_rect(1000.0, 0).x, 954.0);
        assert_eq!(win_button_rect(1000.0, 2).x, 862.0);
    }

    #[test]
    fn button_fill_states() {
        let pal = crate::theme::LIGHT;
        assert_eq!(button_fill(&pal, false, false, false, false), None);
        assert_eq!(button_fill(&pal, false, false, true, false), Some(pal.on));
        assert_eq!(
            button_fill(&pal, true, false, false, false),
            Some(pal.hover)
        );
        assert_eq!(button_fill(&pal, true, true, false, false), Some(pal.press));
        // A held press that left the button shows nothing.
        assert_eq!(button_fill(&pal, false, true, false, false), None);
        assert_eq!(
            button_fill(&pal, false, false, false, true),
            Some(pal.press)
        );
    }
}
