//! Bridge from the recovered control-map output ([`CarControls`]) to the
//! race-state sim's per-tick [`Controls`].
//!
//! **What is recovered** (the routing): the control map writes independent
//! longitudinal demands (throttle `+0x304`, brake `+0x308`), a steer split
//! (left `+0x2fc` / right `+0x300`) and a handbrake flag, each a magnitude
//! `0..0xffbe` ([`FULL_SCALE`]). The net longitudinal command is throttle minus
//! brake; the net steering command is right minus left; the handbrake passes
//! through. That routing and sign convention is faithful to `FUN_00449580`.
//!
//! **What is caller config** (the magnitudes): the shipped build turns those
//! demands into motion through the drivetrain actuator/torque stage
//! (`FUN_004088c0` -> gearbox/RPM/torque -> the physics integrator, G02/G03), and
//! the [`RaceState`](crate::race_state) harness is a documented simplification
//! that takes an explicit forward force and heading rate. Its heading sign
//! matches the recovered right-minus-left command at this boundary. So the full-scale
//! response is supplied by the caller as a [`DriveResponse`], not invented here -
//! the same choice as the harness [`SimParams`](crate::race_state::SimParams).
//! When the demand->force stage is recovered, the caller fills [`DriveResponse`]
//! with the recovered constants. The simplified [`RaceState`](crate::race_state::RaceState)
//! heading convention increases toward the requested right side on the X/Z
//! plane; the recovered input-map half channels remain unchanged.

use crate::fixed_point::Fixed16;
use crate::input_map::{CarControls, FULL_SCALE, GearCommand};
use crate::pedal_response::RawPedals;
use crate::race_state::Controls;
use crate::vehicle_dynamics::RawSteer;

/// The sim's full-scale response to a full-scale control demand. The caller owns
/// these magnitudes (a documented harness choice until the drivetrain
/// demand->force stage is recovered); the bridge only scales linearly by them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriveResponse {
    /// Forward driving force (Q16.16) at full throttle (demand [`FULL_SCALE`]);
    /// braking applies the same magnitude in the reverse direction.
    pub max_forward_force: Fixed16,
    /// Heading rate (signed 16-bit BAM per tick) at full steer (demand
    /// [`FULL_SCALE`]).
    pub max_steer_rate: i16,
}

/// Map one frame's [`CarControls`] to the race-state [`Controls`], scaling the
/// net longitudinal and steering demands linearly by the caller's
/// [`DriveResponse`].
#[must_use]
pub fn car_controls_to_sim(controls: &CarControls, response: &DriveResponse) -> Controls {
    // Throttle drives forward, brake drives back: the shipped demands are
    // independent magnitudes, so the net longitudinal command is their signed
    // difference (both full -> cancel, matching holding both pedals).
    let net_drive = controls.throttle - controls.brake;
    let forward_force = scale_fixed(net_drive, response.max_forward_force);

    // The recovered command is right-minus-left. Keep that sign at the explicit
    // planar harness edge; swapping it here inverted both keyboard and analog
    // steering in the controlled native AU1 session.
    let net_steer = controls.steer_right - controls.steer_left;
    let steer_rate = scale_bam(net_steer, response.max_steer_rate);

    Controls {
        steer_rate,
        forward_force,
        handbrake: controls.handbrake,
        raw_steer: RawSteer {
            left_2fc: controls.steer_left,
            right_300: controls.steer_right,
            analog_410: controls.steer_analog,
        },
        raw_pedals: RawPedals {
            throttle_304: controls.throttle,
            brake_308: controls.brake,
            throttle_analog_414: controls.throttle_analog,
            brake_analog_418: controls.brake_analog,
        },
        raw_gear_command_314: match controls.gear_command {
            GearCommand::None => 0,
            GearCommand::Up => 1,
            GearCommand::Down => -1,
        },
    }
}

/// Scale a signed demand in `[-FULL_SCALE, FULL_SCALE]` linearly onto `[-max,
/// max]` as a Q16.16 value: `max * demand / FULL_SCALE`.
fn scale_fixed(demand: i32, max: Fixed16) -> Fixed16 {
    let raw = (i64::from(max.raw()) * i64::from(demand)) / i64::from(FULL_SCALE);
    Fixed16::from_raw(raw as i32)
}

