//! The per-car ground pass `FUN_00410a60`, run by the in-race mechanics
//! routine `FUN_0043afd0` after the mechanics driver has integrated the car.
//!
//! Observations (pseudocode level unless noted, `FUN_00410a60`,
//! `FUN_00419c20`, `FUN_00408fe0`, `FUN_00412750`, `FUN_004126b0`):
//!
//! - It runs only while the car is in control (`+0x680 == 0`); otherwise the
//!   tumbling path `FUN_004115d0` runs instead (not reconstructed here).
//! - If the previous scan found every probe clear of the ground (`+0x743 ==
//!   1`), it requeries the ground (the four-lane `FUN_00419c20`) and rescans.
//! - While some probe touches (`+0x743 == 0`), it queries (unless it just did),
//!   derives the ground normal `FUN_00408fe0`, and computes `vn = v . n`. When
//!   `qmul(1 - min(qmul(+0xec, 0x15555), 1), 0x1999) < vn` the car is lifting
//!   off fast and the previous normal is restored. Otherwise the normal is
//!   kept and the tilt `t = n . row1` decides: `t < 0xb334` sends the car out
//!   of control (`+0x680 = +0x688 = 1`); a moving or noticeably tilted car
//!   (`+0x700 == 0 || t < 0xfd70`) re-orthonormalizes its basis onto `n`; a
//!   resting, nearly aligned car restores its start-of-tick basis (`+0x27c`).
//!   It then snapshots `n . rows` into `+0xbc..+0xc4`, rebuilds the probe
//!   points from the new basis (`FUN_00409c10`) and requeries (the eight-lane
//!   `FUN_00419a40` when it just went out of control).
//! - `FUN_00412750` lifts the car and every probe by the largest positive
//!   `lane_word - probe_y` among probes whose clear flag `+0x73a + i` is zero.
//! - `FUN_004126b0` rescans the probes (`+0x73a..`, `+0x742`, `+0x743`).
//! - If some probe touches after that scan, an into-ground velocity
//!   (`vn < 0`) loses its normal component (`v -= n * vn`) and the pitch and
//!   roll rates `+0xf0/+0xf8` are scaled by `qmul(DAT_004eac6c, 0x1eb8)`.
//!
//! Reconstruction policies: the native damping block can read `vn` before the
//! ground branch ever assigned it (car clear at entry, touching only after the
//! lift); this module uses `vn = 0` there. Suspension travel and wheel
//! animation (`FUN_00410350`) are not part of this pass.

use crate::compiled_car_initializer::{
    CompiledEightLaneExecutionFailure, CompiledEightLaneExecutionInputs,
    CompiledEightLaneExecutionState, CompiledLaneConfigProvider, CompiledLaneGeometryProvider,
    CompiledLaneThresholdEffects, CompiledLaneThresholdInputs, CompiledPostLaneBasisInputs,
    CompiledPostLaneNormalInput, CompiledSetupPoseLaneInputs, CompiledStateBasisInputs,
    derive_compiled_lane_thresholds, derive_compiled_post_lane_basis,
    derive_compiled_post_lane_global_normal, derive_compiled_state_basis_effects,
    execute_compiled_eight_lanes, execute_runtime_four_lanes,
};
use crate::fixed_point::{Fixed16, Vec3};
use crate::tumble::{
    TumbleContactInput, TumbleState, cross, ease_basis_toward_start, normalize,
    tumble_contact_response,
};

/// Tilt below which the ground pass sends the car out of control.
pub const GROUND_OUT_OF_CONTROL_TILT: i32 = 0xb334;
/// Tilt at or above which a resting car keeps its start-of-tick basis.
pub const GROUND_RESTING_ALIGNED_TILT: i32 = 0xfd70;
/// Pitch/roll damping factor applied as `qmul(DAT_004eac6c, 0x1eb8)`.
pub const GROUND_ANGULAR_DAMPING: i32 = 0x1eb8;

