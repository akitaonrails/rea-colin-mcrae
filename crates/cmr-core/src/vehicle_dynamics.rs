//! Reconstruction of the suspension/tyre/grip response and the velocity
//! integrator (the confidently-recovered Q16.16 formulas of G03).
//!
//! From the dispatcher `FUN_00407690` solver passes:
//!
//! - **Lateral-grip constraint** (`FUN_0041b270`, runs before forces): removes
//!   the sideways component of the car velocity each step - `perp = v -
//!   fwd * dot(fwd, v)`, then `v -= perp * K`, with `K = 0x51e` (~0.02) normally
//!   and `0x28f` (~0.01) while the handbrake flag is set (half lateral grip ->
//!   slides).
//! - **Slope-grip loss** (`FUN_00408f50`): available grip falls as the car tilts.
//!   The loss is zero below tilt `0xb333` (0.70), then ramps linearly with slope
//!   `0x42ab5` (~4.1667) up to a cap of `0xe666` (0.90).
//! - **Integrator** (`FUN_0040a4a0`): `accel = force * invMass + gravity`;
//!   quadratic drag `drag = v * speed` (drag coefficient `1.0`); velocity
//!   integrates as `v += accel * 0x3333` (0.2 gain, the 40 ms step folded into
//!   the tuned coefficient).
//! - **Surface grip**: a per-wheel surface id `ctrl[0x42a + k*2]` indexes 8-byte
//!   grip-table records; the recovered coefficient tiers are `0x3d70` (~0.24,
//!   high grip) and `0x2666` (~0.15, low grip). The full per-surface table and
//!   the per-car-type parameter block (`DAT_0050e150`, stride 0xcd0, BSS) are
//!   game data loaded at runtime - they are not vendored here.
//!
//! Values are signed Q16.16 ([`crate::fixed_point`]). See evidence
//! `rea.official-eu.sim-grip`.
//!
//! Observations: the formulas and constants above, read from the solver passes.
//! Inferences: the slope ramp reduces grip with tilt; the two grip tiers are
//! high/low surfaces. Unknowns (not reproduced): the friction-circle/slip-angle
//! tyre saturation in `FUN_0040d810`, the exact semi-implicit position update's
//! half-accel term, the suspension damper magnitude source, and the numeric
//! contents of the runtime-loaded per-surface and per-car tables.

use crate::fixed_point::{Fixed16, Vec3};

/// Velocity-integration gain applied to acceleration (`0x3333` ~ 0.2); the
/// fixed 40 ms step is folded into this tuned coefficient.
pub const VELOCITY_GAIN: Fixed16 = Fixed16::from_raw(0x3333);

/// Lateral-grip strength removed from the sideways velocity each step
/// (`0x51e` ~ 0.02).
pub const LATERAL_GRIP_K: Fixed16 = Fixed16::from_raw(0x51e);

/// Lateral-grip strength while the handbrake is held (`0x28f` ~ 0.01, half grip).
pub const LATERAL_GRIP_K_HANDBRAKE: Fixed16 = Fixed16::from_raw(0x28f);

/// Tilt below which no slope-grip loss applies (`0xb333` ~ 0.70).
pub const SLOPE_GRIP_MIN_TILT: Fixed16 = Fixed16::from_raw(0xb333);

/// Slope of the slope-grip loss ramp (`0x42ab5` ~ 4.1667).
pub const SLOPE_GRIP_RAMP: Fixed16 = Fixed16::from_raw(0x42ab5);

/// Maximum slope-grip loss (`0xe666` ~ 0.90).
pub const SLOPE_GRIP_MAX_LOSS: Fixed16 = Fixed16::from_raw(0xe666);

/// Recovered high-grip surface coefficient tier (`0x3d70` ~ 0.24).
pub const SURFACE_GRIP_HIGH: Fixed16 = Fixed16::from_raw(0x3d70);

/// Recovered low-grip surface coefficient tier (`0x2666` ~ 0.15).
pub const SURFACE_GRIP_LOW: Fixed16 = Fixed16::from_raw(0x2666);

/// Stride, in bytes, of a surface grip-table record (the `surfId * 8` index).
pub const SURFACE_GRIP_RECORD_BYTES: usize = 8;

/// Remove the sideways velocity component, reproducing `FUN_0041b270`:
/// `perp = v - fwd * dot(fwd, v); v -= perp * K`. `forward_unit` is the car's
/// forward direction (a unit vector); `handbrake` halves the grip.
pub fn apply_lateral_grip(velocity: Vec3, forward_unit: Vec3, handbrake: bool) -> Vec3 {
    let along = forward_unit.dot(velocity);
    let perpendicular = velocity.sub(forward_unit.scale(along));
    let k = if handbrake {
        LATERAL_GRIP_K_HANDBRAKE
    } else {
        LATERAL_GRIP_K
    };
    velocity.sub(perpendicular.scale(k))
}

