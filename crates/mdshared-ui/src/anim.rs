//! Animation: values that move to their targets over a few frames, for an application that draws
//! a whole frame at a time and sleeps between events.
//!
//! The model is immediate, like the drawing. There is no list of running animations to start,
//! stop and clean up. While it draws, the application asks the [`Animator`] for the current
//! value of a thing it names ("where is the selection pill of this switch now?") and says where
//! that thing should be; the animator remembers each named value from frame to frame, moves it
//! by the [`Motion`] asked for, and at the end of the frame says whether another frame is needed
//! and when. When every value has arrived it says "none", the application goes back to
//! `ControlFlow::Wait`, and an idle window costs nothing.
//!
//! One frame:
//!
//! ```
//! # use mdshared_ui::anim::{Animator, Clock, ManualClock, fluent};
//! # use std::time::Duration;
//! # let clock = ManualClock::new();
//! # let mut anim = Animator::new();
//! # let hovered = true;
//! anim.begin_frame(clock.now());
//! // ... while drawing:
//! let glow = anim.toggle("save-button-hover", hovered, fluent::FADE);
//! # let _ = glow;
//! // ... after drawing:
//! match anim.end_frame() {
//!     Some(deadline) => { /* ControlFlow::WaitUntil(deadline), then request a redraw */ }
//!     None => { /* ControlFlow::Wait: nothing moves */ }
//! }
//! # let _ = Duration::ZERO;
//! ```
//!
//! The pieces: [`ease`] (easing curves and the CSS cubic-bezier solver), [`spring`], this file
//! (the clock, [`Motion`], [`Animatable`], the [`Animator`] and the [`fluent`] presets),
//! [`region`] (a saved piece of the bitmap and the blends of it), [`page`] (a page or tab
//! switch), [`presence`] (things that appear and disappear) and [`cycle`] (things that repeat
//! for as long as they are shown: a spinner, a blinking caret). `docs/ANIMATION.md` shows how an
//! application wires it into its window loop.

pub mod cycle;
pub mod ease;
pub mod page;
pub mod presence;
pub mod region;
pub mod spring;

pub use cycle::Periodic;
pub use ease::{Bezier, Easing, cubic_bezier};
pub use page::{Edge, PageEffect, PageTransition};
pub use presence::{Transient, UNFOLD_FROM, pop, unfold};
pub use region::Snapshot;
pub use spring::Spring;

use crate::gfx::{Rect, Rgb};
use std::cell::Cell;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

// ---- time --------------------------------------------------------------------------------------

/// Where the time of a frame comes from. An application uses [`SystemClock`]; a test, or a
/// render that must come out the same every time (a screenshot of a half-way state), uses a
/// [`ManualClock`] and moves it by hand.
pub trait Clock {
    fn now(&self) -> Instant;
}

/// The real time: `Instant::now()`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// A clock that stands still until it is moved. Its instants are real `Instant`s (the moment it
/// was made, plus what it was advanced by), so the deadline an [`Animator`] gives back can be
/// compared with them; only the differences between them mean anything, and those are exactly
/// what the clock was advanced by.
#[derive(Clone, Debug)]
pub struct ManualClock {
    start: Instant,
    elapsed: Cell<Duration>,
}

impl Default for ManualClock {
    fn default() -> ManualClock {
        ManualClock::new()
    }
}

impl ManualClock {
    pub fn new() -> ManualClock {
        ManualClock {
            start: Instant::now(),
            elapsed: Cell::new(Duration::ZERO),
        }
    }

    /// Moves the clock forward by `by`.
    pub fn advance(&self, by: Duration) {
        self.elapsed.set(self.elapsed.get() + by);
    }

    /// Moves the clock forward by `ms` milliseconds.
    pub fn advance_ms(&self, ms: u64) {
        self.advance(Duration::from_millis(ms));
    }

    /// Puts the clock at `elapsed` after its start (it may go back).
    pub fn set(&self, elapsed: Duration) {
        self.elapsed.set(elapsed);
    }

    /// How far the clock is from its start.
    pub fn elapsed(&self) -> Duration {
        self.elapsed.get()
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Instant {
        self.start + self.elapsed.get()
    }
}

// ---- names -------------------------------------------------------------------------------------

/// The name of one animated value. Anything that hashes names a value, so the calls take
/// `impl AsId` and a string, a number or a tuple of them does (`"sidebar"`, `("tab", 3)`); an
/// `Id` is that name already hashed, for a widget that is handed one name and needs several
/// values under it ([`with`](Id::with)). The hash has fixed keys: the same name is the same id in
/// every run.
///
/// An `Id` given where a name is asked for is itself, not hashed again: `anim.get(Id::new("a"))`
/// is `anim.get("a")`, and a value a widget keeps under `Id::new(name).with("fill")` can be read
/// by anyone who makes the same `Id`. For that an `Id` does not implement `Hash` (a thing that
/// hashes is a name to be hashed): a name under an `Id` is `id.with(part)`, not `(id, part)`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Id(u64);

impl Id {
    /// The id of `name`: its hash, or `name` itself when it is an `Id` already.
    pub fn new(name: impl AsId) -> Id {
        name.as_id()
    }