/// The car's persistent ground-contact words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroundContact {
    /// Lane handles, surface type words (`+0x42a..`), lane vectors
    /// (`+0x33c..`), lane words (`+0x2e4..`), accumulators and latch.
    pub lanes: CompiledEightLaneExecutionState,
    /// The persistent per-lane gate/drive/subtraction words.
    pub lane_inputs: CompiledSetupPoseLaneInputs,
    /// The compiled half-sign words and deformation scales the probe rebuild
    /// reads (`+0x2c..+0x34`, `+0x638..+0x640`).
    pub half_words: [i32; 3],
    pub deformation: [i32; 3],
    /// The eight probe points `+0xfc..+0x158`.
    pub probes: [[i32; 3]; 8],
    /// The latest threshold scan (`+0x73a..+0x74b`).
    pub thresholds: CompiledLaneThresholdEffects,
    /// The compiled state words `+0x648` (front) and `+0x644` (rear) that
    /// the tumbling pass scales the up row by to find the lowest wheel.
    pub wheel_reach_648_644: [i32; 2],
    /// Compiled state word `+0x20`; the barrier end-post reach is twice it.
    pub reach_20: i32,
    /// `+0x44`: the half diagonal `sqrt(sum((word >> 1)^2))` of `+0x20..+0x28`
    /// (`derive_compiled_suffix_defaults`), the trigger plane thickness.
    pub radius_44: i32,
}

/// The `vn` admission test: the new normal is kept unless the car moves away
/// from the ground faster than the speed-scaled allowance.
#[must_use]
pub fn ground_normal_admitted(velocity: Vec3, normal: Vec3, speed_ec: i32) -> bool {
    let normal_speed = velocity.dot(normal);
    let speed_share = qmul(speed_ec, 0x1_5555).min(0x1_0000);
    let allowance = qmul(0x1_0000_i32.wrapping_sub(speed_share), 0x1999);
    allowance >= normal_speed.raw()
}

/// What the ground pass does to the basis once the normal is admitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroundBasisDecision {
    OutOfControl,
    Realign,
    RestoreStart,
}

/// The tilt decision on an admitted normal.
#[must_use]
pub fn ground_basis_decision(normal: Vec3, row1: Vec3, resting_700: bool) -> GroundBasisDecision {
    let tilt = normal.dot(row1).raw();
    if tilt < GROUND_OUT_OF_CONTROL_TILT {
        GroundBasisDecision::OutOfControl
    } else if !resting_700 || tilt < GROUND_RESTING_ALIGNED_TILT {
        GroundBasisDecision::Realign
    } else {
        GroundBasisDecision::RestoreStart
    }
}

/// `FUN_00412750`: the largest positive `lane_word - probe_y` among probes
/// whose clear flag is zero, scanned from probe 7 down; zero when none.
#[must_use]
pub fn ground_penetration_lift(
    lane_words: [i32; 8],
    probes: &[[i32; 3]; 8],
    flags_73a: [u8; 8],
) -> i32 {
    let mut lift = 0;
    for probe in (0..8).rev() {
        let depth = lane_words[probe].wrapping_sub(probes[probe][1]);
        if flags_73a[probe] == 0 && depth > 0 && depth > lift {
            lift = depth;
        }
    }
    lift
}

/// The into-ground velocity removal and pitch/roll damping.
#[must_use]
pub fn constrain_ground_motion(
    velocity: Vec3,
    angular_velocity: Vec3,
    normal: Vec3,
    normal_speed: Fixed16,
    time_step: Fixed16,
) -> (Vec3, Vec3) {
    let velocity = if normal_speed.raw() < 0 {
        velocity.sub(normal.scale(normal_speed))
    } else {
        velocity
    };
    let damping = time_step.mul(Fixed16::from_raw(GROUND_ANGULAR_DAMPING));
    let angular = Vec3::new(
        angular_velocity.x.mul(damping),
        angular_velocity.y,
        angular_velocity.z.mul(damping),
    );
    (velocity, angular)
}

fn qmul(left: i32, right: i32) -> i32 {
    Fixed16::from_raw(left).mul(Fixed16::from_raw(right)).raw()
}

fn raw(vector: Vec3) -> [i32; 3] {
    [vector.x.raw(), vector.y.raw(), vector.z.raw()]
}

fn vector(words: [i32; 3]) -> Vec3 {
    Vec3::new(
        Fixed16::from_raw(words[0]),
        Fixed16::from_raw(words[1]),
        Fixed16::from_raw(words[2]),
    )
}

/// The car words the ground pass reads and writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroundPassCar {
    pub position: Vec3,
    pub velocity: Vec3,
    pub angular_velocity: Vec3,
    pub basis: [Vec3; 3],
    /// `+0x324`, the ground normal / projection axis.
    pub normal: Vec3,
    pub speed_ec: i32,
    pub resting_700: bool,
    pub out_of_control_680: bool,
    /// `+0x688`, set with `+0x680` and cleared when the car recovers.
    pub recovery_688: bool,
    /// `+0xbc..+0xc4`.
    pub axis_snapshot_bc: Vec3,
    /// The tumbling words (`ctl +0x3ec..`, `car +0x678..`).
    pub tumble: TumbleState,
    /// This tick's contact vectors `+0x4bc..+0x518`.
    pub contact_vectors: [Vec3; 8],
    /// A tumbling impact stored in `+0x3cc..+0x3d4` this tick.
    pub impact_3cc: Option<Vec3>,
}

