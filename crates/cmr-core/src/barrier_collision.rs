//! The barrier-wall response `FUN_00401fd0` run by the trigger walk
//! `FUN_00403a00` for `.dat` records of type 2 and every unlisted type, with
//! its end-post test `FUN_00401e20`/`FUN_004017a0` and the car's horizontal box
//! `FUN_00401450`.
//!
//! Observations (pseudocode level):
//!
//! - The record's plane test is `FUN_00403470` (event flag set) or the
//!   directional `FUN_004035a0` (flag clear); a flag-clear record then tests
//!   its two end posts (`FUN_00401e20`).
//! - `FUN_004036e0` splits the probes by plane depth. With admitted probes on
//!   both sides, the push depth is the smallest negative depth when the car
//!   faced the plane, else the largest non-negative one. With only one side
//!   admitted, the car is pushed only when all eight probes lie on the far
//!   side (the largest depth when it did not face the plane, the smallest when
//!   it did); otherwise only the end-post result is returned. The car and all
//!   probes move by `normal * depth`.
//! - The into-wall velocity `vn = v . n` (skipped when zero or `i32::MIN`) is
//!   replaced: `v += qmul(n*vn, -0x1999) - n*vn`. The body-frame impact point
//!   `+0x3f0` is the plane normal times two (signed toward the car), clamped to
//!   the half extents `+0x2c..+0x34`.
//! - While in control, an impulse above `0x13333` sends the car out of control
//!   (`+0x680 = +0x688 = 1`). The impulse is scaled by 2.5 and limited to
//!   `0x34ccc`, and its body-frame moment about the hit probe (lowered by 20
//!   and weighted `40, 1, 20` when the car just went out of control, with
//!   `+0xd8 += 0x8000`) is clamped to `+-8.0` per axis and added to `+0x3d8`.
//! - Every contact sets `+0x6dc`.
//! - End posts: within `2 * +0x20` on x and z and closer than `+0x44` on the
//!   ground plane, the car's horizontal box (`FUN_00401450`, built once per
//!   walk from row 0, or row 1 when row 0 is nearly vertical) is pushed back
//!   along its travel from the saved centre `+0x3e4` until the post leaves the
//!   box.
//!
//! Not reconstructed: the damage call `FUN_0041f630` (on the first contact
//! tick), sparks, sounds, camera shake and the scrape records. The pending
//! correction `+0x3cc` is swapped for the damage call and restored, so it is
//! unchanged.

use cmr_formats::stage::TriggerVolume;

use crate::fixed_point::Vec3;
use crate::legacy_math::sqrt_q16;
use crate::stage_triggers::{
    TriggerCarBounds, TriggerGeometryMode, probe_plane_lists, trigger_plane_side,
};
use crate::tumble::{cross, q, raw, vec};
use crate::vehicle_dynamics::transform_contact_force;

/// Impulse above which a barrier hit sends the car out of control.
pub const BARRIER_OUT_OF_CONTROL_IMPULSE: i32 = 0x1_3333;
/// The scaled impulse limit.
pub const BARRIER_IMPULSE_LIMIT: i32 = 0x3_4ccc;
/// The per-axis moment limit.
pub const BARRIER_MOMENT_LIMIT: i32 = 0x8_0000;

/// The car words the barrier response reads and writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BarrierCar {
    /// Centre, saved centre, `+0x44`, probes and their prior snapshot.
    pub bounds: TriggerCarBounds,
    /// `+0x20`: the end-post reach is twice this word.
    pub reach_20: i32,
    /// `+0xd4..+0xdc`.
    pub velocity: [i32; 3],
    /// The published matrix rows `+0x2a0`.
    pub basis: [Vec3; 3],
    /// `+0x2c..+0x34`.
    pub half_words: [i32; 3],
    /// `+0x3f0..+0x3f8`: the last body-frame impact point.
    pub impact_3f0: [i32; 3],
    /// `+0x3d8..+0x3e0`.
    pub moment_3d8: Vec3,
    pub out_of_control_680: bool,
    pub recovery_688: bool,
    /// `+0x6dc`: a barrier touched the car this walk.
    pub contact_6dc: bool,
}