    /// A value under this name: `Id::new("switch").with("pill")`.
    pub fn with(self, part: impl Hash) -> Id {
        hashed(&(self.0, part))
    }
}

fn hashed(name: &(impl Hash + ?Sized)) -> Id {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut h);
    Id(h.finish())
}

/// What can name an animated value: anything that hashes (a string, a number, a tuple of
/// them), which is hashed to its [`Id`], and an [`Id`], which is itself. Every call that takes
/// a name takes `impl AsId`, so a widget that has made an `Id` passes it on at no cost and with
/// no second hash.
pub trait AsId {
    /// The id this names.
    fn as_id(&self) -> Id;
}

impl AsId for Id {
    fn as_id(&self) -> Id {
        *self
    }
}

impl AsId for &Id {
    fn as_id(&self) -> Id {
        **self
    }
}

impl<T: Hash + ?Sized> AsId for T {
    fn as_id(&self) -> Id {
        hashed(self)
    }
}

// ---- what can move -----------------------------------------------------------------------------

/// A value an [`Animator`] can move: one that is up to four numbers, each of which moves on its
/// own. Implemented for `f32` and `f64`, a pair of `f32` (a point or a size), a [`Rect`], an
/// [`Rgb`] colour and `[f32; 4]`.
///
/// The numbers are kept as `f64` while they move, whatever the value's own type. An `f32` holds
/// whole numbers only up to sixteen million, and has steps of four at fifty million: a scroll
/// offset far down a long list (a terminal's scrollback, a folder of a hundred thousand rows)
/// would move in jumps, or not at all. A value that can be that large is asked for as an `f64`;
/// the usual values of a widget (a fraction, a width, a colour) stay `f32` and lose nothing.
///
/// A colour moves channel by channel in the stored (gamma) space, the space
/// [`gfx::mix`](crate::gfx::mix) and every blend of the painter work in. That is on purpose: a
/// colour half way between two others is then exactly what a box of the second colour drawn at
/// half alpha over the first gives, so a thing that changes colour and a thing that fades look
/// the same. The channels are kept as fractions while they move and rounded only when read.
pub trait Animatable: Copy {
    /// The value as numbers; the ones it does not use are zero.
    fn parts(self) -> [f64; 4];
    /// The value those numbers stand for.
    fn from_parts(parts: [f64; 4]) -> Self;

    /// How close to its target, in the numbers' own unit, a value of this type is as good as
    /// there: a difference that small cannot be seen. A spring has no fixed end, so this is
    /// what ends it: the value is put on its target, and the window stops drawing frames, once
    /// every number is this close (and too slow to leave again). It is a thousandth unless the
    /// type says otherwise, which suits a fraction from 0 to 1; a position in pixels says a
    /// quarter of a pixel, a colour just under half a step of a channel (the colour read is
    /// then already the target's). A spring given a coarser
    /// [`threshold`](Spring::with_threshold) of its own uses that.
    fn precision() -> f64 {
        0.001
    }

    /// The value `t` of the way from `self` to `to` (`t` is not clamped: an easing that
    /// overshoots is followed).
    fn lerp(self, to: Self, t: f32) -> Self {
        Self::from_parts(lerp_parts(self.parts(), to.parts(), t))
    }
}

fn lerp_parts(a: [f64; 4], b: [f64; 4], t: f32) -> [f64; 4] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t as f64)
}

/// A bare number is taken for a fraction or another small quantity: its
/// [`precision`](Animatable::precision) is a thousandth. One that is a position in pixels on a
/// spring is given a spring with a threshold of its own (`Spring::with_threshold(0.25)`), or it
/// asks for a few frames more than can be seen.
impl Animatable for f32 {
    fn parts(self) -> [f64; 4] {
        [self as f64, 0.0, 0.0, 0.0]
    }
    fn from_parts(p: [f64; 4]) -> f32 {
        p[0] as f32
    }
}

/// A number that may be too large for an `f32` to hold exactly: a scroll offset in a very long
/// list. As for `f32`, its [`precision`](Animatable::precision) is a thousandth.
impl Animatable for f64 {
    fn parts(self) -> [f64; 4] {
        [self, 0.0, 0.0, 0.0]
    }
    fn from_parts(p: [f64; 4]) -> f64 {
        p[0]
    }
}

