//! The linear mode of the recovered generic scalar tween engine
//! (`rea.official-eu.scalar-tween`, asm/rizin-confirmed).
//!
//! The shipped game drives ~10 scalar animations (screen fades, menu slides,
//! camera crossfades) through one engine: a table of up to 32 records (base
//! `0x5046d8`, stride `0x2c`) advanced by the per-frame evaluator `FUN_004ad5b0`
//! and programmed by the trigger `FUN_004ad720`. Each record holds a curve mode
//! (`+0x00`: `0` linear, `2`/`3` ease, `1`/`4` variants, `5` finished), a target
//! duration (`+0x04`), base/end output values (`+0x08`/`+0x0c`), an active flag
//! (`+0x14`), a Q12 per-tick slope (`+0x1c` = `(end-start)*0x1000/target`), an
//! initial-delay countdown (`+0x24`), and the current counter (`+0x28`,
//! incremented by one per evaluation and clamped below `target+3`).
//!
//! This module reconstructs the **linear** curve (mode `0`), which the screen
//! fade uses ([`crate::screen_fade::ScreenFade`]), and the two triangular ease curves the
//! front-end menus use: mode `2` ([`ScalarTween::ease_out`]) and mode `3`
//! ([`ScalarTween::ease_in`]). Modes `1`/`4` (a table-driven tail) are not
//! reconstructed. The Q12 product is rounded toward zero (`cdq; and edx,0xfff;
//! add; sar 0xc`, `0x4ad6d8`).
//!
//! The evaluator order is preserved exactly: the output uses the counter value
//! from *before* this frame's increment (`fraction`), the increment is clamped at
//! `target+3`, and once the (post-increment) counter reaches `target` the tween
//! holds at its end value and marks itself finished.

/// The Q12 fixed-point shift the slope interpolation uses (`FUN_004ad5b0`:
/// `(slope * fraction) >> 12`).
pub const TWEEN_SLOPE_SHIFT: u32 = 12;

/// The curve a [`ScalarTween`] follows (`+0x00`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Curve {
    /// Mode 0.
    Linear,
    /// Mode 2.
    EaseOut,
    /// Mode 3.
    EaseIn,
}

/// One linear scalar tween: a value that ramps from `start` to `end` over
/// `target` evaluations (frames). Reconstructs the linear path of `FUN_004ad5b0`
/// / `FUN_004ad720`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScalarTween {
    curve: Curve,
    /// `+0x08`: the base the slope is added to.
    start: i32,
    /// `+0x10`: the value returned while the delay runs (the trigger's
    /// first value).
    first: i32,
    /// `+0x0c`: the end value.
    end: i32,
    target: i32,
    counter: i32,
    slope_q12: i32,
    delay: i32,
    finished: bool,
}

impl ScalarTween {
    /// Program a linear tween from `start` to `end` over `target` frames, after an
    /// initial `delay` of frames holding at `start` (`FUN_004ad720` with mode `0`).
    /// The Q12 slope is `(end-start)*0x1000/target`, matching the trigger. A
    /// non-positive `target` yields a tween that is already finished at `end`.
    #[must_use]
    pub fn linear(start: i32, end: i32, target: i32, delay: i32) -> Self {
        let (target, slope_q12, finished) = if target > 0 {
            let slope = ((end - start) << TWEEN_SLOPE_SHIFT) / target;
            (target, slope, false)
        } else {
            (0, 0, true)
        };
        Self {
            curve: Curve::Linear,
            start,
            first: start,
            end,
            target,
            counter: 0,
            slope_q12,
            delay: delay.max(0),
            finished,
        }
    }

    /// `FUN_004ad720` mode 2: an ease-out from `from` to `to` over `steps`
    /// evaluations after `delay`: the slope is `((from - to) << 12) / T` with
    /// `T = steps * (steps + 1) / 2`, the base `to`, and each step weighs
    /// `(n - c + 1)(n - c) / 2`, so the value starts at `from` and slows into
    /// `to`.
    #[must_use]
    pub fn ease_out(from: i32, to: i32, steps: i32, delay: i32) -> Self {
        Self::triangular(Curve::EaseOut, from, to, steps, delay)
    }