/// `FUN_00401450`'s horizontal box: unit axis `(x, z)`, its perpendicular
/// `(z, -x)`, and the largest positive probe extent along each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CarBox {
    pub axis: [i32; 2],
    pub half_along: i32,
    pub half_across: i32,
}

/// Per-walk scratch: the lazily built box (`DAT_0050c51c`) and the selected
/// probe `DAT_0050c574`, which keeps its value across calls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BarrierScratch {
    pub car_box: Option<CarBox>,
    pub selected_probe_574: u8,
}

fn reciprocal(root: i32) -> i32 {
    (0x1_0000_0000_i64 / i64::from(root)) as i32
}

fn normalize_xz(x: i32, z: i32) -> [i32; 2] {
    let root = sqrt_q16(q(x, x).wrapping_add(q(z, z)) as u32) as i32;
    if root == 0 {
        return [0, 0];
    }
    let inverse = reciprocal(root);
    [q(x, inverse), q(z, inverse)]
}

/// `FUN_00401450`.
#[must_use]
pub fn build_car_box(car: &BarrierCar) -> CarBox {
    let [row0, row1, _] = car.basis.map(raw);
    let axis = if row0[1].wrapping_abs() < 0xfd71 {
        normalize_xz(row0[0], row0[2])
    } else {
        normalize_xz(row1[0], row1[2])
    };
    let mut half_along = 0;
    let mut half_across = 0;
    let center = car.bounds.current_position;
    for probe in car.bounds.current_points {
        let dx = probe[0].wrapping_sub(center[0]);
        let dz = probe[2].wrapping_sub(center[2]);
        let along = q(axis[0], dx).wrapping_add(q(axis[1], dz));
        let across = q(axis[1], dx).wrapping_add(q(axis[0].wrapping_neg(), dz));
        if along > 0 && half_along < along {
            half_along = along;
        }
        if across > 0 && half_across < across {
            half_across = across;
        }
    }
    CarBox {
        axis,
        half_along,
        half_across,
    }
}

fn move_car(car: &mut BarrierCar, delta: [i32; 3]) {
    let bounds = &mut car.bounds;
    for axis in 0..3 {
        bounds.current_position[axis] = bounds.current_position[axis].wrapping_add(delta[axis]);
        for probe in &mut bounds.current_points {
            probe[axis] = probe[axis].wrapping_add(delta[axis]);
        }
    }
}

fn box_exit_time(half: i32, offset: i32, rate: i32) -> Option<i32> {
    if rate >= 1 {
        let room = half.wrapping_sub(offset);
        Some(((i64::from(room) << 16) / i64::from(rate)) as i32)
    } else if rate < 0 {
        let room = offset.wrapping_add(half).wrapping_abs();
        let speed = rate.wrapping_neg();
        Some(if speed == 0 {
            -1
        } else {
            ((i64::from(room) << 16) / i64::from(speed)) as i32
        })
    } else {
        None
    }
}

/// `FUN_004017a0`: push the car's box off one end post.
fn push_off_post(point: [i32; 3], car: &mut BarrierCar, scratch: &mut BarrierScratch) -> bool {
    let car_box = *scratch.car_box.get_or_insert_with(|| build_car_box(car));
    let [ux, uz] = car_box.axis;
    let (vx, vz) = (uz, ux.wrapping_neg());
    let center = car.bounds.current_position;
    let dx = point[0].wrapping_sub(center[0]);
    let dz = point[2].wrapping_sub(center[2]);
    let along = q(ux, dx).wrapping_add(q(uz, dz));
    let across = q(vx, dx).wrapping_add(q(vz, dz));
    if along.wrapping_abs() > car_box.half_along || across.wrapping_abs() > car_box.half_across {
        return false;
    }
    let saved = car.bounds.saved_position;
    let sx = point[0].wrapping_sub(saved[0]);
    let sz = point[2].wrapping_sub(saved[2]);
    let length = sqrt_q16(q(sx, sx).wrapping_add(q(sz, sz)) as u32) as i32;
    if length < 1 {
        return false;
    }
    let inverse = reciprocal(length);
    let (tx, tz) = (q(sx, inverse), q(sz, inverse));
    let rate_along = q(uz, tz).wrapping_add(q(ux, tx));
    let rate_across = q(vz, tz).wrapping_add(q(vx, tx));
    let mut time = 0x7d00_0000;
    if let Some(t) = box_exit_time(car_box.half_along, along, rate_along)
        && t < 0x7d00_0000
    {
        time = t;
    }
    if let Some(t) = box_exit_time(car_box.half_across, across, rate_across)
        && t < time
    {
        time = t;
    }
    let push = [q(q(tx, -0x1_0000), time), 0, q(q(tz, -0x1_0000), time)];
    move_car(car, push);
    true
}

