//! A tick box: on, off, and a quiet look for a tick that cannot be changed, with a focus mark
//! that has to know nothing of what is behind the box. Drawing only; the caller owns the value,
//! the hit and the keyboard.

use crate::gfx::{Painter, Rect, mix};
use crate::glyph;
use crate::theme::Palette;

/// The side of a tick box at the size the editor's dialogs and its replace-in-files list use.
pub const TICK: f32 = 16.0;
const RADIUS: f32 = 4.0;
const CHECK: f32 = 11.0;

/// What a tick box shows: its value, whether a click could change it, and whether the keyboard is
/// on it. It holds no hit and nothing records one: a caller that draws a disabled box also keeps
/// it out of its own hits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tick {
    /// Ticked.
    pub on: bool,
    /// A click changes it. Disabled, the box draws quiet whichever way it is set.
    pub enabled: bool,
    /// The keyboard is on it. The box marks that on its own edge, so it needs no room round it.
    pub focused: bool,
}

impl Default for Tick {
    fn default() -> Tick {
        Tick::new(false)
    }
}

impl Tick {
    /// A tick a click can change; `on` is its value.
    pub fn new(on: bool) -> Tick {
        Tick {
            on,
            enabled: true,
            focused: false,
        }
    }

    /// A tick that cannot be changed; `on` is what it keeps showing.
    pub fn disabled(on: bool) -> Tick {
        Tick {
            on,
            enabled: false,
            focused: false,
        }
    }

    /// The same tick with the keyboard on it. A disabled tick takes no keyboard, so this marks
    /// nothing there.
    pub fn with_focus(self) -> Tick {
        Tick {
            focused: true,
            ..self
        }
    }
}

/// Draws a tick box in `r`. An on box is the accent fill with the check on it; an off box is the
/// field with an edge; a disabled box is quiet either way; with the keyboard on it the box's own
/// edge turns to the accent, or, for a box already filled with the accent, a lighter line runs
/// inside it. It draws nothing outside `r` and changes no state.
pub fn draw_tick(p: &mut Painter, pal: &Palette, r: Rect, tick: Tick) {
    if tick.on {
        let fill = if tick.enabled {
            pal.accent
        } else {
            mix(pal.field, pal.accent, 0.4)
        };
        p.rbox(r, RADIUS, fill);
        if tick.focused && tick.enabled {
            p.frame(r.inset(1.0), 3.0, fill, mix(pal.accent, pal.on_accent, 0.6));
        }
        p.icon(
            glyph::CHECK,
            CHECK,
            r.cx(),
            r.cy(),
            if tick.enabled {
                pal.on_accent
            } else {
                pal.faint
            },
        );
    } else {
        let edge = if !tick.enabled {
            pal.line
        } else if tick.focused {
            pal.accent
        } else {
            mix(pal.line, pal.text, 0.3)
        };
        p.frame(r, RADIUS, pal.field, edge);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfx::Gfx;
    use crate::theme::{DARK, LIGHT};
    use mdshared_text::Bitmap;

    /// A tick box on a dialog's popup, as the editor draws it in its confirmation.
    fn drawn(pal: &Palette, tick: Tick) -> Bitmap {
        let mut bmp = Bitmap::new(40, 40);
        let mut gfx = Gfx::new();
        {
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            p.fill(Rect::new(0.0, 0.0, 40.0, 40.0), pal.popup);
            draw_tick(&mut p, pal, Rect::new(12.0, 12.0, TICK, TICK), tick);
        }
        bmp
    }

    #[test]
    fn the_three_states_of_a_tick_box_draw_differently() {
        for pal in [&LIGHT, &DARK] {
            let off = drawn(pal, Tick::new(false)).pixels;
            let on = drawn(pal, Tick::new(true)).pixels;
            let quiet = drawn(pal, Tick::disabled(false)).pixels;
            let quiet_on = drawn(pal, Tick::disabled(true)).pixels;
            assert_ne!(off, on, "off and on look the same");
            assert_ne!(off, quiet, "a disabled box looks like an off one");
            assert_ne!(on, quiet_on, "a disabled tick looks like an enabled one");
            assert_ne!(quiet, quiet_on, "a disabled box hides its value");
            // A tick box changes only its own square and no more.
            let blank = {
                let mut bmp = Bitmap::new(40, 40);
                let mut gfx = Gfx::new();
                Painter::new(&mut bmp, &mut gfx, 1.0)
                    .fill(Rect::new(0.0, 0.0, 40.0, 40.0), pal.popup);
                bmp.pixels
            };
            for (name, px) in [("off", &off), ("on", &on), ("disabled", &quiet)] {
                let outside = (0..40).any(|y| {
                    (0..40).any(|x| {
                        let inside = (12..28).contains(&x) && (12..28).contains(&y);
                        !inside
                            && px[(y * 40 + x) * 4..(y * 40 + x) * 4 + 4]
                                != blank[(y * 40 + x) * 4..(y * 40 + x) * 4 + 4]
                    })
                });
                assert!(!outside, "a {name} box drew outside its rectangle");
            }
        }
    }

    #[test]
    fn the_keyboard_on_a_tick_box_shows_and_a_disabled_one_shows_nothing_of_it() {
        for pal in [&LIGHT, &DARK] {
            for on in [false, true] {
                let plain = drawn(pal, Tick::new(on)).pixels;
                let focused = drawn(pal, Tick::new(on).with_focus()).pixels;
                assert_ne!(plain, focused, "the keyboard leaves no mark, on {on}");
                let quiet = drawn(pal, Tick::disabled(on)).pixels;
                let quiet_focus = drawn(pal, Tick::disabled(on).with_focus()).pixels;
                assert_eq!(quiet, quiet_focus, "a disabled box took the keyboard");
            }
        }
    }

    /// `replace_ui::tick_box` as the editor draws it today, kept here as the shape this box must
    /// keep: adopting the shared one changes no pixel of the Replace-in-files screen.
    fn editor_tick(p: &mut Painter, pal: &Palette, r: Rect, on: bool) {
        if on {
            p.rbox(r, 4.0, pal.accent);
            p.icon(glyph::CHECK, 11.0, r.cx(), r.cy(), pal.on_accent);
        } else {
            p.frame(r, 4.0, pal.field, mix(pal.line, pal.text, 0.3));
        }
    }

    #[test]
    fn the_tick_box_draws_as_the_editor_draws_it_today() {
        for pal in [&LIGHT, &DARK] {
            for on in [false, true] {
                let mut before = Bitmap::new(40, 40);
                let mut gfx = Gfx::new();
                let mut p = Painter::new(&mut before, &mut gfx, 1.0);
                p.fill(Rect::new(0.0, 0.0, 40.0, 40.0), pal.popup);
                editor_tick(&mut p, pal, Rect::new(12.0, 12.0, TICK, TICK), on);
                assert_eq!(
                    before.pixels,
                    drawn(pal, Tick::new(on)).pixels,
                    "on {on}: the shared box draws elsewhere"
                );
            }
        }
    }
}
