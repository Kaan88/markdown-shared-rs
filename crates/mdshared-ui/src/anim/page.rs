//! A page or tab switch: the old content of a region cross-fades, slides or is pushed out as the
//! new content comes in.
//!
//! The application draws only the new page; the old one exists as a [`Snapshot`] of the region,
//! taken from the last frame's bitmap at the moment of the switch. Each frame after it, the new
//! page is drawn as always and [`PageTransition::apply`] then blends the snapshot with it in
//! place. The cost is one copy of the region at the switch and one pass over it per frame, for
//! a quarter of a second.
//!
//! The old page is pixels, so it is only right where and as it was taken. A switch remembers
//! the bitmap's size, the scale and the region's device rectangle at its start, and
//! [`apply`](PageTransition::apply) is told the region each frame: if the window was resized,
//! moved to a screen of another scale, or laid out anew so that the region is elsewhere, the
//! switch ends there and then and the frame shows the new page plainly. A switch cut short is
//! never wrong; an old page blended in the wrong place is.
//!
//! A resize in the middle of a switch need not cut it short. An application that wants the
//! switch to go on draws the old page once more, laid out at the new size, takes a
//! [`Painter::snapshot`] of the region, and gives it to [`rebase`](PageTransition::rebase) in
//! that same frame before drawing the new page and calling `apply`: the progress carries on
//! from where it was. [`fits`](PageTransition::fits) says when that is needed; it is false once
//! the bitmap is no longer the size the saved old page was taken at.
//!
//! Deliberately not here: drawing the old page again (that is the application's, which has the
//! widgets and the new layout), and transitions of more than one region at once (keep one
//! `PageTransition` for each).

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
///
/// ```
/// # use mdshared_ui::anim::{Animator, Edge, PageEffect, PageTransition};
/// # use mdshared_ui::gfx::{Gfx, Painter, Rect};
/// # use mdshared_text::Bitmap;
/// # struct State { page: u8 }
/// # fn draw_page(p: &mut Painter, r: Rect, page: u8) { p.fill(r, [page * 80; 3]); }
/// # let (mut bitmap, mut gfx, scale) = (Bitmap::new(64, 48), Gfx::new(), 1.0);
/// # let mut anim = Animator::new();
/// # let content_rect = Rect::new(8.0, 8.0, 48.0, 32.0);
/// # let (mut state, next) = (State { page: 1 }, 2);
/// # anim.begin_frame(std::time::Instant::now());
/// // State: one for each region that switches pages.
/// let mut page_switch = PageTransition::new("content");
///
/// // When the switch is decided (the bitmap still holds the old page):
/// let p = Painter::new(&mut bitmap, &mut gfx, scale);
/// let effect = PageEffect::SlideFade { from: Edge::Bottom, distance: 24.0 };
/// page_switch.begin(&mut anim, &p, content_rect, effect);
/// state.page = next;
///
/// // Every frame, after the content region is drawn and before anything that lies over it:
/// let mut p = Painter::new(&mut bitmap, &mut gfx, scale);
/// draw_page(&mut p, content_rect, state.page);
/// page_switch.apply(&mut anim, &mut p, content_rect);
/// # assert!(page_switch.running());
/// # assert!(anim.end_frame().is_some());
/// ```
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
    /// The snapshot is taken of the whole region, with no clip in force that cuts it: its
    /// rectangle is what [`apply`](Self::apply) holds the region to.
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

    /// Whether the saved old page still fits `bmp`, the bitmap this frame draws into: it is of
    /// the size the switch began at, so [`apply`](Self::apply) would blend it. False after the
    /// window was resized, larger or smaller, and true when no switch runs (there is then no
    /// old page to be wrong).
    ///
    /// When it is false under a running switch, the application that wants the switch to go on
    /// draws the old page again at the new size, takes [`Painter::snapshot`] of the region, and
    /// gives it to [`rebase`](Self::rebase) before the new page is drawn and `apply` is called.
    /// An application that does not do that is as before: `apply` ends the switch and the frame
    /// shows the new page plainly.
    pub fn fits(&self, bmp: &Bitmap) -> bool {
        self.old.as_ref().is_none_or(|old| old.fits(bmp))
    }

    /// As [`begin_with`](Self::begin_with), for a switch already running: replaces its old page
    /// with one drawn again at this frame's size, and the recorded scale with this frame's, and
    /// lets the switch go on from where it is rather than starting its time again. Call it when
    /// [`fits`](Self::fits) says the saved page is of another size, which is after the window
    /// was resized: draw the old page as it was when the switch began but laid out in this
    /// frame's bitmap, take a [`Painter::snapshot`] of the region, give it here, then draw the
    /// new page and call [`apply`](Self::apply), all in that same frame. The effect and the
    /// timing stay as they were.
    ///
    /// The progress survives only if `apply` runs every frame, this one included: a frame that
    /// does not ask for it lets the animator forget the progress, and the next `apply` then finds
    /// the switch finished and leaves the new page alone.
    ///
    /// It does nothing and answers false when no switch runs (the animator is not touched), and
    /// ends the switch when the snapshot is empty or the animator does not animate. A snapshot
    /// of another rectangle than the one `apply` is given still ends the switch, as any other
    /// mismatch does. A switch that began over another running switch has a saved page that is a
    /// blend of two pages, which cannot be drawn again: call [`cancel`](Self::cancel) for that
    /// one instead.
    pub fn rebase(&mut self, anim: &mut Animator, old: Snapshot, scale: f32) -> bool {
        if self.old.is_none() {
            return false;
        }
        if old.is_empty() || !anim.animates() {
            self.cancel(anim);
            return false;
        }
        self.old = Some(old);
        self.scale = scale;
        true
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

    /// Blends the old page into the region `r` of the painter's bitmap, which holds the new
    /// page as this frame drew it. Call it every frame, after the region's content is drawn and
    /// before anything that lies over the region (a popup, a toast) is; `r` is the region as
    /// this frame laid it out, the rectangle given to [`begin`](Self::begin). It does nothing
    /// when no switch runs, and ends the switch when its time is up. It also ends it, at once
    /// and with the new page left as drawn, when the old page would no longer be in its place:
    /// the bitmap has another size than at the start (the window was resized), the painter
    /// another scale (the window moved to another screen), or `r` is another device rectangle
    /// (the layout changed). A switch that should survive such a change is given the old page
    /// again first ([`rebase`](Self::rebase)). [`fits`](Self::fits) detects the size change
    /// only: another scale, or a moved region in a bitmap of the same size, is not caught by
    /// it, and an application that wants those to survive notices them itself and calls
    /// `rebase` with the old page drawn at the new place.
    /// Returns whether the switch is still running.
    ///
    /// The blend is of the whole region and does not heed the painter's clip. Fades use the
    /// painter's frost key to mix marked chrome with premultiplied pixels as they will appear.
    /// Cancel the switch or replace the old snapshot if the frost key changes between frames.
    pub fn apply(&mut self, anim: &mut Animator, p: &mut Painter, r: Rect) -> bool {
        let rect = p.dev_rect(r);
        let scale = p.scale;
        let key = p.frost_key().copied();
        self.apply_with_key(anim, p.bmp, rect, scale, key.as_ref())
    }

    /// As [`apply`](Self::apply), for a device rectangle (far edges outside) of a bitmap held
    /// without a painter or frost key, as [`begin_dev`](Self::begin_dev) takes them. `rect` and `scale` are
    /// this frame's; the switch ends if they, or the bitmap's size, are not those of its start.
    pub fn apply_dev(
        &mut self,
        anim: &mut Animator,
        bmp: &mut Bitmap,
        rect: (i32, i32, i32, i32),
        scale: f32,
    ) -> bool {
        self.apply_with_key(anim, bmp, rect, scale, None)
    }

    fn apply_with_key(
        &mut self,
        anim: &mut Animator,
        bmp: &mut Bitmap,
        rect: (i32, i32, i32, i32),
        scale: f32,
        key: Option<&crate::frost::Key>,
    ) -> bool {
        let Some(old) = &self.old else {
            return false;
        };
        // The region as a snapshot would hold it: cut to the bitmap.
        let region = (
            rect.0.max(0),
            rect.1.max(0),
            rect.2.min(bmp.width as i32),
            rect.3.min(bmp.height as i32),
        );
        if !old.fits(bmp) || old.rect() != region || scale != self.scale {
            self.cancel(anim);
            return false;
        }
        let t: f32 = anim.value(self.id, 1.0, Motion::tween(self.duration, Easing::Linear));
        if t >= 1.0 {
            self.old = None;
            return false;
        }
        let (x0, y0, x1, y1) = old.rect();
        // How far the new page still is from its place, of the way it has to go.
        let left = 1.0 - self.easing.at(t);
        match self.effect {
            PageEffect::Crossfade => old.crossfade_keyed(bmp, 1.0 - left, key),
            PageEffect::SlideFade { from, distance } => {
                let (ux, uy) = from.direction();
                let d = distance * self.scale * left;
                old.slide_fade_keyed(
                    bmp,
                    (ux * d).round() as i32,
                    (uy * d).round() as i32,
                    t * 2.0,
                    key,
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
    const GREEN: [u8; 3] = [0, 200, 0];
    const BACK: [u8; 3] = [9, 9, 9];
    const REGION: Rect = Rect::new(10.0, 5.0, 40.0, 20.0);

    #[test]
    fn page_fades_mix_marked_chrome_and_translucent_pixels_as_shown_in_both_directions() {
        use crate::frost::{Key, key_out};
        let chrome = [255, 255, 255, 254];
        let translucent = [180; 4];
        let key = Key::new([255; 3], 0);
        for (effect, halfway) in [
            (PageEffect::Crossfade, 50),
            (
                PageEffect::SlideFade {
                    from: Edge::Right,
                    distance: 2.0,
                },
                25,
            ),
        ] {
            for (old, new) in [(chrome, translucent), (translucent, chrome)] {
                let area = Rect::new(0.0, 0.0, 4.0, 1.0);
                let mut r = Rig::sized(4, 1, area);
                let paint = |bmp: &mut Bitmap, px| {
                    bmp.pixels.as_chunks_mut::<4>().0.fill(px);
                };
                paint(&mut r.bmp, old);
                let mut p = Painter::new(&mut r.bmp, &mut r.gfx, 1.0);
                p.set_frost_key(Some(key));
                r.tr.begin(&mut r.anim, &p, area, effect);
                for (ms, expected) in [(0, old), (halfway, [90; 4]), (100 - halfway, new)] {
                    r.clock.advance_ms(ms);
                    r.anim.begin_frame(r.clock.now());
                    paint(&mut r.bmp, new);
                    let mut p = Painter::new(&mut r.bmp, &mut r.gfx, 1.0);
                    p.set_frost_key(Some(key));
                    r.tr.apply(&mut r.anim, &mut p, area);
                    r.anim.end_frame();
                    assert_eq!(
                        r.bmp.pixels,
                        expected.repeat(4),
                        "{effect:?}, {old:?}, +{ms}ms"
                    );
                    if expected == [90; 4] {
                        let mut pixels =
                            [u32::from_le_bytes(r.bmp.pixels[..4].try_into().unwrap())];
                        key_out(&key, &mut pixels, 1, (0, 0, 1, 1));
                        assert_eq!(pixels, [0x5a5a_5a5a]);
                    }
                }
            }
        }
    }

    fn at(bmp: &Bitmap, x: u32, y: u32) -> [u8; 3] {
        let o = ((y * bmp.width + x) * 4) as usize;
        [bmp.pixels[o + 2], bmp.pixels[o + 1], bmp.pixels[o]]
    }

    /// The first pixel at which two bitmaps of one size differ: its place and both colours,
    /// so a mismatch says where it is rather than printing whole frames.
    fn first_difference(a: &Bitmap, b: &Bitmap) -> Option<(u32, u32, [u8; 3], [u8; 3])> {
        (0..a.height).find_map(|y| {
            (0..a.width)
                .find(|&x| at(a, x, y) != at(b, x, y))
                .map(|x| (x, y, at(a, x, y), at(b, x, y)))
        })
    }

    /// A frame's background and `page` in the region, its last column in white (to see it
    /// move).
    fn draw_page(p: &mut Painter, region: Rect, page: [u8; 3]) {
        let (w, h) = (p.bmp.width as f32, p.bmp.height as f32);
        p.fill(Rect::new(0.0, 0.0, w, h), BACK);
        p.fill(region, page);
        p.fill(
            Rect::new(region.right() - 1.0, region.y, 1.0, region.h),
            [255; 3],
        );
    }

    /// The old page laid out at the size of `p`'s bitmap: `BACK` outside the region, `OLD`
    /// inside it, and a green block of a fixed ten px against the region's right edge. A page
    /// of forty px stretched to sixty would have that block fifteen wide, which is how a
    /// stretch is told from a drawing at the new size.
    fn draw_old(p: &mut Painter, region: Rect) {
        let (w, h) = (p.bmp.width as f32, p.bmp.height as f32);
        p.fill(Rect::new(0.0, 0.0, w, h), BACK);
        p.fill(region, OLD);
        p.fill(
            Rect::new(region.right() - 10.0, region.y, 10.0, region.h),
            GREEN,
        );
    }

    /// A window whose region shows one flat page, and a switch of it under test.
    struct Rig {
        bmp: Bitmap,
        gfx: Gfx,
        clock: ManualClock,
        anim: Animator,
        tr: PageTransition,
        region: Rect,
    }

    impl Rig {
        fn new() -> Rig {
            let mut rig = Rig::sized(60, 30, REGION);
            rig.frame(0, OLD);
            rig
        }

        /// A window `w` by `h` holding the old page in `region`, and a switch of it under test.
        fn sized(w: u32, h: u32, region: Rect) -> Rig {
            let mut rig = Rig {
                bmp: Bitmap::new(w, h),
                gfx: Gfx::new(),
                clock: ManualClock::new(),
                anim: Animator::new(),
                tr: PageTransition::new("page")
                    .with_timing(Duration::from_millis(100), Easing::Linear),
                region,
            };
            {
                let mut p = Painter::new(&mut rig.bmp, &mut rig.gfx, 1.0);
                draw_old(&mut p, region);
            }
            rig
        }

        fn begin(&mut self, effect: PageEffect) {
            let p = Painter::new(&mut self.bmp, &mut self.gfx, 1.0);
            self.tr.begin(&mut self.anim, &p, self.region, effect);
        }

        /// One frame `ms` later: the window cleared, the region filled with `page` (its last
        /// column in white, to see it move), the switch applied. Returns whether a further
        /// frame was asked for.
        fn frame(&mut self, ms: u64, page: [u8; 3]) -> bool {
            self.clock.advance_ms(ms);
            self.anim.begin_frame(self.clock.now());
            {
                let mut p = Painter::new(&mut self.bmp, &mut self.gfx, 1.0);
                draw_page(&mut p, self.region, page);
                self.tr.apply(&mut self.anim, &mut p, self.region);
            }
            self.anim.end_frame().is_some()
        }

        /// One frame `ms` later after the window was resized: the bitmap already holds the old
        /// page as the application drew it at the new size; this frame takes the snapshot
        /// again and hands it over, then draws the new page and applies the switch.
        fn frame_resized(&mut self, ms: u64, page: [u8; 3]) -> bool {
            self.clock.advance_ms(ms);
            self.anim.begin_frame(self.clock.now());
            {
                let mut p = Painter::new(&mut self.bmp, &mut self.gfx, 1.0);
                assert!(!self.tr.fits(p.bmp), "the old page is of the old size");
                let old = p.snapshot(self.region);
                assert!(self.tr.rebase(&mut self.anim, old, p.scale));
                draw_page(&mut p, self.region, page);
                self.tr.apply(&mut self.anim, &mut p, self.region);
            }
            self.anim.end_frame().is_some()
        }

        /// What `frame` draws with no switch at all: the new page plainly.
        fn plain(&mut self, page: [u8; 3]) -> Vec<u8> {
            let mut bmp = Bitmap::new(self.bmp.width, self.bmp.height);
            let mut p = Painter::new(&mut bmp, &mut self.gfx, 1.0);
            draw_page(&mut p, self.region, page);
            bmp.pixels
        }
    }

    #[test]
    fn a_resize_a_new_scale_or_a_moved_region_ends_the_switch_with_the_new_page_plain() {
        // The window grew: the same device rectangle is another part of it.
        for size in [(80, 30), (60, 40), (90, 60)] {
            let mut r = Rig::new();
            r.begin(PageEffect::Crossfade);
            assert!(r.frame(0, NEW));
            assert!(r.frame(30, NEW));
            assert_ne!(r.plain(NEW), r.bmp.pixels, "the switch is under way");
            r.bmp = Bitmap::new(size.0, size.1);
            assert!(!r.frame(10, NEW), "{size:?}: no further frame");
            assert!(!r.tr.running());
            assert_eq!(
                r.plain(NEW),
                r.bmp.pixels,
                "{size:?}: the new page as drawn"
            );
            assert!(r.anim.is_empty(), "its progress is forgotten at once");
        }

        // The layout changed: the region is elsewhere, or of another size.
        for moved in [
            Rect::new(15.0, 5.0, 40.0, 20.0),
            Rect::new(10.0, 6.0, 40.0, 20.0),
            Rect::new(10.0, 5.0, 30.0, 20.0),
            Rect::new(10.0, 5.0, 40.0, 24.0),
            Rect::new(f32::NAN, 5.0, 40.0, 20.0),
        ] {
            for effect in [
                PageEffect::Crossfade,
                PageEffect::Push { from: Edge::Left },
                PageEffect::SlideFade {
                    from: Edge::Bottom,
                    distance: 8.0,
                },
            ] {
                let mut r = Rig::new();
                r.begin(effect);
                r.frame(0, NEW);
                r.frame(30, NEW);
                // This frame is drawn as always, and the switch is told the region moved.
                let plain = r.plain(NEW);
                r.clock.advance_ms(10);
                r.anim.begin_frame(r.clock.now());
                {
                    let mut p = Painter::new(&mut r.bmp, &mut r.gfx, 1.0);
                    p.fill(Rect::new(0.0, 0.0, 60.0, 30.0), [9, 9, 9]);
                    p.fill(REGION, NEW);
                    p.fill(Rect::new(49.0, 5.0, 1.0, 20.0), [255, 255, 255]);
                    assert!(!r.tr.apply(&mut r.anim, &mut p, moved), "{moved:?}");
                }
                assert_eq!(r.anim.end_frame(), None);
                assert!(!r.tr.running());
                assert_eq!(r.bmp.pixels, plain, "{moved:?} {effect:?}");
            }
        }

        // Another scale (the window went to another screen), the device rectangle as it was.
        let mut r = Rig::new();
        r.begin(PageEffect::SlideFade {
            from: Edge::Right,
            distance: 20.0,
        });
        r.frame(0, NEW);
        r.frame(30, NEW);
        let before = r.plain(NEW);
        r.bmp.pixels.copy_from_slice(&before);
        r.clock.advance_ms(10);
        r.anim.begin_frame(r.clock.now());
        assert!(
            !r.tr
                .apply_dev(&mut r.anim, &mut r.bmp, (10, 5, 50, 25), 1.25)
        );
        assert!(!r.tr.running());
        assert_eq!(r.bmp.pixels, before);
        // The same rectangle and scale would have gone on.
        r.anim.end_frame();
        r.frame(0, OLD);
        r.begin(PageEffect::Crossfade);
        r.frame(0, NEW);
        r.clock.advance_ms(50);
        r.anim.begin_frame(r.clock.now());
        r.bmp.pixels.copy_from_slice(&before);
        assert!(r.tr.apply_dev(&mut r.anim, &mut r.bmp, (10, 5, 50, 25), 1.0));
        assert_eq!(at(&r.bmp, 20, 10), [100, 0, 100]);
        // A region that reaches past the bitmap is held to as it was cut at the start.
        assert!(r.tr.apply_dev(&mut r.anim, &mut r.bmp, (10, 5, 50, 25), 1.0));
        r.anim.end_frame();
        let mut r = Rig::new();
        r.tr.begin_dev(
            &mut r.anim,
            &r.bmp,
            (40, 20, 90, 70),
            1.0,
            PageEffect::Crossfade,
        );
        r.anim.begin_frame(r.clock.now());
        assert!(r.tr.apply_dev(&mut r.anim, &mut r.bmp, (40, 20, 90, 70), 1.0));
        assert!(
            !r.tr
                .apply_dev(&mut r.anim, &mut r.bmp, (40, 20, 55, 70), 1.0)
        );
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

    #[test]
    fn a_rebase_keeps_the_progress_and_an_empty_or_idle_one_does_not() {
        // Nothing runs: there is no old page to be of another size, and a rebase does nothing.
        let mut r = Rig::new();
        assert!(r.tr.fits(&r.bmp));
        let old = Snapshot::take(&r.bmp, 10, 5, 50, 25);
        assert!(!r.tr.rebase(&mut r.anim, old, 1.0));
        assert!(!r.tr.running());

        // A running switch fits the window it began on, and not one of another size.
        r.begin(PageEffect::Crossfade);
        r.frame(0, NEW);
        r.frame(30, NEW);
        let t: f32 = r.anim.get("page").expect("the progress is the animator's");
        assert!((0.25..0.35).contains(&t), "about 0.3 of the way: {t}");
        assert!(r.tr.fits(&r.bmp));
        r.bmp = Bitmap::new(80, 40);
        assert!(!r.tr.fits(&r.bmp), "another size");

        // The old page drawn again at the new size, handed over: the same switch, the same
        // progress, and the new size from now on.
        let old = Snapshot::take(&r.bmp, 10, 5, 70, 35);
        assert!(r.tr.rebase(&mut r.anim, old, 1.0));
        assert!(r.tr.running());
        assert!(r.tr.fits(&r.bmp));
        let same: f32 = r.anim.get("page").expect("the progress is still there");
        assert_eq!(t, same, "the progress carries on");

        // An empty snapshot ends it, and its progress goes with it.
        assert!(!r.tr.rebase(&mut r.anim, Snapshot::default(), 1.0));
        assert!(!r.tr.running());
        assert!(r.anim.is_empty(), "its progress is forgotten");

        // So does reduced motion asked for in the middle.
        r.begin(PageEffect::Crossfade);
        r.frame(0, NEW);
        r.anim.set_system_reduced(true);
        let old = Snapshot::take(&r.bmp, 10, 5, 70, 35);
        assert!(!r.tr.rebase(&mut r.anim, old, 1.0));
        assert!(!r.tr.running());
        r.anim.set_system_reduced(false);

        // A new scale is taken too: after a move to another screen the snapshot's rectangle
        // and the frame's are both in the new scale's device pixels.
        r.bmp = Bitmap::new(80, 40);
        r.begin(PageEffect::Crossfade);
        r.clock.advance_ms(10);
        r.anim.begin_frame(r.clock.now());
        assert!(r.tr.apply_dev(&mut r.anim, &mut r.bmp, (10, 5, 50, 25), 1.0));
        let old = Snapshot::take(&r.bmp, 15, 8, 75, 38);
        assert!(r.tr.rebase(&mut r.anim, old, 1.5));
        assert!(
            r.tr.apply_dev(&mut r.anim, &mut r.bmp, (15, 8, 75, 38), 1.5),
            "the new scale is the one in force"
        );
        // And a rectangle that is not the snapshot's still ends it.
        assert!(
            !r.tr
                .apply_dev(&mut r.anim, &mut r.bmp, (10, 5, 70, 35), 1.5)
        );
        assert!(!r.tr.running());
        r.anim.end_frame();
    }

    #[test]
    fn an_idle_rebase_does_not_forget_a_finished_switch() {
        let mut r = Rig::new();
        r.begin(PageEffect::Crossfade);
        r.frame(0, NEW);
        r.frame(50, NEW);
        assert!(!r.frame(50, NEW), "at the end of its time");
        assert!(!r.tr.running());
        // The finished switch leaves its track at rest in the animator, and an idle rebase
        // leaves that track alone rather than forgetting it.
        let done: Option<f32> = r.anim.get("page");
        assert_eq!(done, Some(1.0), "the finished track is the animator's");
        let old = Snapshot::take(&r.bmp, 10, 5, 50, 25);
        assert!(!r.tr.rebase(&mut r.anim, old, 1.0));
        assert!(!r.tr.running());
        assert_eq!(r.anim.get::<f32>("page"), done, "and it is still there");
        assert!(!r.anim.is_empty());
    }

    #[test]
    fn a_resize_mid_switch_draws_the_old_page_at_the_new_size_and_does_not_stretch_it() {
        // The switch runs 40 ms of its 100 on the 60 by 30 window.
        let mut r = Rig::new();
        r.begin(PageEffect::Crossfade);
        r.frame(0, NEW);
        assert!(r.frame(40, NEW));
        assert!(r.tr.running());

        // The window grows to 80 by 40: the application draws the old page again, laid out
        // for the new size, with its green block of ten px against the region's new right
        // edge.
        let region = Rect::new(10.0, 5.0, 60.0, 30.0);
        r.bmp = Bitmap::new(80, 40);
        r.region = region;
        {
            let mut p = Painter::new(&mut r.bmp, &mut r.gfx, 1.0);
            draw_old(&mut p, region);
        }
        assert!(!r.tr.fits(&r.bmp), "the saved old page is of the old size");

        // The next frame is at 50 ms, half way: it hands the old page over and goes on,
        // rather than being cut short.
        assert!(r.frame_resized(10, NEW));
        assert!(r.tr.running(), "the switch was not cut short");
        assert!(r.tr.fits(&r.bmp), "and it follows the new size now");

        // A switch that had begun on the 80 by 40 window itself, at the same moment.
        let mut want = Rig::sized(80, 40, region);
        want.begin(PageEffect::Crossfade);
        want.frame(0, NEW);
        assert!(want.frame(50, NEW));
        assert!(want.tr.running());

        assert_eq!((r.bmp.width, r.bmp.height), (80, 40), "the new size");
        let differing = first_difference(&r.bmp, &want.bmp);
        assert!(
            differing.is_none(),
            "laid out anew, not stretched: {differing:?}"
        );
        // Where the old page is red at its new size too, the mix is half of each page...
        assert_eq!(at(&r.bmp, 57, 10), [100, 0, 100]);
        // ...and inside the green block, which a stretched 40 px page would have put at
        // x = 57 (fifteen wide) but a page drawn at the new size has from x = 60.
        assert_eq!(at(&r.bmp, 65, 10), [0, 100, 100]);
        // Nothing of the old page is pasted outside the region.
        assert_eq!(at(&r.bmp, 75, 35), BACK);
    }
}