/// A point or a size in pixels: as good as there within a quarter of a pixel.
impl Animatable for (f32, f32) {
    fn parts(self) -> [f64; 4] {
        [self.0 as f64, self.1 as f64, 0.0, 0.0]
    }
    fn from_parts(p: [f64; 4]) -> (f32, f32) {
        (p[0] as f32, p[1] as f32)
    }
    fn precision() -> f64 {
        0.25
    }
}

impl Animatable for [f32; 4] {
    fn parts(self) -> [f64; 4] {
        self.map(f64::from)
    }
    fn from_parts(p: [f64; 4]) -> [f32; 4] {
        p.map(|v| v as f32)
    }
}

/// A rectangle in pixels: as good as there within a quarter of a pixel.
impl Animatable for Rect {
    fn parts(self) -> [f64; 4] {
        [self.x as f64, self.y as f64, self.w as f64, self.h as f64]
    }
    fn from_parts(p: [f64; 4]) -> Rect {
        Rect::new(p[0] as f32, p[1] as f32, p[2] as f32, p[3] as f32)
    }
    fn precision() -> f64 {
        0.25
    }
}

/// A colour: as good as there once every channel rounds to the target's.
impl Animatable for Rgb {
    fn parts(self) -> [f64; 4] {
        [self[0] as f64, self[1] as f64, self[2] as f64, 0.0]
    }
    fn from_parts(p: [f64; 4]) -> Rgb {
        let ch = |v: f64| v.round().clamp(0.0, 255.0) as u8;
        [ch(p[0]), ch(p[1]), ch(p[2])]
    }
    fn precision() -> f64 {
        0.49
    }
}

// ---- how it moves ------------------------------------------------------------------------------

/// How a value gets to its target: at once, over a fixed time along an [`Easing`], or on a
/// [`Spring`]; and how long it waits before it starts.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Motion {
    kind: Kind,
    delay: Duration,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Snap,
    Tween { duration: Duration, easing: Easing },
    Spring(Spring),
}

impl Motion {
    /// No motion: the value is its target.
    pub const SNAP: Motion = Motion {
        kind: Kind::Snap,
        delay: Duration::ZERO,
    };

    /// From where the value is to its target in `duration`, along `easing`. Given a new target
    /// on the way, it starts again from where it is (no jump) and takes the whole duration.
    pub const fn tween(duration: Duration, easing: Easing) -> Motion {
        Motion {
            kind: Kind::Tween { duration, easing },
            delay: Duration::ZERO,
        }
    }

    /// The same, with the duration in milliseconds.
    pub const fn tween_ms(ms: u64, easing: Easing) -> Motion {
        Motion::tween(Duration::from_millis(ms), easing)
    }

    /// On a spring. Given a new target on the way, it keeps its speed.
    pub const fn spring(spring: Spring) -> Motion {
        Motion {
            kind: Kind::Spring(spring),
            delay: Duration::ZERO,
        }
    }

    /// The same motion, started `delay` after the target changes; until then the value stays
    /// where it is, and the window sleeps until the delay is over (one wake-up, not a frame
    /// every sixtieth of a second).
    ///
    /// A delay is a wait, not a movement, so it is always kept: `Motion::SNAP.delayed(d)` is
    /// the old value for `d` and then the new one at once, and with reduced motion every
    /// delayed motion is exactly that. A tooltip that appears half a second after the pointer
    /// stops still waits its half second when the system asks for less motion; it only does
    /// not fade.
    pub const fn delayed(mut self, delay: Duration) -> Motion {
        self.delay = delay;
        self
    }

    /// How long a tween takes, its delay included; `None` for a spring, which has no fixed
    /// length, and the delay alone (zero unless one was set) for [`SNAP`](Motion::SNAP). A
    /// length too long for a `Duration` is `Duration::MAX`.
    pub fn length(self) -> Option<Duration> {
        match self.kind {
            Kind::Snap => Some(self.delay),
            Kind::Tween { duration, .. } => Some(self.delay.saturating_add(duration)),
            Kind::Spring(_) => None,
        }
    }
}

