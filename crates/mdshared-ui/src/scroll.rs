//! The overlay scrollbar: geometry (pure functions of rectangles and lengths) and drawing; and
//! a scroll offset that glides to where the wheel sent it.

use crate::anim::{Animator, Id, Motion, Spring, fluent};
use crate::gfx::{Painter, Rect};
use crate::theme::Palette;
use std::hash::Hash;

/// The track and thumb of an overlay scrollbar for `area`, or `None` when everything fits.
/// `content`, `view` and `offset` may be in any one unit.
pub fn scrollbar(area: Rect, content: f32, view: f32, offset: f32) -> Option<(Rect, Rect)> {
    if content <= view + 0.5 || area.h < 24.0 {
        return None;
    }
    let track = Rect::new(area.right() - 12.0, area.y + 2.0, 12.0, area.h - 4.0);
    let len = (track.h * view / content).clamp(28.0_f32.min(track.h), track.h);
    let t = (offset / (content - view)).clamp(0.0, 1.0);
    let thumb = Rect::new(track.x, track.y + t * (track.h - len), track.w, len);
    Some((track, thumb))
}

/// The scroll offset that puts the thumb's top at `thumb_y`.
pub fn offset_for_thumb(track: Rect, thumb_h: f32, thumb_y: f32, content: f32, view: f32) -> f32 {
    let span = (track.h - thumb_h).max(1.0);
    ((thumb_y - track.y) / span).clamp(0.0, 1.0) * (content - view).max(0.0)
}

/// Thin at rest, wider while hovered or dragged.
pub fn draw_scrollbar(p: &mut Painter, pal: &Palette, track: Rect, thumb: Rect, active: bool) {
    let w = if active { 8.0 } else { 3.0 };
    if active {
        p.rbox_a(
            Rect::new(track.right() - w - 3.0, track.y, w, track.h),
            w / 2.0,
            pal.thumb,
            0.10,
        );
    }
    let r = Rect::new(track.right() - w - 3.0, thumb.y, w, thumb.h);
    p.rbox_a(r, w / 2.0, pal.thumb, if active { 0.85 } else { 0.55 });
}

/// [`draw_scrollbar`] with motion: the thumb widens and the track fades in over a few frames
/// when the bar becomes `active`, and back. `id` names this scrollbar in `anim`. At rest, and
/// on every frame when the animator does not animate, it draws exactly what [`draw_scrollbar`]
/// draws.
pub fn draw_scrollbar_animated(
    p: &mut Painter,
    pal: &Palette,
    anim: &mut Animator,
    id: impl Hash,
    track: Rect,
    thumb: Rect,
    active: bool,
) {
    let t = anim.toggle(id, active, fluent::FADE);
    if t <= 0.0 || t >= 1.0 {
        return draw_scrollbar(p, pal, track, thumb, t >= 1.0);
    }
    let w = 3.0 + 5.0 * t;
    p.rbox_a(
        Rect::new(track.right() - w - 3.0, track.y, w, track.h),
        w / 2.0,
        pal.thumb,
        0.10 * t,
    );
    let r = Rect::new(track.right() - w - 3.0, thumb.y, w, thumb.h);
    p.rbox_a(r, w / 2.0, pal.thumb, 0.55 + 0.30 * t);
}

/// A scroll offset that glides: the wheel and the keyboard move a target, and what is drawn
/// follows it over a few frames.
///
/// The application keeps one beside each thing that scrolls, in place of the bare offset. Its
/// input handlers call [`scroll_by`](Self::scroll_by) (a wheel notch, an arrow key, Page Down)
/// or [`scroll_to`](Self::scroll_to) (bring a line into view), and [`jump_to`](Self::jump_to)
/// for what must not glide: a thumb that is being dragged, a document opened at a heading. Its
/// drawing asks [`offset`](Self::offset) once a frame and draws there. Everything is in one
/// unit of the caller's choice, as the functions above are.
///
/// It glides on a critically damped spring, so a wheel that keeps turning gives one smooth
/// run, not a start and stop a notch: each notch moves the target and the offset keeps its
/// speed. With reduced motion the offset is the target on every frame, which is what an
/// application without this type has today.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SmoothScroll {
    target: f32,
    /// The next `offset` goes straight to the target.
    jump: bool,
    motion: Motion,
}

