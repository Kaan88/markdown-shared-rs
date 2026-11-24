//! A damped spring: motion with no fixed length that carries its speed when its target moves.
//! Where a tween that is given a new target starts again from rest, a spring keeps going, so a
//! value that is pushed again and again (a scroll offset under a turning wheel, a pill that
//! follows a pointer) never stops and starts.

/// A spring of unit mass. `stiffness` pulls the value to its target, `damping` slows it.
///
/// The step is the exact solution of the spring's equation over the time asked for, not a
/// numerical integration, so any time step gives the same path and a long one cannot make it
/// blow up.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Spring {
    /// The pull per unit of distance from the target (per second squared, the mass being one).
    pub stiffness: f32,
    /// The drag per unit of speed (per second).
    pub damping: f32,
    /// How close to the target, in the value's own unit, counts as arrived: see
    /// [`settled`](Spring::settled). A value in 0..=1 is well served by the default of 0.001; a
    /// position in pixels may take 0.1.
    pub threshold: f32,
}

impl Default for Spring {
    /// Critically damped, settling in about a quarter of a second: no overshoot.
    fn default() -> Spring {
        Spring::SMOOTH
    }
}

impl Spring {
    /// The default: critically damped (it never passes its target) with a response of 0.25 s.
    pub const SMOOTH: Spring = Spring::response(0.25, 1.0);
    /// Critically damped and quick, for a value that follows the hand: response 0.15 s.
    pub const SNAPPY: Spring = Spring::response(0.15, 1.0);
    /// A little under-damped: it passes its target once by about a twentieth and comes back.
    pub const BOUNCY: Spring = Spring::response(0.3, 0.7);

    /// A spring from its two physical numbers.
    pub const fn new(stiffness: f32, damping: f32) -> Spring {
        Spring {
            stiffness,
            damping,
            threshold: 0.001,
        }
    }

    /// A spring from how it feels: `response` is the period, in seconds, the spring would swing
    /// with if nothing damped it (a critically damped one is within a hundredth of its target
    /// after about one response), and `ratio` is the damping ratio: 1 is critical (the fastest
    /// arrival without passing the target), below 1 overshoots and rings, above 1 creeps.
    pub const fn response(response: f32, ratio: f32) -> Spring {
        let omega = std::f32::consts::TAU / response;
        Spring {
            stiffness: omega * omega,
            damping: 2.0 * ratio * omega,
            threshold: 0.001,
        }
    }

    /// The same spring with another [`threshold`](Spring::threshold).
    pub const fn with_threshold(mut self, threshold: f32) -> Spring {
        self.threshold = threshold;
        self
    }

    /// The speed of the undamped swing, in radians per second.
    fn omega(self) -> f32 {
        self.stiffness.max(0.0).sqrt()
    }

    /// The damping ratio: 1 is critical.
    pub fn ratio(self) -> f32 {
        let omega = self.omega();
        if omega <= 0.0 {
            return 1.0;
        }
        self.damping / (2.0 * omega)
    }

    /// The value and its speed `dt` seconds on, from `value` moving at `velocity` (per second)
    /// towards `target`. A `dt` of zero or less changes nothing. A spring with no stiffness (or a
    /// number that is not one) goes straight to the target and stops: it could never arrive.
    pub fn step(self, value: f32, velocity: f32, target: f32, dt: f32) -> (f32, f32) {
        let omega = self.omega();
        if !(omega > 0.0 && omega.is_finite() && self.damping.is_finite()) {
            return (target, 0.0);
        }
        // A non-number check that also refuses a negative step.
        if dt.is_nan() || dt <= 0.0 {
            return (value, velocity);
        }
        let ratio = self.ratio().max(0.0);
        // The distance from the target and the speed are the two numbers the equation is in.
        let (x0, v0) = (value - target, velocity);
        let (x, v) = if (ratio - 1.0).abs() < 1e-4 {
            // Critical: (x0 + c t) e^(-w t).
            let c = v0 + omega * x0;
            let decay = (-omega * dt).exp();
            ((x0 + c * dt) * decay, (v0 - omega * c * dt) * decay)
        } else if ratio < 1.0 {
            // Under-damped: a decaying swing.
            let a = ratio * omega;
            let w = omega * (1.0 - ratio * ratio).sqrt();
            let b = (v0 + a * x0) / w;
            let decay = (-a * dt).exp();
            let (sin, cos) = (w * dt).sin_cos();
            (
                decay * (x0 * cos + b * sin),
                decay * (v0 * cos - (a * v0 + omega * omega * x0) / w * sin),
            )
        } else {
            // Over-damped: two decays, one slow and one fast.
            let s = (ratio * ratio - 1.0).sqrt();
            let (r1, r2) = (-omega * (ratio - s), -omega * (ratio + s));
            let c2 = (v0 - r1 * x0) / (r2 - r1);
            let c1 = x0 - c2;
            let (e1, e2) = ((r1 * dt).exp(), (r2 * dt).exp());
            (c1 * e1 + c2 * e2, c1 * r1 * e1 + c2 * r2 * e2)
        };
        if !(x.is_finite() && v.is_finite()) {
            return (target, 0.0);
        }
        (target + x, v)
    }