/// The timings and curves of Windows 11, and motions made of them.
///
/// Sources, read on 2026-11-24:
///
/// - Microsoft Learn, "Timing and easing" (`windows/apps/design/motion/timing-and-easing`):
///   WinUI's `ControlFasterAnimationDuration` 83 ms, `ControlFastAnimationDuration` 167 ms and
///   `ControlNormalAnimationDuration` 250 ms; "Fast Out, Slow In" `cubic-bezier(0, 0, 0, 1)` for
///   what enters, "Slow Out, Fast In" `cubic-bezier(1, 0, 1, 1)` for what leaves.
/// - Microsoft Learn, "Motion in Windows 11" (`windows/apps/design/signature-experiences/motion`),
///   the table "Animation properties": entrances `cubic-bezier(0, 0, 0, 1)` at 167, 250 or
///   333 ms; point to point `cubic-bezier(0.55, 0.55, 0, 1)` at 167, 250 or 333 ms; a direct
///   exit `cubic-bezier(0, 0, 0, 1)` at 167 ms, always with a fade; a gentle exit
///   `cubic-bezier(1, 0, 1, 1)` at 167 ms; a fade in or out, linear, 83 ms.
/// - The Fluent 2 tokens for the web (`microsoft/fluentui`, `packages/tokens/src/global`,
///   `durations.ts` and `curves.ts`), in [`web`]: a second, rounder scale (50 to 500 ms).
///
/// The durations are whole frames of a 60 Hz screen: 5, 10, 15 and 20. `SLOW` (333 ms) has no
/// `Control…AnimationDuration` name on the pages read; it is the third timing of the table.
pub mod fluent {
    use super::{Easing, Motion};
    use std::time::Duration;

    /// 83 ms: a fade, a hover.
    pub const FASTER: Duration = Duration::from_millis(83);
    /// 167 ms: most changes of a control, and every exit.
    pub const FAST: Duration = Duration::from_millis(167);
    /// 250 ms: something larger entering or moving.
    pub const NORMAL: Duration = Duration::from_millis(250);
    /// 333 ms: a page, or something that crosses much of the window.
    pub const SLOW: Duration = Duration::from_millis(333);

    /// "Fast out, slow in": leaves its start at once and settles. For what enters.
    pub const DECELERATE: Easing = Easing::bezier(0.0, 0.0, 0.0, 1.0);
    /// "Slow out, fast in": gathers speed and is gone. For what leaves.
    pub const ACCELERATE: Easing = Easing::bezier(1.0, 0.0, 1.0, 1.0);
    /// For what stays on screen and moves from one place to another.
    pub const POINT_TO_POINT: Easing = Easing::bezier(0.55, 0.55, 0.0, 1.0);

    /// A fade in or out, a hover or a press: linear, 83 ms.
    pub const FADE: Motion = Motion::tween(FASTER, Easing::Linear);
    /// Something appearing (a menu, a tooltip, a toast, a dialog): 167 ms, decelerating.
    pub const ENTER: Motion = Motion::tween(FAST, DECELERATE);
    /// Something larger appearing: 250 ms, decelerating.
    pub const ENTER_LARGE: Motion = Motion::tween(NORMAL, DECELERATE);
    /// Something going away: 167 ms, accelerating.
    pub const EXIT: Motion = Motion::tween(FAST, ACCELERATE);
    /// A part of a control moving to a new place (a selection pill, a thumb): 167 ms.
    pub const MOVE: Motion = Motion::tween(FAST, POINT_TO_POINT);
    /// The same over a longer way: 250 ms.
    pub const MOVE_LARGE: Motion = Motion::tween(NORMAL, POINT_TO_POINT);
    /// A page or tab changing: 250 ms, decelerating.
    pub const PAGE: Motion = Motion::tween(NORMAL, DECELERATE);

    /// The Fluent 2 design tokens as the web library has them, for an application that wants
    /// the same numbers as a web page next to it.
    pub mod web {
        use super::Easing;
        use std::time::Duration;

        pub const DURATION_ULTRA_FAST: Duration = Duration::from_millis(50);
        pub const DURATION_FASTER: Duration = Duration::from_millis(100);
        pub const DURATION_FAST: Duration = Duration::from_millis(150);
        pub const DURATION_NORMAL: Duration = Duration::from_millis(200);
        pub const DURATION_GENTLE: Duration = Duration::from_millis(250);
        pub const DURATION_SLOW: Duration = Duration::from_millis(300);
        pub const DURATION_SLOWER: Duration = Duration::from_millis(400);
        pub const DURATION_ULTRA_SLOW: Duration = Duration::from_millis(500);

        pub const CURVE_ACCELERATE_MAX: Easing = Easing::bezier(0.9, 0.1, 1.0, 0.2);
        pub const CURVE_ACCELERATE_MID: Easing = Easing::bezier(1.0, 0.0, 1.0, 1.0);
        pub const CURVE_ACCELERATE_MIN: Easing = Easing::bezier(0.8, 0.0, 0.78, 1.0);
        pub const CURVE_DECELERATE_MAX: Easing = Easing::bezier(0.1, 0.9, 0.2, 1.0);
        pub const CURVE_DECELERATE_MID: Easing = Easing::bezier(0.0, 0.0, 0.0, 1.0);
        pub const CURVE_DECELERATE_MIN: Easing = Easing::bezier(0.33, 0.0, 0.1, 1.0);
        pub const CURVE_EASY_EASE_MAX: Easing = Easing::bezier(0.8, 0.0, 0.2, 1.0);
        pub const CURVE_EASY_EASE: Easing = Easing::bezier(0.33, 0.0, 0.67, 1.0);
        pub const CURVE_LINEAR: Easing = Easing::Linear;
    }
}

