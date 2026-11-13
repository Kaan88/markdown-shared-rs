//! Bars across the top of the page: a text button for them, and the bar that offers a newer
//! version of the application. Pure drawing; the caller records the hits.

use crate::gfx::{Face, Painter, Rect, mix};
use crate::glyph;
use crate::theme::Palette;
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
}

/// Where the update bar's controls were drawn, for the caller's hits.
#[derive(Clone, Copy, Debug, Default)]
pub struct UpdateBarHits {
    /// The button that starts the update, while one is on offer.
    pub go: Option<Rect>,
    /// The close button; absent while an update downloads.
    pub close: Option<Rect>,
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
    let label = Updates::bar_label(app, stage);
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
