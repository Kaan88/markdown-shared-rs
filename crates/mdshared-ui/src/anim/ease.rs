//! Easing curves: how far a tween has moved (0 at its start, 1 at its end) for how much of its
//! time has passed. Pure functions of one number, with no state and no clock.

/// The shape of a tween over its time. Every curve gives exactly 0 at 0 and exactly 1 at 1; the
/// time is clamped to that range first, so a curve can be asked about any number.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Easing {
    /// The same speed throughout. Right for a fade, where the eye reads no speed.
    Linear,
    /// Starts slow: `t` squared, cubed, to the fifth.
    InQuad,
    InCubic,
    InQuint,
    /// Ends slow: the mirror of the `In` curve of the same name.
    OutQuad,
    OutCubic,
    OutQuint,
    /// Slow at both ends: the `In` curve to the middle, the `Out` curve from it.
    InOutQuad,
    InOutCubic,
    InOutQuint,
    /// A quarter of a sine wave: the gentlest of the families.
    InSine,
    OutSine,
    InOutSine,
    /// A power of two: nearly still, then very fast (or the mirror).
    InExpo,
    OutExpo,
    InOutExpo,
    /// Ends slow after passing its end by about a tenth of the way: for something that lands.
    OutBack,
    /// The CSS `cubic-bezier(x1, y1, x2, y2)`: a cubic Bézier from (0, 0) to (1, 1) with two
    /// control points, read as progress `y` for time `x`. The curve inside is a [`Bezier`],
    /// which only exists checked, so this variant is made by [`Easing::cubic_bezier`] (which
    /// refuses a curve CSS refuses) or [`Easing::bezier`] (for constants; it pulls a wrong
    /// curve to the nearest right one).
    CubicBezier(Bezier),
}

/// The two control points of a CSS `cubic-bezier` curve, known to be a curve CSS accepts: all
/// four numbers finite, and `x1` and `x2` inside 0..=1 (time cannot run backwards). `y1` and
/// `y2` may leave that range, which makes the curve overshoot. Its numbers cannot be set from
/// outside, so an [`Easing`] never holds a curve that is not one.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Bezier {
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
}

impl Bezier {
    /// The curve with these control points, or `None` for what CSS refuses: a number that is
    /// not finite, or an `x1` or `x2` outside 0..=1.
    pub const fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Option<Bezier> {
        let finite = x1.is_finite() && y1.is_finite() && x2.is_finite() && y2.is_finite();
        if finite && x1 >= 0.0 && x1 <= 1.0 && x2 >= 0.0 && x2 <= 1.0 {
            Some(Bezier { x1, y1, x2, y2 })
        } else {
            None
        }
    }

    /// The control points: `(x1, y1, x2, y2)`.
    pub const fn points(self) -> (f32, f32, f32, f32) {
        (self.x1, self.y1, self.x2, self.y2)
    }

    /// Progress at time `x`: see [`cubic_bezier`].
    pub fn at(self, x: f32) -> f32 {
        cubic_bezier(self.x1, self.y1, self.x2, self.y2, x)
    }
}

impl Easing {
    /// The CSS keywords `ease`, `ease-in`, `ease-out` and `ease-in-out`.
    pub const EASE: Easing = Easing::bezier(0.25, 0.1, 0.25, 1.0);
    pub const EASE_IN: Easing = Easing::bezier(0.42, 0.0, 1.0, 1.0);
    pub const EASE_OUT: Easing = Easing::bezier(0.0, 0.0, 0.58, 1.0);
    pub const EASE_IN_OUT: Easing = Easing::bezier(0.42, 0.0, 0.58, 1.0);