// ---- the animator ------------------------------------------------------------------------------

/// Whether an [`Animator`] moves anything.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Policy {
    /// Animate unless the system asks for less motion ([`Animator::set_system_reduced`]).
    #[default]
    System,
    /// Animate whatever the system says: the user chose so in the application.
    Always,
    /// Never animate: every value is its target at once. Also what a render that must not
    /// depend on time (a screenshot, a golden image) uses.
    Never,
}

/// One named value on its way.
#[derive(Clone, Copy, Debug)]
struct Track {
    value: [f64; 4],
    /// Per second; only a spring uses it.
    velocity: [f32; 4],
    target: [f64; 4],
    /// Where the tween in flight started.
    from: [f64; 4],
    /// How close to the target counts as there, from the value's type: what ends a spring.
    precision: f64,
    /// The animator's time at which the motion in flight starts (its delay is in it).
    start: Duration,
    /// The animator's time `value` is of.
    updated: Duration,
    motion: Motion,
    settled: bool,
    /// The frame that last asked for it, or kept it.
    seen: u64,
    /// The frame that last gave it a target: two different ones in one frame are a mistake.
    asked: u64,
}

impl Track {
    /// Moves the value to the time `now`.
    fn advance(&mut self, now: Duration) {
        if self.settled || now < self.start {
            return;
        }
        match self.motion.kind {
            Kind::Snap => self.arrive(),
            Kind::Tween { duration, easing } => {
                let t = if duration.is_zero() {
                    1.0
                } else {
                    (now - self.start).as_secs_f32() / duration.as_secs_f32()
                };
                if t >= 1.0 {
                    self.arrive();
                } else {
                    self.value = lerp_parts(self.from, self.target, easing.at(t));
                }
            }
            Kind::Spring(spring) => {
                let dt = (now - self.updated.max(self.start)).as_secs_f32();
                // The coarser of what the type can show and what the spring was told.
                let near = spring.threshold.max(self.precision as f32);
                let mut settled = true;
                for i in 0..4 {
                    // The spring is stepped on the distance from the target, which is small
                    // when it matters, and not on the value, which may be too large for the
                    // spring's `f32`: at an offset of fifty million the value itself has
                    // steps of four, the distance has all its digits.
                    let away = (self.value[i] - self.target[i]) as f32;
                    let (x, v) = spring.step(away, self.velocity[i], 0.0, dt);
                    self.value[i] = self.target[i] + x as f64;
                    self.velocity[i] = v;
                    settled &= spring.settled_within(x, v, 0.0, near);
                }
                if settled {
                    self.arrive();
                }
            }
        }
        self.updated = now;
    }

    fn arrive(&mut self) {
        self.value = self.target;
        self.velocity = [0.0; 4];
        self.settled = true;
    }
}

/// The values in flight of one window, and the time they move in. See the [module](self).
///
/// **Names.** A value is known by the name it is asked for with. The first time a name is seen
/// its value *is* its target: nothing animates on the frame a thing first appears (a window does
/// not open with every switch sliding into place). [`value_from`](Self::value_from) says
/// otherwise, for a thing that should enter. A name that a frame does not ask for is forgotten
/// at the end of that frame, so nothing has to be cleaned up when a widget goes away, and a
/// thing that comes back starts fresh. An application that leaves a part of its window undrawn
/// in some frames (it repaints only what is damaged) calls [`keep`](Self::keep) for the names
/// there, or accepts that they start fresh.
///
/// **Time.** The animator's time is the frames' time, but a frame never moves it by more than
/// [`max_step`](Self::set_max_step) (50 ms unless set), except by a wait the animator itself
/// asked for. After a stall, or a minute spent minimised, the animations in flight go on from
/// where they were instead of jumping to their ends; at a steady 20 frames a second or more
/// they run in real time.
#[derive(Debug)]
pub struct Animator {
    tracks: HashMap<u64, Track>,
    /// The things that repeat ([`cycle`](Self::cycle), [`blink`](Self::blink)).
    runs: HashMap<u64, cycle::Running>,
    /// The animator's own time: the sum of the frames' clamped steps.
    time: Duration,
    /// The real time of the frame in progress (or of the last one).
    frame_at: Option<Instant>,
    frame: u64,
    /// The wait the last `end_frame` asked for: a step this long is not a stall.
    planned: Duration,
    /// A wake-up the application asked for, on top of the tracks' own.
    wake: Option<Instant>,
    animating: bool,
    max_step: Duration,
    frame_interval: Duration,
    policy: Policy,
    system_reduced: bool,
}

