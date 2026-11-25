//! What decides a frame and sends it: the damage, the two deadlines that wake the loop, whether
//! the window is seen at all, and the copy of the bitmap into the window's buffer. All but
//! [`present`] are pure and tested in `host_tests.rs`.

use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::Instant;

use mdshared_text::Bitmap;
use winit::window::Window;

use super::input::clean_scale;
use crate::frost;
use crate::gfx::Rect;

/// The rectangles asked for before the next frame, joined into the one that holds them all.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Damage {
    pub(super) rect: Option<Rect>,
    pub(super) full: bool,
}

impl Damage {
    /// Adds `r`. A rectangle with a side that is not a positive number, or with a corner that
    /// is not a number, adds nothing.
    pub(super) fn add(&mut self, r: Rect) {
        let finite = r.x.is_finite() && r.y.is_finite() && r.w.is_finite() && r.h.is_finite();
        if !finite || r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        self.rect = Some(match self.rect {
            None => r,
            Some(o) => {
                let (x0, y0) = (o.x.min(r.x), o.y.min(r.y));
                let (x1, y1) = (o.right().max(r.right()), o.bottom().max(r.bottom()));
                Rect::new(x0, y0, x1 - x0, y1 - y0)
            }
        });
    }

    /// The whole window.
    pub(super) fn all(&mut self) {
        self.full = true;
    }

    pub(super) fn is_empty(&self) -> bool {
        !self.full && self.rect.is_none()
    }

    /// Joins `other` in.
    pub(super) fn merge(&mut self, other: Damage) {
        self.full |= other.full;
        if let Some(r) = other.rect {
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
        let r = self.rect?;
        let s = clean_scale(f64::from(scale));
        // `as` saturates: a rectangle far outside the bitmap is cut to it.
        let x0 = ((r.x * s).floor().max(0.0) as u32).min(w);
        let y0 = ((r.y * s).floor().max(0.0) as u32).min(h);
        let x1 = ((r.right() * s).ceil().max(0.0) as u32).min(w);
        let y1 = ((r.bottom() * s).ceil().max(0.0) as u32).min(h);
        (x0 < x1 && y0 < y1).then_some((x0, y0, x1, y1))
    }
}

/// Whether the window can be seen: not covered (winit's `Occluded`, which winit 0.30 sends on
/// some systems only) and not minimized (which every system shows as a size of zero or
/// `is_minimized`). A window that cannot be seen is not drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Visibility {
    occluded: bool,
    minimized: bool,
}

impl Visibility {
    /// True while the window is covered or minimized.
    pub(super) fn hidden(self) -> bool {
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

/// Copies the device rectangle `dev` of `bmp` to the window and presents that rectangle: all of
/// it when the window's buffer does not hold the last frame (its age is not 1). `key` takes the
/// frosted chrome's flat colour out ([`frost::key_out`]).
pub(super) fn present(
    surface: &mut softbuffer::Surface<Rc<Window>, Rc<Window>>,
    bmp: &Bitmap,
    dev: (u32, u32, u32, u32),
    key: Option<&frost::Key>,
) {
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
    let r = if buffer.age() == 1 { dev } else { (0, 0, w, h) };
    let r = (r.0 as usize, r.1 as usize, r.2 as usize, r.3 as usize);
    copy_rect(&bmp.pixels, &mut buffer, w as usize, r);
    if let Some(key) = key {
        frost::key_out(key, &mut buffer, w as usize, r);
    }
    let (Some(rw), Some(rh)) = (
        NonZeroU32::new((r.2 - r.0) as u32),
        NonZeroU32::new((r.3 - r.1) as u32),
    ) else {
        return;
    };
    let rect = softbuffer::Rect {
        x: r.0 as u32,
        y: r.1 as u32,
        width: rw,
        height: rh,
    };
    let _ = buffer.present_with_damage(&[rect]);
}
