//! The overlay scrollbar: geometry (pure functions of rectangles and lengths) and drawing; and
//! a scroll offset that glides to where the wheel sent it.

use crate::anim::{Animator, AsId, Motion, Spring, fluent};
use crate::gfx::{Painter, Rect};
use crate::theme::Palette;

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
    id: impl AsId,
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

/// [`scrollbar`] on its side: the track along the bottom of `area`, the thumb moving across,
/// or `None` when everything fits or the area is too narrow for a bar. For a table wider than
/// its view, a terminal's long lines.
pub fn hscrollbar(area: Rect, content: f32, view: f32, offset: f32) -> Option<(Rect, Rect)> {
    if content <= view + 0.5 || area.w < 24.0 {
        return None;
    }
    let track = Rect::new(area.x + 2.0, area.bottom() - 12.0, area.w - 4.0, 12.0);
    let len = (track.w * view / content).clamp(28.0_f32.min(track.w), track.w);
    let t = (offset / (content - view)).clamp(0.0, 1.0);
    let thumb = Rect::new(track.x + t * (track.w - len), track.y, len, track.h);
    Some((track, thumb))
}

/// The scroll offset that puts the horizontal thumb's left edge at `thumb_x`.
pub fn offset_for_hthumb(track: Rect, thumb_w: f32, thumb_x: f32, content: f32, view: f32) -> f32 {
    let span = (track.w - thumb_w).max(1.0);
    ((thumb_x - track.x) / span).clamp(0.0, 1.0) * (content - view).max(0.0)
}

/// Both bars of a view that scrolls both ways, the vertical first. Each is [`scrollbar`] or
/// [`hscrollbar`], except that when both show each stops short of the corner the other runs
/// into, so the two never cross. Sizes and offsets are `(across, down)`.
#[allow(clippy::type_complexity)] // two optional (track, thumb) pairs: what each bar answers
pub fn scrollbars(
    area: Rect,
    content: (f32, f32),
    view: (f32, f32),
    offset: (f32, f32),
) -> (Option<(Rect, Rect)>, Option<(Rect, Rect)>) {
    let down = scrollbar(area, content.1, view.1, offset.1);
    let across = hscrollbar(area, content.0, view.0, offset.0);
    if down.is_none() || across.is_none() {
        return (down, across);
    }
    // The corner is the bar's thickness: each loses it from its far end.
    let down = scrollbar(
        Rect::new(area.x, area.y, area.w, area.h - 12.0),
        content.1,
        view.1,
        offset.1,
    );
    let across = hscrollbar(
        Rect::new(area.x, area.y, area.w - 12.0, area.h),
        content.0,
        view.0,
        offset.0,
    );
    (down, across)
}

/// [`draw_scrollbar`] on its side, for [`hscrollbar`]'s track and thumb: thin at rest, taller
/// while hovered or dragged, along the bottom of the track.
pub fn draw_hscrollbar(p: &mut Painter, pal: &Palette, track: Rect, thumb: Rect, active: bool) {
    let h = if active { 8.0 } else { 3.0 };
    if active {
        p.rbox_a(
            Rect::new(track.x, track.bottom() - h - 3.0, track.w, h),
            h / 2.0,
            pal.thumb,
            0.10,
        );
    }
    let r = Rect::new(thumb.x, track.bottom() - h - 3.0, thumb.w, h);
    p.rbox_a(r, h / 2.0, pal.thumb, if active { 0.85 } else { 0.55 });
}

