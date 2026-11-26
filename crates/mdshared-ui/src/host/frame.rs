//! What decides a frame and sends it: the damage (each rectangle kept, as well as the one that
//! holds them all), the two deadlines that wake the loop, whether the window is seen at all, and
//! the copy of the bitmap into the window's buffer. All but [`present`] are pure and tested in
//! `host_tests.rs`.

use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::Instant;

use mdshared_text::Bitmap;
use winit::window::Window;

use super::input::clean_scale;
use super::timers::NamedTimers;
use crate::frost;
use crate::gfx::Rect;

/// The most pieces a frame keeps before it falls back to their union. Every `request_redraw`
/// adds a piece when the application draws regions, and a window left minimized or hidden for
/// long would otherwise collect them without end.
pub(super) const KEEP_PIECES: usize = 16;

/// The rectangles asked for before the next frame: the one that holds them all (what a frame
/// drawn whole is given) and each rectangle on its own (what an application that draws regions
/// separately is given, one call each).
///
/// Pieces are kept only for an application that asked ([`Damage::with_pieces`]); every other
/// one pays one rectangle and no allocation, as before, and its pieces are empty. Pieces that
/// overlap are joined into one as they are added, and the list is capped at [`KEEP_PIECES`]:
/// beyond that the union is drawn as one rectangle, so a long-hidden window cannot grow it
/// without bound.
///
/// Pieces are disjoint in logical px. At a fractional scale the outward rounding can make two
/// neighbours share one device column: the present then copies that column twice (the same
/// pixels), which is harmless. Rectangles that only touch, or that are apart, stay pieces of
/// their own.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Damage {
    /// The rectangle holding every piece; what the old one-rectangle frame drew.
    pub(super) rect: Option<Rect>,
    pub(super) full: bool,
    /// The pieces, overlapping ones already joined. Empty while `full` is set, while the list
    /// overflowed, and for an application that did not ask for regions.
    regions: Vec<Rect>,
    /// Whether pieces are kept at all (the application's `damage_regions`).
    collect: bool,
    /// More than [`KEEP_PIECES`] disjoint pieces were asked for: the union is drawn instead.
    capped: bool,
}

impl Damage {
    /// Damage for an application that does (`true`) or does not draw regions separately.
    pub(super) fn with_pieces(collect: bool) -> Damage {
        Damage {
            collect,
            ..Damage::default()
        }
    }

    /// Whether this damage keeps pieces.
    pub(super) fn collects(&self) -> bool {
        self.collect
    }

    /// Empties it for the next frame, keeping whether pieces are collected.
    pub(super) fn reset(&mut self) {
        self.rect = None;
        self.full = false;
        self.capped = false;
        self.regions.clear();
    }

    /// Takes what was asked for and leaves it empty for the next frame, keeping whether pieces
    /// are collected.
    pub(super) fn take(&mut self) -> Damage {
        std::mem::replace(self, Damage::with_pieces(self.collect))
    }

    /// Adds `r`. A rectangle with a side that is not a positive number, or with a corner that
    /// is not a number, adds nothing.
    pub(super) fn add(&mut self, r: Rect) {
        let finite = r.x.is_finite() && r.y.is_finite() && r.w.is_finite() && r.h.is_finite();
        if !finite || r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        self.rect = Some(match self.rect {
            None => r,
            Some(o) => join(o, r),
        });
        // A whole-window frame already covers everything; only the union is needed then.
        if self.collect && !self.full && !self.capped {
            self.add_piece(r);
        }
    }

    /// Adds `r` to the pieces, joining it with every piece it overlaps. A join makes `r` larger,
    /// which can reach a piece the scan has already passed, so the scan restarts from the top.
    fn add_piece(&mut self, mut r: Rect) {
        let mut i = 0;
        while i < self.regions.len() {
            if overlaps(self.regions[i], r) {
                r = join(self.regions.remove(i), r);
                i = 0;
            } else {
                i += 1;
            }
        }
        if self.regions.len() >= KEEP_PIECES {
            // Too many to be worth several draws: the union, one rectangle.
            self.capped = true;
            self.regions.clear();
            return;
        }
        self.regions.push(r);
    }

    /// The whole window.
    pub(super) fn all(&mut self) {
        self.full = true;
        self.capped = false;
        self.regions.clear();
    }

    pub(super) fn is_empty(&self) -> bool {
        !self.full && self.rect.is_none()
    }

    /// The pieces asked for, in the order they were added (overlaps joined). Empty while the
    /// whole window is due, after the list overflowed, and when pieces are not collected.
    pub(super) fn regions(&self) -> &[Rect] {
        &self.regions
    }

    /// Joins `other` in.
    pub(super) fn merge(&mut self, other: Damage) {
        if other.full {
            self.all();
            return;
        }
        if self.full {
            return;
        }
        // `other`'s pieces carry its union too; the fallback covers a union set without a
        // piece (a full window, an overflowed list, or an application that kept none).
        if other.regions.is_empty() {
            if let Some(r) = other.rect {
                self.add(r);
            }
            return;
        }
        for r in other.regions {
            self.add(r);
        }
    }

