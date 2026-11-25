//! Things that appear and disappear: a menu, a dialog, a toast, a tooltip.
//!
//! Two problems, two pieces. A thing that leaves has to be drawn for a few frames after the
//! application has let go of it: [`Transient`] holds it until its exit has played. And a thing
//! that enters or leaves is drawn faded and a little out of place, whatever it is made of:
//! [`pop`] wraps any drawing in that.

use super::{Animator, AsId, Motion, fluent};
use crate::gfx::{Painter, Rect};

/// One slot for a thing that comes and goes (the toast, the tooltip, the open menu), which keeps
/// the thing alive while it leaves.
///
/// The application's logic calls [`show`](Self::show) and [`hide`](Self::hide) and asks
/// [`current`](Self::current) for what is logically there (what takes clicks and keys). Its
/// drawing asks [`frame`](Self::frame) each frame, which gives the thing and how present it is,
/// also for the frames after `hide` in which it is still fading out, and drops it once it is
/// gone. With reduced motion `frame` gives the thing at 1 until it is hidden and nothing after.
#[derive(Clone, Debug)]
pub struct Transient<T> {
    item: Option<T>,
    shown: bool,
    enter: Motion,
    exit: Motion,
}

impl<T> Default for Transient<T> {
    fn default() -> Transient<T> {
        Transient::new()
    }
}

impl<T> Transient<T> {
    /// An empty slot whose things enter by [`fluent::ENTER`] and leave by [`fluent::EXIT`].
    pub const fn new() -> Transient<T> {
        Transient {
            item: None,
            shown: false,
            enter: fluent::ENTER,
            exit: fluent::EXIT,
        }
    }

    /// The same slot with other motions.
    pub fn with_motion(mut self, enter: Motion, exit: Motion) -> Transient<T> {
        self.enter = enter;
        self.exit = exit;
        self
    }

    /// Shows `item`. Over a thing that is showing it takes its place where it stands, with no
    /// new entrance; over one that is leaving it comes back in from where that one had got to.
    pub fn show(&mut self, item: T) {
        self.item = Some(item);
        self.shown = true;
    }

    /// Lets the thing go: it is no longer [`current`](Self::current), and
    /// [`frame`](Self::frame) plays its exit.
    pub fn hide(&mut self) {
        self.shown = false;
    }

    /// Drops the thing at once, with no exit.
    pub fn clear(&mut self) {
        self.item = None;
        self.shown = false;
    }

    /// Whether a thing is logically there.
    pub fn is_shown(&self) -> bool {
        self.shown && self.item.is_some()
    }

    /// The thing that is logically there: not one that is leaving.
    pub fn current(&self) -> Option<&T> {
        self.item.as_ref().filter(|_| self.shown)
    }

    pub fn current_mut(&mut self) -> Option<&mut T> {
        self.item.as_mut().filter(|_| self.shown)
    }

    /// The thing that is there or still leaving.
    pub fn alive(&self) -> Option<&T> {
        self.item.as_ref()
    }

    /// The thing to draw this frame and how present it is (above 0, up to 1), or `None` when
    /// there is nothing to draw. `id` names the presence in the animator. An empty slot asks
    /// the animator nothing.
    pub fn frame(&mut self, anim: &mut Animator, id: impl AsId) -> Option<(&T, f32)> {
        self.item.as_ref()?;
        let t = anim.presence(id, self.shown, self.enter, self.exit);
        if !self.shown && t <= 0.0 {
            self.item = None;
            return None;
        }
        self.item.as_ref().map(|item| (item, t))
    }
}

/// Everything [`pop`] touches for `bounds` and `shift` at any `t`: `bounds` at rest and where
/// the shift moves it, in one, grown by 2 logical px for the device pixel it saves round that
/// (a device pixel is at most 2 logical px down to a scale of 0.5). The rectangle an application
/// that redraws only what changed marks as damaged while something enters or leaves, and once
/// more on the frame after it went.
pub fn pop_bounds(bounds: Rect, shift: (f32, f32)) -> Rect {
    let (dx, dy) = shift;
    Rect::new(
        bounds.x + dx.min(0.0),
        bounds.y + dy.min(0.0),
        bounds.w + dx.abs(),
        bounds.h + dy.abs(),
    )
    .inset(-2.0)
}

