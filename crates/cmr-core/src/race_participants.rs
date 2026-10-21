//! Reconstruction of the race-participant model and multi-car interaction (G06).
//!
//! **Verdict (the decisive G06 finding): the engine has no on-track AI
//! opponent.** The 25 Hz mechanics loop `FUN_0043afd0` simulates exactly
//! `DAT_00550e90` cars, and the race setup `FUN_00481350` never writes that count
//! above `2`. The only non-player car ever simulated is the documented Player-2
//! base (`DAT_00700a68` / `DAT_0060042c`), reached solely when a second *human*
//! is present - split-screen (`FUN_00485bb0`) or network (`FUN_00401060`) - and
//! replay/playback (`DAT_00d3ca88 != 0`) forces the count to `1`. No function
//! drives a second control struct's actuators (`ctrl+0x2b8[4]`) from a
//! racing-line or waypoint spline; the only "line" data in the race loop
//! (`DAT_0057ae6c`, stride `0x1c`) is *read* against a physically-driven car's
//! position to record split/stage times (G05), never to steer a car. Rival WRC
//! drivers exist only as a name roster feeding the championship/standings UI;
//! their stage times are authored data (outside the executable), not produced by
//! an in-exe physics or AI model.
//!
//! The cleanly-recovered, testable logic reconstructed here is therefore the
//! participant-count model and the car-vs-car interaction gate:
//!
//! - the active simulated-car count `DAT_00550e90` (`FUN_00481350`): `1` under
//!   replay or single-car mode, `2` for split-screen (under the config gate),
//!   otherwise `1`;
//! - the dispatcher count `(count == 2) ? 2 : 1` passed to `FUN_00407690`;
//! - car-vs-car collision (`FUN_00442740` -> `FUN_00441f30`) runs only when two
//!   cars are present, not in replay, and either split-screen or network.
//!
//! The roster/record *structure* (counts and strides) is recorded as format
//! metadata; the driver-name and car-name strings are game data and are NOT
//! vendored here.
//!
//! See evidence `rea.official-eu.sim-race-participants`.
//!
//! Observations: the car-count bound, the collision gate, and the table
//! strides/counts. Inference: the absence of any actuator-writing controller
//! proves there is no on-track AI (an absence argument over the complete
//! loop/call structure). Unknowns (not reproduced): the in-exe source (if any) of
//! rival championship times, the exact non-split/non-replay count branch, the
//! `DAT_004f6844` data-slot count (3, or 6 networked) vs the 2 simulated cars,
//! and the driver-attribute tier-table semantics.

/// Maximum number of cars the engine ever physically simulates (`DAT_00550e90`
/// is never written above this).
pub const MAX_SIMULATED_CARS: u32 = 2;

/// Number of named drivers in the championship roster (name strings are game
/// data, not vendored; this is the record count only).
pub const DRIVER_ROSTER_SIZE: usize = 8;

/// Stride of a roster record at `0x507300` (a 2-char code + full name).
pub const DRIVER_ROSTER_RECORD_STRIDE: usize = 0x30;

/// Stride of a driver/car select tier record at `0x506e5c` (tier byte + 8
/// attribute bytes + 2 name pointers).
pub const DRIVER_ATTR_RECORD_STRIDE: usize = 0x1c;

/// Number of per-profile best-time record entries (initialised to `-1`).
pub const BEST_TIME_RECORD_COUNT: usize = 0x12;

/// Stride of a best-time record entry.
pub const BEST_TIME_RECORD_STRIDE: usize = 0x15;

/// The active simulated-car count (`DAT_00550e90`), reproducing the confident
/// branches of the race setup `FUN_00481350`: replay or single-car mode gives
/// one car; split-screen (when its config gate is set) gives two; otherwise one.
pub fn active_car_count(
    replay: bool,
    single_car_mode: bool,
    split_screen: bool,
    split_config_enabled: bool,
) -> u32 {
    if replay || single_car_mode {
        1
    } else if split_screen && split_config_enabled {
        MAX_SIMULATED_CARS
    } else {
        1
    }
}

/// The car count passed to the dynamics dispatcher `FUN_00407690`
/// (`(DAT_00550e90 == 2) ? 2 : 1`).
pub fn dispatcher_car_count(active_cars: u32) -> u32 {
    if active_cars == MAX_SIMULATED_CARS {
        MAX_SIMULATED_CARS
    } else {
        1
    }
}

/// Whether car-vs-car collision (`FUN_00442740` -> `FUN_00441f30`) runs this
/// tick, reproducing the `FUN_0043afd0` gate: two cars present, not in replay,
/// and either split-screen or network present.
pub fn car_collision_enabled(
    car_count: u32,
    replay: bool,
    split_screen: bool,
    network: bool,
) -> bool {
    car_count == MAX_SIMULATED_CARS && !replay && (split_screen || network)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_car_count_is_one_unless_a_second_human_splits() {
        // Single player: one car.
        assert_eq!(active_car_count(false, false, false, false), 1);
        // Split-screen with its config gate set: two cars.
        assert_eq!(active_car_count(false, false, true, true), 2);
        // Split-screen requested but the config gate is off: still one.
        assert_eq!(active_car_count(false, false, true, false), 1);
        // Replay forces one car even if split would otherwise apply.
        assert_eq!(active_car_count(true, false, true, true), 1);
        // Single-car mode forces one.
        assert_eq!(active_car_count(false, true, true, true), 1);
    }

    #[test]
    fn dispatcher_count_mirrors_the_two_car_case() {
        assert_eq!(dispatcher_car_count(1), 1);
        assert_eq!(dispatcher_car_count(2), 2);
        // Never above the simulated maximum.
        assert_eq!(dispatcher_car_count(5), 1);
    }

    #[test]
    fn car_collision_needs_two_cars_and_a_second_human() {
        // Two cars, split-screen, not replay: collision runs.
        assert!(car_collision_enabled(2, false, true, false));
        // Two cars, network, not replay: collision runs.
        assert!(car_collision_enabled(2, false, false, true));
        // One car: never.
        assert!(!car_collision_enabled(1, false, true, true));
        // Two cars but replay: suppressed.
        assert!(!car_collision_enabled(2, true, true, true));
        // Two cars but neither split nor network (cannot normally occur): off.
        assert!(!car_collision_enabled(2, false, false, false));
    }

    #[test]
    fn roster_structure_constants_match_recovery() {
        assert_eq!(MAX_SIMULATED_CARS, 2);
        assert_eq!(DRIVER_ROSTER_SIZE, 8);
        assert_eq!(DRIVER_ROSTER_RECORD_STRIDE, 0x30);
        assert_eq!(DRIVER_ATTR_RECORD_STRIDE, 0x1c);
        assert_eq!(BEST_TIME_RECORD_COUNT, 0x12);
        assert_eq!(BEST_TIME_RECORD_STRIDE, 0x15);
    }
}