/// [`draw_scrollbar_animated`] on its side.
pub fn draw_hscrollbar_animated(
    p: &mut Painter,
    pal: &Palette,
    anim: &mut Animator,
    id: impl AsId,
    track: Rect,
    thumb: Rect,
    active: bool,
) {
    let t = anim.toggle(id, active, fluent::FADE);
    if t <= 0.0 || t >= 1.0 {
        return draw_hscrollbar(p, pal, track, thumb, t >= 1.0);
    }
    let h = 3.0 + 5.0 * t;
    p.rbox_a(
        Rect::new(track.x, track.bottom() - h - 3.0, track.w, h),
        h / 2.0,
        pal.thumb,
        0.10 * t,
    );
    let r = Rect::new(thumb.x, track.bottom() - h - 3.0, thumb.w, h);
    p.rbox_a(r, h / 2.0, pal.thumb, 0.55 + 0.30 * t);
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
/// The offset is kept as an `f64`, and every number given may be an `f32` or an `f64`. A view
/// whose content is a few screens long loses nothing by reading it back as an `f32`
/// ([`offset`](Self::offset), [`target`](Self::target), [`current`](Self::current)). One whose
/// content is very long (a terminal's scrollback, a folder of a hundred thousand rows) reads
/// it as an `f64` ([`offset_f64`](Self::offset_f64), [`target_f64`](Self::target_f64),
/// [`current_f64`](Self::current_f64)) and subtracts the first visible row's position before
/// it goes to `f32`: past sixteen million an `f32` no longer holds every whole pixel.
///
/// It glides on a critically damped spring, so a wheel that keeps turning gives one smooth
/// run, not a start and stop a notch: each notch moves the target and the offset keeps its
/// speed. With reduced motion the offset is the target on every frame, which is what an
/// application without this type has today.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SmoothScroll {
    target: f64,
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
        self.target as f32
    }

    /// [`target`](Self::target) with all its digits, for content too long for an `f32`.
    pub fn target_f64(&self) -> f64 {
        self.target
    }

    /// The furthest offset: `content` longer than `view` by this much, or zero (also when
    /// either is not a number).
    fn max(content: f64, view: f64) -> f64 {
        let max = content - view;
        if max > 0.0 { max } else { 0.0 }
    }

    /// Moves the target by `delta`, kept inside what `content` and `view` allow. Returns
    /// whether it moved (if not, the wheel event is one for something else: the caller's
    /// parent).
    pub fn scroll_by(
        &mut self,
        delta: impl Into<f64>,
        content: impl Into<f64>,
        view: impl Into<f64>,
    ) -> bool {
        self.scroll_to(self.target + delta.into(), content, view)
    }

    /// Moves the target to `offset`, kept inside what `content` and `view` allow, to glide
    /// there. Returns whether it moved.
    pub fn scroll_to(
        &mut self,
        offset: impl Into<f64>,
        content: impl Into<f64>,
        view: impl Into<f64>,
    ) -> bool {
        let next = offset
            .into()
            .clamp(0.0, SmoothScroll::max(content.into(), view.into()));
        // A target that is not a number stays where it was.
        let moved = next != self.target && !next.is_nan();
        if moved {
            self.target = next;
        }
        moved
    }

    /// Puts the offset at `offset` at once, with no glide.
    pub fn jump_to(
        &mut self,
        offset: impl Into<f64>,
        content: impl Into<f64>,
        view: impl Into<f64>,
    ) {
        self.scroll_to(offset, content, view);
        self.jump = true;
    }

    /// The offset to draw at this frame. `id` names it in `anim`. `content` and `view` are
    /// this frame's: a target left past the end by content that shrank is pulled in (and
    /// glides there), and the offset given is never outside them.
    pub fn offset(
        &mut self,
        anim: &mut Animator,
        id: impl AsId,
        content: impl Into<f64>,
        view: impl Into<f64>,
    ) -> f32 {
        self.offset_f64(anim, id, content, view) as f32
    }

    /// [`offset`](Self::offset) with all its digits, for content too long for an `f32`: far
    /// down such content the glide moves by fractions of a pixel that an `f32` cannot hold,
    /// and ends exactly on its target.
    pub fn offset_f64(
        &mut self,
        anim: &mut Animator,
        id: impl AsId,
        content: impl Into<f64>,
        view: impl Into<f64>,
    ) -> f64 {
        let max = SmoothScroll::max(content.into(), view.into());
        self.target = self.target.clamp(0.0, max);
        let id = id.as_id();
        if std::mem::take(&mut self.jump) {
            anim.set(id, self.target);
        }
        anim.value(id, self.target, self.motion).clamp(0.0, max)
    }

    /// The offset as it is now, between frames: what the last [`offset`](Self::offset) gave
    /// (or where a jump made since will put it), without asking the animator for anything, so
    /// no frame is asked for and the glide is not touched. For hit testing during a glide: a
    /// click lands on the row that is drawn under the pointer, not on the row that will be
    /// there once the glide ends, which is what [`target`](Self::target) would give. Before
    /// the first frame it is the target.
    pub fn current(&self, anim: &Animator, id: impl AsId) -> f32 {
        self.current_f64(anim, id) as f32
    }

    /// [`current`](Self::current) with all its digits, for content too long for an `f32`.
    pub fn current_f64(&self, anim: &Animator, id: impl AsId) -> f64 {
        if self.jump {
            return self.target;
        }
        anim.get::<f64>(id)
            .filter(|v| v.is_finite())
            .unwrap_or(self.target)
    }

    /// Whether the offset has come to rest at its target (as of the last frame that asked).
    pub fn at_rest(&self, anim: &Animator, id: impl AsId) -> bool {
        let id = id.as_id();
        !self.jump && anim.settled(id) && anim.get::<f64>(id) == Some(self.target)
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
    fn the_offset_now_is_what_was_drawn_and_reading_it_asks_for_nothing() {
        let mut r = Rig::new();
        // Before any frame: the target.
        r.scroll.jump_to(300.0, 2000.0, 400.0);
        assert_eq!(r.scroll.current(&r.anim, "page"), 300.0);
        assert_eq!(r.frame(0, 2000.0), (300.0, false));
        r.scroll.scroll_by(120.0, 2000.0, 400.0);
        r.frame(0, 2000.0);
        let (drawn, more) = r.frame(32, 2000.0);
        assert!(more && drawn > 300.0 && drawn < 420.0);
        // During the glide: where the last frame drew, not where it is going; and asking any
        // number of times moves nothing and asks for no frame.
        for _ in 0..3 {
            assert_eq!(r.scroll.current(&r.anim, "page"), drawn);
            assert_eq!(r.scroll.current_f64(&r.anim, "page") as f32, drawn);
        }
        assert_eq!(r.scroll.target(), 420.0);
        let (next, _) = r.frame(16, 2000.0);
        assert!(next > drawn, "the glide went on from where it was");
        // An id made from the name reads the same value.
        let id = crate::anim::Id::new("page");
        assert_eq!(r.scroll.current(&r.anim, id), next);
        // A jump made since the last frame is where the next frame will draw.
        r.scroll.jump_to(50.0, 2000.0, 400.0);
        assert_eq!(r.scroll.current(&r.anim, "page"), 50.0);
        // A name the animator does not know: the target.
        assert_eq!(SmoothScroll::new().current(&r.anim, "other"), 0.0);
    }

    #[test]
    fn a_glide_fifty_million_pixels_down_moves_by_fractions_and_ends_exactly() {
        // A list of five million rows of 24 px, scrolled far down: an `f32` there has steps of
        // four pixels.
        let (content, view) = (1.2e8_f64, 800.0_f64);
        let start = 5.0e7_f64;
        let clock = ManualClock::new();
        let mut anim = Animator::new();
        let mut scroll = SmoothScroll::new();
        let mut frame = |scroll: &mut SmoothScroll, ms: u64| {
            clock.advance_ms(ms);
            anim.begin_frame(clock.now());
            let y = scroll.offset_f64(&mut anim, "rows", content, view);
            (y, anim.end_frame().is_some())
        };
        scroll.jump_to(start, content, view);
        assert_eq!(frame(&mut scroll, 0), (start, false));
        // Three notches of 40.5 px: a target no `f32` holds.
        for _ in 0..3 {
            assert!(scroll.scroll_by(40.5, content, view));
        }
        let target = start + 121.5;
        assert_eq!(scroll.target_f64(), target);
        assert_ne!(
            scroll.target() as f64,
            target,
            "the `f32` reading is the rounded one"
        );
        assert_eq!(frame(&mut scroll, 0), (start, true));
        let (mut last, mut steps) = (start, Vec::new());
        loop {
            let (y, more) = frame(&mut scroll, 8);
            assert!(y >= last && y <= target, "{y} after {last}");
            if y > last {
                steps.push(y - last);
            }
            last = y;
            if !more {
                break;
            }
            assert!(steps.len() < 200, "it never arrives");
        }
        assert_eq!(last, target, "it ends exactly on its target");
        // A second way to the same end: the notches added up by hand.
        assert_eq!(last, 5.0e7 + 40.5 + 40.5 + 40.5);
        let fine = steps.iter().filter(|s| **s < 4.0).count();
        assert!(steps.len() >= 10 && fine >= 5, "{steps:?}");
        // The numbers may be given as `f32` as well, and nonsense moves nothing.
        assert!(scroll.scroll_to(100.0_f32, 2000.0_f32, 400.0_f32));
        assert!(!scroll.scroll_to(f64::NAN, content, view));
        // Content that is not a number has no room: the target is pulled to the top.
        assert!(scroll.scroll_by(10.0, f64::NAN, view));
        assert_eq!(scroll.target_f64(), 0.0);
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

    #[test]
    fn the_horizontal_bar_is_the_vertical_on_its_side() {
        let area = Rect::new(0.0, 0.0, 404.0, 100.0);
        assert!(hscrollbar(area, 300.0, 404.0, 0.0).is_none());
        assert!(hscrollbar(Rect::new(0.0, 0.0, 20.0, 100.0), 800.0, 20.0, 0.0).is_none());
        let (track, thumb) = hscrollbar(area, 800.0, 400.0, 0.0).unwrap();
        assert_eq!((track.w, track.h, track.bottom()), (400.0, 12.0, 100.0));
        assert_eq!((thumb.w, thumb.x), (200.0, track.x));
        let (_, end) = hscrollbar(area, 800.0, 400.0, 400.0).unwrap();
        assert_eq!(end.right(), track.right());
        let back = offset_for_hthumb(track, end.w, end.x, 800.0, 400.0);
        assert!((back - 400.0).abs() < 0.01);
        // The same numbers turned a quarter give the same lengths.
        let (vt, vb) = scrollbar(Rect::new(0.0, 0.0, 100.0, 404.0), 800.0, 400.0, 150.0).unwrap();
        let (ht, hb) = hscrollbar(area, 800.0, 400.0, 150.0).unwrap();
        assert_eq!((vt.h, vb.h, vb.y - vt.y), (ht.w, hb.w, hb.x - ht.x));
    }

    #[test]
    fn two_bars_stop_short_of_the_corner() {
        let area = Rect::new(10.0, 20.0, 400.0, 300.0);
        // One way only: as the bar alone.
        assert_eq!(
            scrollbars(area, (300.0, 900.0), (400.0, 300.0), (0.0, 50.0)),
            (scrollbar(area, 900.0, 300.0, 50.0), None)
        );
        assert_eq!(
            scrollbars(area, (900.0, 200.0), (400.0, 300.0), (50.0, 0.0)),
            (None, hscrollbar(area, 900.0, 400.0, 50.0))
        );
        assert_eq!(
            scrollbars(area, (1.0, 1.0), (400.0, 300.0), (0.0, 0.0)),
            (None, None)
        );
        // Both: neither track enters the 12 px square at the bottom right.
        let (down, across) = scrollbars(area, (900.0, 900.0), (400.0, 300.0), (500.0, 600.0));
        let ((dt, db), (at, ab)) = (down.unwrap(), across.unwrap());
        assert_eq!(dt.bottom(), area.bottom() - 12.0 - 2.0);
        assert_eq!(at.right(), area.right() - 12.0 - 2.0);
        assert!(dt.bottom() <= at.y && at.right() <= dt.x);
        // At the far end each thumb reaches its shortened track's end.
        assert_eq!((db.bottom(), ab.right()), (dt.bottom(), at.right()));
    }

    #[test]
    fn the_horizontal_bar_widens_when_active_and_animates_like_the_vertical() {
        let area = Rect::new(0.0, 0.0, 200.0, 100.0);
        let (track, thumb) = hscrollbar(area, 800.0, 200.0, 100.0).unwrap();
        let clock = ManualClock::new();
        let mut anim = Animator::new();
        let mut gfx = Gfx::new();
        let mut frame = |ms: u64, active: bool, animated: bool| {
            let mut bmp = Bitmap::new(200, 100);
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            p.fill(area, LIGHT.page);
            if animated {
                clock.advance_ms(ms);
                anim.begin_frame(clock.now());
                draw_hscrollbar_animated(&mut p, &LIGHT, &mut anim, "bar", track, thumb, active);
                (bmp, anim.end_frame().is_some())
            } else {
                draw_hscrollbar(&mut p, &LIGHT, track, thumb, active);
                (bmp, false)
            }
        };
        // How many rows the bar covers in the thumb's middle column.
        let height = |bmp: &Bitmap| {
            let x = thumb.cx() as usize;
            let page = [LIGHT.page[2], LIGHT.page[1], LIGHT.page[0]];
            (0..100)
                .filter(|y| bmp.pixels[(y * 200 + x) * 4..(y * 200 + x) * 4 + 3] != page)
                .count()
        };
        let (thin, wide) = (frame(0, false, false).0, frame(0, true, false).0);
        assert!(height(&thin) > 0 && height(&thin) < height(&wide));
        let (rest, more) = frame(0, false, true);
        assert!(!more && rest.pixels == thin.pixels);
        let (first, more) = frame(0, true, true);
        assert!(more && first.pixels == thin.pixels);
        let (mid, more) = frame(40, true, true);
        assert!(more && height(&mid) > height(&thin) && height(&mid) < height(&wide));
        let (end, more) = frame(50, true, true);
        assert!(!more && end.pixels == wide.pixels);
    }
}
