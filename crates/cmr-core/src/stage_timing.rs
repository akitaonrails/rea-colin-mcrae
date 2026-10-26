//! Reconstruction of the stage clock, split/elapsed readouts, checkpoint
//! de-duplication, and the finish-rule constants (the confidently-recovered G05
//! pieces).
//!
//! The master stage clock is the global `DAT_005ffc00`, a signed Q16.16 value in
//! **seconds** advanced once per physics tick by the frame drivers
//! `FUN_00437690`/`FUN_00437ac0`. The per-tick step is
//! `2^32 / (rate * 0x10000)` with `rate = 25` Hz (or `50` when the double-rate
//! flag `DAT_00551af0` is set), i.e. exactly one tick period in Q16.16 seconds:
//! `0xa3d` ~ 0.04 s at 25 Hz (the same dt coefficient recovered in
//! [`crate::sim_clock`]) and `0x51e` ~ 0.02 s at 50 Hz. The clock is therefore a
//! deterministic fixed-step tick accumulator, **not** a `timeGetTime` wall clock,
//! and it saturates at `0xe100000` = 3600.0 s.
//!
//! Elapsed times are read out via `FUN_0047fcf0` (whole seconds) and
//! `FUN_0047fd20` (centiseconds, the canonical stored/displayed stage time). A
//! per-car start timestamp is latched once by `FUN_0047fc50`. Checkpoints are
//! co-driver/split trigger volumes whose crossing is de-duplicated per car by a
//! passed-bitmask at `volume+0x2e` (`FUN_00403ee0`); the live clock is sampled
//! for the split rather than written into a per-split array on the anchored path.
//! Finish (trigger type 1) freezes the player time `DAT_00573a80` and per-car
//! time `DAT_005ffbf0[car]`, and the FSM moves from in-race state `5` to the
//! stage-results state `8`.
//!
//! See evidence `rea.official-eu.sim-stage-timing`.
//!
//! Observations: the clock representation, the step formula and `0xe100000` cap,
//! the two elapsed readouts, the start latch, the `1 << (car & 0x1f)` passed-bit,
//! and the `5`/`8` FSM states. Inference: the clock runs continuously, so a reset
//! or off-course costs real elapsed time implicitly. Best/record-time writes
//! use the checked settings-record backend. Each driver's results area contains
//! 18 rows of 20 bytes; `advance_result_row_ttl` covers the recovered hide path.
//! `FUN_00472809` is a print call site, not a standalone table-writer function.
//! Notable negative result: **no additive time-penalty system** (no false-start,
//! jump-start, missed-checkpoint, or off-course surcharge) was found - the clock
//! is never paused or incremented by a discrete penalty.

use crate::fixed_point::Fixed16;

/// Base simulation tick rate used by the stage-clock step formula (Hz).
pub const STAGE_CLOCK_BASE_RATE_HZ: i64 = 25;

/// Maximum stage time the clock saturates at (`0xe100000` = 3600.0 s / 1 hour).
pub const STAGE_CLOCK_MAX: Fixed16 = Fixed16::from_raw(0xe10_0000);

/// FSM state while driving the stage (`DAT_005590ac == 5`).
pub const IN_RACE_STATE: u8 = 5;

/// FSM state entered on stage completion (results / standings).
pub const STAGE_RESULTS_STATE: u8 = 8;

/// Maximum number of simultaneously-timed finishers (`DAT_005ffb88 < 8`).
pub const MAX_FINISHERS: u32 = 8;

/// Results-table byte marking an empty / unset time.
pub const RESULTS_EMPTY_SENTINEL: u8 = 0xff;

/// Result rows per driver and bytes per row at `DAT_0057c178`.
pub const RESULT_ROW_COUNT: usize = 18;
pub const RESULT_ROW_STRIDE: usize = 0x14;

/// Advance one result row's signed TTL. Positive values decrement; reaching
/// zero hides the row through the recovered `FUN_004619e0` route, while zero
/// and negative values remain unchanged.
#[must_use]
pub fn advance_result_row_ttl(ttl: i8) -> (i8, bool) {
    if ttl <= 0 {
        return (ttl, false);
    }
    let next = ttl - 1;
    (next, next == 0)
}

/// The per-tick stage-clock increment, reproducing the frame driver's
/// `step = 2^32 / (rate * 0x10000)` with `rate = 25` (or `50` when
/// `double_rate`). Integer division floors, giving `0xa3d` / `0x51e`.
pub fn stage_clock_step(double_rate: bool) -> Fixed16 {
    let rate = if double_rate { 2 } else { 1 } * STAGE_CLOCK_BASE_RATE_HZ;
    let divisor = rate * 0x1_0000; // the rate as Q16.16 (0x190000 / 0x320000)
    Fixed16::from_raw((0x1_0000_0000i64 / divisor) as i32)
}

/// Advance the stage clock by one tick, reproducing `FUN_0047fcd0`: add the step
/// only while the clock is below [`STAGE_CLOCK_MAX`] (the final tick may cross
/// the cap by up to one step, as in the shipped code).
pub fn advance_stage_clock(clock: Fixed16, step: Fixed16) -> Fixed16 {
    if clock < STAGE_CLOCK_MAX {
        clock.add(step)
    } else {
        clock
    }
}

