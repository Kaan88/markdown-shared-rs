//! Bars across the top of the page: a text button for them, the bar that offers a newer version
//! of the application, and the bar that offers the report of a crash. Pure drawing; the caller
//! records the hits.

use crate::gfx::{Face, Painter, Rect, mix};
use crate::glyph;
use crate::theme::Palette;
use mdshared_app::relaunch::RESTART;
use mdshared_app::updater::{App, GO, Stage, Updates};

/// A text button on a bar, its left edge at `x` and centred on `cy`: quiet until hovered, or
/// filled with the accent when `primary`. Returns its rectangle.
#[allow(clippy::too_many_arguments)] // a bar-button call: position, label and state flags
pub fn bar_button(
    p: &mut Painter,
    pal: &Palette,
    x: f32,
    cy: f32,
    label: &str,
    primary: bool,
    hovered: bool,
    pressed: bool,
) -> Rect {
    let w = bar_button_w(p, label);
    let b = Rect::new(x, cy - 12.0, w, 24.0);
    let (fill, text) = if primary {
        let fill = match (hovered, pressed) {
            (true, true) => mix(pal.accent, pal.page, 0.2),
            (true, false) => mix(pal.accent, pal.page, 0.1),
            _ => pal.accent,
        };
        (fill, pal.on_accent)
    } else {
        let lift = match (hovered, pressed) {
            (true, true) => 0.16,
            (true, false) => 0.10,
            _ => 0.05,
        };
        (mix(pal.bar, pal.text, lift), pal.text)
    };
    p.rbox(b, 5.0, fill);
    p.text_center(Face::Ui, 12.5, b, label, text);
    b
}

/// The width [`bar_button`] gives `label`.
pub fn bar_button_w(p: &mut Painter, label: &str) -> f32 {
    (p.measure(Face::Ui, 12.5, label) + 22.0).round()
}

/// Which of the update bar's controls the pointer is over or holds down.
#[derive(Clone, Copy, Debug, Default)]
pub struct UpdateBarState {
    pub go_hovered: bool,
    pub go_pressed: bool,
    pub close_hovered: bool,
    /// Whether the installed state offers a restart (the application has a file to start).
    pub restart: bool,
    pub restart_hovered: bool,
    pub restart_pressed: bool,
}

/// Where the update bar's controls were drawn, for the caller's hits.
#[derive(Clone, Copy, Debug, Default)]
pub struct UpdateBarHits {
    /// The button that starts the update, while one is on offer.
    pub go: Option<Rect>,
    /// The button that restarts into the installed version, once it is installed.
    pub restart: Option<Rect>,
    /// The close button; absent while an update downloads.
    pub close: Option<Rect>,
}

/// How much of a bar at `bar` shows while it slides in or out, `t` from 0 (gone) to 1 (there),
/// from [`Animator::presence`](crate::anim::Animator::presence) with
/// [`crate::anim::document::MENU`] (roadmap X3): the bar's own height times `t`, rounded to whole
/// logical px, for the caller to lay the page out under it.
pub fn bar_height(bar: Rect, t: f32) -> f32 {
    (bar.h * t.clamp(0.0, 1.0)).round()
}

/// Draws a bar (any of this module's, through `draw`) sliding down into place from the top of
/// its rectangle as `t` goes from 0 to 1, and back up as it goes: `draw` is given the bar's
/// rectangle moved up by what is hidden, and the drawing is cut to the part that shows
/// ([`bar_height`]). At 1 it is `draw(bar)`, pixel for pixel; at 0 nothing is drawn. Returns
/// what `draw` returns (the hits, in their drawn places), or `None` at 0.
pub fn draw_sliding<R>(
    p: &mut Painter,
    bar: Rect,
    t: f32,
    draw: impl FnOnce(&mut Painter, Rect) -> R,
) -> Option<R> {
    if t.is_nan() || t >= 1.0 {
        return Some(draw(p, bar));
    }
    let shown = bar_height(bar, t);
    if shown <= 0.0 {
        return None;
    }
    let old = p.clip(Rect::new(bar.x, bar.y, bar.w, shown));
    let moved = Rect::new(bar.x, bar.y - (bar.h - shown), bar.w, bar.h);
    let out = draw(p, moved);
    p.restore(old);
    Some(out)
}