    /// Whether a value this far from its target and this fast has arrived: within the threshold,
    /// and too slow to leave it again (the speed is weighed by the spring's own time, so the
    /// test means the same for a stiff spring and a soft one).
    pub fn settled(self, value: f32, velocity: f32, target: f32) -> bool {
        let omega = self.omega();
        if !(omega > 0.0 && omega.is_finite()) {
            return true;
        }
        let threshold = self.threshold.max(f32::EPSILON);
        (value - target).abs() <= threshold && velocity.abs() <= threshold * omega
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The path of a spring from 0 to 1, sampled every `dt` for `seconds`.
    fn path(spring: Spring, dt: f32, seconds: f32) -> Vec<f32> {
        let (mut x, mut v) = (0.0, 0.0);
        (0..(seconds / dt) as usize)
            .map(|_| {
                (x, v) = spring.step(x, v, 1.0, dt);
                x
            })
            .collect()
    }

    #[test]
    fn a_critically_damped_spring_arrives_without_passing_its_target() {
        for spring in [Spring::SMOOTH, Spring::SNAPPY, Spring::default()] {
            assert!((spring.ratio() - 1.0).abs() < 1e-5);
            let p = path(spring, 1.0 / 60.0, 2.0);
            assert!(p.windows(2).all(|w| w[1] >= w[0]), "it turned back");
            assert!(p.iter().all(|&x| x <= 1.0 + 1e-6), "it passed its target");
            assert!((p.last().unwrap() - 1.0).abs() < 1e-4);
        }
        // "Response": about one response after the start it is within a hundredth.
        let (x, _) = Spring::response(0.25, 1.0).step(0.0, 0.0, 1.0, 0.25);
        assert!((1.0 - x) < 0.02, "after one response it is at {x}");
    }

    #[test]
    fn an_under_damped_spring_overshoots_once_and_an_over_damped_one_creeps() {
        let bouncy = path(Spring::BOUNCY, 1.0 / 120.0, 3.0);
        let peak = bouncy.iter().cloned().fold(0.0, f32::max);
        assert!(peak > 1.02 && peak < 1.1, "the bounce peaks at {peak}");
        assert!((bouncy.last().unwrap() - 1.0).abs() < 1e-4);

        let slow = path(Spring::response(0.25, 2.0), 1.0 / 120.0, 4.0);
        assert!(slow.iter().all(|&x| x <= 1.0 + 1e-6));
        assert!(slow.windows(2).all(|w| w[1] >= w[0]));
        // It is behind the critical spring of the same response at the same time.
        let critical = path(Spring::response(0.25, 1.0), 1.0 / 120.0, 4.0);
        assert!(slow[60] < critical[60]);
        assert!((slow.last().unwrap() - 1.0).abs() < 1e-3);
    }

    #[test]
    fn the_path_does_not_depend_on_the_size_of_the_steps() {
        for spring in [
            Spring::SMOOTH,
            Spring::BOUNCY,
            Spring::response(0.4, 1.8),
            Spring::new(170.0, 26.0),
        ] {
            // One step of 0.3 s, or 300 of a millisecond, with a start that is already moving.
            let whole = spring.step(5.0, -40.0, 20.0, 0.3);
            let (mut x, mut v) = (5.0, -40.0);
            for _ in 0..300 {
                (x, v) = spring.step(x, v, 20.0, 0.001);
            }
            assert!(
                (whole.0 - x).abs() < 1e-2,
                "{spring:?}: {} and {x}",
                whole.0
            );
            assert!((whole.1 - v).abs() < 0.2, "{spring:?}: {} and {v}", whole.1);
            // A stall of a minute lands on the target, and is not thrown past it.
            let (x, v) = spring.step(5.0, -40.0, 20.0, 60.0);
            assert!((x - 20.0).abs() < 1e-3 && v.abs() < 1e-3);
        }
    }

    #[test]
    fn a_spring_settles_and_says_so() {
        let spring = Spring::SMOOTH;
        assert!(!spring.settled(0.0, 0.0, 1.0), "far away");
        assert!(!spring.settled(1.0, 5.0, 1.0), "there, but moving fast");
        assert!(spring.settled(1.0005, 0.0, 1.0));
        let (mut x, mut v) = (0.0, 0.0);
        let mut frames = 0;
        while !spring.settled(x, v, 1.0) {
            (x, v) = spring.step(x, v, 1.0, 1.0 / 60.0);
            frames += 1;
            assert!(frames < 120, "it never settles");
        }
        assert!(frames > 10, "it settled in {frames} frames");
        // A wider threshold settles sooner.
        let loose = spring.with_threshold(0.05);
        let (mut x, mut v, mut sooner) = (0.0, 0.0, 0);
        while !loose.settled(x, v, 1.0) {
            (x, v) = loose.step(x, v, 1.0, 1.0 / 60.0);
            sooner += 1;
        }
        assert!(sooner < frames);
    }

    #[test]
    fn nonsense_goes_to_the_target_and_no_time_changes_nothing() {
        assert_eq!(Spring::new(0.0, 10.0).step(3.0, 9.0, 7.0, 0.1), (7.0, 0.0));
        assert_eq!(
            Spring::new(f32::NAN, 10.0).step(3.0, 9.0, 7.0, 0.1),
            (7.0, 0.0)
        );
        assert_eq!(Spring::new(-4.0, 10.0).step(3.0, 9.0, 7.0, 0.1), (7.0, 0.0));
        assert!(Spring::new(0.0, 10.0).settled(3.0, 9.0, 7.0));
        assert_eq!(Spring::SMOOTH.step(3.0, 9.0, 7.0, 0.0), (3.0, 9.0));
        assert_eq!(Spring::SMOOTH.step(3.0, 9.0, 7.0, -1.0), (3.0, 9.0));
        assert_eq!(Spring::SMOOTH.step(3.0, 9.0, 7.0, f32::NAN), (3.0, 9.0));
        // No damping at all swings for ever and is never settled away from rest.
        let (x, _) = Spring::new(100.0, 0.0).step(0.0, 0.0, 1.0, 10.0);
        assert!(x.is_finite());
    }
}