/// The slope-grip loss for a tilt value, reproducing `FUN_00408f50`: zero below
/// [`SLOPE_GRIP_MIN_TILT`], then `(tilt - min) * ramp` capped at
/// [`SLOPE_GRIP_MAX_LOSS`].
pub fn slope_grip_loss(tilt: Fixed16) -> Fixed16 {
    if tilt <= SLOPE_GRIP_MIN_TILT {
        return Fixed16::ZERO;
    }
    let loss = tilt.sub(SLOPE_GRIP_MIN_TILT).mul(SLOPE_GRIP_RAMP);
    loss.min(SLOPE_GRIP_MAX_LOSS)
}

/// Net acceleration from the summed force, reproducing the integrator's
/// `accel = force * invMass + gravity`.
pub fn net_accel(force: Vec3, inverse_mass: Fixed16, gravity: Vec3) -> Vec3 {
    force.scale(inverse_mass).add(gravity)
}

/// Quadratic drag `drag = v * speed` (drag coefficient `1.0`), as subtracted
/// from the accumulated force in the integrator.
pub fn drag(velocity: Vec3, speed: Fixed16) -> Vec3 {
    velocity.scale(speed)
}

/// Integrate velocity by one step: `v += accel * 0.2` ([`VELOCITY_GAIN`]).
pub fn integrate_velocity(velocity: Vec3, accel: Vec3) -> Vec3 {
    velocity.add(accel.scale(VELOCITY_GAIN))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: i32, y: i32, z: i32) -> Vec3 {
        Vec3::new(
            Fixed16::from_int(x),
            Fixed16::from_int(y),
            Fixed16::from_int(z),
        )
    }

    #[test]
    fn lateral_grip_leaves_pure_forward_velocity_untouched() {
        let forward = v(1, 0, 0);
        // Velocity entirely along forward: perpendicular is zero, no change.
        assert_eq!(apply_lateral_grip(v(2, 0, 0), forward, false), v(2, 0, 0));
    }

    #[test]
    fn lateral_grip_bleeds_sideways_velocity() {
        let forward = v(1, 0, 0);
        // Sideways unit velocity loses K of itself (0.02 normal).
        let out = apply_lateral_grip(v(0, 1, 0), forward, false);
        assert_eq!(out.x, Fixed16::ZERO);
        assert_eq!(out.y, Fixed16::ONE.sub(Fixed16::ONE.mul(LATERAL_GRIP_K)));
        // Handbrake removes only half as much (0.01), so more sideways speed stays.
        let hb = apply_lateral_grip(v(0, 1, 0), forward, true);
        assert_eq!(
            hb.y,
            Fixed16::ONE.sub(Fixed16::ONE.mul(LATERAL_GRIP_K_HANDBRAKE))
        );
        assert!(hb.y > out.y);
    }

    #[test]
    fn slope_grip_loss_ramps_and_caps() {
        // Below the tilt threshold: no loss.
        assert_eq!(slope_grip_loss(Fixed16::from_raw(0x8000)), Fixed16::ZERO);
        assert_eq!(slope_grip_loss(SLOPE_GRIP_MIN_TILT), Fixed16::ZERO);
        // Well past the threshold: capped at the max loss.
        assert_eq!(slope_grip_loss(Fixed16::ONE), SLOPE_GRIP_MAX_LOSS);
        // Just past the threshold: a small positive loss below the cap.
        let small = slope_grip_loss(Fixed16::from_raw(0xc000));
        assert!(small.raw() > 0 && small.raw() < SLOPE_GRIP_MAX_LOSS.raw());
    }

    #[test]
    fn integrator_core_matches_the_recovered_gains() {
        // accel = force * invMass + gravity.
        let accel = net_accel(v(1, 0, 0), Fixed16::from_raw(0x8000), v(0, -1, 0));
        assert_eq!(
            accel,
            Vec3::new(Fixed16::from_raw(0x8000), -Fixed16::ONE, Fixed16::ZERO)
        );
        // v += accel * 0.2.
        let vel = integrate_velocity(Vec3::ZERO, v(1, 0, 0));
        assert_eq!(vel.x, VELOCITY_GAIN);
        // drag = v * speed.
        let d = drag(v(1, 0, 0), Fixed16::from_raw(0x8000));
        assert_eq!(d.x, Fixed16::from_raw(0x8000));
    }

    #[test]
    fn surface_grip_tiers_match_recovery() {
        assert_eq!(SURFACE_GRIP_HIGH.raw(), 0x3d70);
        assert_eq!(SURFACE_GRIP_LOW.raw(), 0x2666);
        assert!(SURFACE_GRIP_HIGH > SURFACE_GRIP_LOW);
        assert_eq!(SURFACE_GRIP_RECORD_BYTES, 8);
    }
}