/// The bar that offers a newer version, shows its download, and says when it is in place.
pub fn draw_update_bar(
    p: &mut Painter,
    pal: &Palette,
    bar: Rect,
    app: &App,
    stage: &Stage,
    state: UpdateBarState,
) -> UpdateBarHits {
    let mut hits = UpdateBarHits::default();
    if *stage == Stage::None {
        return hits;
    }
    p.fill(bar, pal.bar);
    p.fill(
        Rect::new(bar.x, bar.bottom() - p.px(), bar.w, p.px()),
        pal.line,
    );
    let mut right = bar.right() - 10.0;
    if !matches!(stage, Stage::Installing(_)) {
        let close = Rect::new(bar.right() - 34.0, bar.cy() - 12.0, 24.0, 24.0);
        if state.close_hovered {
            p.rbox(close, 5.0, mix(pal.bar, pal.text, 0.10));
        }
        p.icon(glyph::CLOSE, 10.0, close.cx(), close.cy(), pal.muted);
        hits.close = Some(close);
        right = close.x - 10.0;
    }
    if matches!(stage, Stage::Available(_)) {
        right -= bar_button_w(p, GO);
        hits.go = Some(bar_button(
            p,
            pal,
            right,
            bar.cy(),
            GO,
            true,
            state.go_hovered,
            state.go_pressed,
        ));
        right -= 12.0;
    }
    if matches!(stage, Stage::Installed(_)) && state.restart {
        right -= bar_button_w(p, RESTART);
        hits.restart = Some(bar_button(
            p,
            pal,
            right,
            bar.cy(),
            RESTART,
            true,
            state.restart_hovered,
            state.restart_pressed,
        ));
        right -= 12.0;
    }
    let label = Updates::bar_label_with(app, stage, state.restart);
    p.icon(glyph::DOWNLOAD, 13.0, bar.x + 22.0, bar.cy(), pal.muted);
    p.text(
        Face::Ui,
        12.5,
        bar.x + 38.0,
        bar.cy(),
        &label,
        pal.text,
        (right - (bar.x + 38.0)).max(0.0),
    );
    hits
}

/// The crash bar's buttons.
pub const OPEN_REPORT: &str = "Open report";
pub const COPY_REPORT: &str = "Copy";

/// Which of the crash bar's controls the pointer is over or holds down.
#[derive(Clone, Copy, Debug, Default)]
pub struct CrashBarState {
    pub open_hovered: bool,
    pub open_pressed: bool,
    pub copy_hovered: bool,
    pub copy_pressed: bool,
    pub close_hovered: bool,
}

/// Where the crash bar's controls were drawn, for the caller's hits.
#[derive(Clone, Copy, Debug, Default)]
pub struct CrashBarHits {
    pub open: Option<Rect>,
    pub copy: Option<Rect>,
    pub close: Option<Rect>,
}

/// What the crash bar says for `app` (its display name).
pub fn crash_bar_label(app: &str) -> String {
    format!("{app} closed because of an error last time. A report was saved; you can send it.")
}

/// The bar that says the application ended on an error last time and offers the report
/// ([`mdshared_app::crash`]): open it, copy it, or close the bar.
pub fn draw_crash_bar(
    p: &mut Painter,
    pal: &Palette,
    bar: Rect,
    app: &str,
    state: CrashBarState,
) -> CrashBarHits {
    let mut hits = CrashBarHits::default();
    p.fill(bar, pal.bar);
    p.fill(
        Rect::new(bar.x, bar.bottom() - p.px(), bar.w, p.px()),
        pal.line,
    );
    let close = Rect::new(bar.right() - 34.0, bar.cy() - 12.0, 24.0, 24.0);
    if state.close_hovered {
        p.rbox(close, 5.0, mix(pal.bar, pal.text, 0.10));
    }
    p.icon(glyph::CLOSE, 10.0, close.cx(), close.cy(), pal.muted);
    hits.close = Some(close);
    let mut right = close.x - 10.0;
    right -= bar_button_w(p, COPY_REPORT);
    hits.copy = Some(bar_button(
        p,
        pal,
        right,
        bar.cy(),
        COPY_REPORT,
        false,
        state.copy_hovered,
        state.copy_pressed,
    ));
    right -= 8.0 + bar_button_w(p, OPEN_REPORT);
    hits.open = Some(bar_button(
        p,
        pal,
        right,
        bar.cy(),
        OPEN_REPORT,
        true,
        state.open_hovered,
        state.open_pressed,
    ));
    right -= 12.0;
    p.icon(glyph::WARN, 13.0, bar.x + 22.0, bar.cy(), pal.muted);
    p.text(
        Face::Ui,
        12.5,
        bar.x + 38.0,
        bar.cy(),
        &crash_bar_label(app),
        pal.text,
        (right - (bar.x + 38.0)).max(0.0),
    );
    hits
}