    /// The damage in device px of a `w` x `h` bitmap at `scale`: x0, y0, x1, y1, rounded
    /// outwards and cut to the bitmap. `None` when nothing of it is in the bitmap.
    pub(super) fn device(&self, scale: f32, w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
        if w == 0 || h == 0 {
            return None;
        }
        if self.full {
            return Some((0, 0, w, h));
        }
        rect_device(self.rect?, scale, w, h)
    }

    /// The damage in device px, one rectangle per piece (the whole bitmap when `full`, the
    /// union when the list overflowed), each rounded outwards and cut to the bitmap. Pieces
    /// wholly outside the bitmap are left out.
    pub(super) fn pieces(&self, scale: f32, w: u32, h: u32) -> Vec<(u32, u32, u32, u32)> {
        if w == 0 || h == 0 {
            return Vec::new();
        }
        if self.full {
            return vec![(0, 0, w, h)];
        }
        if self.capped {
            return self.device(scale, w, h).into_iter().collect();
        }
        self.regions()
            .iter()
            .filter_map(|r| rect_device(*r, scale, w, h))
            .collect()
    }
}

/// The least rectangle holding both.
fn join(a: Rect, b: Rect) -> Rect {
    let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
    let (x1, y1) = (a.right().max(b.right()), a.bottom().max(b.bottom()));
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

/// Whether the two rectangles share area (touching edges do not count).
fn overlaps(a: Rect, b: Rect) -> bool {
    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
}

/// `r` in device px of a `w` x `h` bitmap at `scale`, rounded outwards and cut to the bitmap.
fn rect_device(r: Rect, scale: f32, w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
    let s = clean_scale(f64::from(scale));
    // `as` saturates: a rectangle far outside the bitmap is cut to it.
    let x0 = ((r.x * s).floor().max(0.0) as u32).min(w);
    let y0 = ((r.y * s).floor().max(0.0) as u32).min(h);
    let x1 = ((r.right() * s).ceil().max(0.0) as u32).min(w);
    let y1 = ((r.bottom() * s).ceil().max(0.0) as u32).min(h);
    (x0 < x1 && y0 < y1).then_some((x0, y0, x1, y1))
}

/// Whether a frame is wanted now: there is damage and the window can be seen. A window the
/// application hid is not drawn, though its timers still come. Both the redraw request and
/// `after` read this one decision.
pub(super) fn wants_frame(hidden: bool, damage: &Damage) -> bool {
    !hidden && !damage.is_empty()
}

/// When the loop must wake next: the earliest of the animator's and one-shot deadlines
/// ([`Deadlines::next`]) and the earliest named timer. A hidden window leaves out the
/// animator's deadline inside `Deadlines::next`; named timers always count.
pub(super) fn next_wake(
    deadlines: &Deadlines,
    timers: &NamedTimers,
    hidden: bool,
) -> Option<Instant> {
    match (deadlines.next(hidden), timers.next()) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// Whether the window can be seen: not covered (winit's `Occluded`, which winit 0.30 sends on
/// some systems only), not minimized (which every system shows as a size of zero or
/// `is_minimized`), and not hidden by the application (`Cx::hide_window`, close to tray). A
/// window that cannot be seen is not drawn, though its timers and worker messages still come.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Visibility {
    occluded: bool,
    minimized: bool,
    /// The application hid the window on purpose.
    app: bool,
}

impl Visibility {
    /// True while the window is covered, minimized, or hidden by the application.
    pub(super) fn hidden(self) -> bool {
        self.occluded || self.minimized || self.app
    }

    /// The application hid the window (`true`) or showed it again. True when that makes it seen
    /// again: the caller then redraws everything.
    pub(super) fn set_app_hidden(&mut self, hidden: bool) -> bool {
        let was = self.hidden();
        self.app = hidden;
        was && !self.hidden()
    }

    /// Whether the application hid the window (whether or not it is also covered or minimized).
    pub(super) fn app_hidden(self) -> bool {
        self.app
    }

    /// Whether the system hid it: covered or minimized, whatever the application asked.
    pub(super) fn os_hidden(self) -> bool {
        self.occluded || self.minimized
    }

    /// The window was covered (`true`) or uncovered. True when that makes it seen again: the
    /// caller then redraws everything.
    pub(super) fn set_occluded(&mut self, occluded: bool) -> bool {
        let was = self.hidden();
        self.occluded = occluded;
        was && !self.hidden()
    }

    /// The window was minimized (`true`) or restored. True when that makes it seen again.
    pub(super) fn set_minimized(&mut self, minimized: bool) -> bool {
        let was = self.hidden();
        self.minimized = minimized;
        was && !self.hidden()
    }
}

/// Whether a window whose client area is `size` device px, and which says `is_minimized`
/// (winit's answer, `None` where the system does not say), is minimized: a size of zero either
/// way is (Windows reports a minimized window so), or the system says it.
pub(super) fn minimized(size: (u32, u32), is_minimized: Option<bool>) -> bool {
    size.0 == 0 || size.1 == 0 || is_minimized == Some(true)
}

/// The two things that wake the loop: the animator's deadline and the application's timer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Deadlines {
    pub(super) anim: Option<Instant>,
    pub(super) timer: Option<Instant>,
}

