//! The driving simulation's fixed-step clock and in-race state gate.
//!
//! The high-level timer driver (`FUN_00443e90`) advances the in-race state
//! machine on a fixed **25 Hz / 40 ms** step derived from `timeGetTime`
//! (WINMM). The step is implemented by integer-millisecond *bucketing*: a tick
//! boundary is crossed when `t / 0x28` changes (`FUN_00443e30` returns
//! `before/0x28 != after/0x28`), and `FUN_00443e60` busy-waits until the bucket
//! advances. `0x28 = 40` ms, so the simulation runs at a fixed 25 Hz with
//! catch-up while render is decoupled (a separate present pump). There is no
//! float dt accumulator - the per-step dt is the implicit constant 40 ms,
//! appearing in the physics as the Q16.16 coefficient `0xa3d` (~`0.04` s).
//!
//! The state machine (`FUN_00443db0`, current-state enum `DAT_005590a8`) runs
//! the in-race/gameplay handlers for state ids `5..=10` under this fixed step,
//! gated by the recovered range test `4 < state && state < 0xb`; ids below 5
//! are the untimed frontend/menu states.
//!
//! This module reconstructs the pure, testable pieces: the step constants, the
//! bucket function and boundary-crossing predicate, and the in-race state gate.
//! The `timeGetTime` read and the busy-wait spin are host/runtime glue, left to
//! the platform host (see `cmr-platform-sdl3`). See evidence
//! `rea.official-eu.sim-fixed-step`.

use crate::fixed_point::Fixed16;

/// Fixed simulation step in milliseconds (`0x28`).
pub const SIM_STEP_MS: u32 = 40;

/// Simulation tick rate (`1000 / SIM_STEP_MS`).
pub const SIM_TICK_HZ: u32 = 1000 / SIM_STEP_MS;

/// The per-step delta time in seconds as a Q16.16 constant, matching the
/// recovered physics coefficient `0xa3d` (~`0.04`).
pub const SIM_STEP_SECONDS: Fixed16 = Fixed16::from_raw(0xa3d);

/// First in-race (fixed-step) state id.
pub const FIRST_IN_RACE_STATE: u32 = 5;

/// Last in-race (fixed-step) state id.
pub const LAST_IN_RACE_STATE: u32 = 10;

/// The millisecond bucket a timestamp falls in, reproducing `t / 0x28`.
pub const fn step_bucket(ms: u32) -> u32 {
    ms / SIM_STEP_MS
}

/// Whether a tick that started at `before_ms` and ended at `after_ms` crossed a
/// step boundary, reproducing `FUN_00443e30` (`before/0x28 != after/0x28`). The
/// driver loops (runs another tick) while this holds, catching the sim up when
/// a tick overran its 40 ms budget.
pub const fn step_boundary_crossed(before_ms: u32, after_ms: u32) -> bool {
    step_bucket(before_ms) != step_bucket(after_ms)
}

/// Whether a state id runs under the fixed-step loop, reproducing the recovered
/// range gate `4 < state && state < 0xb`.
pub const fn is_in_race_state(state: u32) -> bool {
    FIRST_IN_RACE_STATE <= state && state <= LAST_IN_RACE_STATE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_constants_match_the_recovered_25hz() {
        assert_eq!(SIM_STEP_MS, 0x28);
        assert_eq!(SIM_TICK_HZ, 25);
        // 0.04 s in Q16.16 is 0xa3d (2621); the recovered coefficient.
        assert_eq!(SIM_STEP_SECONDS.raw(), 0xa3d);
        assert_eq!((0.04_f64 * 65_536.0).round() as i32, 0xa3d);
    }

    #[test]
    fn bucketing_matches_the_40ms_boundary() {
        assert_eq!(step_bucket(0), 0);
        assert_eq!(step_bucket(39), 0);
        assert_eq!(step_bucket(40), 1);
        assert_eq!(step_bucket(81), 2);
        // Same bucket: no boundary crossed (tick finished within its step).
        assert!(!step_boundary_crossed(10, 39));
        // Different bucket: a boundary crossed (tick overran into the next step).
        assert!(step_boundary_crossed(39, 40));
        assert!(step_boundary_crossed(10, 120));
    }

    #[test]
    fn in_race_state_gate_matches_the_recovered_range() {
        // 4 < state && state < 0xb  ==>  5..=10.
        for state in 0..5 {
            assert!(!is_in_race_state(state), "state {state} should be untimed");
        }
        for state in 5..=10 {
            assert!(is_in_race_state(state), "state {state} should be in-race");
        }
        assert!(!is_in_race_state(11));
        assert!(!is_in_race_state(0xc));
    }
}