    /// `FUN_004ad720` mode 3: an ease-in from `from` to `to`: slope
    /// `((to - from) << 12) / T`, base `from`, weight `c (c + 1) / 2`.
    #[must_use]
    pub fn ease_in(from: i32, to: i32, steps: i32, delay: i32) -> Self {
        Self::triangular(Curve::EaseIn, from, to, steps, delay)
    }

    fn triangular(curve: Curve, from: i32, to: i32, steps: i32, delay: i32) -> Self {
        let total = steps.wrapping_mul(steps.wrapping_add(1)) / 2;
        let (start, numerator) = match curve {
            Curve::EaseOut => (to, from.wrapping_sub(to)),
            _ => (from, to.wrapping_sub(from)),
        };
        let slope_q12 = if total != 0 {
            numerator
                .wrapping_shl(TWEEN_SLOPE_SHIFT)
                .wrapping_div(total)
        } else {
            0
        };
        Self {
            curve,
            start,
            first: from,
            end: to,
            target: steps,
            counter: 0,
            slope_q12,
            delay: delay.max(0),
            finished: steps <= 0,
        }
    }

    /// The weight of the pre-increment counter `c = min(counter, n)` (a
    /// byte), by curve.
    fn fraction(&self) -> i32 {
        let c = i32::from(self.counter.min(self.target) as u8);
        let n = self.target;
        match self.curve {
            Curve::Linear => c,
            Curve::EaseOut => (n - c + 1).wrapping_mul(n - c) / 2,
            Curve::EaseIn => (c + 1).wrapping_mul(c) / 2,
        }
    }

    fn value(&self, fraction: i32) -> i32 {
        let product = self.slope_q12.wrapping_mul(fraction);
        // Rounded toward zero.
        let rounded = product.wrapping_add((product >> 31) & 0xfff) >> TWEEN_SLOPE_SHIFT;
        rounded.wrapping_add(self.start)
    }

    /// The current output without advancing (the value the last
    /// [`evaluate_and_advance`](Self::evaluate_and_advance) produced, or `start`
    /// before the first evaluation / during the initial delay, or `end` once
    /// finished).
    #[must_use]
    pub fn level(&self) -> i32 {
        if self.finished {
            self.end
        } else if self.delay > 0 || self.counter == 0 {
            self.first
        } else {
            self.value(self.fraction())
        }
    }

    /// `FUN_004ad560`'s in-progress test of the owning handle: an active
    /// tween runs until its counter reaches `target + 3`, an inactive one
    /// (the instant mode) until it reaches `target`.
    #[must_use]
    pub fn in_progress(&self, active: bool) -> bool {
        let limit = if active { self.target + 3 } else { self.target };
        self.counter < limit
    }