    /// The CSS `cubic-bezier(x1, y1, x2, y2)`, checked as CSS checks it: `None` when a number
    /// is not finite or when `x1` or `x2` is outside 0..=1. For a curve that comes from
    /// outside the program (a settings file, a theme).
    pub const fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32) -> Option<Easing> {
        match Bezier::new(x1, y1, x2, y2) {
            Some(curve) => Some(Easing::CubicBezier(curve)),
            None => None,
        }
    }

    /// The CSS `cubic-bezier(x1, y1, x2, y2)` for a constant in the code, where there is no
    /// one to hand a refusal to: a wrong curve becomes the nearest right one and nothing
    /// panics. An `x1` or `x2` outside 0..=1 is pulled to that range; if any number is not
    /// finite the curve is [`Easing::Linear`]. A right curve is taken as it is, so for right
    /// numbers this and [`cubic_bezier`](Easing::cubic_bezier) give the same easing.
    pub const fn bezier(x1: f32, y1: f32, x2: f32, y2: f32) -> Easing {
        const fn unit(v: f32) -> f32 {
            if v < 0.0 {
                0.0
            } else if v > 1.0 {
                1.0
            } else {
                v
            }
        }
        match Bezier::new(unit(x1), y1, unit(x2), y2) {
            Some(curve) => Easing::CubicBezier(curve),
            None => Easing::Linear,
        }
    }

    /// Progress at time `t`. `t` below 0 reads as 0, above 1 as 1, and a `t` that is not a
    /// number reads as 1, so a tween fed nonsense ends instead of sticking.
    pub fn at(self, t: f32) -> f32 {
        if t.is_nan() || t >= 1.0 {
            return 1.0;
        }
        if t <= 0.0 {
            return 0.0;
        }
        // The mirror of an `In` curve, and the two halves of an `InOut` one.
        let out = |f: fn(f32) -> f32| 1.0 - f(1.0 - t);
        let in_out = |f: fn(f32) -> f32| {
            if t < 0.5 {
                f(2.0 * t) / 2.0
            } else {
                1.0 - f(2.0 - 2.0 * t) / 2.0
            }
        };
        fn quad(t: f32) -> f32 {
            t * t
        }
        fn cubic(t: f32) -> f32 {
            t * t * t
        }
        fn quint(t: f32) -> f32 {
            t * t * t * t * t
        }
        fn sine(t: f32) -> f32 {
            1.0 - (t * std::f32::consts::FRAC_PI_2).cos()
        }
        // 2^(10t - 10), moved so that it starts at exactly 0 and still ends at 1.
        fn expo(t: f32) -> f32 {
            const START: f32 = 1.0 / 1024.0;
            ((10.0 * t - 10.0).exp2() - START) / (1.0 - START)
        }
        match self {
            Easing::Linear => t,
            Easing::InQuad => quad(t),
            Easing::InCubic => cubic(t),
            Easing::InQuint => quint(t),
            Easing::OutQuad => out(quad),
            Easing::OutCubic => out(cubic),
            Easing::OutQuint => out(quint),
            Easing::InOutQuad => in_out(quad),
            Easing::InOutCubic => in_out(cubic),
            Easing::InOutQuint => in_out(quint),
            Easing::InSine => sine(t),
            Easing::OutSine => out(sine),
            Easing::InOutSine => in_out(sine),
            Easing::InExpo => expo(t),
            Easing::OutExpo => out(expo),
            Easing::InOutExpo => in_out(expo),
            Easing::OutBack => {
                // Penner's "back" with the usual overshoot constant.
                const C1: f32 = 1.70158;
                const C3: f32 = C1 + 1.0;
                let u = t - 1.0;
                1.0 + C3 * u * u * u + C1 * u * u
            }
            Easing::CubicBezier(curve) => curve.at(t),
        }
    }
}