/// `FUN_00401e20`: the record's two end posts.
pub fn barrier_end_posts(
    volume: &TriggerVolume,
    car: &mut BarrierCar,
    scratch: &mut BarrierScratch,
) -> bool {
    let reach = car.reach_20.wrapping_mul(2);
    let radius = car.bounds.radius_q16;
    let radius_squared = q(radius, radius);
    let mut hit = [false; 2];
    for (index, point) in [volume.point_a, volume.point_b].into_iter().enumerate() {
        let center = car.bounds.current_position;
        let dx = point[0].wrapping_sub(center[0]);
        let dz = point[2].wrapping_sub(center[2]);
        if dx.wrapping_abs() <= reach
            && reach >= 0
            && dz.wrapping_abs() <= reach
            && q(dz, dz).wrapping_add(q(dx, dx)) < radius_squared
        {
            hit[index] = push_off_post(point, car, scratch);
        }
    }
    if hit[0] || hit[1] {
        car.contact_6dc = true;
        return true;
    }
    false
}

fn select_push_depth(
    lists: &crate::stage_triggers::ProbePlaneLists,
    facing: bool,
    scratch: &mut BarrierScratch,
) -> Option<i32> {
    const ALL: [u8; 8] = [0, 1, 2, 3, 4, 5, 6, 7];
    let pick = |indices: &[u8], smallest: bool| {
        let mut best = if smallest { 0x2710_0000 } else { -0x2710_0000 };
        let mut chosen = None;
        for &index in indices {
            let depth = lists.depths[usize::from(index)];
            if (smallest && depth < best) || (!smallest && best < depth) {
                best = depth;
                chosen = Some(index);
            }
        }
        (best, chosen)
    };
    let (best, chosen) = if lists.negative_count != 0 && lists.nonnegative_count != 0 {
        if facing {
            pick(&lists.negative[..usize::from(lists.negative_count)], true)
        } else {
            pick(
                &lists.nonnegative[..usize::from(lists.nonnegative_count)],
                false,
            )
        }
    } else if lists.nonnegative_total == 8 && !facing {
        pick(&ALL, false)
    } else if lists.negative_total == 8 && facing {
        pick(&ALL, true)
    } else {
        return None;
    };
    if let Some(index) = chosen {
        scratch.selected_probe_574 = index;
    }
    Some(best)
}

fn clamp_moment(value: i32) -> i32 {
    if value.wrapping_abs() > BARRIER_MOMENT_LIMIT {
        if value < 1 {
            -BARRIER_MOMENT_LIMIT
        } else {
            BARRIER_MOMENT_LIMIT
        }
    } else {
        value
    }
}