    /// Whether the tween has reached its end value.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Evaluate one frame and advance, returning this frame's output
    /// (`FUN_004ad5b0`, linear mode). While the initial delay remains it is spent
    /// and `start` is returned; otherwise the output uses the pre-increment
    /// counter (`fraction`), the counter advances by one (clamped below
    /// `target+3`), and once the post-increment counter reaches `target` the tween
    /// holds at `end` and is marked finished.
    pub fn evaluate_and_advance(&mut self) -> i32 {
        if self.finished {
            // A finished record (mode 5) still counts on to `target+3`.
            if self.counter < self.target + 3 {
                self.counter += 1;
            }
            return self.end;
        }
        if self.delay > 0 {
            self.delay -= 1;
            return self.first;
        }
        let fraction = self.fraction();
        if self.counter < self.target + 3 {
            self.counter += 1;
        }
        if self.counter < self.target {
            self.value(fraction)
        } else {
            self.finished = true;
            self.end
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_ramp_reaches_the_end_at_the_target() {
        // 0 -> 0x10000 over 4 frames, no delay. The output uses the pre-increment
        // counter (`fraction`), but completion is checked on the post-increment
        // counter, so the final fraction is skipped and the tween snaps to `end`
        // when the counter reaches the target.
        let mut t = ScalarTween::linear(0, 0x10000, 4, 0);
        assert_eq!(t.level(), 0);
        assert_eq!(t.evaluate_and_advance(), 0); // fraction 0
        assert_eq!(t.evaluate_and_advance(), 0x10000 / 4); // fraction 1
        assert_eq!(t.evaluate_and_advance(), 0x10000 / 2); // fraction 2
        // Counter reaches the target -> hold at end, finished (fraction 3 skipped).
        assert_eq!(t.evaluate_and_advance(), 0x10000);
        assert!(t.is_finished());
        // Further evaluations stay at end.
        assert_eq!(t.evaluate_and_advance(), 0x10000);
    }

    #[test]
    fn descending_ramp_and_slope_sign() {
        // 0x10000 -> 0 over 2 frames: start, then end.
        let mut t = ScalarTween::linear(0x10000, 0, 2, 0);
        assert_eq!(t.evaluate_and_advance(), 0x10000); // fraction 0 -> start
        assert_eq!(t.evaluate_and_advance(), 0); // counter reaches target -> end
        assert!(t.is_finished());
    }

    #[test]
    fn initial_delay_holds_at_start() {
        let mut t = ScalarTween::linear(0, 0x1000, 2, 3);
        // Three delay frames return start without advancing the ramp.
        for _ in 0..3 {
            assert_eq!(t.evaluate_and_advance(), 0);
            assert!(!t.is_finished());
        }
        // Then the ramp proceeds: start, then end (target 2 -> no midpoint).
        assert_eq!(t.evaluate_and_advance(), 0); // fraction 0 -> start
        assert_eq!(t.evaluate_and_advance(), 0x1000); // end
        assert!(t.is_finished());
    }

    #[test]
    fn a_descending_ramp_rounds_toward_zero() {
        // (0 - 10) << 12 / 3 = -13653; at fraction 1 the product -13653 >> 12
        // rounds toward zero to -3 (a floor would give -4).
        let mut t = ScalarTween::linear(10, 0, 3, 0);
        assert_eq!(t.evaluate_and_advance(), 10);
        assert_eq!(t.evaluate_and_advance(), 7);
        assert_eq!(t.evaluate_and_advance(), 0);
    }

    #[test]
    fn the_ease_out_starts_fast_and_lands_on_the_end() {
        // 15 steps from 0x10000 to 0 (the setup info anchor's slide-in).
        let mut t = ScalarTween::ease_out(0x10000, 0, 15, 0);
        let values: Vec<i32> = (0..16).map(|_| t.evaluate_and_advance()).collect();
        // c = 0 weighs T = 120: the start value, give or take the Q12 slope.
        assert!((values[0] - 0x10000).abs() < 16);
        // The steps shrink: an ease-out.
        assert!(values[0] - values[1] > values[12] - values[13]);
        assert_eq!(values[14], 0);
        assert!(t.is_finished());
    }

    #[test]
    fn the_ease_in_starts_slow_and_delays_return_the_first_value() {
        let mut t = ScalarTween::ease_in(0, 0x10000, 15, 2);
        assert_eq!(t.evaluate_and_advance(), 0);
        assert_eq!(t.evaluate_and_advance(), 0);
        let values: Vec<i32> = (0..15).map(|_| t.evaluate_and_advance()).collect();
        assert_eq!(values[0], 0);
        assert!(values[1] - values[0] < values[13] - values[12]);
        assert_eq!(values[14], 0x10000);
        // Mode 2 with equal ends (the "no animation" option) is the end at once.
        let mut instant = ScalarTween::ease_out(0x8000, 0x8000, 15, 0);
        assert_eq!(instant.evaluate_and_advance(), 0x8000);
    }

    #[test]
    fn zero_or_negative_target_is_finished_immediately() {
        let mut t = ScalarTween::linear(0x10000, 0, 0, 0);
        assert!(t.is_finished());
        assert_eq!(t.level(), 0);
        assert_eq!(t.evaluate_and_advance(), 0);
        let neg = ScalarTween::linear(5, 9, -4, 0);
        assert!(neg.is_finished());
        assert_eq!(neg.level(), 9);
    }
}