/// Which of the [`Deadlines`] came.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Due {
    pub(super) anim: bool,
    pub(super) timer: bool,
}

impl Deadlines {
    /// Asks for the timer at `at`; the earliest asked for wins.
    pub(super) fn wake_at(&mut self, at: Instant) {
        self.timer = Some(self.timer.map_or(at, |t| t.min(at)));
    }

    /// When the loop must wake next: the earlier of the two. The animator's is left out while
    /// the window cannot be seen (`hidden`), since nothing is drawn then. `None`: wait for an
    /// event.
    pub(super) fn next(&self, hidden: bool) -> Option<Instant> {
        let anim = self.anim.filter(|_| !hidden);
        match (anim, self.timer) {
            (Some(a), Some(t)) => Some(a.min(t)),
            (a, t) => a.or(t),
        }
    }

    /// Takes the deadlines that have come by `now`. A hidden window's animator deadline is
    /// dropped instead: the window is drawn whole when it is seen again.
    pub(super) fn take_due(&mut self, now: Instant, hidden: bool) -> Due {
        let anim = self.anim.is_some_and(|a| a <= now);
        let timer = self.timer.is_some_and(|t| t <= now);
        if anim || hidden {
            self.anim = None;
        }
        if timer {
            self.timer = None;
        }
        Due {
            anim: anim && !hidden,
            timer,
        }
    }
}

/// Copies the device rectangle `r` (x0, y0, x1, y1, inside both) of `src`, BGRA bytes in rows of
/// `stride` pixels, into `dst`, pixels of `0xAARRGGBB` in rows of the same stride.
pub(super) fn copy_rect(
    src: &[u8],
    dst: &mut [u32],
    stride: usize,
    r: (usize, usize, usize, usize),
) {
    let (x0, y0, x1, y1) = r;
    let rows = (dst.len() / stride.max(1)).min(src.len() / (stride * 4).max(1));
    let (x1, y1) = (x1.min(stride), y1.min(rows));
    if x0 >= x1 {
        return;
    }
    for y in y0..y1 {
        let from = &src[(y * stride + x0) * 4..(y * stride + x1) * 4];
        let to = &mut dst[y * stride + x0..y * stride + x1];
        for (d, px) in to.iter_mut().zip(from.as_chunks::<4>().0) {
            *d = u32::from_le_bytes(*px);
        }
    }
}

/// Copies the device rectangles `devs` of `bmp` to the window and presents them: the whole
/// buffer when the window's buffer does not hold the last frame (its age is not 1). `key` takes
/// the frosted chrome's flat colour out ([`frost::key_out`]).
pub(super) fn present(
    surface: &mut softbuffer::Surface<Rc<Window>, Rc<Window>>,
    bmp: &Bitmap,
    devs: &[(u32, u32, u32, u32)],
    key: Option<&frost::Key>,
) {
    if devs.is_empty() {
        return;
    }
    let (w, h) = (bmp.width, bmp.height);
    let (Some(nw), Some(nh)) = (NonZeroU32::new(w), NonZeroU32::new(h)) else {
        return;
    };
    if surface.resize(nw, nh).is_err() {
        return;
    }
    let Ok(mut buffer) = surface.buffer_mut() else {
        return;
    };
    // A buffer that does not hold the last frame needs all of it, whichever pieces changed.
    let rects: Vec<(u32, u32, u32, u32)> = if buffer.age() == 1 {
        devs.to_vec()
    } else {
        vec![(0, 0, w, h)]
    };
    for &d in &rects {
        let r = (d.0 as usize, d.1 as usize, d.2 as usize, d.3 as usize);
        copy_rect(&bmp.pixels, &mut buffer, w as usize, r);
        if let Some(key) = key {
            frost::key_out(key, &mut buffer, w as usize, r);
        }
    }
    let damage: Vec<softbuffer::Rect> = rects
        .iter()
        .filter_map(|&(x0, y0, x1, y1)| {
            Some(softbuffer::Rect {
                x: x0,
                y: y0,
                width: NonZeroU32::new(x1.checked_sub(x0)?)?,
                height: NonZeroU32::new(y1.checked_sub(y0)?)?,
            })
        })
        .collect();
    if !damage.is_empty() {
        let _ = buffer.present_with_damage(&damage);
    }
}