/// Draws something that is entering or leaving: what `draw` draws, at opacity `t` as a whole and
/// moved by `shift` (logical px) times what is left of the way, `1 - t`. `t` is a presence from
/// [`Animator::presence`] or [`Transient::frame`]: at 1 `draw` runs as if it were called
/// directly (the same pixels, at no cost), at 0 it does not run at all.
///
/// `bounds` is everything `draw` touches, its shadow included; only that rectangle (and where
/// the shift moves it) is saved and blended. A shift of `(0.0, 8.0)` makes the thing rise 8 px
/// into place as it appears and sink as it goes; `(0.0, 0.0)` is a plain fade. The move is in
/// whole device pixels, so text stays as sharp on the way as at rest.
///
/// The move is [`Painter::offset`], which moves what is drawn in logical px. It does not move
/// what `draw` puts down in device px ([`Painter::fill_dev`], [`Painter::blit`]): those calls
/// say where in the bitmap, so such a part fades with the rest but stays in its place while the
/// rest moves. A `draw` that uses them adds [`Painter::origin`] to its device coordinates, or
/// is given no shift.
///
/// There is no scaling: a popup that grows from 95 % would need its drawing resampled, which
/// costs more than it shows at these sizes and times. The Windows popups move and fade.
pub fn pop(
    p: &mut Painter,
    bounds: Rect,
    t: f32,
    shift: (f32, f32),
    draw: impl FnOnce(&mut Painter),
) {
    if t.is_nan() || t >= 1.0 {
        return draw(p);
    }
    if t <= 0.0 {
        return;
    }
    let (dx, dy) = (shift.0 * (1.0 - t), shift.1 * (1.0 - t));
    // The rectangle at rest and where it is drawn now, in one.
    let covered = Rect::new(
        bounds.x + dx.min(0.0),
        bounds.y + dy.min(0.0),
        bounds.w + dx.abs(),
        bounds.h + dy.abs(),
    );
    let under = p.snapshot(covered.inset(-p.px()));
    let origin = p.offset(dx, dy);
    draw(p);
    p.restore_offset(origin);
    p.fade(&under, t);
}

/// How opaque an unfolding thing is when it starts: it opens from its top edge already mostly
/// there, as the owner's prototype has it (`opacity: .6` with the clip), so it reads as unfolding
/// rather than fading in.
pub const UNFOLD_FROM: f32 = 0.6;

