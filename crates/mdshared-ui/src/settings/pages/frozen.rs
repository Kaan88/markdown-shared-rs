//! A frozen copy of the paged form's drawing, `Form::draw` and `Form::draw_row` exactly as
//! pages.rs stood on main at 6ec1dee, before the opt-in motion (`draw_animated`, motion.rs)
//! was added beside them. Tests only: `the_paged_form_still_draws_what_it_drew` in
//! motion_tests.rs draws the live form and this copy through the same states and compares every
//! pixel, so a change to `draw` that nobody opted into fails there. Nothing in it is to be
//! edited; the only changes from the original are the two names (`draw_as_on_main`,
//! `draw_row_as_on_main`) and the imports.

#![allow(dead_code, clippy::all)]

use super::*;
use crate::glyph;
use crate::overlay::{draw_backdrop, draw_dialog_button, draw_dialog_button_disabled};
use crate::textfield::FieldColors;

impl Form {
    pub(super) fn draw_as_on_main(
        &mut self,
        p: &mut Painter,
        pal: &Colors,
        window: Rect,
        caret_on: bool,
    ) {
        self.layout(p, window);
        draw_backdrop(p, pal, window.w, window.h);
        let g = self.geo;
        let r = g.card;
        p.shadow(r, RADIUS, pal.dark);
        if pal.dark {
            p.shadow(r, RADIUS, true);
        }
        p.frame(r, RADIUS, pal.popup, pal.popup_line);
        let px = p.px().max(1.0);

        // The header: the title, the ×, a hairline under both columns.
        p.text(
            Face::Semibold,
            17.0,
            r.x + PAD,
            r.y + HEADER_H / 2.0,
            &self.title,
            pal.text,
            (g.close.x - r.x - PAD - 8.0).max(0.0),
        );
        let close_hover = self.hover == Some(Target::Close);
        if self.focus_visible && self.focus == Focus::Close {
            ring(p, pal, g.close, 6.0, pal.popup);
        }
        if close_hover {
            let fill = if self.pressed == Some(Target::Close) {
                pal.press
            } else {
                pal.hover
            };
            p.rbox(g.close, 6.0, fill);
        }
        p.icon(
            glyph::CLOSE,
            13.0,
            g.close.cx(),
            g.close.cy(),
            if close_hover { pal.text } else { pal.muted },
        );
        p.fill(
            Rect::new(r.x + 1.0, r.y + HEADER_H, r.w - 2.0, px),
            pal.popup_line,
        );
        p.fill(
            Rect::new(
                g.divider_x,
                r.y + HEADER_H,
                px,
                (r.h - HEADER_H - 1.0).max(0.0),
            ),
            pal.popup_line,
        );

        // The list.
        let old = p.clip(Rect::new(r.x, r.y + HEADER_H, g.divider_x - r.x, g.page.h));
        for c in 0..self.pages.len() {
            let it = g.item(c);
            let cur = c == self.current;
            if cur {
                if self.focus_visible && self.focus == Focus::List {
                    ring(p, pal, it, 6.0, pal.popup);
                }
                p.rbox(it, 6.0, pal.on);
                p.rbox(
                    Rect::new(it.x + MARK_INSET, it.cy() - MARK_H / 2.0, MARK_W, MARK_H),
                    MARK_W / 2.0,
                    pal.accent,
                );
            } else if self.hover == Some(Target::Category(c)) {
                p.rbox(it, 6.0, pal.hover);
            }
            p.text(
                Face::Ui,
                CONTROL_SIZE,
                it.x + NAV_TEXT,
                it.cy(),
                &self.pages[c].name,
                if cur { pal.text } else { pal.muted },
                it.w - NAV_TEXT - 8.0,
            );
        }
        p.restore(old);

        // The page.
        let old = p.clip(g.page);
        let n = self.placed.len();
        for i in 0..n {
            if self.placed[i].kind == Kind::Gap {
                continue;
            }
            let next_is_row = self
                .placed
                .get(i + 1)
                .is_some_and(|pl| pl.kind != Kind::Gap);
            self.draw_row_as_on_main(p, pal, i, next_is_row, caret_on);
        }
        p.restore(old);

        // An open dropdown, over everything.
        if let Some((_, menu)) = &mut self.menu {
            menu.draw_in(p, pal, window);
        }
    }

