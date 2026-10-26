//! Reconstruction of the race-participant model and multi-car interaction (G06).
//!
//! **Verdict (the decisive G06 finding): the engine has no on-track AI
//! opponent.** The 25 Hz mechanics loop `FUN_0043afd0` simulates exactly
//! `DAT_00550e90` cars, and the race setup `FUN_00481350` never writes that count
//! above `2`. The only non-player car ever simulated is the documented Player-2
//! base (`DAT_00700a68` / `DAT_0060042c`), reached solely when a second *human*
//! is present - split-screen (`FUN_00485bb0`) or network (`FUN_00401060`) - and
//! replay/playback (`DAT_00d3ca88 != 0`) forces the count to `1`. No function
//! drives a second car's command region (`ctrl+0x2fc..+0x318`) or raw wheel
//! speeds (`ctrl+0x2a8[4]`) from a racing-line or waypoint spline; the only
//! "line" data in the race loop
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

/// Native setup predicates for replay-slot and simulated-car counts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ParticipantSetupInput {
    pub replay: bool,
    pub record_mode: bool,
    pub special_514478: bool,
    pub head_to_head: bool,
    pub branch_443b00: bool,
    pub replay_car_count: u32,
    pub championship: bool,
    pub network: bool,
    pub two_player: bool,
    pub config_mode: u8,
}

/// Distinct replay data-slot and physically simulated car counts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParticipantSetupCounts {
    pub data_slots: u32,
    pub simulated_cars: u32,
}

/// Recover `FUN_00481350`'s distinct `DAT_004f6844` and `DAT_00550e90` counts.
#[must_use]
pub fn participant_setup_counts(input: ParticipantSetupInput) -> ParticipantSetupCounts {
    let (data_slots, simulated_cars) = if input.replay || input.record_mode {
        (1, 1)
    } else if input.special_514478 {
        (3, 1)
    } else if input.head_to_head {
        (3, 2)
    } else if input.branch_443b00 || (input.replay_car_count != 0 && input.championship) {
        if !input.championship || input.network || input.config_mode != 2 {
            (2, 2)
        } else {
            (1, 1)
        }
    } else {
        (input.replay_car_count, input.replay_car_count)
    };
    ParticipantSetupCounts {
        data_slots: data_slots * if input.two_player { 2 } else { 1 },
        simulated_cars,
    }
}

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
    fn participant_setup_distinguishes_replay_slots_and_simulated_cars() {
        let base = ParticipantSetupInput::default();
        assert_eq!(
            participant_setup_counts(ParticipantSetupInput {
                replay: true,
                ..base
            }),
            ParticipantSetupCounts {
                data_slots: 1,
                simulated_cars: 1,
            }
        );
        assert_eq!(
            participant_setup_counts(ParticipantSetupInput {
                special_514478: true,
                ..base
            })
            .data_slots,
            3
        );
        assert_eq!(
            participant_setup_counts(ParticipantSetupInput {
                head_to_head: true,
                two_player: true,
                ..base
            }),
            ParticipantSetupCounts {
                data_slots: 6,
                simulated_cars: 2,
            }
        );
        assert_eq!(
            participant_setup_counts(ParticipantSetupInput {
                replay_car_count: 1,
                championship: true,
                config_mode: 2,
                ..base
            })
            .data_slots,
            1
        );
        assert_eq!(
            participant_setup_counts(ParticipantSetupInput {
                replay_car_count: 1,
                championship: true,
                network: true,
                config_mode: 2,
                ..base
            })
            .data_slots,
            2
        );
        assert_eq!(
            participant_setup_counts(ParticipantSetupInput {
                replay_car_count: 2,
                ..base
            })
            .simulated_cars,
            2
        );
        assert_eq!(participant_setup_counts(base).data_slots, 0);
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
