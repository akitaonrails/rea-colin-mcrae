//! The out-of-control (tumbling) helpers of the per-car ground pass.
//!
//! Observations (pseudocode level unless noted):
//!
//! - `FUN_004102e0`, called by the mechanics driver every tick while the
//!   righting animation `ctl +0x3ec` is idle, arms it: a set roof flag
//!   `+0x3f4` starts it with the 20-tick counter `+0x43c`, otherwise a set
//!   side flag `+0x3f8` with `+0x43d`; a set end flag `+0x3fc` is just
//!   cleared with its counter `+0x43e`.
//! - `FUN_00412860`, the tumbling contact response, damps the angular
//!   velocity about an axis whenever the ground normal crosses that body
//!   row's plane, and counts the ticks the car spends on its end
//!   (`|n . row0| > 0xe666`), roof (`n . row1 < -0xf333`) or side
//!   (`|n . row2| > 0xf333`); more than ten in a row raises the matching flag.
//!   After a crossing it stores the first contact vector speed below
//!   `-0x3333` along `n` in `+0x3cc` (with `+0x3d0/+0x3d4` zeroed) and
//!   starts the impact effects.
//! - `FUN_00413a80` eases the up and side rows back toward the start-of-tick
//!   basis `+0x27c` (with the strong gains when its argument is nonzero) and
//!   re-orthonormalizes the basis.
//!
//! Reconstruction policies: the impact effects (damage `FUN_0041f630`, debris
//! and sound) and the published render matrix are outside this module.

use crate::compiled_car_initializer::normalize_compiled_lane_vector;
use crate::fixed_point::{Fixed16, Vec3};

/// The tumbling words of the car and control records.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TumbleState {
    /// `ctl +0x3ec`: the righting animation is running.
    pub righting_3ec: bool,
    /// `ctl +0x3f4/+0x3f8/+0x3fc`: on the roof, side or end for long enough.
    pub roof_3f4: bool,
    pub side_3f8: bool,
    pub end_3fc: bool,
    /// `ctl +0x43c/+0x43d/+0x43e`: the matching tick counters.
    pub roof_ticks_43c: i8,
    pub side_ticks_43d: i8,
    pub end_ticks_43e: i8,
    /// `car +0x678`: ticks a hull probe has touched while out of control.
    pub hull_ticks_678: i32,
    /// `car +0x684`: the up row pointed down at some tumbling tick.
    pub inverted_684: bool,
    /// `car +0x64c/+0x650`: the lowest wheel's clearance and index (`-1`).
    pub lowest_wheel_64c: i32,
    pub lowest_wheel_650: i8,
}

/// `FUN_004102e0`.
pub fn arm_righting(state: &mut TumbleState) {
    if state.roof_3f4 {
        state.righting_3ec = true;
        state.roof_ticks_43c = 20;
    } else if state.side_3f8 {
        state.righting_3ec = true;
        state.side_ticks_43d = 20;
    } else if state.end_3fc {
        state.end_3fc = false;
        state.end_ticks_43e = 0;
    }
}

pub(crate) fn q(left: i32, right: i32) -> i32 {
    Fixed16::from_raw(left).mul(Fixed16::from_raw(right)).raw()
}

pub(crate) fn raw(vector: Vec3) -> [i32; 3] {
    [vector.x.raw(), vector.y.raw(), vector.z.raw()]
}

pub(crate) fn vec(words: [i32; 3]) -> Vec3 {
    Vec3::new(
        Fixed16::from_raw(words[0]),
        Fixed16::from_raw(words[1]),
        Fixed16::from_raw(words[2]),
    )
}

pub(crate) fn normalize(vector: Vec3) -> Vec3 {
    vec(normalize_compiled_lane_vector(raw(vector)))
}