/// The CSS `cubic-bezier(x1, y1, x2, y2)` timing function at time `x`: the `y` of the point of
/// the curve whose `x` is the one asked for. The curve is given by a parameter, not by `x`, so
/// the parameter is found first: a few steps of Newton's method, which lands within a hundred
/// thousandth for every curve an interface uses, and bisection for the rare curve whose slope is
/// too flat for Newton (a control point at the very start or end). The answer is within 1e-4 of
/// the true curve; the tests hold it to that against a fine sampling of the curve itself.
///
/// This is the bare arithmetic, for a caller that has four numbers and a time; an [`Easing`]
/// holds a checked [`Bezier`]. Unchecked numbers are made harmless here: `x1` and `x2` are
/// pulled into 0..=1, and control points that are not finite give the straight line, `x`.
pub fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    if x.is_nan() || x >= 1.0 {
        return 1.0;
    }
    if x <= 0.0 {
        return 0.0;
    }
    if !(x1.is_finite() && y1.is_finite() && x2.is_finite() && y2.is_finite()) {
        return x;
    }
    let (x1, x2) = (x1.clamp(0.0, 1.0), x2.clamp(0.0, 1.0));
    // A curve whose control points lie on the diagonal is the diagonal.
    if x1 == y1 && x2 == y2 {
        return x;
    }
    // One coordinate of the curve as a polynomial in the parameter `s`, in Horner form:
    // ((a s + b) s + c) s, with the end points (0, 0) and (1, 1) already put in.
    let coefficients = |p1: f32, p2: f32| {
        let c = 3.0 * p1;
        let b = 3.0 * (p2 - p1) - c;
        (1.0 - c - b, b, c)
    };
    let (ax, bx, cx) = coefficients(x1, x2);
    let (ay, by, cy) = coefficients(y1, y2);
    let sample_x = |s: f32| ((ax * s + bx) * s + cx) * s;
    let slope_x = |s: f32| (3.0 * ax * s + 2.0 * bx) * s + cx;

    const CLOSE: f32 = 1e-6;
    let mut s = x;
    let mut found = false;
    for _ in 0..8 {
        let err = sample_x(s) - x;
        if err.abs() < CLOSE {
            found = true;
            break;
        }
        let d = slope_x(s);
        if d.abs() < 1e-6 {
            break;
        }
        s -= err / d;
    }
    if !found || !(0.0..=1.0).contains(&s) {
        // `x` rises with `s` (the control points' `x` are inside 0..=1), so halving finds it.
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        s = x;
        for _ in 0..32 {
            let err = sample_x(s) - x;
            if err.abs() < CLOSE {
                break;
            }
            if err < 0.0 {
                lo = s;
            } else {
                hi = s;
            }
            s = (lo + hi) / 2.0;
        }
    }
    ((ay * s + by) * s + cy) * s
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Easing; 22] = [
        Easing::Linear,
        Easing::InQuad,
        Easing::InCubic,
        Easing::InQuint,
        Easing::OutQuad,
        Easing::OutCubic,
        Easing::OutQuint,
        Easing::InOutQuad,
        Easing::InOutCubic,
        Easing::InOutQuint,
        Easing::InSine,
        Easing::OutSine,
        Easing::InOutSine,
        Easing::InExpo,
        Easing::OutExpo,
        Easing::InOutExpo,
        Easing::OutBack,
        Easing::EASE,
        Easing::EASE_IN,
        Easing::EASE_OUT,
        Easing::EASE_IN_OUT,
        Easing::bezier(0.0, 0.0, 0.0, 1.0),
    ];

    /// The point of a cubic Bézier coordinate at parameter `s`, the long way.
    fn bezier(p1: f64, p2: f64, s: f64) -> f64 {
        let u = 1.0 - s;
        3.0 * u * u * s * p1 + 3.0 * u * s * s * p2 + s * s * s
    }

    /// The curve's `y` at `x`, found by walking the parameter in very small steps: slow, and
    /// independent of the solver it checks.
    fn reference(x1: f64, y1: f64, x2: f64, y2: f64, x: f64) -> f64 {
        let (mut lo, mut hi) = (0.0f64, 1.0f64);
        for _ in 0..60 {
            let mid = (lo + hi) / 2.0;
            if bezier(x1, x2, mid) < x {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        bezier(y1, y2, (lo + hi) / 2.0)
    }

    #[test]
    fn every_curve_starts_at_zero_ends_at_one_and_clamps_its_time() {
        for e in ALL {
            assert_eq!(e.at(0.0), 0.0, "{e:?} at 0");
            assert_eq!(e.at(1.0), 1.0, "{e:?} at 1");
            assert_eq!(e.at(-3.0), 0.0, "{e:?} before its start");
            assert_eq!(e.at(7.0), 1.0, "{e:?} after its end");
            assert_eq!(e.at(f32::NAN), 1.0, "{e:?} of no number");
            assert_eq!(e.at(f32::INFINITY), 1.0, "{e:?} of infinity");
            // Close to the ends it is close to them: no curve jumps.
            assert!(e.at(0.001) < 0.05, "{e:?} jumps at its start");
            assert!(e.at(0.999) > 0.95, "{e:?} jumps at its end");
        }
    }

    #[test]
    fn the_curves_without_overshoot_only_rise() {
        for e in ALL {
            if e == Easing::OutBack {
                continue;
            }
            let mut last = 0.0;
            for i in 0..=1000 {
                let v = e.at(i as f32 / 1000.0);
                assert!(v >= last - 1e-4, "{e:?} falls at {i}: {v} after {last}");
                assert!((0.0..=1.0 + 1e-4).contains(&v), "{e:?} leaves 0..1: {v}");
                last = v;
            }
        }
        // The one that lands passes its end and comes back.
        let peak = (0..=100)
            .map(|i| Easing::OutBack.at(i as f32 / 100.0))
            .fold(0.0, f32::max);
        assert!(peak > 1.05 && peak < 1.15, "OutBack peaks at {peak}");
    }

    #[test]
    fn in_and_out_curves_mirror_each_other_and_in_out_passes_the_middle() {
        for (i, o, io) in [
            (Easing::InQuad, Easing::OutQuad, Easing::InOutQuad),
            (Easing::InCubic, Easing::OutCubic, Easing::InOutCubic),
            (Easing::InQuint, Easing::OutQuint, Easing::InOutQuint),
            (Easing::InSine, Easing::OutSine, Easing::InOutSine),
            (Easing::InExpo, Easing::OutExpo, Easing::InOutExpo),
        ] {
            for k in 0..=20 {
                let t = k as f32 / 20.0;
                assert!((i.at(t) + o.at(1.0 - t) - 1.0).abs() < 1e-5, "{i:?} {t}");
            }
            assert!((io.at(0.5) - 0.5).abs() < 1e-5, "{io:?} at the middle");
            assert!(i.at(0.5) < 0.5 && o.at(0.5) > 0.5, "{i:?} and {o:?}");
        }
        assert_eq!(Easing::InQuad.at(0.5), 0.25);
        assert_eq!(Easing::OutCubic.at(0.5), 0.875);
    }

    #[test]
    fn the_bezier_solver_is_within_a_ten_thousandth_of_the_curve() {
        let curves: [(f32, f32, f32, f32); 12] = [
            (0.25, 0.1, 0.25, 1.0),
            (0.42, 0.0, 1.0, 1.0),
            (0.0, 0.0, 0.58, 1.0),
            (0.42, 0.0, 0.58, 1.0),
            // The Fluent curves: the steepest starts and ends an interface uses.
            (0.0, 0.0, 0.0, 1.0),
            (1.0, 0.0, 1.0, 1.0),
            (0.55, 0.55, 0.0, 1.0),
            (0.8, 0.0, 0.2, 1.0),
            (0.1, 0.9, 0.2, 1.0),
            (0.9, 0.1, 1.0, 0.2),
            // Overshoot on both sides.
            (0.13, 1.62, 0.0, 0.92),
            (0.68, -0.55, 0.27, 1.55),
        ];
        for (x1, y1, x2, y2) in curves {
            let mut worst = 0.0f64;
            for i in 0..=2000 {
                let x = i as f32 / 2000.0;
                let got = cubic_bezier(x1, y1, x2, y2, x) as f64;
                let want = reference(x1 as f64, y1 as f64, x2 as f64, y2 as f64, x as f64);
                worst = worst.max((got - want).abs());
            }
            assert!(
                worst < 1e-4,
                "cubic-bezier({x1}, {y1}, {x2}, {y2}) is off by {worst}"
            );
        }
    }

    #[test]
    fn the_bezier_solver_knows_the_css_keywords_and_the_diagonal() {
        // Values a browser gives (Chrome and Firefox agree to these digits).
        assert!((Easing::EASE.at(0.5) - 0.8024).abs() < 1e-3);
        assert!((Easing::EASE_IN.at(0.5) - 0.3154).abs() < 1e-3);
        assert!((Easing::EASE_OUT.at(0.5) - 0.6846).abs() < 1e-3);
        assert!((Easing::EASE_IN_OUT.at(0.5) - 0.5).abs() < 1e-4);
        for i in 0..=10 {
            let x = i as f32 / 10.0;
            assert_eq!(cubic_bezier(0.0, 0.0, 1.0, 1.0, x), x);
            assert_eq!(cubic_bezier(0.3, 0.3, 0.7, 0.7, x), x);
        }
        // Control points outside 0..=1 in x are pulled in, not followed backwards in time.
        let pulled = cubic_bezier(-1.0, 0.0, 2.0, 1.0, 0.5);
        assert_eq!(pulled, cubic_bezier(0.0, 0.0, 1.0, 1.0, 0.5));
    }

    #[test]
    fn a_checked_bezier_refuses_what_css_refuses_and_a_constant_is_pulled_right() {
        // What CSS accepts, overshoot in y included.
        let ease = Easing::cubic_bezier(0.25, 0.1, 0.25, 1.0).unwrap();
        assert_eq!(ease, Easing::EASE);
        assert!(Easing::cubic_bezier(0.68, -0.55, 0.27, 1.55).is_some());
        assert!(Easing::cubic_bezier(0.0, 0.0, 1.0, 1.0).is_some());
        let Easing::CubicBezier(curve) = ease else {
            panic!("not a bezier");
        };
        assert_eq!(curve.points(), (0.25, 0.1, 0.25, 1.0));
        assert_eq!(curve.at(0.5), ease.at(0.5));
        // What it refuses: time running backwards, and numbers that are not numbers.
        for (x1, y1, x2, y2) in [
            (-0.1, 0.0, 0.5, 1.0),
            (0.5, 0.0, 1.1, 1.0),
            (f32::NAN, 0.0, 0.5, 1.0),
            (0.5, f32::NAN, 0.5, 1.0),
            (0.5, 0.0, f32::INFINITY, 1.0),
            (0.5, 0.0, 0.5, f32::NEG_INFINITY),
        ] {
            let what = format!("{x1} {y1} {x2} {y2}");
            assert_eq!(Easing::cubic_bezier(x1, y1, x2, y2), None, "{what}");
            assert_eq!(Bezier::new(x1, y1, x2, y2), None, "{what}");
            // The constant form never fails and never gives a curve that is not one: every
            // value it gives is a number, from 0 at the start to 1 at the end.
            let pulled = Easing::bezier(x1, y1, x2, y2);
            if let Easing::CubicBezier(c) = pulled {
                let (a, b, c, d) = c.points();
                assert!(
                    (0.0..=1.0).contains(&a) && (0.0..=1.0).contains(&c),
                    "{what}"
                );
                assert!(b.is_finite() && d.is_finite(), "{what}");
            } else {
                assert_eq!(pulled, Easing::Linear, "{what}");
            }
            for i in 0..=10 {
                assert!(pulled.at(i as f32 / 10.0).is_finite(), "{what}");
            }
            assert_eq!((pulled.at(0.0), pulled.at(1.0)), (0.0, 1.0), "{what}");
            // The bare arithmetic is as harmless.
            assert!(cubic_bezier(x1, y1, x2, y2, 0.3).is_finite(), "{what}");
        }
        assert_eq!(
            Easing::bezier(-1.0, 0.0, 2.0, 1.0),
            Easing::bezier(0.0, 0.0, 1.0, 1.0)
        );
        assert_eq!(Easing::bezier(f32::NAN, 0.0, 0.5, 1.0), Easing::Linear);
        // Usable where a constant is needed.
        const C: Easing = Easing::bezier(0.2, 0.0, 0.0, 1.0);
        assert_eq!(Some(C), Easing::cubic_bezier(0.2, 0.0, 0.0, 1.0));
    }
}
