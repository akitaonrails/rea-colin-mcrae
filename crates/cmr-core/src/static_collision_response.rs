//! Pure compatibility contract for the static-scenery response routine
//! `FUN_0043d760` from the official-EU executable.
//!
//! The native routine consumes a normal left in globals by the narrow phase,
//! mutates the car's pending linear/angular corrections and emits damage,
//! sound, and force-feedback calls. This module keeps those external calls as
//! ordered [`StaticResponseEvent`] values. The caller supplies the recovered
//! damage result explicitly, allowing the response arithmetic to remain pure.
//!
//! The narrow-phase output pointer passed as the native third argument is not
//! represented: the response routine does not read it. The square-root table
//! and secondary state changes inside `FUN_0041f630` are likewise outside this
//! contract; [`DamageResult`] is the explicit boundary for that work.

use crate::fixed_point::{Fixed16, Vec3};
pub use crate::static_world_collision::StaticContactHistory as StaticResponseHistory;

pub const STATIC_RESPONSE_HISTORY_CAPACITY: usize = 5;
pub const STATIC_RESPONSE_CONTACT_ID: i32 = 0;

const HALF: Fixed16 = Fixed16::from_raw(0x8000);
const TWO: Fixed16 = Fixed16::from_raw(0x2_0000);
const ROLLOVER_KICK: Fixed16 = Fixed16::from_raw(-0x32_0000);
const ROLLOVER_X_SCALE: Fixed16 = Fixed16::from_raw(0x64_0000);
const ROLLOVER_Z_SCALE: Fixed16 = Fixed16::from_raw(0x32_0000);

/// The three populated rows at `car+0x2a0`, each separated by 16 bytes in the
/// original object. Padding is intentionally absent from the source-owned form.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResponseBasis {
    pub rows: [Vec3; 3],
}

impl ResponseBasis {
    /// Recovered `FUN_0048e100`: dot the input with each stored row.
    #[must_use]
    pub const fn world_to_body(self, value: Vec3) -> Vec3 {
        Vec3::new(
            self.rows[0].dot(value),
            self.rows[1].dot(value),
            self.rows[2].dot(value),
        )
    }

    /// Recovered `FUN_0048dfe0`: multiply by the transpose of the stored rows.
    #[must_use]
    pub const fn body_to_world(self, value: Vec3) -> Vec3 {
        Vec3::new(
            self.rows[0]
                .x
                .mul(value.x)
                .add(self.rows[1].x.mul(value.y))
                .add(self.rows[2].x.mul(value.z)),
            self.rows[0]
                .y
                .mul(value.x)
                .add(self.rows[1].y.mul(value.y))
                .add(self.rows[2].y.mul(value.z)),
            self.rows[0]
                .z
                .mul(value.x)
                .add(self.rows[1].z.mul(value.y))
                .add(self.rows[2].z.mul(value.z)),
        )
    }
}

/// Mutable car fields read or written by the recovered response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticCollisionResponseState {
    pub velocity_d4: Vec3,
    pub projected_axis_270: Vec3,
    pub basis_2a0: ResponseBasis,
    pub position_c8: Vec3,
    /// Local contact/lever arm at `+0x3f0`, not angular velocity.
    pub contact_3f0: Vec3,
    /// Clamp values at `+0x2c/+0x30/+0x34`.
    pub contact_limits_2c: Vec3,
    pub linear_correction_3cc: Vec3,
    pub angular_correction_3d8: Vec3,
    pub out_of_control_680: bool,
    pub out_of_control_688: bool,
    pub car_type_738: i8,
    pub history: StaticResponseHistory,
}

/// Outputs supplied by the separately reconstructed damage boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub struct DamageResult {
    pub effect_handle: i32,
    pub severity: Fixed16,
}