/// Rebuild the eight probe points from the basis and position
/// (`FUN_00409c10`/`FUN_00418120`).
#[must_use]
pub fn rebuild_ground_probes(
    ground: &GroundContact,
    basis: [Vec3; 3],
    position: Vec3,
) -> [[i32; 3]; 8] {
    let rows = basis.map(raw);
    derive_compiled_state_basis_effects(CompiledStateBasisInputs {
        half_words: ground.half_words,
        matrix_rows: rows,
        center: raw(position),
        adjustment_u: rows[0],
        adjustment_v: rows[2],
        scale_638: ground.deformation[0],
        scale_63c: ground.deformation[1],
        scale_640: ground.deformation[2],
    })
    .vectors
}

pub(crate) fn scan(ground: &GroundContact) -> CompiledLaneThresholdEffects {
    derive_compiled_lane_thresholds(CompiledLaneThresholdInputs {
        lane_words: ground.lanes.lane_words,
        source_y: ground.probes.map(|probe| probe[1]),
    })
}

pub(crate) fn query<P, C>(
    ground: &mut GroundContact,
    eight_lanes: bool,
    providers: &mut [P; 8],
    config: &mut C,
) -> Result<(), CompiledEightLaneExecutionFailure<P::Error, C::Error>>
where
    P: CompiledLaneGeometryProvider,
    C: CompiledLaneConfigProvider,
{
    let input = CompiledEightLaneExecutionInputs {
        source_vectors: ground.probes,
        gate_words: ground.lane_inputs.gate_words,
        drive_words: ground.lane_inputs.drive_words,
        first_four_subtractions: ground.lane_inputs.first_four_subtractions,
    };
    ground.lanes = if eight_lanes {
        execute_compiled_eight_lanes(input, ground.lanes, providers, config)?.state
    } else {
        execute_runtime_four_lanes(input, ground.lanes, providers, config)?
    };
    Ok(())
}

/// Run `FUN_00410a60`'s in-control path on one car.
///
/// `start_basis` is the basis at the start of the tick (`+0x27c`). On error
/// nothing is written back.
///
/// # Errors
///
/// Propagates a ground-query dispatch or config failure.
pub fn run_ground_pass<P, C>(
    car: GroundPassCar,
    ground: GroundContact,
    start_basis: [Vec3; 3],
    time_step: Fixed16,
    providers: &mut [P; 8],
    config: &mut C,
) -> Result<(GroundPassCar, GroundContact), CompiledEightLaneExecutionFailure<P::Error, C::Error>>
where
    P: CompiledLaneGeometryProvider,
    C: CompiledLaneConfigProvider,
{
    let mut car = car;
    let mut ground = ground;
    if car.out_of_control_680 {
        return run_tumble_pass(car, ground, start_basis, time_step, providers, config);
    }
    ground.probes = rebuild_ground_probes(&ground, car.basis, car.position);
    let was_clear = ground.thresholds.flag_743 == 1;
    if was_clear {
        query(&mut ground, false, providers, config)?;
        ground.thresholds = scan(&ground);
    }
    let mut normal_speed = Fixed16::ZERO;
    if ground.thresholds.flag_743 == 0 {
        if !was_clear {
            query(&mut ground, false, providers, config)?;
        }
        let previous = car.normal;
        let normal = vector(
            derive_compiled_post_lane_global_normal(CompiledPostLaneNormalInput {
                source_vectors: ground.probes,
                lane_words: ground.lanes.lane_words,
                selector_words: car.basis.map(|row| row.y.raw()),
            })
            .final_published_normal,
        );
        normal_speed = car.velocity.dot(normal);
        if ground_normal_admitted(car.velocity, normal, car.speed_ec) {
            car.normal = normal;
            match ground_basis_decision(normal, car.basis[1], car.resting_700) {
                GroundBasisDecision::OutOfControl => {
                    car.out_of_control_680 = true;
                    car.recovery_688 = true;
                }
                GroundBasisDecision::Realign => {
                    let rows = derive_compiled_post_lane_basis(CompiledPostLaneBasisInputs {
                        vector_324: raw(normal),
                        vector_258: raw(car.basis[0]),
                    });
                    car.basis = [
                        vector(rows.vector_258),
                        vector(rows.vector_264),
                        vector(rows.vector_270),
                    ];
                }
                GroundBasisDecision::RestoreStart => car.basis = start_basis,
            }
            car.axis_snapshot_bc = Vec3::new(
                car.normal.dot(car.basis[0]),
                car.normal.dot(car.basis[1]),
                car.normal.dot(car.basis[2]),
            );
            ground.probes = rebuild_ground_probes(&ground, car.basis, car.position);
            query(&mut ground, car.out_of_control_680, providers, config)?;
        } else {
            car.normal = previous;
        }
    }
    let lift = ground_penetration_lift(
        ground.lanes.lane_words,
        &ground.probes,
        ground.thresholds.flags_73a,
    );
    car.position.y = Fixed16::from_raw(car.position.y.raw().wrapping_add(lift));
    for probe in &mut ground.probes {
        probe[1] = probe[1].wrapping_add(lift);
    }
    ground.thresholds = scan(&ground);
    if ground.thresholds.flag_743 == 0 {
        (car.velocity, car.angular_velocity) = constrain_ground_motion(
            car.velocity,
            car.angular_velocity,
            car.normal,
            normal_speed,
            time_step,
        );
    }
    Ok((car, ground))
}

