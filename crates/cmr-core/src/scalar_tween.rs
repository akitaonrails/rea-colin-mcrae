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
//! This module reconstructs the **linear** curve (mode `0`) only - the mode the
//! screen fade uses ([`crate::render_compose::fade_in`] /
//! [`crate::render_compose::fade_out`]). The ease modes (`2`/`3` triangular, `1`/`4`
//! variants) are recovered in the evidence ledger but not yet reconstructed; a
//! [`ScalarTween`] is therefore always linear here.
//!
//! The evaluator order is preserved exactly: the output uses the counter value
//! from *before* this frame's increment (`fraction`), the increment is clamped at
//! `target+3`, and once the (post-increment) counter reaches `target` the tween
//! holds at its end value and marks itself finished.

/// The Q12 fixed-point shift the slope interpolation uses (`FUN_004ad5b0`:
/// `(slope * fraction) >> 12`).
pub const TWEEN_SLOPE_SHIFT: u32 = 12;

/// One linear scalar tween: a value that ramps from `start` to `end` over
/// `target` evaluations (frames). Reconstructs the linear path of `FUN_004ad5b0`
/// / `FUN_004ad720`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScalarTween {
    start: i32,
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
            start,
            end,
            target,
            counter: 0,
            slope_q12,
            delay: delay.max(0),
            finished,
        }
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
            self.start
        } else {
            let fraction = self.counter;
            ((self.slope_q12 * fraction) >> TWEEN_SLOPE_SHIFT) + self.start
        }
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
            return self.end;
        }
        if self.delay > 0 {
            self.delay -= 1;
            return self.start;
        }
        let fraction = self.counter;
        if self.counter < self.target + 3 {
            self.counter += 1;
        }
        if self.counter < self.target {
            ((self.slope_q12 * fraction) >> TWEEN_SLOPE_SHIFT) + self.start
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