/// `a x b` with the native product order.
pub(crate) fn cross(a: Vec3, b: Vec3) -> Vec3 {
    let [ax, ay, az] = raw(a);
    let [bx, by, bz] = raw(b);
    vec([
        q(ay, bz).wrapping_sub(q(az, by)),
        q(az, bx).wrapping_sub(q(ax, bz)),
        q(ax, by).wrapping_sub(q(ay, bx)),
    ])
}

fn length(vector: Vec3) -> i32 {
    crate::legacy_math::sqrt_q16(vector.dot(vector).raw() as u32) as i32
}

fn reject(vector: Vec3, axis: Vec3, along: i32) -> Vec3 {
    vector.sub(axis.scale(Fixed16::from_raw(along)))
}

fn sign_changed(previous: i32, current: i32) -> bool {
    (previous > 0 && current < 0) || (previous < 0 && current > 0)
}

/// Inputs to one `FUN_00412860` call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TumbleContactInput {
    pub angular_velocity: Vec3,
    pub velocity: Vec3,
    pub normal: Vec3,
    pub basis: [Vec3; 3],
    pub speed_ec: i32,
    /// `+0xbc/+0xc0/+0xc4` from the previous call.
    pub axis_snapshot_bc: Vec3,
    /// This tick's contact vectors `+0x4bc..+0x518`.
    pub contact_vectors: [Vec3; 8],
}

/// What `FUN_00412860` writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TumbleContactEffects {
    pub angular_velocity: Vec3,
    pub axis_snapshot_bc: Vec3,
    /// The new `+0x3cc..+0x3d4` when an impact speed was stored.
    pub impact_3cc: Option<Vec3>,
}

/// `FUN_00412860`.
#[must_use]
pub fn tumble_contact_response(
    state: &mut TumbleState,
    input: &TumbleContactInput,
) -> TumbleContactEffects {
    let n = input.normal;
    let mut omega = input.angular_velocity;
    let spin = q(length(omega), 0x5_0005).min(0xcccc);
    let mut fast = q(0x1_0000_i32.wrapping_sub(spin), 0x9999);
    let mut slow = q(0x1_0000_i32.wrapping_sub(spin), 0x6666);
    let normal_speed = input.velocity.dot(n).raw().wrapping_abs();
    if input.speed_ec.wrapping_sub(normal_speed) < 0x8000 {
        fast = 0xcccc;
        slow = 0x9999;
    }
    let [row0, row1, row2] = input.basis;
    let previous = raw(input.axis_snapshot_bc);

    let d0 = n.dot(row0).raw();
    let plane0 = normalize(reject(row0, n, d0));
    let crossed0 = sign_changed(previous[0], d0);
    if !state.end_3fc {
        if !(-0xe666..=0xe666).contains(&d0) {
            state.end_ticks_43e = state.end_ticks_43e.wrapping_add(1);
        } else {
            state.end_ticks_43e = 0;
        }
        if state.end_ticks_43e > 10 {
            state.end_3fc = true;
        }
    }

    let d1 = n.dot(row1).raw();
    let plane1 = normalize(reject(row1, n, d1));
    let crossed1 = sign_changed(previous[1], d1);
    if !state.roof_3f4 {
        if d1 < -0xf333 {
            state.roof_ticks_43c = state.roof_ticks_43c.wrapping_add(1);
        } else {
            state.roof_ticks_43c = 0;
        }
        if state.roof_ticks_43c > 10 {
            state.roof_3f4 = true;
        }
    }

    let d2 = n.dot(row2).raw();
    let plane2 = normalize(reject(row2, n, d2));
    let crossed2 = sign_changed(previous[2], d2);
    if !state.side_3f8 {
        if !(-0xf333..=0xf333).contains(&d2) {
            state.side_ticks_43d = state.side_ticks_43d.wrapping_add(1);
        } else {
            state.side_ticks_43d = 0;
        }
        if state.side_ticks_43d > 10 {
            state.side_3f8 = true;
        }
    }
    let snapshot = vec([d0, d1, d2]);
    // Rolls and pitches damp only x and z; the up-row crossing damps all three.
    let damp_xz = |omega: &mut Vec3, plane: Vec3| {
        let axis = normalize(cross(plane, n));
        let along = omega.dot(axis).raw();
        let gain = if d1 < 0xfc29 { slow } else { fast };
        let scale = q(along, gain);
        omega.x = Fixed16::from_raw(omega.x.raw().wrapping_sub(q(axis.x.raw(), scale)));
        omega.z = Fixed16::from_raw(omega.z.raw().wrapping_sub(q(axis.z.raw(), scale)));
    };
    if crossed0 {
        damp_xz(&mut omega, plane0);
    }
    if crossed1 {
        let axis = normalize(cross(plane1, n));
        let scale = q(omega.dot(axis).raw(), slow);
        omega = omega.sub(axis.scale(Fixed16::from_raw(scale)));
    }
    if crossed2 {
        damp_xz(&mut omega, plane2);
    }
    let impact_3cc = if crossed0 || crossed1 || crossed2 {
        input
            .contact_vectors
            .iter()
            .map(|vector| n.dot(*vector).raw())
            .find(|&speed| speed < -0x3333)
            .map(|speed| vec([speed, 0, 0]))
    } else {
        None
    };
    TumbleContactEffects {
        angular_velocity: omega,
        axis_snapshot_bc: snapshot,
        impact_3cc,
    }
}