impl Default for Animator {
    fn default() -> Animator {
        Animator::new()
    }
}

impl Animator {
    /// An animator that animates. It does not ask the system anything: the application reads
    /// [`platform::motion_reduced`](crate::platform::motion_reduced) and passes it to
    /// [`set_system_reduced`](Self::set_system_reduced), at start and when the system's
    /// settings change.
    pub fn new() -> Animator {
        Animator {
            tracks: HashMap::new(),
            runs: HashMap::new(),
            time: Duration::ZERO,
            frame_at: None,
            frame: 0,
            planned: Duration::ZERO,
            wake: None,
            animating: false,
            max_step: Duration::from_millis(50),
            frame_interval: Duration::from_nanos(1_000_000_000 / 60),
            policy: Policy::System,
            system_reduced: false,
        }
    }

    // ---- settings ----

    /// What the system says about motion: true when it asks for less (see
    /// [`platform::motion_reduced`](crate::platform::motion_reduced)). Under
    /// [`Policy::System`] every value then is its target at once: slides, fades and smooth
    /// scrolling alike. A fade is motion to the Windows setting this follows ("Animation
    /// effects", which switches off every animation of the system's own controls), so nothing is
    /// kept back.
    pub fn set_system_reduced(&mut self, reduced: bool) {
        self.system_reduced = reduced;
    }

    /// The application's own switch over the system's: see [`Policy`].
    pub fn set_policy(&mut self, policy: Policy) {
        self.policy = policy;
    }

    pub fn policy(&self) -> Policy {
        self.policy
    }

    /// Whether anything animates, by the policy and the system's wish. When false, every value
    /// asked for is its target and [`end_frame`](Self::end_frame) asks for no frame.
    pub fn animates(&self) -> bool {
        match self.policy {
            Policy::System => !self.system_reduced,
            Policy::Always => true,
            Policy::Never => false,
        }
    }

    /// The most one frame may move the animations: a longer gap between two frames is taken for
    /// a stall and counts as this much. 50 ms unless set.
    pub fn set_max_step(&mut self, step: Duration) {
        self.max_step = step;
    }

    /// The time from one frame to the next while something moves: the deadline
    /// [`end_frame`](Self::end_frame) gives is this long after the frame's start. A sixtieth of
    /// a second unless set; an application that knows its monitor's rate sets it from that.
    /// Never less than a millisecond: a shorter one (zero, from a monitor that gave no rate)
    /// would have the window draw without pause for as long as anything moves.
    pub fn set_frame_interval(&mut self, interval: Duration) {
        self.frame_interval = interval.max(Duration::from_millis(1));
    }

    // ---- the frame ----

    /// Starts a frame at `now` (from a [`Clock`]). Every value asked for until
    /// [`end_frame`](Self::end_frame) is the value at this one time.
    pub fn begin_frame(&mut self, now: Instant) {
        if let Some(last) = self.frame_at {
            let step = now.saturating_duration_since(last);
            self.time += step.min(self.max_step.max(self.planned));
        }
        self.frame_at = Some(now);
        self.frame += 1;
    }

    /// Ends the frame: forgets the names the frame did not ask for, and says when the next
    /// frame is needed. `None` means nothing moves and nothing waits: the window can sleep
    /// until an event (`ControlFlow::Wait`). `Some(deadline)` means draw again at `deadline`
    /// (`ControlFlow::WaitUntil(deadline)` and `request_redraw` when it is reached): one frame
    /// interval after this frame's start while something moves (already past, if the frame took
    /// longer: draw at once), or further away when everything in flight is still waiting out a
    /// delay.
    pub fn end_frame(&mut self) -> Option<Instant> {
        let (frame, time) = (self.frame, self.time);
        self.tracks.retain(|_, t| t.seen == frame);
        // No value leaves a frame behind the frame's time. One that was neither asked for nor
        // moved by `keep` would otherwise stand still and yet count as on its way, and the
        // window would draw sixty frames a second for ever for a value that never arrives.
        for t in self.tracks.values_mut() {
            if !t.settled && t.updated < time {
                t.advance(time);
            }
        }
        self.runs.retain(|_, r| r.seen == frame);
        let mut wait: Option<Duration> = None;
        for t in self.tracks.values().filter(|t| !t.settled) {
            let w = t.start.saturating_sub(time).max(self.frame_interval);
            wait = Some(wait.map_or(w, |old| old.min(w)));
        }
        // What repeats says itself when it next needs a frame: a cycle one interval on, a
        // blink when it changes.
        for r in self.runs.values() {
            let w = r.next.saturating_sub(time);
            wait = Some(wait.map_or(w, |old| old.min(w)));
        }
        self.animating = wait.is_some();
        let at = self.frame_at.unwrap_or_else(Instant::now);
        // A wait too long for the clock to hold (a blink of centuries) is no deadline.
        let mut deadline = wait.and_then(|w| at.checked_add(w));
        if let Some(wake) = self.wake.take() {
            deadline = Some(deadline.map_or(wake, |d| d.min(wake)));
        }
        self.planned = deadline.map_or(Duration::ZERO, |d| d.saturating_duration_since(at));
        deadline
    }