impl Default for SmoothScroll {
    fn default() -> SmoothScroll {
        SmoothScroll::new()
    }
}

impl SmoothScroll {
    /// At the top, gliding on [`Spring::SNAPPY`] (it arrives within a quarter of a pixel in
    /// about a sixth of a second).
    pub const fn new() -> SmoothScroll {
        SmoothScroll {
            target: 0.0,
            jump: true,
            motion: Motion::spring(Spring::SNAPPY.with_threshold(0.25)),
        }
    }

    /// The same with another motion: a tween for a fixed time a step, a softer spring.
    pub const fn with_motion(mut self, motion: Motion) -> SmoothScroll {
        self.motion = motion;
        self
    }

    /// Where the offset is going: the offset once it has come to rest. What scrolling logic
    /// that asks "what is in view" should use, so that a second key press acts on where the
    /// first one leads.
    pub fn target(&self) -> f32 {
        self.target
    }

    /// The furthest offset: `content` longer than `view` by this much, or zero.
    fn max(content: f32, view: f32) -> f32 {
        (content - view).max(0.0)
    }

    /// Moves the target by `delta`, kept inside what `content` and `view` allow. Returns
    /// whether it moved (if not, the wheel event is one for something else: the caller's
    /// parent).
    pub fn scroll_by(&mut self, delta: f32, content: f32, view: f32) -> bool {
        self.scroll_to(self.target + delta, content, view)
    }

    /// Moves the target to `offset`, kept inside what `content` and `view` allow, to glide
    /// there. Returns whether it moved.
    pub fn scroll_to(&mut self, offset: f32, content: f32, view: f32) -> bool {
        let next = offset.clamp(0.0, SmoothScroll::max(content, view));
        // A target that is not a number stays where it was.
        let moved = next != self.target && !next.is_nan();
        if moved {
            self.target = next;
        }
        moved
    }

    /// Puts the offset at `offset` at once, with no glide.
    pub fn jump_to(&mut self, offset: f32, content: f32, view: f32) {
        self.scroll_to(offset, content, view);
        self.jump = true;
    }

    /// The offset to draw at this frame. `id` names it in `anim`. `content` and `view` are
    /// this frame's: a target left past the end by content that shrank is pulled in (and
    /// glides there), and the offset given is never outside them.
    pub fn offset(&mut self, anim: &mut Animator, id: impl Hash, content: f32, view: f32) -> f32 {
        let max = SmoothScroll::max(content, view);
        self.target = self.target.clamp(0.0, max);
        let id = Id::new(id);
        if std::mem::take(&mut self.jump) {
            anim.set(id, self.target);
        }
        anim.value(id, self.target, self.motion).clamp(0.0, max)
    }