/// Scale a signed demand in `[-FULL_SCALE, FULL_SCALE]` linearly onto `[-max,
/// max]` as a BAM rate: `max * demand / FULL_SCALE`.
fn scale_bam(demand: i32, max: i16) -> i16 {
    ((i32::from(max) * demand) / FULL_SCALE) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response() -> DriveResponse {
        DriveResponse {
            max_forward_force: Fixed16::from_raw(8 << 16),
            max_steer_rate: 0x200,
        }
    }

    fn controls() -> CarControls {
        CarControls::default()
    }

    #[test]
    fn full_throttle_gives_the_full_forward_force() {
        let cc = CarControls {
            throttle: FULL_SCALE,
            ..controls()
        };
        let out = car_controls_to_sim(&cc, &response());
        assert_eq!(out.forward_force, Fixed16::from_raw(8 << 16));
        assert_eq!(out.steer_rate, 0);
        assert!(!out.handbrake);
    }

    #[test]
    fn raw_steer_magnitudes_pass_through_unscaled_and_unswapped() {
        let cc = CarControls {
            steer_left: 0xfc00,
            steer_right: 0x400,
            ..controls()
        };
        let out = car_controls_to_sim(&cc, &response());
        assert_eq!(
            out.raw_steer,
            RawSteer {
                left_2fc: 0xfc00,
                right_300: 0x400,
                analog_410: false,
            }
        );
    }

    #[test]
    fn gear_commands_become_the_signed_314_byte() {
        for (command, raw) in [
            (GearCommand::None, 0),
            (GearCommand::Up, 1),
            (GearCommand::Down, -1),
        ] {
            let cc = CarControls {
                gear_command: command,
                ..controls()
            };
            assert_eq!(
                car_controls_to_sim(&cc, &response()).raw_gear_command_314,
                raw
            );
        }
    }

    #[test]
    fn raw_pedal_words_and_analog_flags_pass_through() {
        let cc = CarControls {
            throttle: 0xfc00,
            brake: 0x8000,
            throttle_analog: true,
            brake_analog: false,
            ..controls()
        };
        assert_eq!(
            car_controls_to_sim(&cc, &response()).raw_pedals,
            RawPedals {
                throttle_304: 0xfc00,
                brake_308: 0x8000,
                throttle_analog_414: true,
                brake_analog_418: false,
            }
        );
    }

    #[test]
    fn full_brake_gives_the_full_reverse_force() {
        let cc = CarControls {
            brake: FULL_SCALE,
            ..controls()
        };
        let out = car_controls_to_sim(&cc, &response());
        assert_eq!(out.forward_force, Fixed16::from_raw(-(8 << 16)));
    }

    #[test]
    fn throttle_and_brake_together_cancel() {
        let cc = CarControls {
            throttle: FULL_SCALE,
            brake: FULL_SCALE,
            ..controls()
        };
        assert_eq!(
            car_controls_to_sim(&cc, &response()).forward_force,
            Fixed16::ZERO
        );
    }

    #[test]
    fn steering_turns_toward_the_requested_world_side() {
        use crate::fixed_point::{Vec3, build_sine_table, cos_bam, sin_bam};

        let right = CarControls {
            steer_right: FULL_SCALE,
            ..controls()
        };
        let right_rate = car_controls_to_sim(&right, &response()).steer_rate;
        assert_eq!(right_rate, 0x200);
        let left = CarControls {
            steer_left: FULL_SCALE,
            ..controls()
        };
        let left_rate = car_controls_to_sim(&left, &response()).steer_rate;
        assert_eq!(left_rate, -0x200);

        // At heading zero forward is +X. The native AU1 control contract pins
        // requested right to a positive heading delta and left to a negative
        // delta at this simplified harness boundary.
        let trig = build_sine_table();
        let positive_heading_side = Vec3::new(Fixed16::ZERO, Fixed16::ZERO, Fixed16::ONE);
        let forward_after = |rate: i16| {
            Vec3::new(
                cos_bam(&trig, rate as u16),
                Fixed16::ZERO,
                sin_bam(&trig, rate as u16),
            )
        };
        assert!(forward_after(right_rate).dot(positive_heading_side) > Fixed16::ZERO);
        assert!(forward_after(left_rate).dot(positive_heading_side) < Fixed16::ZERO);
    }

    #[test]
    fn half_demand_scales_linearly() {
        let half = FULL_SCALE / 2;
        let cc = CarControls {
            throttle: half,
            steer_right: half,
            ..controls()
        };
        let out = car_controls_to_sim(&cc, &response());
        // Half throttle -> about half the full force (exact integer division).
        assert_eq!(out.forward_force.raw(), (8 << 16) / 2);
        assert_eq!(out.steer_rate, 0x100);
    }

    #[test]
    fn handbrake_passes_through() {
        let cc = CarControls {
            handbrake: true,
            ..controls()
        };
        assert!(car_controls_to_sim(&cc, &response()).handbrake);
    }
}