/// Steps the righting animation (`0x411648..0x411cf9`): one row turns
/// toward the ground normal (or its negation) by at most `0x28f` per tick,
/// and the other two are rebuilt around it. When the counter runs out the
/// flags clear and the lane latch `+0x67c` asks for a reset.
fn step_righting(car: &mut GroundPassCar, ground: &mut GroundContact) {
    let n = car.normal;
    let negative = Vec3::new(n.x.neg(), n.y.neg(), n.z.neg());
    // (turning row, rejected row, rebuilt row, target).
    let (turn, reject_row, rebuilt, target) = if car.tumble.roof_3f4 {
        car.tumble.roof_ticks_43c = car.tumble.roof_ticks_43c.wrapping_sub(1);
        if car.tumble.roof_ticks_43c < 1 {
            car.tumble.righting_3ec = false;
        }
        (1, 2, 0, negative)
    } else {
        car.tumble.side_ticks_43d = car.tumble.side_ticks_43d.wrapping_sub(1);
        if car.tumble.side_ticks_43d < 1 {
            car.tumble.righting_3ec = false;
        }
        let target = if car.basis[2].y.raw() < 0 {
            negative
        } else {
            n
        };
        (2, 0, 1, target)
    };
    let delta = target.sub(car.basis[turn]);
    let distance = crate::legacy_math::sqrt_q16(delta.dot(delta).raw() as u32) as i32;
    car.basis[turn] = if distance < 0x28f {
        target
    } else {
        let step = normalize(delta).scale(Fixed16::from_raw(0x28f));
        normalize(car.basis[turn].add(step))
    };
    let axis = car.basis[turn];
    let along = car.basis[reject_row].dot(axis);
    car.basis[reject_row] = normalize(car.basis[reject_row].sub(axis.scale(along)));
    car.basis[rebuilt] = normalize(cross(axis, car.basis[reject_row]));
    if !car.tumble.righting_3ec {
        car.tumble.side_3f8 = false;
        car.tumble.roof_3f4 = false;
        car.tumble.roof_ticks_43c = 0;
        car.tumble.side_ticks_43d = 0;
        ground.lanes.latch_67c = 1;
    }
}