/// Which of an offer bar's controls the pointer is over or holds down.
#[derive(Clone, Copy, Debug, Default)]
pub struct OfferBarState {
    pub yes_hovered: bool,
    pub yes_pressed: bool,
    pub no_hovered: bool,
    pub no_pressed: bool,
    pub close_hovered: bool,
}

/// Where an offer bar's controls were drawn, for the caller's hits.
#[derive(Clone, Copy, Debug, Default)]
pub struct OfferBarHits {
    pub yes: Option<Rect>,
    pub no: Option<Rect>,
    pub close: Option<Rect>,
}

/// A bar that offers something: `text` after `icon`, a primary button `yes`, a quiet one `no`
/// and a close button (for "not now"). The stale-tab offer ([`crate::stale`]) is one.
#[allow(clippy::too_many_arguments)] // a bar: where, what it says, its two buttons and state
pub fn draw_offer_bar(
    p: &mut Painter,
    pal: &Palette,
    bar: Rect,
    icon: &str,
    text: &str,
    yes: &str,
    no: &str,
    state: OfferBarState,
) -> OfferBarHits {
    let mut hits = OfferBarHits::default();
    p.fill(bar, pal.bar);
    p.fill(
        Rect::new(bar.x, bar.bottom() - p.px(), bar.w, p.px()),
        pal.line,
    );
    let close = Rect::new(bar.right() - 34.0, bar.cy() - 12.0, 24.0, 24.0);
    if state.close_hovered {
        p.rbox(close, 5.0, mix(pal.bar, pal.text, 0.10));
    }
    p.icon(glyph::CLOSE, 10.0, close.cx(), close.cy(), pal.muted);
    hits.close = Some(close);
    let mut right = close.x - 10.0;
    right -= bar_button_w(p, no);
    hits.no = Some(bar_button(
        p,
        pal,
        right,
        bar.cy(),
        no,
        false,
        state.no_hovered,
        state.no_pressed,
    ));
    right -= 8.0 + bar_button_w(p, yes);
    hits.yes = Some(bar_button(
        p,
        pal,
        right,
        bar.cy(),
        yes,
        true,
        state.yes_hovered,
        state.yes_pressed,
    ));
    right -= 12.0;
    p.icon(icon, 13.0, bar.x + 22.0, bar.cy(), pal.muted);
    p.text(
        Face::Ui,
        12.5,
        bar.x + 38.0,
        bar.cy(),
        text,
        pal.text,
        (right - (bar.x + 38.0)).max(0.0),
    );
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::Gfx;
    use mdshared_text::Bitmap;

    fn offer(p: &mut Painter, pal: &Palette, r: Rect) -> OfferBarHits {
        let state = OfferBarState::default();
        draw_offer_bar(p, pal, r, glyph::INFO, "A note", "Yes", "No", state)
    }

    /// A bar slides down: at 1 it is the plain bar, at 0 nothing, half-way only its lower half
    /// shows, in the top half of its place.
    #[test]
    fn a_bar_slides_down_into_its_place() {
        let pal = crate::theme::LIGHT;
        let bar = Rect::new(0.0, 40.0, 400.0, 44.0);
        let shot = |t: Option<f32>| {
            let mut bmp = Bitmap::new(400, 120);
            let mut gfx = Gfx::new();
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            p.fill(Rect::new(0.0, 0.0, 400.0, 120.0), pal.page);
            match t {
                Some(t) => {
                    draw_sliding(&mut p, bar, t, |p, r| offer(p, &pal, r));
                }
                None => {
                    offer(&mut p, &pal, bar);
                }
            }
            bmp.pixels
        };
        let blank = shot(Some(0.0));
        assert!(shot(Some(1.0)) == shot(None));
        assert!(blank == shot(Some(-1.0)));
        assert_eq!(bar_height(bar, 0.5), 22.0);
        let half = shot(Some(0.5));
        for y in 0..120u32 {
            for x in 0..400u32 {
                let o = ((y * 400 + x) * 4) as usize;
                if half[o..o + 4] != blank[o..o + 4] {
                    assert!(
                        (40..62).contains(&y),
                        "{x},{y} drawn outside the part that shows"
                    );
                }
            }
        }
        assert!(half != blank);
    }
}