    /// Whether any value was still on its way, or anything that repeats was still asked for, at
    /// the end of the last frame.
    pub fn animating(&self) -> bool {
        self.animating
    }

    /// Asks for a frame at `at` whatever the values do: the next [`end_frame`](Self::end_frame)
    /// gives a deadline no later than it. For the application's own timers (a toast that goes
    /// away after four seconds), so one deadline serves both. It is asked for once: a frame
    /// that still needs it asks again.
    pub fn wake_at(&mut self, at: Instant) {
        self.wake = Some(self.wake.map_or(at, |w| w.min(at)));
    }

    // ---- values ----

    /// The current value of `id`, which is on its way to `target` by `motion`.
    ///
    /// The first time `id` is seen the value is `target`. When `target` differs from the one of
    /// the frame before, the value starts towards the new one from where it is: a tween begins
    /// again (the frame that brings the new target still draws the old place, the next one
    /// moves), a spring keeps its speed. `motion` is read when the target changes; a different
    /// motion with the same target changes nothing in flight.
    pub fn value<T: Animatable>(&mut self, id: impl AsId, target: T, motion: Motion) -> T {
        T::from_parts(self.track(id.as_id(), None, target.parts(), motion, T::precision()))
    }

    /// As [`value`](Self::value), but the first time `id` is seen the value is `initial` and
    /// sets off for `target`: for a thing that enters.
    pub fn value_from<T: Animatable>(
        &mut self,
        id: impl AsId,
        initial: T,
        target: T,
        motion: Motion,
    ) -> T {
        T::from_parts(self.track(
            id.as_id(),
            Some(initial.parts()),
            target.parts(),
            motion,
            T::precision(),
        ))
    }

    /// A switch as a number: on its way to 1 while `on`, to 0 while not. For a hover or press
    /// fade, a tick, a pane that opens. First seen, it is where it is going.
    pub fn toggle(&mut self, id: impl AsId, on: bool, motion: Motion) -> f32 {
        self.value(id, if on { 1.0 } else { 0.0 }, motion)
    }

    /// How present a thing that comes and goes is: 0 gone, 1 there. Unlike
    /// [`toggle`](Self::toggle), a thing first seen while `shown` starts from 0 and enters by
    /// `enter`; when `shown` turns false it leaves by `exit`. The caller draws the thing while
    /// the result is above 0 and stops asking once it is 0 and not shown. See
    /// [`Transient`], which also keeps the thing itself until it has left.
    pub fn presence(&mut self, id: impl AsId, shown: bool, enter: Motion, exit: Motion) -> f32 {
        let (target, motion) = if shown { (1.0, enter) } else { (0.0, exit) };
        self.value_from(id, 0.0_f32, target, motion).clamp(0.0, 1.0)
    }

    /// Puts `id` at `value`, at rest, now: for a jump that must not animate (a scroll thumb
    /// dragged, a document opened at a heading). A [`value`](Self::value) asked for afterwards
    /// with another target moves from here.
    pub fn set<T: Animatable>(&mut self, id: impl AsId, value: T) {
        let parts = value.parts();
        let (time, frame) = (self.time, self.frame);
        self.tracks.insert(
            id.as_id().0,
            Track {
                value: parts,
                velocity: [0.0; 4],
                target: parts,
                from: parts,
                precision: T::precision(),
                start: time,
                updated: time,
                motion: Motion::SNAP,
                settled: true,
                seen: frame,
                asked: 0,
            },
        );
    }

    /// The value of `id` as the last frame that asked for it left it, without asking for it:
    /// `None` for a name not known. For hit testing against what was drawn.
    pub fn get<T: Animatable>(&self, id: impl AsId) -> Option<T> {
        self.tracks
            .get(&id.as_id().0)
            .map(|t| T::from_parts(t.value))
    }

    /// Whether `id` is at its target (or not known at all).
    pub fn settled(&self, id: impl AsId) -> bool {
        self.tracks.get(&id.as_id().0).is_none_or(|t| t.settled)
    }