/// Latch the per-car stage timer on the first qualifying tick, reproducing
/// `FUN_0047fc50`: if not yet active, mark it active and store the clock as the
/// start timestamp; otherwise leave both unchanged.
pub fn latch_stage_start(active: bool, start: Fixed16, clock: Fixed16) -> (bool, Fixed16) {
    if active { (true, start) } else { (true, clock) }
}

/// Whole elapsed seconds since the car's start timestamp, reproducing
/// `FUN_0047fcf0`: `(now - start) >> 16`.
pub fn elapsed_seconds(now: Fixed16, start: Fixed16) -> i32 {
    now.sub(start).to_int_floor()
}

/// Elapsed time in centiseconds (hundredths of a second), reproducing
/// `FUN_0047fd20`: `sec*100 + (frac * 100 >> 16)`, where `sec = delta >> 16` and
/// `frac` is the sub-second remainder. This is the canonical stored/displayed
/// stage/split time.
pub fn elapsed_centiseconds(now: Fixed16, start: Fixed16) -> i32 {
    let delta = now.sub(start);
    let seconds = delta.to_int_floor();
    let frac_raw = delta.sub(Fixed16::from_int(seconds)).raw();
    let frac_centis = ((frac_raw as i64 * 100) >> 16) as i32;
    seconds * 100 + frac_centis
}

/// Mark a checkpoint/split volume as passed by `car`, reproducing the per-car
/// bit set in `FUN_00403ee0`: `bits |= 1 << (car & 0x1f)`.
pub fn mark_checkpoint_passed(bits: u32, car: u32) -> u32 {
    bits | (1u32 << (car & 0x1f))
}

/// Whether `car` has already passed this checkpoint/split volume.
pub fn checkpoint_passed(bits: u32, car: u32) -> bool {
    bits & (1u32 << (car & 0x1f)) != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim_clock::SIM_STEP_SECONDS;

    #[test]
    fn tick_step_matches_the_recovered_dt_coefficient() {
        // 25 Hz: 2^32 / 0x190000 = 0xa3d, the same dt coefficient as sim_clock.
        assert_eq!(stage_clock_step(false), Fixed16::from_raw(0xa3d));
        assert_eq!(stage_clock_step(false), SIM_STEP_SECONDS);
        // 50 Hz: 2^32 / 0x320000 = 0x51e.
        assert_eq!(stage_clock_step(true), Fixed16::from_raw(0x51e));
    }

    #[test]
    fn clock_advances_then_saturates_at_one_hour() {
        let step = stage_clock_step(false);
        assert_eq!(advance_stage_clock(Fixed16::ZERO, step), step);
        // At/above the cap the clock stops advancing.
        assert_eq!(advance_stage_clock(STAGE_CLOCK_MAX, step), STAGE_CLOCK_MAX);
        // One step below the cap: the add is allowed (and may cross the cap).
        let just_below = STAGE_CLOCK_MAX.sub(Fixed16::from_raw(1));
        assert_eq!(advance_stage_clock(just_below, step), just_below.add(step));
        // The cap is exactly 3600.0 s.
        assert_eq!(STAGE_CLOCK_MAX.to_int_floor(), 3600);
    }

    #[test]
    fn start_latch_is_idempotent() {
        // Inactive: latches the current clock as the start timestamp.
        let clock = Fixed16::from_int(2);
        assert_eq!(
            latch_stage_start(false, Fixed16::ZERO, clock),
            (true, clock)
        );
        // Already active: start timestamp is preserved.
        let start = Fixed16::from_int(1);
        assert_eq!(latch_stage_start(true, start, clock), (true, start));
    }

    #[test]
    fn elapsed_readouts_match_seconds_and_centiseconds() {
        let start = Fixed16::from_int(10);
        // 1.5 s later: 1 whole second, 150 centiseconds.
        let now = start.add(Fixed16::from_raw(0x1_8000));
        assert_eq!(elapsed_seconds(now, start), 1);
        assert_eq!(elapsed_centiseconds(now, start), 150);
        // Exactly 2.0 s: 200 centiseconds.
        let now2 = start.add(Fixed16::from_int(2));
        assert_eq!(elapsed_centiseconds(now2, start), 200);
        // Zero elapsed.
        assert_eq!(elapsed_centiseconds(start, start), 0);
    }

    #[test]
    fn result_rows_hide_only_when_positive_ttl_reaches_zero() {
        assert_eq!(advance_result_row_ttl(2), (1, false));
        assert_eq!(advance_result_row_ttl(1), (0, true));
        assert_eq!(advance_result_row_ttl(0), (0, false));
        assert_eq!(advance_result_row_ttl(-1), (-1, false));
        assert_eq!(RESULT_ROW_COUNT * RESULT_ROW_STRIDE, 0x168);
    }

    #[test]
    fn checkpoint_bits_dedupe_per_car() {
        let bits = mark_checkpoint_passed(0, 0);
        assert!(checkpoint_passed(bits, 0));
        assert!(!checkpoint_passed(bits, 1));
        // Marking the same car again is idempotent.
        assert_eq!(mark_checkpoint_passed(bits, 0), bits);
        // The car index wraps modulo 32 (the `& 0x1f`).
        let wrapped = mark_checkpoint_passed(0, 33);
        assert!(checkpoint_passed(wrapped, 1));
        assert_eq!(wrapped, 0b10);
    }
}
