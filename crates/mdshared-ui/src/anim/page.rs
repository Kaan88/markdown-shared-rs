//! A page or tab switch: the old content of a region cross-fades, slides or is pushed out as the
//! new content comes in.
//!
//! The application draws only the new page; the old one exists as a [`Snapshot`] of the region,
//! taken from the last frame's bitmap at the moment of the switch. Each frame after it, the new
//! page is drawn as always and [`PageTransition::apply`] then blends the snapshot with it in
//! place. The cost is one copy of the region at the switch and one pass over it per frame, for
//! a quarter of a second.

use super::{Animator, AsId, Easing, Id, Motion, Snapshot, fluent};
use crate::gfx::{Painter, Rect};
use mdshared_text::Bitmap;
use std::time::Duration;

/// A side of the region.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

impl Edge {
    /// The other side: for the way back (a page that came from the right leaves to it, so the
    /// page that returns comes from the left).
    pub fn opposite(self) -> Edge {
        match self {
            Edge::Left => Edge::Right,
            Edge::Right => Edge::Left,
            Edge::Top => Edge::Bottom,
            Edge::Bottom => Edge::Top,
        }
    }

    /// The direction from the region's middle to this side.
    fn direction(self) -> (f32, f32) {
        match self {
            Edge::Left => (-1.0, 0.0),
            Edge::Right => (1.0, 0.0),
            Edge::Top => (0.0, -1.0),
            Edge::Bottom => (0.0, 1.0),
        }
    }
}

/// How the new page replaces the old one.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PageEffect {
    /// The old page fades into the new one, nothing moves. Right for any content.
    Crossfade,
    /// The new page starts `distance` logical px towards `from`, moves into place and fades in
    /// over the first half of the time. The Windows look for a page of a navigation view (from
    /// the bottom) and for stepping through tabs (from the side the new tab is on). The band
    /// the page has not reached yet repeats the page's own edge
    /// ([`Snapshot::slide_fade`]): give the page padding of its background colour there, or
    /// cross-fade.
    SlideFade { from: Edge, distance: f32 },
    /// The new page comes in from `from` over the whole region and pushes the old one out the
    /// other side. Nothing is mixed, so it is the cheapest; right for pages that sit side by
    /// side in the user's mind.
    Push { from: Edge },
}

/// One region's switch from an old page to a new one. Keep one per region that switches pages,
/// next to the application's other window state.
#[derive(Debug)]
pub struct PageTransition {
    id: Id,
    old: Option<Snapshot>,
    effect: PageEffect,
    /// The device px a logical px was when the switch began.
    scale: f32,
    duration: Duration,
    easing: Easing,
}

impl PageTransition {
    /// A transition that is not running. `id` names its progress in the [`Animator`]; two
    /// regions need two names. It takes [`fluent::NORMAL`] (250 ms) and decelerates.
    pub fn new(id: impl AsId) -> PageTransition {
        PageTransition {
            id: Id::new(id),
            old: None,
            effect: PageEffect::Crossfade,
            scale: 1.0,
            duration: fluent::NORMAL,
            easing: fluent::DECELERATE,
        }
    }

    /// The same with another length and curve. The curve shapes the move (and the cross-fade);
    /// the fade of a [`PageEffect::SlideFade`] is linear over the first half of the time.
    pub fn with_timing(mut self, duration: Duration, easing: Easing) -> PageTransition {
        self.duration = duration;
        self.easing = easing;
        self
    }

    /// Starts a switch: saves the pixels of `r` as the painter's bitmap holds them, which must
    /// still be the old page (the last frame drawn). Call it when the switch is decided, before
    /// the next frame draws the new page. A switch begun while another runs starts from what is
    /// on screen, the half-blended pages, so nothing jumps. When the animator does not animate
    /// (reduced motion) nothing is saved and nothing will run.
    pub fn begin(&mut self, anim: &mut Animator, p: &Painter, r: Rect, effect: PageEffect) {
        if !anim.animates() {
            return self.cancel(anim);
        }
        let (x0, y0, x1, y1) = p.dev_rect(r);
        let mut snap = self.old.take().unwrap_or_default();
        snap.retake(p.bmp, x0, y0, x1, y1);
        self.begin_with(anim, snap, p.scale, effect);
    }

    /// As [`begin`](Self::begin), for a device rectangle (far edges outside) of a bitmap held
    /// without a painter. `scale` is the device px per logical px, for the effect's distance.
    pub fn begin_dev(
        &mut self,
        anim: &mut Animator,
        bmp: &Bitmap,
        rect: (i32, i32, i32, i32),
        scale: f32,
        effect: PageEffect,
    ) {
        if !anim.animates() {
            return self.cancel(anim);
        }
        let mut snap = self.old.take().unwrap_or_default();
        snap.retake(bmp, rect.0, rect.1, rect.2, rect.3);
        self.begin_with(anim, snap, scale, effect);
    }