/// Draws something unfolding from its top edge: what `draw` draws, cut off below a line that
/// runs down `rect` as `t` goes from 0 to 1, at an opacity from [`UNFOLD_FROM`] up to 1. A menu
/// hanging from the control that opens it enters this way. `rect` is the thing itself; `bounds`
/// is everything `draw` touches, its shadow included (saved and blended, as in [`pop`]), and the
/// shadow beside the thing shows as it unfolds, the shadow below it only at the end. At 1 `draw`
/// runs as if called directly; at 0 (or below) it does not run.
///
/// For what opens upward (a menu above its control), [`pop`] with a rise is the entrance; this
/// opens downward only.
pub fn unfold(p: &mut Painter, rect: Rect, bounds: Rect, t: f32, draw: impl FnOnce(&mut Painter)) {
    if t.is_nan() || t >= 1.0 {
        return draw(p);
    }
    if t <= 0.0 {
        return;
    }
    let under = p.snapshot(bounds.inset(-p.px()));
    let bottom = rect.y + rect.h * t;
    let shown = Rect::new(bounds.x, bounds.y, bounds.w, (bottom - bounds.y).max(0.0));
    let clip = p.clip(shown);
    draw(p);
    p.restore(clip);
    p.fade(&under, UNFOLD_FROM + (1.0 - UNFOLD_FROM) * t);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::{Clock, Easing, ManualClock};
    use crate::gfx::Gfx;
    use mdshared_text::Bitmap;

    const LINEAR_100: Motion = Motion::tween_ms(100, Easing::Linear);

    /// Every pixel `pop` changes, at any `t` and either sign of shift, is inside `pop_bounds`.
    #[test]
    fn pop_touches_nothing_outside_its_bounds() {
        let bounds = Rect::new(60.0, 50.0, 80.0, 40.0);
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for shift in [(0.0, 8.0), (0.0, -6.0), (6.0, 0.0), (-6.0, 4.0)] {
                for t in [0.05, 0.3, 0.5, 0.9] {
                    let (w, h) = ((200.0 * scale) as u32, (150.0 * scale) as u32);
                    let mut bmp = Bitmap::new(w, h);
                    let mut gfx = Gfx::new();
                    let mut p = Painter::new(&mut bmp, &mut gfx, scale);
                    p.fill(Rect::new(0.0, 0.0, 200.0, 150.0), crate::gfx::rgb(0x336699));
                    let clean = bmp.pixels.clone();
                    let mut p = Painter::new(&mut bmp, &mut gfx, scale);
                    pop(&mut p, bounds, t, shift, |p| {
                        p.fill(bounds, crate::gfx::rgb(0xffcc00))
                    });
                    let out = pop_bounds(bounds, shift);
                    for y in 0..h {
                        for x in 0..w {
                            let o = ((y * w + x) * 4) as usize;
                            if bmp.pixels[o..o + 4] != clean[o..o + 4] {
                                let (lx, ly) = (x as f32 / scale, y as f32 / scale);
                                assert!(
                                    out.contains(lx, ly),
                                    "{lx},{ly} changed outside {out:?} (scale {scale}, shift {shift:?}, t {t})"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    fn at(bmp: &Bitmap, x: u32, y: u32) -> [u8; 3] {
        let o = ((y * bmp.width + x) * 4) as usize;
        [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
    }

    #[test]
    fn a_transient_keeps_its_thing_until_the_exit_has_played() {
        let clock = ManualClock::new();
        let mut anim = Animator::new();
        // The steps below are longer than the stall guard allows.
        anim.set_max_step(std::time::Duration::from_secs(1));
        let mut toast: Transient<&'static str> =
            Transient::new().with_motion(LINEAR_100, LINEAR_100);
        let mut frame = |toast: &mut Transient<&'static str>, ms: u64| {
            clock.advance_ms(ms);
            anim.begin_frame(clock.now());
            let drawn = toast.frame(&mut anim, "toast").map(|(s, t)| (*s, t));
            (drawn, anim.end_frame().is_some(), anim.len())
        };
        // Empty: nothing to draw, nothing asked of the animator, no frame.
        assert_eq!(frame(&mut toast, 0), (None, false, 0));
        assert!(!toast.is_shown() && toast.current().is_none());

        toast.show("Saved");
        assert_eq!(toast.current(), Some(&"Saved"));
        assert_eq!(frame(&mut toast, 0), (Some(("Saved", 0.0)), true, 1));
        let (drawn, more, _) = frame(&mut toast, 50);
        assert_eq!(drawn.unwrap().0, "Saved");
        assert!((drawn.unwrap().1 - 0.5).abs() < 1e-3 && more);
        assert_eq!(frame(&mut toast, 50), (Some(("Saved", 1.0)), false, 1));
        // Replaced while showing: the new text, no new entrance.
        toast.show("Copied");
        assert_eq!(frame(&mut toast, 500), (Some(("Copied", 1.0)), false, 1));

        // Let go: no longer there for the logic, still drawn while it leaves.
        toast.hide();
        assert!(!toast.is_shown() && toast.current().is_none());
        assert_eq!(toast.alive(), Some(&"Copied"));
        assert_eq!(frame(&mut toast, 0), (Some(("Copied", 1.0)), true, 1));
        let (drawn, more, _) = frame(&mut toast, 60);
        assert!((drawn.unwrap().1 - 0.4).abs() < 1e-3 && more);
        // Shown again on its way out: it comes back from where it was.
        toast.show("Again");
        let (drawn, _, _) = frame(&mut toast, 10);
        assert_eq!(drawn.unwrap().0, "Again");
        assert!((drawn.unwrap().1 - 0.3).abs() < 1e-3);
        toast.hide();
        frame(&mut toast, 10);
        // Gone: dropped, and the frame after that the animator has forgotten it.
        assert_eq!(frame(&mut toast, 200), (None, false, 1));
        assert!(toast.alive().is_none());
        assert_eq!(frame(&mut toast, 10), (None, false, 0));

        toast.show("Once more");
        if let Some(text) = toast.current_mut() {
            *text = "Changed";
        }
        assert_eq!(toast.alive(), Some(&"Changed"));
        toast.clear();
        assert!(toast.alive().is_none());
    }

    #[test]
    fn with_reduced_motion_a_transient_is_there_or_not() {
        let clock = ManualClock::new();
        let mut anim = Animator::new();
        anim.set_system_reduced(true);
        let mut tip: Transient<u8> = Transient::default();
        tip.show(7);
        anim.begin_frame(clock.now());
        assert_eq!(tip.frame(&mut anim, "tip"), Some((&7, 1.0)));
        assert_eq!(anim.end_frame(), None);
        tip.hide();
        anim.begin_frame(clock.now());
        assert_eq!(tip.frame(&mut anim, "tip"), None);
        assert_eq!(anim.end_frame(), None);
    }

    /// A white window with a black box drawn through `pop` at presence `t`.
    fn popped(t: f32, shift: (f32, f32)) -> (Bitmap, bool) {
        let mut bmp = Bitmap::new(40, 40);
        let mut gfx = Gfx::new();
        let mut ran = false;
        {
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            p.fill(Rect::new(0.0, 0.0, 40.0, 40.0), [255, 255, 255]);
            let r = Rect::new(10.0, 10.0, 10.0, 10.0);
            pop(&mut p, r, t, shift, |p| {
                ran = true;
                p.fill(r, [0, 0, 0]);
            });
            // The painter is given back as it was.
            p.fill(Rect::new(30.0, 30.0, 2.0, 2.0), [1, 2, 3]);
        }
        assert_eq!(at(&bmp, 30, 30), [1, 2, 3]);
        (bmp, ran)
    }

    #[test]
    fn pop_fades_the_drawing_as_a_whole_and_moves_it_into_place() {
        // There: exactly the direct drawing.
        let (full, ran) = popped(1.0, (0.0, 8.0));
        assert!(ran);
        assert_eq!(at(&full, 15, 15), [0, 0, 0]);
        assert_eq!(at(&full, 15, 9), [255, 255, 255]);
        assert_eq!(at(&full, 15, 20), [255, 255, 255]);
        // Gone: nothing drawn, and the drawing not even run.
        let (none, ran) = popped(0.0, (0.0, 8.0));
        assert!(!ran);
        assert_eq!(at(&none, 15, 15), [255, 255, 255]);
        // Half way: half opaque, and 4 px below its place.
        let (half, _) = popped(0.5, (0.0, 8.0));
        assert_eq!(at(&half, 15, 12), [255, 255, 255]);
        assert_eq!(at(&half, 15, 14), [128, 128, 128]);
        assert_eq!(at(&half, 15, 23), [128, 128, 128]);
        assert_eq!(at(&half, 15, 24), [255, 255, 255]);
        // A plain fade, and a shift to the left.
        let (fade, _) = popped(0.25, (0.0, 0.0));
        assert_eq!(at(&fade, 10, 10), [191, 191, 191]);
        assert_eq!(at(&fade, 9, 10), [255, 255, 255]);
        let (left, _) = popped(0.5, (-6.0, 0.0));
        assert_eq!(at(&left, 7, 15), [128, 128, 128]);
        assert_eq!(at(&left, 17, 15), [255, 255, 255]);
    }

    /// A white window with a black 10x20 box (its "shadow" a grey frame 4 px round it) drawn
    /// through `unfold` at `t`.
    fn unfolded(t: f32) -> (Bitmap, bool) {
        let mut bmp = Bitmap::new(40, 40);
        let mut gfx = Gfx::new();
        let mut ran = false;
        {
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            p.fill(Rect::new(0.0, 0.0, 40.0, 40.0), [255, 255, 255]);
            let r = Rect::new(10.0, 10.0, 10.0, 20.0);
            let bounds = r.inset(-4.0);
            unfold(&mut p, r, bounds, t, |p| {
                ran = true;
                p.fill(bounds, [200, 200, 200]);
                p.fill(r, [0, 0, 0]);
            });
            // The clip is given back.
            p.fill(Rect::new(36.0, 36.0, 2.0, 2.0), [1, 2, 3]);
        }
        assert_eq!(at(&bmp, 36, 36), [1, 2, 3]);
        (bmp, ran)
    }

    #[test]
    fn unfold_opens_from_the_top_edge_already_mostly_there() {
        let (full, ran) = unfolded(1.0);
        assert!(ran);
        assert_eq!(at(&full, 15, 29), [0, 0, 0]);
        assert_eq!(
            at(&full, 15, 32),
            [200, 200, 200],
            "the shadow below, at the end"
        );
        let (none, ran) = unfolded(0.0);
        assert!(!ran);
        assert_eq!(at(&none, 15, 15), [255, 255, 255]);
        // Half way: the top half shown at 0.8, the rest not yet, nor the shadow below it.
        let (half, _) = unfolded(0.5);
        let k = |c: u8| (255.0 - (255.0 - c as f32) * 0.8).round() as u8;
        assert_eq!(at(&half, 15, 10), [k(0); 3]);
        assert_eq!(at(&half, 15, 19), [k(0); 3]);
        assert_eq!(at(&half, 15, 20), [255, 255, 255]);
        assert_eq!(at(&half, 15, 32), [255, 255, 255]);
        assert_eq!(
            at(&half, 7, 15),
            [k(200); 3],
            "the shadow beside the shown part"
        );
        assert_eq!(at(&half, 7, 25), [255, 255, 255]);
    }
}