    fn draw_row_as_on_main(
        &mut self,
        p: &mut Painter,
        pal: &Colors,
        i: usize,
        next_is_row: bool,
        caret: bool,
    ) {
        let g = self.geo;
        let Some(it) = self.item(i).cloned() else {
            return;
        };
        let pl = self.placed[i].clone();
        let off = it.is_disabled();
        let px = p.px().max(1.0);
        if it.child {
            p.fill(
                Rect::new(g.l + 3.0, pl.y + 1.0, 2.0 * px, pl.h - 2.0),
                mix(pal.popup, pal.text, 0.14),
            );
        }
        if next_is_row {
            p.fill_a(
                Rect::new(pl.label_x, pl.y + pl.h - px, g.r - pl.label_x, px),
                pal.popup_line,
                0.6,
            );
        }
        p.text(
            Face::Ui,
            LABEL_SIZE,
            pl.label_x,
            pl.label_cy,
            it.row.label(),
            if off { pal.faint } else { pal.text },
            pl.label_w,
        );
        let max_lines = if pl.kind == Kind::Stacked {
            STACK_LINES
        } else {
            UNDER_LINES
        };
        for (k, line) in pl.help.iter().take(max_lines).enumerate() {
            p.text(
                Face::Ui,
                HELP_SIZE,
                pl.help_x,
                pl.help_cy + HELP_LINE * k as f32,
                line,
                pal.muted,
                pl.help_w,
            );
        }

        let focus = |f: Focus| self.focus == f;
        let ring_on = self.focus_visible && focus(Focus::Control(i));
        let hov = |t: Target| !off && self.hover == Some(t);
        let held = |t: Target| !off && self.hover == Some(t) && self.pressed == Some(t);

        // The row's own button.
        if let (Some(b), Some(br)) = (&it.button, pl.button) {
            let live = it.button_live();
            if self.focus_visible && focus(Focus::Button(i)) {
                ring(p, pal, br, 5.0, pal.popup);
            }
            let (h, pr) = (
                live && hov(Target::Button(i)),
                live && held(Target::Button(i)),
            );
            match b.icon {
                Some(icon) => {
                    if live {
                        draw_dialog_button(p, pal, br, "", false, h, pr);
                    } else {
                        draw_dialog_button_disabled(p, pal, br, "", false);
                    }
                    let c = if live { pal.text } else { pal.faint };
                    p.icon(icon, 14.0, br.cx(), br.cy(), c);
                }
                None if live => draw_dialog_button(p, pal, br, &b.text, false, h, pr),
                None => draw_dialog_button_disabled(p, pal, br, &b.text, false),
            }
        }

        let ctrl = pl.ctrl;
        let bg = pal.popup;
        match &it.row {
            Row::Toggle { value, .. } => {
                if ring_on {
                    ring(p, pal, ctrl, TOGGLE_H / 2.0, bg);
                }
                draw_toggle(
                    p,
                    pal,
                    ctrl,
                    *value,
                    hov(Target::Toggle(i)),
                    held(Target::Toggle(i)),
                );
                if off {
                    p.rbox_a(ctrl.inset(-1.0), TOGGLE_H / 2.0, pal.popup, 0.55);
                }
            }
            Row::Choice { options, value, .. } if pl.kind == Kind::Segmented => {
                if ring_on {
                    ring(p, pal, ctrl, 6.0, bg);
                }
                p.frame(ctrl, 6.0, pal.field, pal.line);
                for (k, o) in options.iter().enumerate() {
                    let s = self.segment(i, k);
                    let on = k == *value;
                    if on {
                        let fill = if off {
                            mix(pal.accent, pal.popup, 0.55)
                        } else {
                            pal.accent
                        };
                        p.rbox(s, 4.0, fill);
                    } else if hov(Target::Segment(i, k)) {
                        p.rbox(s, 4.0, pal.hover);
                    }
                    let c = if on {
                        pal.on_accent
                    } else if off {
                        pal.faint
                    } else {
                        pal.text
                    };
                    let face = if on { Face::Semibold } else { Face::Ui };
                    p.text_center(face, CONTROL_SIZE, s, o, c);
                }
            }
            Row::Choice { options, value, .. } => {
                if ring_on {
                    ring(p, pal, ctrl, 6.0, bg);
                }
                let open = matches!(&self.menu, Some((k, _)) if *k == i);
                let h = hov(Target::Dropdown(i)) || open;
                let shown = options.get(*value).map_or("", String::as_str);
                // The same field as the standalone drop-down's.
                crate::dropdown::field(p, pal, ctrl, shown, h, off);
            }
            Row::Number {
                value,
                min,
                max,
                unit,
                ..
            } => {
                if ring_on {
                    ring(p, pal, ctrl, 6.0, bg);
                }
                let (m, v, plus) = stepper_parts(ctrl);
                for (rect, gl, enabled, t) in [
                    (m, glyph::MINUS, value > min && !off, Target::Minus(i)),
                    (plus, glyph::PLUS, value < max && !off, Target::Plus(i)),
                ] {
                    let fill = if enabled && held(t) {
                        pal.press
                    } else if enabled && hov(t) {
                        pal.hover
                    } else {
                        mix(pal.popup, pal.text, 0.04)
                    };
                    p.frame(rect, 6.0, fill, pal.popup_line);
                    let c = if enabled { pal.text } else { pal.faint };
                    p.icon(gl, 11.0, rect.cx(), rect.cy(), c);
                }
                p.frame(v, 6.0, pal.field, pal.line);
                let c = if off { pal.faint } else { pal.text };
                p.text_center(Face::Ui, CONTROL_SIZE, v, &number_text(*value, unit), c);
            }
            Row::Text { placeholder, .. } => {
                let focused = focus(Focus::Control(i));
                let edge = if hov(Target::Text(i)) && !focused {
                    mix(pal.line, pal.text, 0.18)
                } else {
                    pal.line
                };
                p.frame(ctrl, 6.0, pal.field, edge);
                if focused {
                    // The accent underline of the find bar's box, following the corners.
                    let old = p.clip(Rect::new(ctrl.x, ctrl.bottom() - 2.0, ctrl.w, 2.0));
                    p.rbox(ctrl, 6.0, pal.accent);
                    p.restore(old);
                }
                let colors = FieldColors {
                    text: if off { pal.faint } else { pal.text },
                    placeholder: pal.faint,
                    selection: pal.selection,
                    caret: pal.text,
                };
                let area = Rect::new(ctrl.x + 10.0, ctrl.y, ctrl.w - 20.0, ctrl.h);
                let c = self.current;
                self.slots[c][i].field.draw(
                    p,
                    area,
                    Face::Ui,
                    CONTROL_SIZE,
                    &colors,
                    placeholder,
                    focused,
                    caret,
                );
            }
            Row::Action { button, .. } => {
                if ring_on {
                    ring(p, pal, ctrl, 6.0, bg);
                }
                if off {
                    draw_dialog_button_disabled(p, pal, ctrl, button, false);
                } else {
                    let t = Target::Action(i);
                    draw_dialog_button(p, pal, ctrl, button, false, hov(t), held(t));
                }
            }
            Row::Header(_) => {}
        }
    }
}