    /// As [`begin`](Self::begin), with a snapshot the application took itself: for one whose
    /// bitmap no longer holds the old page when the switch is decided, and which therefore
    /// draws the old page once more, takes [`Painter::snapshot`] of it, and then draws the new.
    pub fn begin_with(
        &mut self,
        anim: &mut Animator,
        old: Snapshot,
        scale: f32,
        effect: PageEffect,
    ) {
        if !anim.animates() || old.is_empty() {
            return self.cancel(anim);
        }
        self.old = Some(old);
        self.effect = effect;
        self.scale = scale;
        anim.set(self.id, 0.0);
    }

    /// Whether a switch is in flight.
    pub fn running(&self) -> bool {
        self.old.is_some()
    }

    /// Ends the switch now: the next frame is the new page alone.
    pub fn cancel(&mut self, anim: &mut Animator) {
        self.old = None;
        anim.forget(self.id);
    }

    /// Blends the old page into the region of `bmp`, which holds the new page as this frame
    /// drew it. Call it every frame, after the region's content is drawn and before anything
    /// that lies over the region (a popup, a toast) is. It does nothing when no switch runs,
    /// and ends the switch when its time is up or when the bitmap no longer holds the region
    /// (the window shrank). Returns whether the switch is still running.
    pub fn apply(&mut self, anim: &mut Animator, bmp: &mut Bitmap) -> bool {
        let Some(old) = &self.old else {
            return false;
        };
        let t: f32 = anim.value(self.id, 1.0, Motion::tween(self.duration, Easing::Linear));
        if t >= 1.0 || !old.fits(bmp) {
            self.old = None;
            return false;
        }
        let (x0, y0, x1, y1) = old.rect();
        // How far the new page still is from its place, of the way it has to go.
        let left = 1.0 - self.easing.at(t);
        match self.effect {
            PageEffect::Crossfade => old.crossfade(bmp, 1.0 - left),
            PageEffect::SlideFade { from, distance } => {
                let (ux, uy) = from.direction();
                let d = distance * self.scale * left;
                old.slide_fade(
                    bmp,
                    (ux * d).round() as i32,
                    (uy * d).round() as i32,
                    t * 2.0,
                );
            }
            PageEffect::Push { from } => {
                let (ux, uy) = from.direction();
                let (w, h) = ((x1 - x0) as f32, (y1 - y0) as f32);
                old.push(
                    bmp,
                    (ux * w * left).round() as i32,
                    (uy * h * left).round() as i32,
                );
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::{Clock, ManualClock};
    use crate::gfx::Gfx;

    const OLD: [u8; 3] = [200, 0, 0];
    const NEW: [u8; 3] = [0, 0, 200];
    const REGION: Rect = Rect::new(10.0, 5.0, 40.0, 20.0);

    fn at(bmp: &Bitmap, x: u32, y: u32) -> [u8; 3] {
        let o = ((y * bmp.width + x) * 4) as usize;
        [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
    }

    /// A window whose region shows one flat page, and a switch of it under test.
    struct Rig {
        bmp: Bitmap,
        gfx: Gfx,
        clock: ManualClock,
        anim: Animator,
        tr: PageTransition,
    }

    impl Rig {
        fn new() -> Rig {
            let mut rig = Rig {
                bmp: Bitmap::new(60, 30),
                gfx: Gfx::new(),
                clock: ManualClock::new(),
                anim: Animator::new(),
                tr: PageTransition::new("page")
                    .with_timing(Duration::from_millis(100), Easing::Linear),
            };
            rig.frame(0, OLD);
            rig
        }

        fn begin(&mut self, effect: PageEffect) {
            let p = Painter::new(&mut self.bmp, &mut self.gfx, 1.0);
            self.tr.begin(&mut self.anim, &p, REGION, effect);
        }

        /// One frame `ms` later: the window cleared, the region filled with `page` (its last
        /// column in white, to see it move), the switch applied. Returns whether a further
        /// frame was asked for.
        fn frame(&mut self, ms: u64, page: [u8; 3]) -> bool {
            self.clock.advance_ms(ms);
            self.anim.begin_frame(self.clock.now());
            {
                let mut p = Painter::new(&mut self.bmp, &mut self.gfx, 1.0);
                p.fill(Rect::new(0.0, 0.0, 60.0, 30.0), [9, 9, 9]);
                p.fill(REGION, page);
                p.fill(Rect::new(49.0, 5.0, 1.0, 20.0), [255, 255, 255]);
            }
            self.tr.apply(&mut self.anim, &mut self.bmp);
            self.anim.end_frame().is_some()
        }
    }

    #[test]
    fn a_crossfade_runs_from_the_old_page_to_the_new_and_then_costs_nothing() {
        let mut r = Rig::new();
        assert!(!r.tr.running());
        r.begin(PageEffect::Crossfade);
        assert!(r.tr.running());
        // The first frame of the new page still shows the old one.
        assert!(r.frame(0, NEW));
        assert_eq!(at(&r.bmp, 20, 10), OLD);
        assert!(r.frame(50, NEW));
        assert_eq!(at(&r.bmp, 20, 10), [100, 0, 100]);
        // Outside the region nothing is touched.
        assert_eq!(at(&r.bmp, 5, 10), [9, 9, 9]);
        assert_eq!(at(&r.bmp, 20, 26), [9, 9, 9]);
        // At its end: the new page, no further frame, the snapshot dropped.
        assert!(!r.frame(50, NEW));
        assert_eq!(at(&r.bmp, 20, 10), NEW);
        assert!(!r.tr.running());
        assert!(!r.frame(50, NEW));
        assert!(r.anim.is_empty());
    }

    #[test]
    fn a_slide_brings_the_new_page_in_from_its_edge() {
        let mut r = Rig::new();
        r.begin(PageEffect::SlideFade {
            from: Edge::Right,
            distance: 20.0,
        });
        r.frame(0, NEW);
        // A quarter of the way: 15 px still to go, half faded in.
        assert!(r.frame(25, NEW));
        assert_eq!(at(&r.bmp, 20, 10), [100, 0, 100]);
        // Past the half: fully opaque; the white last column is 5 px right of its place, so
        // out of the region, and the band behind the page repeats its edge.
        assert!(r.frame(50, NEW));
        assert_eq!(at(&r.bmp, 20, 10), NEW);
        assert_eq!(at(&r.bmp, 49, 10), NEW);
        assert_eq!(at(&r.bmp, 10, 10), NEW);
        assert!(!r.frame(25, NEW));
        assert_eq!(at(&r.bmp, 49, 10), [255, 255, 255]);
    }

    #[test]
    fn a_push_moves_both_pages_and_mixes_nothing() {
        let mut r = Rig::new();
        r.begin(PageEffect::Push { from: Edge::Left });
        r.frame(0, NEW);
        // A quarter of the way: the new page covers the left 10 px, its white column leading.
        assert!(r.frame(25, NEW));
        assert_eq!(at(&r.bmp, 12, 10), NEW);
        assert_eq!(at(&r.bmp, 19, 10), [255, 255, 255]);
        assert_eq!(at(&r.bmp, 20, 10), OLD);
        // The old page's own white column has left on the right.
        assert_eq!(at(&r.bmp, 49, 10), OLD);
        assert!(r.frame(50, NEW));
        assert!(!r.frame(25, NEW));
        assert_eq!(at(&r.bmp, 20, 10), NEW);
    }

    #[test]
    fn a_switch_begun_during_another_starts_from_what_is_on_screen() {
        let mut r = Rig::new();
        r.begin(PageEffect::Crossfade);
        r.frame(0, NEW);
        r.frame(50, NEW);
        assert_eq!(at(&r.bmp, 20, 10), [100, 0, 100]);
        // Back to the old page, half way: the mix is the start, and no pixel jumps.
        r.begin(PageEffect::Crossfade);
        assert!(r.frame(10, OLD));
        assert_eq!(at(&r.bmp, 20, 10), [100, 0, 100]);
        assert!(r.frame(50, OLD));
        assert_eq!(at(&r.bmp, 20, 10), [150, 0, 50]);
        assert!(!r.frame(50, OLD));
        assert_eq!(at(&r.bmp, 20, 10), OLD);
    }

    #[test]
    fn reduced_motion_a_cancel_and_a_shrunken_window_end_the_switch() {
        let mut r = Rig::new();
        r.anim.set_system_reduced(true);
        r.begin(PageEffect::Crossfade);
        assert!(!r.tr.running());
        assert!(!r.frame(0, NEW));
        assert_eq!(at(&r.bmp, 20, 10), NEW);

        r.anim.set_system_reduced(false);
        r.begin(PageEffect::Crossfade);
        r.frame(0, OLD);
        r.tr.cancel(&mut r.anim);
        assert!(!r.frame(10, OLD));
        assert_eq!(at(&r.bmp, 20, 10), OLD);

        // The window shrinks under a running switch: it ends instead of blending the wrong
        // pixels.
        r.begin(PageEffect::Push { from: Edge::Top });
        r.frame(0, NEW);
        r.bmp = Bitmap::new(40, 30);
        r.frame(10, NEW);
        assert!(!r.tr.running());
        assert_eq!(at(&r.bmp, 20, 10), NEW);

        // The other ways to begin.
        r.bmp = Bitmap::new(60, 30);
        r.frame(10, OLD);
        r.tr.begin_dev(
            &mut r.anim,
            &r.bmp,
            (10, 5, 50, 25),
            1.0,
            PageEffect::Crossfade,
        );
        assert!(r.tr.running());
        r.tr.begin_with(&mut r.anim, Snapshot::default(), 1.0, PageEffect::Crossfade);
        assert!(!r.tr.running(), "an empty snapshot is no switch");
        assert_eq!(Edge::Left.opposite(), Edge::Right);
        assert_eq!(Edge::Bottom.opposite(), Edge::Top);
    }
}