    /// Whether the offset has come to rest at its target (as of the last frame that asked).
    pub fn at_rest(&self, anim: &Animator, id: impl Hash) -> bool {
        let id = Id::new(id);
        !self.jump && anim.settled(id) && anim.get::<f32>(id) == Some(self.target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::{Clock, ManualClock};
    use crate::gfx::Gfx;
    use crate::theme::LIGHT;
    use mdshared_text::Bitmap;

    /// A view 400 high over content 2000 long, scrolled through an animator on a clock moved
    /// by hand.
    struct Rig {
        clock: ManualClock,
        anim: Animator,
        scroll: SmoothScroll,
    }

    impl Rig {
        fn new() -> Rig {
            Rig {
                clock: ManualClock::new(),
                anim: Animator::new(),
                scroll: SmoothScroll::new(),
            }
        }

        /// One frame `ms` later: the offset drawn, and whether another frame is asked for.
        fn frame(&mut self, ms: u64, content: f32) -> (f32, bool) {
            self.clock.advance_ms(ms);
            self.anim.begin_frame(self.clock.now());
            let y = self.scroll.offset(&mut self.anim, "page", content, 400.0);
            (y, self.anim.end_frame().is_some())
        }
    }

    #[test]
    fn a_wheel_notch_glides_to_its_target_and_then_the_window_sleeps() {
        let mut r = Rig::new();
        assert_eq!(r.frame(0, 2000.0), (0.0, false));
        assert!(r.scroll.at_rest(&r.anim, "page"));
        assert!(r.scroll.scroll_by(120.0, 2000.0, 400.0));
        assert_eq!(r.scroll.target(), 120.0);
        assert!(!r.scroll.at_rest(&r.anim, "page"));
        // The first frame is still at the start; then it only moves down, never past.
        assert_eq!(r.frame(0, 2000.0), (0.0, true));
        let mut last = 0.0;
        let mut frames = 0;
        loop {
            let (y, more) = r.frame(16, 2000.0);
            assert!(y >= last && y <= 120.0, "{y} after {last}");
            last = y;
            frames += 1;
            if !more {
                break;
            }
            assert!(frames < 60, "it never arrives");
        }
        assert_eq!(last, 120.0);
        assert!((5..30).contains(&frames), "{frames} frames");
        assert!(r.scroll.at_rest(&r.anim, "page"));
        assert_eq!(r.frame(1000, 2000.0), (120.0, false));
    }

    #[test]
    fn notches_in_a_row_keep_the_speed_and_add_up() {
        let mut r = Rig::new();
        r.frame(0, 2000.0);
        r.scroll.scroll_by(120.0, 2000.0, 400.0);
        r.frame(0, 2000.0);
        let (a, _) = r.frame(16, 2000.0);
        let (b, _) = r.frame(16, 2000.0);
        // A second notch on the way: the target is the sum, and the step does not drop to
        // nothing as a motion started again from rest would.
        r.scroll.scroll_by(120.0, 2000.0, 400.0);
        assert_eq!(r.scroll.target(), 240.0);
        let (c, _) = r.frame(16, 2000.0);
        let (d, _) = r.frame(16, 2000.0);
        assert!(c - b > 0.0 && d - c >= (b - a) * 0.9, "{a} {b} {c} {d}");
        let mut y = d;
        for _ in 0..100 {
            y = r.frame(16, 2000.0).0;
        }
        assert_eq!(y, 240.0);
    }

    #[test]
    fn the_ends_stop_the_target_and_a_jump_does_not_glide() {
        let mut r = Rig::new();
        r.frame(0, 2000.0);
        // Up from the top: nothing moves, and the caller is told.
        assert!(!r.scroll.scroll_by(-120.0, 2000.0, 400.0));
        assert!(r.scroll.scroll_by(99_999.0, 2000.0, 400.0));
        assert_eq!(r.scroll.target(), 1600.0);
        assert!(!r.scroll.scroll_by(120.0, 2000.0, 400.0));
        assert!(!r.scroll.scroll_to(f32::NAN, 2000.0, 400.0));
        // Content that fits does not scroll.
        assert!(!SmoothScroll::new().scroll_by(50.0, 300.0, 400.0));
        // A jump is there on the next frame, with no further frame.
        r.scroll.jump_to(700.0, 2000.0, 400.0);
        assert_eq!(r.frame(16, 2000.0), (700.0, false));
        // And a glide after it starts from there.
        r.scroll.scroll_to(800.0, 2000.0, 400.0);
        assert_eq!(r.frame(16, 2000.0), (700.0, true));
        let (y, _) = r.frame(16, 2000.0);
        assert!(y > 700.0 && y < 800.0);
    }

    #[test]
    fn content_that_shrinks_pulls_the_offset_in() {
        let mut r = Rig::new();
        r.scroll.jump_to(1600.0, 2000.0, 400.0);
        assert_eq!(r.frame(0, 2000.0), (1600.0, false));
        // The content is now 1000 long: the end is 600. The offset drawn is never past it.
        let (y, more) = r.frame(16, 1000.0);
        assert_eq!((y, r.scroll.target()), (600.0, 600.0));
        assert!(more);
        for _ in 0..100 {
            r.frame(16, 1000.0);
        }
        assert_eq!(r.frame(16, 1000.0), (600.0, false));
        // It fits altogether: the top.
        assert_eq!(r.frame(16, 100.0).0, 0.0);
    }

    #[test]
    fn with_reduced_motion_the_offset_is_the_target() {
        let mut r = Rig::new();
        r.anim.set_system_reduced(true);
        r.frame(0, 2000.0);
        r.scroll.scroll_by(120.0, 2000.0, 400.0);
        assert_eq!(r.frame(0, 2000.0), (120.0, false));
        // And another motion can be asked for.
        let tween = SmoothScroll::new().with_motion(fluent::MOVE);
        assert_ne!(tween, SmoothScroll::default());
    }

    #[test]
    fn the_animated_scrollbar_at_rest_is_the_plain_one_and_widens_between() {
        let area = Rect::new(0.0, 0.0, 100.0, 200.0);
        let (track, thumb) = scrollbar(area, 800.0, 200.0, 100.0).unwrap();
        let clock = ManualClock::new();
        let mut anim = Animator::new();
        let mut gfx = Gfx::new();
        let mut frame = |ms: u64, active: bool, animated: bool| {
            let mut bmp = Bitmap::new(100, 200);
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            p.fill(area, LIGHT.page);
            if animated {
                clock.advance_ms(ms);
                anim.begin_frame(clock.now());
                draw_scrollbar_animated(&mut p, &LIGHT, &mut anim, "bar", track, thumb, active);
                (bmp, anim.end_frame().is_some())
            } else {
                draw_scrollbar(&mut p, &LIGHT, track, thumb, active);
                (bmp, false)
            }
        };
        let width = |bmp: &Bitmap| {
            let y = thumb.cy() as usize;
            let page = [LIGHT.page[2], LIGHT.page[1], LIGHT.page[0]];
            (0..100)
                .filter(|x| bmp.pixels[(y * 100 + x) * 4..(y * 100 + x) * 4 + 3] != page)
                .count()
        };
        let (thin, wide) = (frame(0, false, false).0, frame(0, true, false).0);
        assert!(width(&thin) < width(&wide));
        let (rest, more) = frame(0, false, true);
        assert!(!more && rest.pixels == thin.pixels);
        // Hovered: the same frame still, then wider by steps, then exactly the wide bar.
        let (first, more) = frame(0, true, true);
        assert!(more && first.pixels == thin.pixels);
        let (mid, more) = frame(40, true, true);
        assert!(more && width(&mid) > width(&thin) && width(&mid) < width(&wide));
        let (end, more) = frame(50, true, true);
        assert!(!more && end.pixels == wide.pixels);
    }

    #[test]
    fn scrollbar_geometry() {
        let area = Rect::new(0.0, 0.0, 100.0, 404.0);
        assert!(scrollbar(area, 300.0, 404.0, 0.0).is_none());
        let (track, thumb) = scrollbar(area, 800.0, 400.0, 0.0).unwrap();
        assert_eq!(track.h, 400.0);
        assert_eq!(thumb.h, 200.0);
        assert_eq!(thumb.y, track.y);
        let (_, end) = scrollbar(area, 800.0, 400.0, 400.0).unwrap();
        assert_eq!(end.bottom(), track.bottom());
        let back = offset_for_thumb(track, end.h, end.y, 800.0, 400.0);
        assert!((back - 400.0).abs() < 0.01);
        // A huge document keeps a grabbable thumb.
        let (_, tiny) = scrollbar(area, 1e7, 400.0, 0.0).unwrap();
        assert_eq!(tiny.h, 28.0);
    }
}