/// Per-contact inputs and the two external query results needed to keep this
/// contract deterministic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticCollisionResponseInput {
    pub normal: Vec3,
    pub primary_limit: Fixed16,
    pub projected_limit: Fixed16,
    pub raw_material_flags: u8,
    pub contact_id: i32,
    /// Result of `FUN_00443af0`, queried only when `car_type_738 == 1`.
    pub type_one_effect_gate_blocked: bool,
    /// Used only if the contact ID is absent from previous history.
    pub damage_result: DamageResult,
    /// Models the conditional material byte write to global `0x558fb0`.
    pub capture_debug_material: bool,
}

/// Which strict threshold caused the exceptional response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExceptionalResponseKind {
    PrimaryNormalSpeed,
    ProjectedAxisSpeed,
}

/// Ordered stand-ins for calls whose implementation lives outside the pure
/// response contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StaticResponseEvent {
    /// Conditional `FUN_00443af0` query at function entry.
    QueryTypeOneEffectGate,
    /// `FUN_0041f630`, after the full linear correction has been accumulated.
    RequestDamage { linear_correction: Vec3 },
    /// `FUN_0046a2a0`.
    ContactEffect { handle: i32, world_contact: Vec3 },
    /// `FUN_00469e80`.
    MaterialImpact {
        severity: Fixed16,
        world_contact: Vec3,
        material: u8,
    },
    /// `FUN_004330c0`.
    ScalarFeedback {
        car_type: i8,
        strength: Fixed16,
        threshold: Fixed16,
    },
    /// `FUN_004331b0`.
    DirectionalFeedback {
        car_type: i8,
        direction: Vec3,
        strength: Fixed16,
        limit: Fixed16,
        trailing_zero: i32,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticCollisionResponseOutcome {
    pub state: StaticCollisionResponseState,
    pub events: Vec<StaticResponseEvent>,
    pub previous_contact: bool,
    pub exceptional: Option<ExceptionalResponseKind>,
    pub full_world_correction: Vec3,
    pub applied_world_correction: Vec3,
    pub body_correction: Vec3,
    pub world_contact: Vec3,
    pub debug_material_write: Option<u8>,
    /// Arithmetic executed for duplicate materials 5/6/9/12 but discarded by
    /// the native function. Exposing it documents the bounded dead-code match.
    pub discarded_material_tangent_measure: Option<Fixed16>,
}

const fn abs_wrapping(value: Fixed16) -> Fixed16 {
    if value.raw() < 0 { value.neg() } else { value }
}

const fn clamp_contact_axis(value: Fixed16, limit: Fixed16) -> Fixed16 {
    if abs_wrapping(value).raw() > limit.raw() {
        if value.raw() < 0 { limit.neg() } else { limit }
    } else {
        value
    }
}

const fn cross(left: Vec3, right: Vec3) -> Vec3 {
    Vec3::new(
        left.y.mul(right.z).sub(left.z.mul(right.y)),
        left.z.mul(right.x).sub(left.x.mul(right.z)),
        left.x.mul(right.y).sub(left.y.mul(right.x)),
    )
}

const fn clamp_raw(value: Fixed16, low: i32, high: i32) -> Fixed16 {
    if value.raw() > high {
        Fixed16::from_raw(high)
    } else if value.raw() < low {
        Fixed16::from_raw(low)
    } else {
        value
    }
}

/// Apply one official-EU static response using only source-owned state.
///
/// The native routine has no effective-mass denominator, tangential impulse,
/// material-dependent physics, or penetration correction. It applies
/// `normal * -dot(normal, velocity)` directly. When `+0x680` is set, only the
/// linear contribution is reduced to one half; damage and body-space torque
/// retain the full correction due to native call/order dependencies.
#[must_use]
pub fn apply_static_collision_response(
    mut state: StaticCollisionResponseState,
    input: StaticCollisionResponseInput,
) -> StaticCollisionResponseOutcome {
    let mut events = Vec::new();
    let effects_enabled = if state.car_type_738 == 1 {
        events.push(StaticResponseEvent::QueryTypeOneEffectGate);
        !input.type_one_effect_gate_blocked
    } else {
        true
    };

    state.contact_3f0.x = clamp_contact_axis(state.contact_3f0.x, state.contact_limits_2c.x);
    state.contact_3f0.y = if state.contact_limits_2c.y.raw() < 0 {
        state.contact_limits_2c.y
    } else {
        Fixed16::ZERO
    };
    state.contact_3f0.z = clamp_contact_axis(state.contact_3f0.z, state.contact_limits_2c.z);

    let exceptional = if state.out_of_control_680 {
        None
    } else if abs_wrapping(input.normal.dot(state.velocity_d4)) > input.primary_limit {
        Some(ExceptionalResponseKind::PrimaryNormalSpeed)
    } else {
        let projected = state
            .projected_axis_270
            .scale(state.projected_axis_270.dot(state.velocity_d4));
        if abs_wrapping(input.normal.dot(projected)) > input.projected_limit {
            Some(ExceptionalResponseKind::ProjectedAxisSpeed)
        } else {
            None
        }
    };

    let magnitude = input.normal.dot(state.velocity_d4).neg();
    let full_world_correction = input.normal.scale(magnitude);
    let body_correction = state.basis_2a0.world_to_body(full_world_correction);
    state.linear_correction_3cc = state.linear_correction_3cc.add(full_world_correction);

    let previous_contact = state.history.contained_last_tick(input.contact_id);
    if !previous_contact {
        events.push(StaticResponseEvent::RequestDamage {
            linear_correction: state.linear_correction_3cc,
        });
    }

    let world_contact = state
        .basis_2a0
        .body_to_world(state.contact_3f0)
        .add(state.position_c8);
    let material = input.raw_material_flags & 0x7f;
    let debug_material_write = input.capture_debug_material.then_some(material);

    let mut discarded_material_tangent_measure = None;
    if effects_enabled {
        if previous_contact {
            if matches!(material, 5 | 6 | 9 | 12) {
                let tangent = Vec3::new(input.normal.z.neg(), Fixed16::ZERO, input.normal.x);
                discarded_material_tangent_measure = Some(
                    abs_wrapping(state.velocity_d4.dot(tangent)).mul(Fixed16::from_raw(0xaaaa)),
                );
            }
        } else {
            let damage = input.damage_result;
            if damage.effect_handle != 0 {
                events.push(StaticResponseEvent::ContactEffect {
                    handle: damage.effect_handle,
                    world_contact,
                });
            }
            if damage.severity.raw() > 0 {
                events.push(StaticResponseEvent::MaterialImpact {
                    severity: damage.severity,
                    world_contact,
                    material,
                });
                events.push(StaticResponseEvent::ScalarFeedback {
                    car_type: state.car_type_738,
                    strength: damage.severity.mul(Fixed16::from_raw(0x1_8000)),
                    threshold: Fixed16::from_raw(0x4ccc),
                });
                events.push(StaticResponseEvent::DirectionalFeedback {
                    car_type: state.car_type_738,
                    direction: full_world_correction,
                    strength: damage.severity.mul(TWO),
                    limit: TWO,
                    trailing_zero: 0,
                });
            }
        }
    }

    if exceptional.is_some() {
        state.out_of_control_680 = true;
        state.out_of_control_688 = true;
    }

    let applied_world_correction = if state.out_of_control_680 {
        state.linear_correction_3cc = state.linear_correction_3cc.sub(full_world_correction);
        let half = full_world_correction.scale(HALF);
        state.linear_correction_3cc = state.linear_correction_3cc.add(half);
        half
    } else {
        full_world_correction
    };

    // The effect location uses the negative underside value; torque does not.
    state.contact_3f0.y = Fixed16::ZERO;
    if !state.out_of_control_680 {
        state.angular_correction_3d8 = state
            .angular_correction_3d8
            .add(cross(body_correction, state.contact_3f0));
    } else if let Some(kind) = exceptional {
        let torque = match kind {
            ExceptionalResponseKind::ProjectedAxisSpeed => {
                let torque = cross(body_correction, state.contact_3f0);
                Vec3::new(torque.x, torque.y.mul(TWO), torque.z)
            }
            ExceptionalResponseKind::PrimaryNormalSpeed => {
                let kick = state.basis_2a0.world_to_body(Vec3::new(
                    Fixed16::ZERO,
                    ROLLOVER_KICK,
                    Fixed16::ZERO,
                ));
                let torque = cross(body_correction, state.contact_3f0.add(kick));
                state.velocity_d4.y = state.velocity_d4.y.add(HALF);
                Vec3::new(
                    torque.x.mul(ROLLOVER_X_SCALE),
                    torque.y,
                    torque.z.mul(ROLLOVER_Z_SCALE),
                )
            }
        };
        state.angular_correction_3d8 = state.angular_correction_3d8.add(torque);
    }

    state.angular_correction_3d8.x = clamp_raw(state.angular_correction_3d8.x, -0x8_0000, 0x8_0000);
    state.angular_correction_3d8.y = clamp_raw(state.angular_correction_3d8.y, -0x4_0000, 0x4_0000);
    state.angular_correction_3d8.z = clamp_raw(state.angular_correction_3d8.z, -0x8_0000, 0x8_0000);
    state.history.push(input.contact_id);

    StaticCollisionResponseOutcome {
        state,
        events,
        previous_contact,
        exceptional,
        full_world_correction,
        applied_world_correction,
        body_correction,
        world_contact,
        debug_material_write,
        discarded_material_tangent_measure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZERO: Fixed16 = Fixed16::ZERO;
    const ONE: Fixed16 = Fixed16::ONE;

    const fn q(raw: i32) -> Fixed16 {
        Fixed16::from_raw(raw)
    }

    const fn v(x: i32, y: i32, z: i32) -> Vec3 {
        Vec3::new(q(x), q(y), q(z))
    }

    fn identity() -> ResponseBasis {
        ResponseBasis {
            rows: [v(0x10000, 0, 0), v(0, 0x10000, 0), v(0, 0, 0x10000)],
        }
    }

    fn state() -> StaticCollisionResponseState {
        StaticCollisionResponseState {
            velocity_d4: Vec3::ZERO,
            projected_axis_270: v(0x10000, 0, 0),
            basis_2a0: identity(),
            position_c8: Vec3::ZERO,
            contact_3f0: Vec3::ZERO,
            contact_limits_2c: v(0x10_0000, -0x1_0000, 0x10_0000),
            linear_correction_3cc: Vec3::ZERO,
            angular_correction_3d8: Vec3::ZERO,
            out_of_control_680: false,
            out_of_control_688: false,
            car_type_738: 0,
            history: StaticResponseHistory::default(),
        }
    }

    fn input() -> StaticCollisionResponseInput {
        StaticCollisionResponseInput {
            normal: v(0x10000, 0, 0),
            primary_limit: q(0x7fff_ffff),
            projected_limit: q(0x7fff_ffff),
            raw_material_flags: 0,
            contact_id: STATIC_RESPONSE_CONTACT_ID,
            type_one_effect_gate_blocked: false,
            damage_result: DamageResult::default(),
            capture_debug_material: false,
        }
    }

    #[test]
    fn cancels_normal_velocity_without_separation_or_mass_clamps() {
        let mut car = state();
        car.velocity_d4 = v(-0x2_0000, 0x3_0000, 0);
        let hit = apply_static_collision_response(car, input());
        assert_eq!(hit.full_world_correction, v(0x2_0000, 0, 0));
        assert_eq!(hit.state.linear_correction_3cc, v(0x2_0000, 0, 0));

        let mut separating = state();
        separating.velocity_d4 = v(0x2_0000, 0, 0);
        let hit = apply_static_collision_response(separating, input());
        assert_eq!(hit.full_world_correction, v(-0x2_0000, 0, 0));

        let mut non_unit = state();
        non_unit.velocity_d4 = v(-0x1_0000, 0, 0);
        let mut non_unit_input = input();
        non_unit_input.normal = v(0x2_0000, 0, 0);
        let hit = apply_static_collision_response(non_unit, non_unit_input);
        assert_eq!(hit.full_world_correction, v(0x4_0000, 0, 0));
    }

    #[test]
    fn matrix_directions_are_not_interchangeable() {
        let basis = ResponseBasis {
            rows: [
                v(0x1_0000, 0x2_0000, 0),
                v(0, 0x1_0000, 0),
                v(0, 0, 0x1_0000),
            ],
        };
        let value = v(0x1_0000, 0x3_0000, 0);
        assert_eq!(basis.world_to_body(value), v(0x7_0000, 0x3_0000, 0));
        assert_eq!(basis.body_to_world(value), v(0x1_0000, 0x5_0000, 0));
    }

    #[test]
    fn clamps_contact_for_effect_location_then_zeros_y_for_torque() {
        let mut car = state();
        car.velocity_d4 = v(0, -0x1_0000, 0);
        car.contact_3f0 = v(-0x3_0000, 0x9_0000, 0x4_0000);
        car.contact_limits_2c = v(0x2_0000, -0x1_0000, 0x3_0000);
        car.position_c8 = v(0x10_0000, 0x20_0000, 0x30_0000);
        let mut request = input();
        request.normal = v(0, 0x1_0000, 0);
        let hit = apply_static_collision_response(car, request);
        assert_eq!(hit.world_contact, v(0xe_0000, 0x1f_0000, 0x33_0000));
        assert_eq!(hit.state.contact_3f0, v(-0x2_0000, 0, 0x3_0000));
        assert_eq!(hit.state.angular_correction_3d8, v(0x3_0000, 0, 0x2_0000));
    }

    #[test]
    fn previous_literal_zero_suppresses_damage_but_zero_is_still_appended() {
        let mut car = state();
        car.history.previous_ids[0] = 0;
        car.history.previous_len = 1;
        let hit = apply_static_collision_response(car, input());
        assert!(hit.previous_contact);
        assert!(hit.events.is_empty());
        assert_eq!(hit.state.history.current_ids[0], 0);
        assert_eq!(hit.state.history.current_len, 1);
    }

    #[test]
    fn damage_and_feedback_observe_full_correction_before_halving() {
        let mut car = state();
        car.velocity_d4 = v(-0x2_0000, 0, 0);
        car.out_of_control_680 = true;
        let mut request = input();
        request.raw_material_flags = 0x85;
        request.damage_result = DamageResult {
            effect_handle: 7,
            severity: q(0x8000),
        };
        let hit = apply_static_collision_response(car, request);
        assert_eq!(hit.applied_world_correction, v(0x1_0000, 0, 0));
        assert_eq!(hit.state.linear_correction_3cc, v(0x1_0000, 0, 0));
        assert_eq!(hit.state.angular_correction_3d8, Vec3::ZERO);
        assert_eq!(
            hit.events,
            vec![
                StaticResponseEvent::RequestDamage {
                    linear_correction: v(0x2_0000, 0, 0)
                },
                StaticResponseEvent::ContactEffect {
                    handle: 7,
                    world_contact: v(0, -0x1_0000, 0)
                },
                StaticResponseEvent::MaterialImpact {
                    severity: q(0x8000),
                    world_contact: v(0, -0x1_0000, 0),
                    material: 5
                },
                StaticResponseEvent::ScalarFeedback {
                    car_type: 0,
                    strength: q(0xc000),
                    threshold: q(0x4ccc)
                },
                StaticResponseEvent::DirectionalFeedback {
                    car_type: 0,
                    direction: v(0x2_0000, 0, 0),
                    strength: ONE,
                    limit: TWO,
                    trailing_zero: 0
                }
            ]
        );
    }

    #[test]
    fn blocked_type_one_gate_still_requests_damage_but_suppresses_effects() {
        let mut car = state();
        car.car_type_738 = 1;
        let mut request = input();
        request.type_one_effect_gate_blocked = true;
        request.damage_result = DamageResult {
            effect_handle: 9,
            severity: ONE,
        };
        let hit = apply_static_collision_response(car, request);
        assert_eq!(
            hit.events,
            vec![
                StaticResponseEvent::QueryTypeOneEffectGate,
                StaticResponseEvent::RequestDamage {
                    linear_correction: Vec3::ZERO
                }
            ]
        );
    }

    #[test]
    fn duplicate_special_material_calculation_is_dead_physics() {
        let mut car = state();
        car.velocity_d4 = v(0, 0, 0x3_0000);
        car.history.previous_ids[0] = 12;
        car.history.previous_len = 1;
        let mut request = input();
        request.contact_id = 12;
        request.raw_material_flags = 12;
        let hit = apply_static_collision_response(car, request);
        assert_eq!(hit.discarded_material_tangent_measure, Some(q(0x1_fffe)));
        assert_eq!(hit.state.linear_correction_3cc, Vec3::ZERO);
        assert_eq!(hit.state.angular_correction_3d8, Vec3::ZERO);
    }

    #[test]
    fn projected_exception_doubles_only_middle_torque_component() {
        let mut car = state();
        car.velocity_d4 = v(-0x1_0000, 0, 0);
        car.contact_3f0 = v(0, 0, 0x1_0000);
        let mut request = input();
        request.primary_limit = q(0x2_0000);
        request.projected_limit = q(0x8000);
        let hit = apply_static_collision_response(car, request);
        assert_eq!(
            hit.exceptional,
            Some(ExceptionalResponseKind::ProjectedAxisSpeed)
        );
        assert_eq!(hit.state.linear_correction_3cc, v(0x8000, 0, 0));
        assert_eq!(hit.state.angular_correction_3d8, v(0, -0x2_0000, 0));
        assert!(hit.state.out_of_control_680);
        assert!(hit.state.out_of_control_688);
    }

    #[test]
    fn primary_exception_uses_kick_scales_and_velocity_y_nudge() {
        let mut car = state();
        car.velocity_d4 = v(-0x2_0000, 0, 0);
        car.contact_3f0 = v(0, 0, 0x1_0000);
        let mut request = input();
        request.primary_limit = q(0x1_0000);
        let hit = apply_static_collision_response(car, request);
        assert_eq!(
            hit.exceptional,
            Some(ExceptionalResponseKind::PrimaryNormalSpeed)
        );
        assert_eq!(hit.state.velocity_d4.y, HALF);
        // Raw torque is (0, -2, -100); the native final clamps apply.
        assert_eq!(hit.state.angular_correction_3d8, v(0, -0x2_0000, -0x8_0000));
    }

    #[test]
    fn final_angular_clamps_and_history_capacity_are_exact() {
        let mut car = state();
        car.angular_correction_3d8 = v(0x8_0001, -0x4_0001, -0x8_0001);
        car.history.current_len = 5;
        car.history.current_ids = [1, 2, 3, 4, 5];
        let hit = apply_static_collision_response(car, input());
        assert_eq!(
            hit.state.angular_correction_3d8,
            v(0x8_0000, -0x4_0000, -0x8_0000)
        );
        assert_eq!(hit.state.history.current_ids, [1, 2, 3, 4, 5]);
        assert_eq!(hit.state.history.current_len, 5);
    }

    #[test]
    fn threshold_comparisons_are_strict() {
        let mut car = state();
        car.velocity_d4 = v(-0x1_0000, 0, 0);
        let mut request = input();
        request.primary_limit = ONE;
        request.projected_limit = ONE;
        let hit = apply_static_collision_response(car, request);
        assert_eq!(hit.exceptional, None);
        assert!(!hit.state.out_of_control_680);
    }

    #[test]
    fn debug_material_masks_high_flag() {
        let mut request = input();
        request.raw_material_flags = 0x8c;
        request.capture_debug_material = true;
        let hit = apply_static_collision_response(state(), request);
        assert_eq!(hit.debug_material_write, Some(12));
    }

    #[test]
    fn constants_preserve_static_zero_identity() {
        assert_eq!(STATIC_RESPONSE_CONTACT_ID, 0);
        assert_eq!(ZERO, q(0));
    }
}