    /// Keeps `id` through this frame though nothing asked for its value: for a part of the
    /// window this frame does not draw. A value kept this way goes on moving towards the target
    /// it was last given: it is moved to this frame's time here, so a [`get`](Self::get) after
    /// it reads the value of this frame, and it arrives and stops asking for frames as if it
    /// had been asked for. A name not known is not made.
    ///
    /// What is on the screen in that part is still the frame that last drew it. An application
    /// that skips a part while a value in it moves draws the part once more when
    /// [`settled`](Self::settled) says the value has arrived, or the part is left showing the
    /// value half way.
    pub fn keep(&mut self, id: impl AsId) {
        let (now, frame, animates) = (self.time, self.frame, self.animates());
        if let Some(t) = self.tracks.get_mut(&id.as_id().0) {
            t.seen = frame;
            if !animates {
                t.motion.kind = Kind::Snap;
            }
            t.advance(now);
        }
    }

    /// Forgets `id` now: the next time it is asked for, it is first seen. A
    /// [`cycle`](Self::cycle) or [`blink`](Self::blink) of that name starts again from its
    /// beginning.
    pub fn forget(&mut self, id: impl AsId) {
        let id = id.as_id().0;
        self.tracks.remove(&id);
        self.runs.remove(&id);
    }

    /// Puts every value at its target, now.
    pub fn finish_all(&mut self) {
        for t in self.tracks.values_mut() {
            t.arrive();
        }
    }

    /// How many names are remembered: values, and things that repeat.
    pub fn len(&self) -> usize {
        self.tracks.len() + self.runs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty() && self.runs.is_empty()
    }

    fn track(
        &mut self,
        id: Id,
        initial: Option<[f64; 4]>,
        target: [f64; 4],
        motion: Motion,
        precision: f64,
    ) -> [f64; 4] {
        let (now, frame) = (self.time, self.frame);
        let in_frame = self.frame_at.is_some();
        // A target that is not a number cannot be moved to, nor waited for: it is taken as it
        // is, at once.
        let finite = target.iter().all(|v| v.is_finite());
        // With reduced motion the way to the target is skipped, but not the wait before it: a
        // delay is not a movement. So the motion becomes a snap that keeps its delay.
        let instant = !self.animates() || !finite;
        let motion = if instant {
            Motion {
                kind: Kind::Snap,
                delay: if finite { motion.delay } else { Duration::ZERO },
            }
        } else {
            motion
        };
        let fresh = |from: [f64; 4]| Track {
            value: from,
            velocity: [0.0; 4],
            target,
            from,
            precision,
            // A delay too long for the clock never ends: the motion never starts, and
            // `end_frame` asks for no frame for it.
            start: now.saturating_add(motion.delay),
            updated: now,
            motion,
            settled: from == target,
            seen: frame,
            asked: frame,
        };
        let t = match self.tracks.entry(id.0) {
            Entry::Vacant(slot) => slot.insert(fresh(initial.filter(|_| finite).unwrap_or(target))),
            Entry::Occupied(slot) => {
                let t = slot.into_mut();
                // Two targets for one name in one frame: two things drawn under the same
                // name, each of which would start the other's motion again on every frame.
                // (Targets that are not numbers never compare equal and are let through.)
                debug_assert!(
                    !(in_frame
                        && t.asked == frame
                        && (0..4).any(|i| {
                            t.target[i] != target[i]
                                && t.target[i].is_finite()
                                && target[i].is_finite()
                        })),
                    "anim: one name was given two targets in one frame ({:?} and {:?}); \
                     two things are drawn under the same name",
                    t.target,
                    target,
                );
                t
            }
        };
        (t.seen, t.asked, t.precision) = (frame, frame, precision);
        if instant {
            // Motion was switched off while this one was on its way: it keeps only its wait.
            t.motion.kind = Kind::Snap;
        }
        t.advance(now);
        let changed = t.target != target;
        if changed {
            // Starts again from where it is. A spring keeps its speed; a tween has none.
            let velocity = match (t.motion.kind, motion.kind) {
                (Kind::Spring(_), Kind::Spring(_)) => t.velocity,
                _ => [0.0; 4],
            };
            // A value that is not a number (its last target was not one) has no "where it is".
            let from = if t.value.iter().all(|v| v.is_finite()) {
                t.value
            } else {
                target
            };
            *t = Track {
                velocity,
                ..fresh(from)
            };
            if t.settled {
                // Already there (the target came back to where the value stands): it is at
                // rest, and must not carry a speed into the next move.
                t.velocity = [0.0; 4];
            }
        }
        // A snap is read only when the target changes (or motion is off): a snap given for an
        // unchanged target leaves the motion in flight alone, as for any other motion.
        if (changed || instant) && motion.kind == Kind::Snap {
            if motion.delay.is_zero() {
                // Nothing to move through and nothing to wait for: it is there on this frame.
                t.arrive();
            } else {
                // The wait may be over on this very frame (a delay of nothing left).
                t.advance(now);
            }
        }
        t.value
    }
}

#[cfg(test)]
mod tests;