/// `FUN_004115d0`, the out-of-control path of the per-car ground pass.
///
/// The prelude either steps the righting animation or counts hull-probe
/// contact ticks (the lane latch `+0x67c` after 100). The common part
/// queries all eight lanes; while no probe touches it adopts the normal of a
/// nearly touching probe and runs the contact response, otherwise it adopts
/// the ground normal, lifts the car out of penetration, runs the contact
/// response (not on the landing tick) and removes the into-ground velocity.
/// It then damps the spin when the lowest wheel changes, eases the basis
/// toward the start of the tick and clears `+0x680/+0x688` once the car is
/// upright and nearly still. The control word `ctl +0x41c` (initializer
/// constant `0`) and `car +0x74e` are taken as zero; the impact and landing
/// effects, the wheel animation `FUN_00410350` and the render matrix are
/// left out.
///
/// # Errors
///
/// Propagates a ground-query dispatch or config failure.
pub fn run_tumble_pass<P, C>(
    car: GroundPassCar,
    ground: GroundContact,
    start_basis: [Vec3; 3],
    time_step: Fixed16,
    providers: &mut [P; 8],
    config: &mut C,
) -> Result<(GroundPassCar, GroundContact), CompiledEightLaneExecutionFailure<P::Error, C::Error>>
where
    P: CompiledLaneGeometryProvider,
    C: CompiledLaneConfigProvider,
{
    let mut car = car;
    let mut ground = ground;
    car.impact_3cc = None;
    if car.tumble.righting_3ec {
        step_righting(&mut car, &mut ground);
    } else if let Some(_probe) = (4..8).find(|&probe| ground.thresholds.flags_73a[probe] == 0) {
        if car.tumble.hull_ticks_678 < 100 {
            car.tumble.hull_ticks_678 += 1;
        } else {
            ground.lanes.latch_67c = 1;
        }
    }
    let previous_count = ground.thresholds.count_742;
    if car.basis[1].y.raw() < 0 {
        car.tumble.inverted_684 = true;
    }
    ground.probes = rebuild_ground_probes(&ground, car.basis, car.position);
    query(&mut ground, true, providers, config)?;
    ground.thresholds = scan(&ground);
    let ground_normal = |ground: &GroundContact, car: &GroundPassCar| {
        vector(
            derive_compiled_post_lane_global_normal(CompiledPostLaneNormalInput {
                source_vectors: ground.probes,
                lane_words: ground.lanes.lane_words,
                selector_words: car.basis.map(|row| row.y.raw()),
            })
            .final_published_normal,
        )
    };
    let respond = |car: &mut GroundPassCar| {
        let effects = tumble_contact_response(
            &mut car.tumble,
            &TumbleContactInput {
                angular_velocity: car.angular_velocity,
                velocity: car.velocity,
                normal: car.normal,
                basis: car.basis,
                speed_ec: car.speed_ec,
                axis_snapshot_bc: car.axis_snapshot_bc,
                contact_vectors: car.contact_vectors,
            },
        );
        car.angular_velocity = effects.angular_velocity;
        car.axis_snapshot_bc = effects.axis_snapshot_bc;
        if effects.impact_3cc.is_some() {
            car.impact_3cc = effects.impact_3cc;
        }
    };
    if ground.thresholds.count_742 == 0 {
        let near = (0..8).any(|probe| {
            ground.probes[probe][1]
                .wrapping_sub(ground.lanes.lane_words[probe])
                .wrapping_abs()
                < 0x3333
        });
        if near {
            car.normal = ground_normal(&ground, &car);
            respond(&mut car);
        }
    } else {
        car.normal = ground_normal(&ground, &car);
        let lift = ground_penetration_lift(
            ground.lanes.lane_words,
            &ground.probes,
            ground.thresholds.flags_73a,
        );
        car.position.y = Fixed16::from_raw(car.position.y.raw().wrapping_add(lift));
        for probe in &mut ground.probes {
            probe[1] = probe[1].wrapping_add(lift);
        }
        ground.thresholds = scan(&ground);
        if previous_count != 0 {
            respond(&mut car);
        }
        let normal_speed = car.velocity.dot(car.normal);
        if normal_speed.raw() < 0 {
            car.velocity = car.velocity.sub(car.normal.scale(normal_speed));
        }
    }
    car.axis_snapshot_bc = Vec3::new(
        car.normal.dot(car.basis[0]),
        car.normal.dot(car.basis[1]),
        car.normal.dot(car.basis[2]),
    );
    track_lowest_wheel(&mut car, &ground);
    car.basis = ease_basis_toward_start(
        car.basis,
        start_basis,
        car.axis_snapshot_bc,
        time_step,
        true,
    );
    let up = car.axis_snapshot_bc.y.raw();
    if !car.recovery_688 && up < 0 {
        car.recovery_688 = true;
    }
    if up > 0xe666
        && car.angular_velocity.x.raw().wrapping_abs() < 0x28f
        && car.angular_velocity.z.raw().wrapping_abs() < 0x28f
    {
        car.out_of_control_680 = false;
        car.recovery_688 = false;
    }
    Ok((car, ground))
}