/// `FUN_00401fd0`: one barrier record against the car. Returns whether the
/// barrier touched the car.
pub fn barrier_response(
    volume: &TriggerVolume,
    car: &mut BarrierCar,
    scratch: &mut BarrierScratch,
) -> bool {
    let flagged = volume.event_flag != 0;
    let mode = if flagged {
        TriggerGeometryMode::Bidirectional
    } else {
        TriggerGeometryMode::RequireSavedNonPositive
    };
    let Some(facing) = trigger_plane_side(volume, &car.bounds, mode) else {
        return false;
    };
    let posts = if flagged {
        false
    } else {
        barrier_end_posts(volume, car, scratch)
    };
    let lists = probe_plane_lists(volume, &car.bounds, facing);
    if lists.negative_count == 0 && lists.nonnegative_count == 0 {
        return posts;
    }
    let Some(depth) = select_push_depth(&lists, facing, scratch) else {
        return posts;
    };
    let normal = volume.normal_q16;
    if depth != 0 {
        move_car(car, normal.map(|n| q(n, depth)));
    }
    let velocity = car.velocity;
    let normal_speed = q(normal[2], velocity[2])
        .wrapping_add(q(normal[0], velocity[0]))
        .wrapping_add(q(normal[1], velocity[1]));
    if normal_speed == 0 || normal_speed == i32::MIN {
        car.contact_6dc = true;
        return true;
    }
    let impulse = normal.map(|n| q(n, normal_speed));
    let change = impulse.map(|part| q(part, -0x1999).wrapping_sub(part));
    for (speed, delta) in car.velocity.iter_mut().zip(change) {
        *speed = speed.wrapping_add(delta);
    }
    let center = car.bounds.current_position;
    let side = (0..3).fold(0_i32, |sum, axis| {
        sum.wrapping_add(q(
            normal[axis],
            volume.point_a[axis].wrapping_sub(center[axis]),
        ))
    });
    let toward = if side < 1 { -0x2_0000 } else { 0x2_0000 };
    let local = raw(transform_contact_force(
        car.basis,
        vec(normal.map(|n| q(n, toward))),
    ));
    car.impact_3f0 = std::array::from_fn(|axis| {
        let half = car.half_words[axis];
        if half < local[axis].wrapping_abs() {
            if local[axis] < 0 {
                half.wrapping_neg()
            } else {
                half
            }
        } else {
            local[axis]
        }
    });
    if !car.out_of_control_680 {
        let magnitude = sqrt_q16(
            q(change[0], change[0])
                .wrapping_add(q(change[1], change[1]))
                .wrapping_add(q(change[2], change[2])) as u32,
        ) as i32;
        if magnitude > BARRIER_OUT_OF_CONTROL_IMPULSE {
            car.out_of_control_680 = true;
            car.recovery_688 = true;
        }
        let mut scaled = change.map(|part| q(part, 0x2_8000));
        let scaled_magnitude = q(magnitude, 0x2_8000);
        if scaled_magnitude > BARRIER_IMPULSE_LIMIT {
            let inverse = reciprocal(scaled_magnitude);
            scaled = scaled.map(|part| q(q(part, inverse), BARRIER_IMPULSE_LIMIT));
        }
        let body_impulse = transform_contact_force(car.basis, vec(scaled));
        let probe = car.bounds.current_points[usize::from(scratch.selected_probe_574)];
        let mut arm: [i32; 3] = std::array::from_fn(|axis| probe[axis].wrapping_sub(center[axis]));
        let moment = if !car.out_of_control_680 {
            raw(cross(
                body_impulse,
                transform_contact_force(car.basis, vec(arm)),
            ))
        } else {
            arm[1] = arm[1].wrapping_sub(0x14_0000);
            let turn = raw(cross(
                body_impulse,
                transform_contact_force(car.basis, vec(arm)),
            ));
            car.velocity[1] = car.velocity[1].wrapping_add(0x8000);
            [
                q(turn[0], 0x28_0000),
                q(turn[1], 0x1_0000),
                q(turn[2], 0x14_0000),
            ]
        };
        car.moment_3d8 = car.moment_3d8.add(vec(moment.map(clamp_moment)));
    }
    car.contact_6dc = true;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: i32 = 0x1_0000;

    /// A wall in the plane `x = 0` from `z = -50` to `z = 50`, facing `+x`,
    /// with its y span covering the probes.
    fn wall(event_flag: u8) -> TriggerVolume {
        TriggerVolume {
            point_a: [0, -10 * ONE, -50 * ONE],
            point_b: [0, 10 * ONE, 50 * ONE],
            normal_q16: [ONE, 0, 0],
            payload: 0,
            spatial_node_index: 0,
            raw_type: 2,
            event_flag,
            raw_state: 0,
            unknown_2f: 0,
        }
    }

    fn identity() -> [Vec3; 3] {
        [vec([ONE, 0, 0]), vec([0, ONE, 0]), vec([0, 0, ONE])]
    }

    /// A 2 x 1 x 4 box of probes around `center`.
    fn probes(center: [i32; 3]) -> [[i32; 3]; 8] {
        let mut points = [[0; 3]; 8];
        for (index, point) in points.iter_mut().enumerate() {
            let sx = if index & 1 == 0 { ONE } else { -ONE };
            let sy = if index & 2 == 0 { -ONE / 2 } else { ONE / 2 };
            let sz = if index & 4 == 0 { 2 * ONE } else { -2 * ONE };
            *point = [center[0] + sx, center[1] + sy, center[2] + sz];
        }
        points
    }

    fn car(center: [i32; 3], saved: [i32; 3], velocity: [i32; 3]) -> BarrierCar {
        BarrierCar {
            bounds: TriggerCarBounds {
                current_position: center,
                saved_position: saved,
                radius_q16: 3 * ONE,
                current_points: probes(center),
                prior_points: probes(saved),
            },
            reach_20: 2 * ONE,
            velocity,
            basis: identity(),
            half_words: [ONE, ONE / 2, 2 * ONE],
            impact_3f0: [0; 3],
            moment_3d8: Vec3::ZERO,
            out_of_control_680: false,
            recovery_688: false,
            contact_6dc: false,
        }
    }

    #[test]
    fn a_car_crossing_the_wall_is_pushed_back_and_bounced() {
        // Moving -x through the wall at x = 0 from the +x side (facing).
        let mut hit = car([ONE / 2, 0, 0], [3 * ONE, 0, 0], [-ONE / 2, 0, 0]);
        let mut scratch = BarrierScratch::default();
        assert!(barrier_response(&wall(1), &mut hit, &mut scratch));
        assert!(hit.contact_6dc);
        // Probes at x = -0.5 cross; the smallest negative depth is
        // `a - p = 0 - 1.5 = -1.5`... the push moves the car by the selected
        // depth along +x so no probe stays behind the wall.
        let min_x = hit
            .bounds
            .current_points
            .iter()
            .map(|p| p[0])
            .min()
            .unwrap();
        assert!(min_x >= 0, "{min_x:#x}");
        // vn = -0.5 becomes +0.55.
        assert_eq!(
            hit.velocity[0],
            -ONE / 2 + q(q(ONE, -ONE / 2), -0x1999) + ONE / 2
        );
        assert!(hit.velocity[0] > 0);
        assert!(!hit.out_of_control_680);
        assert_ne!(hit.moment_3d8, Vec3::ZERO);
        // The impact point faces the wall (-x) and is clamped to the half width.
        assert_eq!(hit.impact_3f0, [-ONE, 0, 0]);
    }

    #[test]
    fn a_hard_hit_sends_the_car_out_of_control_and_lifts_it() {
        // vn = -2.0 gives an impulse of 2.2, above 0x13333 (1.2).
        let mut hit = car([ONE / 2, 0, 0], [3 * ONE, 0, 0], [-2 * ONE, 0, 0]);
        let mut scratch = BarrierScratch::default();
        assert!(barrier_response(&wall(1), &mut hit, &mut scratch));
        assert!(hit.out_of_control_680 && hit.recovery_688);
        assert_eq!(hit.velocity[1], 0x8000);
        for axis in [hit.moment_3d8.x, hit.moment_3d8.y, hit.moment_3d8.z] {
            assert!(axis.raw().wrapping_abs() <= BARRIER_MOMENT_LIMIT);
        }
    }

    #[test]
    fn an_out_of_control_car_is_pushed_without_a_moment() {
        let mut hit = car([ONE / 2, 0, 0], [3 * ONE, 0, 0], [-ONE / 2, 0, 0]);
        hit.out_of_control_680 = true;
        let mut scratch = BarrierScratch::default();
        assert!(barrier_response(&wall(1), &mut hit, &mut scratch));
        assert_eq!(hit.moment_3d8, Vec3::ZERO);
        assert!(hit.velocity[0] > 0);
    }

    #[test]
    fn a_car_far_from_the_plane_or_behind_a_one_way_wall_is_untouched() {
        let far = car([10 * ONE, 0, 0], [12 * ONE, 0, 0], [-ONE, 0, 0]);
        let mut moved = far;
        let mut scratch = BarrierScratch::default();
        assert!(!barrier_response(&wall(1), &mut moved, &mut scratch));
        assert_eq!(moved, far);
        // Directional (flag clear): a saved centre on the plane's positive
        // side (`(a - p) . n > 0`, here x < 0) rejects.
        let behind = car([-ONE / 2, 0, 0], [-3 * ONE, 0, 0], [ONE, 0, 0]);
        let mut moved = behind;
        assert!(!barrier_response(&wall(0), &mut moved, &mut scratch));
        assert_eq!(moved, behind);
    }

    #[test]
    fn a_zero_normal_speed_still_counts_as_contact() {
        let mut hit = car([ONE / 2, 0, 0], [3 * ONE, 0, 0], [0, 0, ONE]);
        let mut scratch = BarrierScratch::default();
        assert!(barrier_response(&wall(1), &mut hit, &mut scratch));
        assert!(hit.contact_6dc);
        assert_eq!(hit.velocity, [0, 0, ONE]);
        assert_eq!(hit.moment_3d8, Vec3::ZERO);
    }

    #[test]
    fn the_car_box_uses_row_zero_unless_it_is_nearly_vertical() {
        let upright = car([0, 0, 0], [0, 0, 0], [0; 3]);
        let car_box = build_car_box(&upright);
        // The legacy normalization lands at 65522.
        assert_eq!(car_box.axis, [65522, 0]);
        assert_eq!(car_box.half_along, q(65522, ONE));
        assert_eq!(car_box.half_across, q(65522, 2 * ONE));
        let mut nose_up = upright;
        nose_up.basis = [vec([0, ONE, 0]), vec([0, 0, ONE]), vec([ONE, 0, 0])];
        assert_eq!(build_car_box(&nose_up).axis, [0, 65522]);
    }

    #[test]
    fn an_end_post_inside_the_box_pushes_the_car_back_along_its_travel() {
        // The car moved +z from z = -3 to z = 0; a post at (0.5, 0, 1.5) is
        // inside the 1 x 2 half box.
        let mut hit = car([0, 0, 0], [0, 0, -3 * ONE], [0; 3]);
        let post = TriggerVolume {
            point_a: [ONE / 2, 0, 3 * ONE / 2],
            point_b: [40 * ONE, 0, 40 * ONE],
            ..wall(0)
        };
        let mut scratch = BarrierScratch::default();
        assert!(barrier_end_posts(&post, &mut hit, &mut scratch));
        assert!(hit.contact_6dc);
        // Pushed back against the travel from the saved centre toward the post
        // (+x and +z), so both coordinates decrease.
        assert!(hit.bounds.current_position[0] < 0);
        assert!(hit.bounds.current_position[2] < 0);
        assert_eq!(hit.bounds.current_position[1], 0);
        assert!(scratch.car_box.is_some());
        // A post outside the reach is ignored.
        let mut clear = car([0, 0, 0], [0, 0, -3 * ONE], [0; 3]);
        let far_post = TriggerVolume {
            point_a: [20 * ONE, 0, 0],
            point_b: [30 * ONE, 0, 0],
            ..wall(0)
        };
        assert!(!barrier_end_posts(
            &far_post,
            &mut clear,
            &mut BarrierScratch::default()
        ));
    }
}