/// `FUN_00413a80`: ease the up and side rows toward the start-of-tick basis
/// and re-orthonormalize. `strong` selects the `(0x8000, 0x8000)` gains of
/// the tumbling path over the `(0x1999, 0x4ccc)` of argument zero.
#[must_use]
pub fn ease_basis_toward_start(
    basis: [Vec3; 3],
    start_basis: [Vec3; 3],
    axis_snapshot_bc: Vec3,
    time_step: Fixed16,
    strong: bool,
) -> [Vec3; 3] {
    let (base, gain) = if strong {
        (0x8000, 0x8000)
    } else {
        (0x1999, 0x4ccc)
    };
    let rate = q(time_step.raw(), 0xc_0030);
    let [bc, c0, c4] = raw(axis_snapshot_bc);
    let [_, mut row1, mut row2] = basis;
    if c0.wrapping_abs() > 0xfae1 {
        let delta = row1.sub(start_basis[1]);
        let distance = length(delta);
        if distance > 0 {
            let progress = q(distance, rate).min(0x1_0000);
            let factor = q(progress, gain).wrapping_add(base);
            row1 = normalize(start_basis[1].add(delta.scale(Fixed16::from_raw(factor))));
        }
    }
    if c4.wrapping_abs() > 0x3333 && bc.wrapping_abs() < 0x1999 {
        let delta = row2.sub(start_basis[2]);
        let delta = reject(delta, row1, delta.dot(row1).raw());
        let distance = length(delta);
        if distance > 0 {
            let progress = q(distance, rate).min(0x1_0000);
            let factor = q(progress, gain).wrapping_sub(0x1_0000).wrapping_add(base);
            row2 = normalize(row2.add(delta.scale(Fixed16::from_raw(factor))));
        }
    }
    row2 = normalize(reject(row2, row1, row2.dot(row1).raw()));
    let row0 = normalize(cross(row1, row2));
    [row0, row1, row2]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: i32, y: i32, z: i32) -> Vec3 {
        vec([x, y, z])
    }

    fn identity() -> [Vec3; 3] {
        [v(0x1_0000, 0, 0), v(0, 0x1_0000, 0), v(0, 0, 0x1_0000)]
    }

    #[test]
    fn arming_prefers_roof_then_side_and_clears_end() {
        let mut roof = TumbleState {
            roof_3f4: true,
            side_3f8: true,
            ..TumbleState::default()
        };
        arm_righting(&mut roof);
        assert!(roof.righting_3ec);
        assert_eq!((roof.roof_ticks_43c, roof.side_ticks_43d), (20, 0));
        let mut side = TumbleState {
            side_3f8: true,
            ..TumbleState::default()
        };
        arm_righting(&mut side);
        assert!(side.righting_3ec);
        assert_eq!(side.side_ticks_43d, 20);
        let mut end = TumbleState {
            end_3fc: true,
            end_ticks_43e: 7,
            ..TumbleState::default()
        };
        arm_righting(&mut end);
        assert!(!end.righting_3ec && !end.end_3fc);
        assert_eq!(end.end_ticks_43e, 0);
        let mut idle = TumbleState::default();
        arm_righting(&mut idle);
        assert_eq!(idle, TumbleState::default());
    }

    fn contact_input(basis: [Vec3; 3], previous: Vec3) -> TumbleContactInput {
        TumbleContactInput {
            angular_velocity: v(0x100, 0x200, 0x300),
            velocity: Vec3::ZERO,
            normal: v(0, 0x1_0000, 0),
            basis,
            speed_ec: 0x1_0000,
            axis_snapshot_bc: previous,
            contact_vectors: [v(0, -0x8000, 0); 8],
        }
    }

    #[test]
    fn roof_ticks_raise_the_roof_flag_after_ten() {
        let upside_down = [v(0x1_0000, 0, 0), v(0, -0x1_0000, 0), v(0, 0, -0x1_0000)];
        let mut state = TumbleState::default();
        let mut snapshot = v(0, -0x1_0000, 0);
        for tick in 1..=11 {
            let effects =
                tumble_contact_response(&mut state, &contact_input(upside_down, snapshot));
            snapshot = effects.axis_snapshot_bc;
            assert_eq!(state.roof_3f4, tick == 11, "tick {tick}");
        }
        assert_eq!(snapshot, v(0, -0x1_0000, 0));
        assert!(!state.side_3f8 && !state.end_3fc);
    }

    #[test]
    fn a_crossing_damps_the_spin_and_stores_the_impact_speed() {
        let mut state = TumbleState::default();
        // The previous snapshot had the side row on the other side of the plane.
        let tilted = [
            v(0x1_0000, 0, 0),
            v(0, 0xf000, 0x6000),
            v(0, -0x6000, 0xf000),
        ];
        let calm = tumble_contact_response(&mut state, &contact_input(tilted, v(0, 0xf000, -0x10)));
        assert_eq!(calm.impact_3cc, None);
        assert_eq!(calm.angular_velocity, v(0x100, 0x200, 0x300));
        let crossed =
            tumble_contact_response(&mut state, &contact_input(tilted, v(0, 0xf000, 0x10)));
        assert_eq!(crossed.impact_3cc, Some(v(-0x8000, 0, 0)));
        assert_ne!(crossed.angular_velocity, v(0x100, 0x200, 0x300));
        // The roll/pitch damping leaves the y component alone.
        assert_eq!(crossed.angular_velocity.y.raw(), 0x200);
    }

    #[test]
    fn easing_pulls_the_up_row_back_and_keeps_an_orthonormal_basis() {
        let start = identity();
        let tipped = [
            v(0x1_0000, 0, 0),
            normalize(v(0, 0xff00, 0x1000)),
            normalize(v(0, -0x1000, 0xff00)),
        ];
        let snapshot = v(0, 0xff00, 0x10);
        let eased = ease_basis_toward_start(tipped, start, snapshot, Fixed16::ONE, true);
        assert!(eased[1].z.raw() < tipped[1].z.raw());
        assert!(eased[0].dot(eased[1]).raw().abs() < 0x10);
        assert!(eased[1].dot(eased[2]).raw().abs() < 0x10);
        let unchanged =
            ease_basis_toward_start(start, start, v(0, 0x1_0000, 0), Fixed16::ONE, false);
        for (row, expected) in unchanged.iter().zip(start) {
            assert!((row.x.raw() - expected.x.raw()).abs() < 0x20);
            assert!((row.y.raw() - expected.y.raw()).abs() < 0x20);
            assert!((row.z.raw() - expected.z.raw()).abs() < 0x20);
        }
    }
}