/// The lowest wheel (`0x411fa0..0x4120b0`): each wheel probe lowered along the
/// up row by `+0x648` (front) or `+0x644` (rear) against the mean of its two
/// lane words. When the lowest wheel changes, the spin loses `0xccc` of its
/// part off the body-frame normal.
fn track_lowest_wheel(car: &mut GroundPassCar, ground: &GroundContact) {
    let previous = car.tumble.lowest_wheel_650;
    car.tumble.lowest_wheel_64c = 0;
    car.tumble.lowest_wheel_650 = -1;
    for wheel in 0..4 {
        let reach = ground.wheel_reach_648_644[usize::from(wheel >= 2)];
        let lowered_y = qmul(car.basis[1].y.raw(), reach).wrapping_add(ground.probes[wheel][1]);
        let surface = qmul(
            ground.lanes.lane_words[wheel + 4].wrapping_add(ground.lanes.lane_words[wheel]),
            0x8000,
        );
        let clearance = surface.wrapping_sub(lowered_y);
        if car.tumble.lowest_wheel_64c < clearance {
            car.tumble.lowest_wheel_64c = clearance;
            car.tumble.lowest_wheel_650 = wheel as i8;
        }
    }
    let lowest = car.tumble.lowest_wheel_650;
    if lowest != -1 && lowest != previous {
        let body_normal = Vec3::new(
            car.basis[0].dot(car.normal),
            car.basis[1].dot(car.normal),
            car.basis[2].dot(car.normal),
        );
        let along = car.angular_velocity.dot(body_normal);
        let off = car.angular_velocity.sub(body_normal.scale(along));
        car.angular_velocity = car
            .angular_velocity
            .sub(off.scale(Fixed16::from_raw(0xccc)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiled_car_initializer::CompiledLaneBuildCompletion;

    fn v(x: i32, y: i32, z: i32) -> Vec3 {
        vector([x, y, z])
    }

    /// Flat ground at a fixed height with an upward lane vector.
    struct FlatGround {
        height: i32,
    }

    impl CompiledLaneGeometryProvider for FlatGround {
        type Error = ();
        fn validate(&mut self, _source: [i32; 3], _handle: i16) -> Result<bool, ()> {
            Ok(true)
        }
        fn search(&mut self, _source: [i32; 3], _fallback: i32) -> Result<Option<i16>, ()> {
            Ok(Some(0))
        }
        fn build(
            &mut self,
            _source: [i32; 3],
            _handle: i16,
            _fallback: i32,
        ) -> Result<CompiledLaneBuildCompletion, ()> {
            Ok(CompiledLaneBuildCompletion::Built {
                lane_word: self.height,
                type_word: 0,
                output_vector: [0, 0x1_0000, 0],
            })
        }
    }

    fn flat(height: i32) -> [FlatGround; 8] {
        std::array::from_fn(|_| FlatGround { height })
    }

    fn ground() -> GroundContact {
        GroundContact {
            lanes: CompiledEightLaneExecutionState {
                handles: [0; 8],
                type_words: [0; 8],
                output_vectors: [[0; 3]; 8],
                lane_words: [0; 8],
                accumulator_words: [0; 8],
                latch_67c: 0,
            },
            lane_inputs: CompiledSetupPoseLaneInputs {
                gate_words: [0; 8],
                drive_words: [0; 8],
                first_four_subtractions: [0; 4],
            },
            half_words: [0x1_0000, 0x8000, 0x2_0000],
            deformation: [0x1_0000; 3],
            probes: [[0; 3]; 8],
            thresholds: CompiledLaneThresholdEffects {
                flags_73a: [0; 8],
                flag_743: 0,
                count_742: 8,
                indices_744: [0; 8],
            },
            wheel_reach_648_644: [0; 2],
            reach_20: 0,
            radius_44: 0,
        }
    }

    fn identity() -> [Vec3; 3] {
        [v(0x1_0000, 0, 0), v(0, 0x1_0000, 0), v(0, 0, 0x1_0000)]
    }

    fn car_at(height: i32) -> GroundPassCar {
        GroundPassCar {
            position: v(0, height, 0),
            velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
            basis: identity(),
            normal: v(0, 0x1_0000, 0),
            speed_ec: 0,
            resting_700: false,
            out_of_control_680: false,
            recovery_688: false,
            axis_snapshot_bc: Vec3::ZERO,
            tumble: TumbleState::default(),
            contact_vectors: [Vec3::ZERO; 8],
            impact_3cc: None,
        }
    }

    #[test]
    fn admission_allows_slow_separation_and_rejects_fast_lift_off() {
        let up = v(0, 0x1_0000, 0);
        // At rest the allowance is 0x1999.
        assert!(ground_normal_admitted(v(0, 0x1999, 0), up, 0));
        assert!(!ground_normal_admitted(v(0, 0x199a, 0), up, 0));
        // At speed 0.75 or more (`qmul(speed, 0x15555) >= 1`) the allowance is 0.
        assert!(ground_normal_admitted(Vec3::ZERO, up, 0xc000));
        assert!(!ground_normal_admitted(v(0, 1, 0), up, 0xc000));
        // Moving into the ground is always admitted.
        assert!(ground_normal_admitted(v(0, -0x10_0000, 0), up, 0x10_0000));
    }

    #[test]
    fn tilt_decides_out_of_control_realign_or_restore() {
        let row1 = v(0, 0x1_0000, 0);
        let tilted = |y: i32| v(0, y, 0);
        assert_eq!(
            ground_basis_decision(tilted(0xb333), row1, false),
            GroundBasisDecision::OutOfControl
        );
        assert_eq!(
            ground_basis_decision(tilted(0xb334), row1, false),
            GroundBasisDecision::Realign
        );
        assert_eq!(
            ground_basis_decision(tilted(0xfd6f), row1, true),
            GroundBasisDecision::Realign
        );
        assert_eq!(
            ground_basis_decision(tilted(0xfd70), row1, true),
            GroundBasisDecision::RestoreStart
        );
        assert_eq!(
            ground_basis_decision(tilted(0xfd70), row1, false),
            GroundBasisDecision::Realign
        );
    }

    #[test]
    fn penetration_lift_takes_the_deepest_touching_probe() {
        let mut probes = [[0; 3]; 8];
        probes[2][1] = -0x8000;
        probes[5][1] = -0x2_0000;
        let mut flags = [0; 8];
        assert_eq!(ground_penetration_lift([0; 8], &probes, flags), 0x2_0000);
        // A clear probe never contributes.
        flags[5] = 1;
        assert_eq!(ground_penetration_lift([0; 8], &probes, flags), 0x8000);
        // Probes above their lane word give no lift.
        assert_eq!(ground_penetration_lift([-0x10_0000; 8], &probes, [0; 8]), 0);
    }

    #[test]
    fn ground_constraint_removes_into_ground_speed_and_damps_pitch_and_roll() {
        let up = v(0, 0x1_0000, 0);
        let (velocity, angular) = constrain_ground_motion(
            v(0x1_0000, -0x8000, 0),
            v(0x1_0000, 0x1_0000, 0x1_0000),
            up,
            Fixed16::from_raw(-0x8000),
            Fixed16::ONE,
        );
        assert_eq!(velocity, v(0x1_0000, 0, 0));
        assert_eq!(angular, v(0x1eb8, 0x1_0000, 0x1eb8));
        let (lifting, _) = constrain_ground_motion(
            v(0, 0x8000, 0),
            Vec3::ZERO,
            up,
            Fixed16::from_raw(0x8000),
            Fixed16::ONE,
        );
        assert_eq!(lifting, v(0, 0x8000, 0));
    }

    #[test]
    fn a_sunken_car_is_lifted_onto_flat_ground_and_its_fall_is_stopped() {
        let mut car = car_at(-0x1_0000);
        car.velocity = v(0x2_0000, -0x4000, 0);
        let mut config = |_lane| -> Result<u8, ()> { Ok(0) };
        let (after, ground) = run_ground_pass(
            car,
            ground(),
            identity(),
            Fixed16::ONE,
            &mut flat(0),
            &mut config,
        )
        .unwrap();
        assert!(after.position.y > car.position.y, "lifted");
        // The generated-table normalize keeps the flat normal within a few LSB.
        assert_eq!((after.normal.x.raw(), after.normal.z.raw()), (0, 0));
        assert!((0xfff0..=0x1_0000).contains(&after.normal.y.raw()));
        // The downward speed is removed to within that precision.
        assert_eq!(after.velocity.x.raw(), 0x2_0000);
        assert!(after.velocity.y.raw().abs() <= 4, "{:?}", after.velocity);
        assert_eq!(ground.lanes.lane_words[0], 0);
        assert_eq!(
            ground.lanes.lane_words[4],
            -crate::compiled_car_initializer::RUNTIME_HULL_LANE_OFFSET
        );
        assert!(!after.out_of_control_680);
    }

    #[test]
    fn a_slightly_tilted_car_realigns_its_up_axis_to_the_ground_normal() {
        // About 10 degrees of roll: row1 = (sin, cos, 0).
        let (sin, cos) = (0x2c71, 0xfc1c);
        let tilted = [v(cos, -sin, 0), v(sin, cos, 0), v(0, 0, 0x1_0000)];
        let car = GroundPassCar {
            basis: tilted,
            ..car_at(0)
        };
        let mut config = |_lane| -> Result<u8, ()> { Ok(0) };
        let (after, _) = run_ground_pass(
            car,
            ground(),
            tilted,
            Fixed16::ONE,
            &mut flat(0),
            &mut config,
        )
        .unwrap();
        assert!(!after.out_of_control_680);
        assert_eq!(
            after.basis[1], after.normal,
            "row1 becomes the ground normal"
        );
        assert!(after.basis[1].y.raw() > 0xfff0);
        // The snapshot is the normal in the new frame.
        assert_eq!(after.axis_snapshot_bc.y, after.normal.dot(after.basis[1]));
    }

    #[test]
    fn a_car_far_above_the_ground_is_left_alone() {
        let car = GroundPassCar {
            velocity: v(0, -0x1000, 0),
            ..car_at(0x100_0000)
        };
        let mut clear = ground();
        clear.thresholds.flag_743 = 1;
        let mut config = |_lane| -> Result<u8, ()> { Ok(0) };
        let (after, ground) = run_ground_pass(
            car,
            clear,
            identity(),
            Fixed16::ONE,
            &mut flat(0),
            &mut config,
        )
        .unwrap();
        assert_eq!(ground.thresholds.flag_743, 1);
        assert_eq!(after.position, car.position);
        assert_eq!(after.velocity, car.velocity);
    }

    fn tumble(car: GroundPassCar, ground: GroundContact) -> (GroundPassCar, GroundContact) {
        let mut config = |_lane| -> Result<u8, ()> { Ok(0) };
        run_ground_pass(
            car,
            ground,
            identity(),
            Fixed16::ONE,
            &mut flat(0),
            &mut config,
        )
        .unwrap()
    }

    #[test]
    fn a_settled_out_of_control_car_is_lifted_and_recovers() {
        let car = GroundPassCar {
            out_of_control_680: true,
            recovery_688: true,
            velocity: v(0x100, -0x4000, 0),
            ..car_at(-0x1_0000)
        };
        let (after, ground) = tumble(car, ground());
        assert!(after.position.y.raw() > car.position.y.raw(), "lifted");
        // Removed up to the legacy-normalized normal's rounding.
        assert!(
            after.velocity.y.raw().abs() < 8,
            "the fall into the ground is removed"
        );
        assert_eq!(after.velocity.x.raw(), 0x100);
        assert!(!after.out_of_control_680 && !after.recovery_688);
        assert_eq!(ground.thresholds.flag_743, 0);
        assert_eq!(
            after.axis_snapshot_bc.y.raw(),
            after.normal.dot(after.basis[1]).raw()
        );
    }

    #[test]
    fn a_spinning_out_of_control_car_stays_out_of_control() {
        let car = GroundPassCar {
            out_of_control_680: true,
            recovery_688: true,
            angular_velocity: v(0x1000, 0, 0x1000),
            ..car_at(-0x1_0000)
        };
        let (after, _) = tumble(car, ground());
        assert!(after.out_of_control_680 && after.recovery_688);
    }

    #[test]
    fn righting_turns_the_roof_toward_the_ground_and_latches_at_the_end() {
        let upside_down = [
            v(0x1_0000, 0, 0),
            v(0, -0xf000, 0x6000),
            v(0, -0x6000, -0xf000),
        ];
        let mut car = GroundPassCar {
            out_of_control_680: true,
            basis: upside_down.map(crate::tumble::normalize),
            angular_velocity: v(0x1000, 0, 0x1000),
            ..car_at(0x8_0000)
        };
        car.tumble.righting_3ec = true;
        car.tumble.roof_3f4 = true;
        car.tumble.roof_ticks_43c = 2;
        let (first, ground_after) = tumble(car, ground());
        assert!(first.tumble.righting_3ec);
        assert_eq!(first.tumble.roof_ticks_43c, 1);
        assert!(first.tumble.inverted_684);
        assert_eq!(ground_after.lanes.latch_67c, 0);
        let (second, ground_after) = tumble(first, ground_after);
        assert!(!second.tumble.righting_3ec && !second.tumble.roof_3f4);
        assert_eq!(second.tumble.roof_ticks_43c, 0);
        assert_eq!(ground_after.lanes.latch_67c, 1);
    }

    #[test]
    fn hull_contact_counts_to_one_hundred_then_latches() {
        let mut car = GroundPassCar {
            out_of_control_680: true,
            angular_velocity: v(0x1000, 0, 0x1000),
            ..car_at(-0x1_0000)
        };
        car.tumble.hull_ticks_678 = 99;
        let (after, ground_after) = tumble(car, ground());
        assert_eq!(after.tumble.hull_ticks_678, 100);
        assert_eq!(ground_after.lanes.latch_67c, 0);
        // Still touching with a hull probe: the counter is full, so it latches.
        let (_, ground_after) = tumble(after, ground());
        assert_eq!(ground_after.lanes.latch_67c, 1);
    }
}
